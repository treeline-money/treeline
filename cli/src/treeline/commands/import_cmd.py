"""Import command - import transactions from CSV files."""

import asyncio
import json as json_module
import os
from pathlib import Path
from typing import Any, Dict, List, Optional
from uuid import UUID

import typer
from rich.console import Console
from rich.prompt import Prompt
from rich.table import Table

from treeline.app.account_service import AccountService
from treeline.app.import_service import ImportService
from treeline.domain import Account, Transaction
from treeline.theme import get_theme

console = Console()
theme = get_theme()

ACCOUNT_TYPES = ["depository", "credit", "investment", "loan", "other"]


def register(app: typer.Typer, get_container: callable, ensure_initialized: callable) -> None:
    """Register the import command with the app."""

    @app.command(name="import")
    def import_command(
        file_path: str = typer.Argument(None, help="Path to CSV file (omit for interactive mode)"),
        account_id: str = typer.Option(None, "--account-id", help="Account ID to import into"),
        date_column: str = typer.Option(None, "--date-column", help="CSV column name for date"),
        amount_column: str = typer.Option(None, "--amount-column", help="CSV column name for amount"),
        description_column: str = typer.Option(None, "--description-column", help="CSV column name for description"),
        debit_column: str = typer.Option(None, "--debit-column", help="CSV column name for debits"),
        credit_column: str = typer.Option(None, "--credit-column", help="CSV column name for credits"),
        flip_signs: bool = typer.Option(False, "--flip-signs", help="Flip transaction signs (for credit cards)"),
        debit_negative: bool = typer.Option(False, "--debit-negative", help="Negate debit amounts"),
        preview: bool = typer.Option(False, "--preview", help="Preview only, don't import"),
        json_output: bool = typer.Option(False, "--json", help="Output as JSON"),
    ) -> None:
        """Import transactions from CSV file.

        Run 'tl import' with no arguments for interactive mode with auto-detection.

        Examples:
          tl import
          tl import transactions.csv --account-id <uuid>
          tl import transactions.csv --account-id <uuid> --preview
        """
        ensure_initialized()

        container = get_container()
        import_service = container.import_service()
        account_service = container.account_service()

        # Get user's currency preference for display
        from treeline.app.preferences_service import DEFAULT_CURRENCY

        preferences_service = container.preferences_service()
        currency_result = preferences_service.get_currency()
        user_currency = currency_result.data if currency_result.success else DEFAULT_CURRENCY

        # Interactive mode - collect parameters interactively
        if file_path is None:
            params = _collect_params_interactive(import_service, account_service, user_currency)
            if params is None:
                return  # User cancelled

            file_path = params["file_path"]
            account_id = params["account_id"]
            flip_signs = params["flip_signs"]
            debit_negative = params["debit_negative"]
            column_mapping = params["column_mapping"]
        else:
            # Scriptable mode - validate required params
            csv_path = Path(file_path).expanduser()
            if not csv_path.exists():
                console.print(f"[{theme.error}]Error: File not found: {file_path}[/{theme.error}]")
                raise typer.Exit(1)
            file_path = str(csv_path)

            if not account_id:
                console.print(f"[{theme.error}]Error: --account-id is required for scriptable import[/{theme.error}]")
                console.print(f"[{theme.muted}]Run 'tl status --json' to see account IDs[/{theme.muted}]")
                raise typer.Exit(1)

            # Build column mapping from CLI args or auto-detect
            column_mapping = _build_column_mapping(
                date_column, amount_column, description_column, debit_column, credit_column
            )
            if not column_mapping:
                column_mapping = _detect_columns(import_service, file_path, json_output)
                if column_mapping is None:
                    raise typer.Exit(1)

        # Preview mode
        if preview:
            _do_preview(import_service, file_path, column_mapping, flip_signs, debit_negative, json_output, user_currency)
            return

        # Import mode
        _do_import(
            import_service, file_path, UUID(account_id) if isinstance(account_id, str) else account_id,
            column_mapping, flip_signs, debit_negative, json_output
        )


# =============================================================================
# Core import operations (shared by both modes)
# =============================================================================

