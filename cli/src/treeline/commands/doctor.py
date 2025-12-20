"""Doctor command - database health checks and diagnostics."""

import asyncio
import json as json_module

import typer
from rich.console import Console

from treeline.theme import get_theme

console = Console()
theme = get_theme()

# Status icons
ICON_PASS = "✓"
ICON_WARNING = "⚠"
ICON_ERROR = "✗"


def register(
    app: typer.Typer, get_container: callable, ensure_initialized: callable
) -> None:
    """Register the doctor command with the app."""

    @app.command(name="doctor")
    def doctor_command(
        verbose: bool = typer.Option(
            False,
            "--verbose",
            "-v",
            help="Show detailed findings for each check",
        ),
        json_output: bool = typer.Option(
            False,
            "--json",
            help="Output as JSON",
        ),
    ) -> None:
        """Run database health checks and diagnostics.

        Performs comprehensive diagnostics on your Treeline database to detect:

        \b
        - Orphaned transactions (referencing deleted accounts)
        - Orphaned balance snapshots
        - Duplicate transactions (same fingerprint, last 90 days)
        - Transactions with unreasonable dates
        - Untagged transactions
        - Integration connectivity issues

        Exit codes:
          0 = All checks pass (warnings are OK)
          1 = At least one error found

        Examples:
          tl doctor              # Run health checks
          tl doctor --verbose    # Show detailed findings
          tl doctor --json       # Output as JSON for scripting
        """
        ensure_initialized()

        container = get_container()
        doctor_service = container.doctor_service()

        if not json_output:
            with console.status(f"[{theme.status_loading}]Running health checks..."):
                result = asyncio.run(doctor_service.run_all_checks())
        else:
            result = asyncio.run(doctor_service.run_all_checks())

        if not result.success:
            if json_output:
                print(json_module.dumps({"error": result.error}))
            else:
                console.print(f"[{theme.error}]Error: {result.error}[/{theme.error}]")
            raise typer.Exit(1)

        report = result.data

        # Get currency preference for display formatting
        from treeline.app.preferences_service import DEFAULT_CURRENCY

        preferences_service = container.preferences_service()
        currency_result = preferences_service.get_currency()
        currency = currency_result.data if currency_result.success else DEFAULT_CURRENCY

        if json_output:
            output = {
                "summary": {
                    "passed": report.passed,
                    "warnings": report.warnings,
                    "errors": report.errors,
                },
                "checks": {
                    check.name: {
                        "status": check.status,
                        "message": check.message,
                        "details": check.details,
                    }
                    for check in report.checks
                },
            }
            print(json_module.dumps(output, indent=2))
        else:
            display_report(report, verbose, currency)

        # Exit with error code if any errors found
        if report.errors > 0:
            raise typer.Exit(1)


def display_report(report, verbose: bool, currency: str) -> None:
    """Display health report in human-readable format."""
    console.print(f"\n[{theme.ui_header}]🩺 Database Health Check[/{theme.ui_header}]\n")

    # Display each check
    for check in report.checks:
        display_check(check, verbose, currency)

    # Summary line
    console.print()
    summary_parts = []

    if report.passed > 0:
        summary_parts.append(f"[{theme.success}]{report.passed} passed[/{theme.success}]")

    if report.warnings > 0:
        summary_parts.append(f"[{theme.warning}]{report.warnings} warning(s)[/{theme.warning}]")

    if report.errors > 0:
        summary_parts.append(f"[{theme.error}]{report.errors} error(s)[/{theme.error}]")

    console.print(f"Summary: {', '.join(summary_parts)}")

    if not verbose and (report.warnings > 0 or report.errors > 0):
        console.print(f"\n[{theme.muted}]Run with --verbose to see details[/{theme.muted}]")

    console.print()