def _detect_columns(
    import_service: ImportService, file_path: str, json_output: bool = False
) -> Optional[Dict[str, str]]:
    """Auto-detect CSV columns. Returns None on failure."""
    if not json_output:
        with console.status(f"[{theme.status_loading}]Detecting CSV columns..."):
            result = asyncio.run(import_service.detect_columns(source_type="csv", file_path=file_path))
    else:
        result = asyncio.run(import_service.detect_columns(source_type="csv", file_path=file_path))

    if not result.success:
        console.print(f"[{theme.error}]Error: Column detection failed: {result.error}[/{theme.error}]")
        return None
    return result.data


def _do_preview(
    import_service: ImportService,
    file_path: str,
    column_mapping: Dict[str, str],
    flip_signs: bool,
    debit_negative: bool,
    json_output: bool,
    currency: str = "USD",
) -> None:
    """Preview transactions without importing."""
    preview_result = asyncio.run(
        import_service.preview_csv_import(
            file_path=file_path,
            column_mapping=column_mapping,
            date_format="auto",
            limit=10,
            flip_signs=flip_signs,
            debit_negative=debit_negative,
        )
    )

    if not preview_result.success:
        console.print(f"[{theme.error}]Error: Preview failed: {preview_result.error}[/{theme.error}]")
        raise typer.Exit(1)

    if json_output:
        preview_data = {
            "file": file_path,
            "flip_signs": flip_signs,
            "debit_negative": debit_negative,
            "preview": [
                {"date": str(tx.transaction_date), "description": tx.description, "amount": float(tx.amount)}
                for tx in preview_result.data
            ],
        }
        print(json_module.dumps(preview_data, indent=2))
    else:
        console.print(f"\n[{theme.ui_header}]Import Preview[/{theme.ui_header}]\n")
        console.print(f"File: {file_path}")
        console.print(f"Flip signs: {flip_signs}")
        if debit_negative:
            console.print(f"Debit negative: {debit_negative}")
        console.print()
        _display_preview_table(preview_result.data[:10], currency)
        console.print(f"\n[{theme.muted}]Remove --preview flag to import[/{theme.muted}]\n")


def _do_import(
    import_service: ImportService,
    file_path: str,
    account_id: UUID,
    column_mapping: Dict[str, str],
    flip_signs: bool,
    debit_negative: bool,
    json_output: bool,
) -> None:
    """Execute the import."""
    source_options = {
        "file_path": file_path,
        "column_mapping": column_mapping,
        "date_format": "auto",
        "flip_signs": flip_signs,
        "debit_negative": debit_negative,
    }

    if not json_output:
        with console.status(f"[{theme.status_loading}]Importing transactions..."):
            result = asyncio.run(
                import_service.import_transactions(
                    source_type="csv", account_id=account_id, source_options=source_options
                )
            )
    else:
        result = asyncio.run(
            import_service.import_transactions(
                source_type="csv", account_id=account_id, source_options=source_options
            )
        )

    if not result.success:
        console.print(f"[{theme.error}]Error: {result.error}[/{theme.error}]")
        raise typer.Exit(1)

    if json_output:
        print(json_module.dumps(result.data, indent=2, default=str))
    else:
        stats = result.data
        console.print(f"\n[{theme.success}]✓ Import complete![/{theme.success}]")
        console.print(f"  Discovered: {stats['discovered']} transactions")
        console.print(f"  Imported: {stats['imported']} new transactions")
        console.print(f"  Skipped: {stats['skipped']} duplicates\n")


# =============================================================================
# Interactive parameter collection
# =============================================================================

def _collect_params_interactive(
    import_service: ImportService, account_service: AccountService, currency: str = "USD"
) -> Optional[Dict[str, Any]]:
    """Interactively collect all parameters needed for import.

    Returns dict with: file_path, account_id, column_mapping, flip_signs, debit_negative
    Returns None if user cancels.
    """
    console.print(f"\n[{theme.ui_header}]CSV Import[/{theme.ui_header}]\n")

    # 1. Get file path
    console.print(f"[{theme.info}]Enter path to CSV file:[/{theme.info}]")
    try:
        file_path = _prompt_file_path("")
    except (KeyboardInterrupt, EOFError):
        console.print(f"\n[{theme.warning}]Import cancelled[/{theme.warning}]\n")
        return None

    if not file_path:
        console.print(f"[{theme.warning}]Import cancelled[/{theme.warning}]\n")
        return None

    expanded_path = os.path.expanduser(file_path)
    csv_path = Path(expanded_path)
    if not csv_path.exists():
        console.print(f"[{theme.error}]Error: File not found: {file_path}[/{theme.error}]\n")
        return None

    # 2. Get account selection
    console.print(f"\n[{theme.muted}]Fetching accounts...[/{theme.muted}]")
    accounts_result = asyncio.run(account_service.get_accounts())
    if not accounts_result.success:
        console.print(f"[{theme.error}]Error fetching accounts: {accounts_result.error}[/{theme.error}]\n")
        return None

    account_id = _prompt_account_selection(accounts_result.data or [])
    if account_id is None:
        console.print(f"[{theme.warning}]Import cancelled[/{theme.warning}]\n")
        return None

    # Handle account creation
    if account_id == "CREATE_NEW":
        account_details = _prompt_create_account()
        if not account_details:
            console.print(f"[{theme.warning}]Import cancelled[/{theme.warning}]\n")
            return None

        console.print(f"\n[{theme.muted}]Creating account...[/{theme.muted}]")
        create_result = asyncio.run(
            account_service.create_account(
                name=account_details["name"],
                account_type=account_details["account_type"],
                institution=account_details.get("institution"),
                currency=account_details["currency"],
            )
        )
        if not create_result.success:
            console.print(f"[{theme.error}]Error creating account: {create_result.error}[/{theme.error}]\n")
            return None

        account_id = create_result.data.id
        console.print(f"[{theme.success}]✓ Created account '{account_details['name']}' ({account_details['account_type']})[/{theme.success}]")

    # 3. Auto-detect columns
    console.print(f"\n[{theme.muted}]Detecting CSV columns...[/{theme.muted}]")
    detect_result = asyncio.run(import_service.detect_columns(source_type="csv", file_path=str(csv_path)))
    if not detect_result.success:
        console.print(f"[{theme.error}]Error detecting columns: {detect_result.error}[/{theme.error}]\n")
        return None

    column_mapping = detect_result.data
    console.print(f"\n[{theme.success}]Detected columns:[/{theme.success}]")
    for field, column in column_mapping.items():
        console.print(f"  {field}: {column}")

    if not column_mapping.get("date") or (not column_mapping.get("amount") and not column_mapping.get("debit")):
        console.print(f"\n[{theme.warning}]Warning: Required columns not detected![/{theme.warning}]")
        console.print(f"[{theme.muted}]For manual column mapping, use scriptable mode:[/{theme.muted}]")
        console.print(f'[{theme.muted}]  tl import {csv_path.name} --date-column "YourDateColumn" --amount-column "YourAmountColumn"[/{theme.muted}]\n')
        return None

    # 4. Interactive preview loop to confirm/adjust sign settings
    flip_signs = False
    debit_negative = False
    flip_signs, debit_negative = _interactive_preview_loop(
        import_service, str(csv_path), column_mapping, flip_signs, debit_negative
    )
    if flip_signs is None:  # User cancelled
        return None

    return {
        "file_path": str(csv_path),
        "account_id": account_id,
        "column_mapping": column_mapping,
        "flip_signs": flip_signs,
        "debit_negative": debit_negative,
    }