def display_check(check, verbose: bool, currency: str) -> None:
    """Display a single health check result."""

    # Choose icon and color based on status
    if check.status == "pass":
        icon = ICON_PASS
        color = theme.success
    elif check.status == "warning":
        icon = ICON_WARNING
        color = theme.warning
    else:
        icon = ICON_ERROR
        color = theme.error

    # Format check name for display
    display_name = check.name.replace("_", " ").title()

    # Main line
    console.print(f"[{color}]{icon}[/{color}] {display_name:<24} [{theme.muted}]{check.message}[/{theme.muted}]")

    # Details in verbose mode
    if verbose and check.details:
        display_details(check, currency=currency)


def display_details(check, show_all: bool = True, currency: str = "USD") -> None:
    """Display detailed findings for a check."""
    from treeline.app.preferences_service import format_currency

    details_to_show = check.details if show_all else check.details[:5]
    for detail in details_to_show:
        if check.name == "orphaned_transactions":
            console.print(f"    [{theme.muted}]txn {detail['transaction_id'][:8]}... → account {detail['account_id'][:8]}...[/{theme.muted}]")

        elif check.name == "orphaned_snapshots":
            console.print(f"    [{theme.muted}]snapshot {detail['snapshot_id'][:8]}... → account {detail['account_id'][:8]}...[/{theme.muted}]")

        elif check.name == "duplicate_fingerprints":
            console.print(f"    [{theme.muted}]Fingerprint {detail['fingerprint']} ({detail['duplicate_count']} copies):[/{theme.muted}]")
            for txn in detail.get("transactions", [])[:3]:
                amount = txn['amount'] if txn['amount'] is not None else 0
                desc = (txn['description'] or "")[:30]
                console.print(f"      [{theme.muted}]{txn['date']}  {format_currency(amount, currency)}  {desc}[/{theme.muted}]")

        elif check.name == "date_sanity":
            amount = detail['amount'] if detail['amount'] is not None else 0
            desc = (detail['description'] or "")[:30]
            console.print(f"    [{theme.muted}]{detail['date']}  {format_currency(amount, currency)}  {desc}[/{theme.muted}]")

        elif check.name == "untagged_transactions":
            console.print(f"    [{theme.muted}]{detail['untagged_count']} of {detail['total_count']} transactions untagged[/{theme.muted}]")

        elif check.name == "budget_double_counting":
            amount = detail['amount'] if detail['amount'] is not None else 0
            desc = (detail['description'] or "")[:30]
            matches = detail.get('category_matches', 0)
            tags = detail.get('tags', [])
            tag_str = ", ".join(tags[:3]) + ("..." if len(tags) > 3 else "") if tags else "no tags"
            console.print(f"    [{theme.muted}]{detail['date']}  {format_currency(abs(amount), currency)}  {desc}  ({matches} categories, tags: {tag_str})[/{theme.muted}]")

        elif check.name == "uncategorized_expenses":
            # First item is summary
            if "uncategorized_count" in detail:
                uncategorized = detail.get('uncategorized_count', 0)
                uncategorized_amt = detail.get('uncategorized_amount', 0)
                total = detail.get('total_expense_count', 0)
                total_amt = detail.get('total_expense_amount', 0)
                console.print(f"    [{theme.muted}]{uncategorized} of {total} expenses ({format_currency(uncategorized_amt, currency)} of {format_currency(total_amt, currency)})[/{theme.muted}]")
            else:
                # Individual transaction
                amount = detail['amount'] if detail['amount'] is not None else 0
                desc = (detail['description'] or "")[:30]
                tags = detail.get('tags', [])
                tag_str = ", ".join(tags[:3]) + ("..." if len(tags) > 3 else "") if tags else "no tags"
                console.print(f"    [{theme.muted}]{detail['date']}  {format_currency(abs(amount), currency)}  {desc}  (tags: {tag_str})[/{theme.muted}]")

        elif check.name == "integration_connectivity":
            integration = detail.get("integration", "unknown")
            message = detail.get("message", "Unknown issue")
            console.print(f"    [{theme.muted}]{integration}: {message}[/{theme.muted}]")

    if not show_all and check.details and len(check.details) > 5:
        console.print(f"    [{theme.muted}]... and {len(check.details) - 5} more[/{theme.muted}]")