def _interactive_preview_loop(
    import_service: ImportService,
    file_path: str,
    column_mapping: Dict[str, str],
    flip_signs: bool,
    debit_negative: bool,
) -> tuple[Optional[bool], Optional[bool]]:
    """Interactive preview loop allowing user to adjust sign settings.

    Returns (flip_signs, debit_negative) or (None, None) if cancelled.
    """
    show_initial_preview = True

    while True:
        console.print(f"\n[{theme.muted}]Generating preview...[/{theme.muted}]")
        preview_result = asyncio.run(
            import_service.preview_csv_import(
                file_path=file_path,
                column_mapping=column_mapping,
                date_format="auto",
                limit=15,
                flip_signs=flip_signs,
                debit_negative=debit_negative,
            )
        )

        if not preview_result.success:
            console.print(f"[{theme.error}]Error: {preview_result.error}[/{theme.error}]\n")
            return None, None

        preview_txs = preview_result.data
        if len(preview_txs) == 0:
            console.print(f"\n[{theme.error}]No transactions found in CSV![/{theme.error}]")
            console.print(f"[{theme.muted}]This could mean:[/{theme.muted}]")
            console.print("  - The CSV has no data rows")
            console.print("  - Date or amount parsing failed")
            console.print(f"\n[{theme.muted}]Try scriptable mode with explicit columns:[/{theme.muted}]")
            console.print(f'[{theme.muted}]  tl import file.csv --preview --date-column "YourDateColumn" --amount-column "YourAmountColumn"[/{theme.muted}]\n')
            return None, None

        if show_initial_preview:
            console.print(f"\n[{theme.ui_header}]Preview - First 5 Transactions:[/{theme.ui_header}]\n")
            _display_preview_table(preview_txs[:5])
            console.print(f"\n[{theme.muted}]({len(preview_txs)} total transactions in file)[/{theme.muted}]")
            console.print(f"[{theme.ui_header}]Preview Check[/{theme.ui_header}]")
            console.print(f"[{theme.muted}]Spending should appear as NEGATIVE ({theme.negative_amount}), income/refunds as POSITIVE ({theme.positive_amount})[/{theme.muted}]\n")

        console.print(f"[{theme.info}]What would you like to do?[/{theme.info}]")
        console.print("  [1] Proceed with import")
        console.print("  [2] View more transactions (next 10)")
        console.print("  [3] Flip all signs (if spending shows positive)")
        console.print("  [4] Negate debits (for unsigned debit/credit CSVs)")
        console.print("  [5] Cancel import")
        console.print(f"[{theme.muted}](Press Ctrl+C to cancel)[/{theme.muted}]")

        try:
            choice = Prompt.ask(f"\n[{theme.info}]Choice[/{theme.info}]", choices=["1", "2", "3", "4", "5"], default="1")
        except (KeyboardInterrupt, EOFError):
            console.print(f"\n[{theme.warning}]Import cancelled[/{theme.warning}]\n")
            return None, None

        if choice == "1":
            return flip_signs, debit_negative
        elif choice == "2":
            console.print(f"\n[{theme.ui_header}]Extended Preview - First 15 Transactions:[/{theme.ui_header}]\n")
            _display_preview_table(preview_txs[:15])
            console.print()
            show_initial_preview = False
        elif choice == "3":
            flip_signs = not flip_signs
            console.print(f"[{theme.muted}]Signs flipped, regenerating preview...[/{theme.muted}]")
            show_initial_preview = True
        elif choice == "4":
            debit_negative = not debit_negative
            console.print(f"[{theme.muted}]Debit negation {'enabled' if debit_negative else 'disabled'}, regenerating preview...[/{theme.muted}]")
            show_initial_preview = True
        else:
            console.print(f"[{theme.warning}]Import cancelled[/{theme.warning}]\n")
            return None, None


# =============================================================================
# Helper functions
# =============================================================================

def _build_column_mapping(
    date_column: Optional[str],
    amount_column: Optional[str],
    description_column: Optional[str],
    debit_column: Optional[str],
    credit_column: Optional[str],
) -> Optional[Dict[str, str]]:
    """Build column mapping from CLI args. Returns None if no args provided."""
    if not any([date_column, amount_column, debit_column, credit_column]):
        return None

    mapping = {}
    if date_column:
        mapping["date"] = date_column
    if amount_column:
        mapping["amount"] = amount_column
    if description_column:
        mapping["description"] = description_column
    if debit_column:
        mapping["debit"] = debit_column
    if credit_column:
        mapping["credit"] = credit_column
    return mapping


def _display_preview_table(transactions: List[Transaction], currency: str = "USD") -> None:
    """Display transaction preview table."""
    from treeline.app.preferences_service import format_currency

    table = Table(show_header=True, box=None, padding=(0, 1))
    table.add_column("Date", width=12)
    table.add_column("Description", width=40)
    table.add_column("Amount", justify="right", width=15)

    for tx in transactions:
        date_str = tx.transaction_date.strftime("%Y-%m-%d")
        desc = (tx.description or "")[:38]

        amount_str = format_currency(tx.amount, currency)
        amount_style = theme.negative_amount if tx.amount < 0 else theme.positive_amount

        table.add_row(date_str, desc, f"[{amount_style}]{amount_str}[/{amount_style}]")

    console.print(table)


def _prompt_file_path(prompt_text: str = "") -> str:
    """Prompt user for a file path with autocomplete."""
    from prompt_toolkit import PromptSession
    from prompt_toolkit.completion import PathCompleter
    from prompt_toolkit.key_binding import KeyBindings
    from prompt_toolkit.filters import completion_is_selected

    completer = PathCompleter(expanduser=True, only_directories=False)
    kb = KeyBindings()

    @kb.add("enter", filter=completion_is_selected)
    def _(event):
        completion = event.current_buffer.complete_state.current_completion
        if completion:
            event.current_buffer.apply_completion(completion)
            current_text = event.current_buffer.text
            expanded_path = os.path.expanduser(current_text)
            if os.path.isdir(expanded_path) and not current_text.endswith("/"):
                event.current_buffer.insert_text("/")

    session = PromptSession(completer=completer, key_bindings=kb)
    return session.prompt(f"{prompt_text}: " if prompt_text else ">: ")


def _prompt_account_selection(accounts: List[Account]) -> Optional[UUID | str]:
    """Display accounts and get user selection. Returns UUID, 'CREATE_NEW', or None."""
    console.print(f"\n[{theme.info}]Select account to import into:[/{theme.info}]")
    console.print(f"[{theme.muted}](Press Ctrl+C to cancel)[/{theme.muted}]\n")

    if not accounts:
        console.print(f"[{theme.muted}]No accounts found. Let's create one.[/{theme.muted}]")
        try:
            Prompt.ask(f"\n[{theme.info}]Press Enter to continue[/{theme.info}]", default="")
            return "CREATE_NEW"
        except (KeyboardInterrupt, EOFError):
            return None

    for i, account in enumerate(accounts, 1):
        institution = f" - {account.institution_name}" if account.institution_name else ""
        account_type_display = f" ({account.account_type})" if account.account_type else ""
        console.print(f"  [{i}] {account.name}{institution}{account_type_display}")

    console.print("  [c] Create new account")

    try:
        choice = Prompt.ask(f"\n[{theme.info}]Choose account (1-{len(accounts)} or 'c' to create)[/{theme.info}]", default="1")
    except (KeyboardInterrupt, EOFError):
        return None

    if choice.lower() == "c":
        return "CREATE_NEW"

    try:
        idx = int(choice) - 1
        if 0 <= idx < len(accounts):
            return accounts[idx].id
    except ValueError:
        pass

    console.print(f"[{theme.error}]Invalid account selection[/{theme.error}]")
    return None


def _prompt_create_account() -> Optional[Dict[str, Any]]:
    """Prompt user to create a new account."""
    console.print(f"\n[{theme.info}]Create new account:[/{theme.info}]")

    try:
        account_name = Prompt.ask(f"[{theme.info}]Account name[/{theme.info}]")
        if not account_name.strip():
            console.print(f"[{theme.error}]Account name cannot be empty[/{theme.error}]")
            return None

        institution = Prompt.ask(f"[{theme.info}]Institution (optional, press Enter to skip)[/{theme.info}]", default="")

        console.print(f"\n[{theme.info}]Account type:[/{theme.info}]")
        for i, acc_type in enumerate(ACCOUNT_TYPES, 1):
            console.print(f"  [{i}] {acc_type}")

        type_choice = Prompt.ask(f"[{theme.info}]Choose account type (1-{len(ACCOUNT_TYPES)})[/{theme.info}]", default="1")

        try:
            type_idx = int(type_choice) - 1
            if 0 <= type_idx < len(ACCOUNT_TYPES):
                account_type = ACCOUNT_TYPES[type_idx]
            else:
                console.print(f"[{theme.error}]Invalid account type[/{theme.error}]")
                return None
        except ValueError:
            console.print(f"[{theme.error}]Invalid account type[/{theme.error}]")
            return None

        currency = Prompt.ask(f"[{theme.info}]Currency[/{theme.info}]", default="USD").upper()

        return {
            "name": account_name.strip(),
            "institution": institution.strip() if institution.strip() else None,
            "account_type": account_type,
            "currency": currency,
        }

    except (KeyboardInterrupt, EOFError):
        console.print(f"\n[{theme.warning}]Cancelled[/{theme.warning}]")
        return None
