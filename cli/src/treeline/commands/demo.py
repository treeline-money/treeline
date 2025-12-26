"""Demo command - toggle demo mode with scenario support."""

import asyncio
from abc import ABC, abstractmethod
from enum import Enum
from typing import Dict

import typer
from rich.console import Console

from treeline.config import is_demo_mode, set_demo_mode
from treeline.utils import get_treeline_dir
from treeline.theme import get_theme

console = Console()
theme = get_theme()


# =============================================================================
# Scenario Abstraction
# =============================================================================

class DemoScenarioBase(ABC):
    """Abstract base class for demo scenarios.

    Each scenario defines how to set up the demo database with specific data.
    This follows hexagonal architecture - the demo command doesn't need to know
    the details of each scenario's setup logic.
    """

    @property
    @abstractmethod
    def name(self) -> str:
        """Scenario identifier used in CLI."""
        pass

    @property
    @abstractmethod
    def description(self) -> str:
        """Human-readable description for help text."""
        pass

    @abstractmethod
    def setup(self, get_container: callable) -> None:
        """Set up the scenario data in the demo database.

        Args:
            get_container: Factory function to get the DI container
        """
        pass


class EmptyScenario(DemoScenarioBase):
    """Empty database for testing new user experience."""

    @property
    def name(self) -> str:
        return "empty"

    @property
    def description(self) -> str:
        return "Empty database for testing new user experience"

    def setup(self, get_container: callable) -> None:
        # Nothing to do - database is already empty after initialization
        pass


class DefaultScenario(DemoScenarioBase):
    """Full sample data with accounts, transactions, and budget."""

    @property
    def name(self) -> str:
        return "default"

    @property
    def description(self) -> str:
        return "Full sample data (accounts, transactions, budget)"

    def setup(self, get_container: callable) -> None:
        """Load full demo data including accounts, transactions, and budget."""
        container = get_container()

        # Create demo integration if it doesn't exist
        integration_service = container.integration_service()
        integrations_result = asyncio.run(integration_service.get_integrations())

        has_demo = False
        if integrations_result.success:
            for integration in integrations_result.data or []:
                if integration.get("integrationName") == "demo":
                    has_demo = True
                    break

        demo_provider = container.get_integration_provider("demo")

        if not has_demo:
            asyncio.run(integration_service.create_integration(demo_provider, "demo", {}))

        # Sync demo accounts and transactions
        sync_service = container.sync_service()
        console.print(f"[{theme.muted}]Syncing demo data...[/{theme.muted}]")
        with console.status(f"[{theme.status_loading}]Syncing demo accounts and transactions..."):
            result = asyncio.run(sync_service.sync_all_integrations())

        if result.success:
            console.print(f"[{theme.success}]Demo data synced successfully![/{theme.success}]")
        else:
            console.print(f"[{theme.warning}]Note: {result.error}[/{theme.warning}]")

        # Generate balance history
        db_service = container.db_service()
        account_service = container.account_service()
        accounts_result = asyncio.run(account_service.get_accounts())

        if accounts_result.success and accounts_result.data:
            account_id_map = {}
            for account in accounts_result.data:
                demo_id = account.external_ids.get("demo")
                if demo_id:
                    account_id_map[demo_id] = str(account.id)

            if account_id_map:
                with console.status(f"[{theme.status_loading}]Generating balance history..."):
                    balance_sql = demo_provider.generate_demo_balance_history_sql(account_id_map)
                    balance_result = asyncio.run(db_service.execute_write_query(balance_sql))

                if balance_result.success:
                    console.print(f"[{theme.success}]Created balance history for {len(account_id_map)} accounts[/{theme.success}]")
                else:
                    console.print(f"[{theme.warning}]Note: {balance_result.error}[/{theme.warning}]")

        # Seed budget categories
        with console.status(f"[{theme.status_loading}]Setting up demo budget..."):
            budget_sql = demo_provider.generate_demo_budget_sql()
            budget_result = asyncio.run(db_service.execute_write_query(budget_sql))

        if budget_result.success:
            console.print(f"[{theme.success}]Demo budget configured[/{theme.success}]")
        else:
            console.print(f"[{theme.warning}]Note: {budget_result.error}[/{theme.warning}]")


# =============================================================================
# Scenario Registry
# =============================================================================

# Register all available scenarios
SCENARIOS: Dict[str, DemoScenarioBase] = {
    scenario.name: scenario
    for scenario in [
        DefaultScenario(),
        EmptyScenario(),
        # Future scenarios can be added here:
        # MinimalScenario(),
        # HeavyScenario(),
    ]
}


class ScenarioChoice(str, Enum):
    """Enum for CLI scenario choices (auto-generated from registry)."""
    DEFAULT = "default"
    EMPTY = "empty"


def _build_scenario_help() -> str:
    """Build help text for available scenarios."""
    lines = ["Available scenarios:"]
    for scenario in SCENARIOS.values():
        lines.append(f"  {scenario.name:<10} - {scenario.description}")
    return "\n".join(lines)


def register(app: typer.Typer, get_container: callable, ensure_initialized: callable) -> None:
    """Register the demo command with the app."""

    @app.command(name="demo")
    def demo_command(
        action: str = typer.Argument(
            None, help="Action: 'on', 'off', or 'status' (default: status)"
        ),
        scenario: ScenarioChoice = typer.Option(
            ScenarioChoice.DEFAULT,
            "--scenario", "-s",
            help="Demo scenario to use",
            case_sensitive=False,
        ),
    ) -> None:
        """Toggle demo mode on/off.

        Demo mode uses a separate database, allowing you to explore Treeline
        without affecting your real data.

        Examples:
          tl demo                       # Show current status
          tl demo on                    # Enable with sample data
          tl demo on --scenario empty   # Enable with empty database
          tl demo on -s empty           # Short form
          tl demo off                   # Disable demo mode

        Available scenarios:
          default  - Full sample data (accounts, transactions, budget)
          empty    - Empty database for testing new user experience
        """
        # Default to status if no action provided
        if action is None:
            action = "status"

        action_lower = action.lower()

        if action_lower == "status":
            _show_status()
        elif action_lower == "on":
            # Look up scenario from registry
            scenario_impl = SCENARIOS.get(scenario.value)
            if not scenario_impl:
                console.print(f"[{theme.error}]Unknown scenario: {scenario.value}[/{theme.error}]")
                raise typer.Exit(1)
            _enable_demo(get_container, ensure_initialized, scenario_impl)
        elif action_lower == "off":
            _disable_demo()
        else:
            console.print(f"[{theme.error}]Unknown action: {action}[/{theme.error}]")
            console.print(f"[{theme.muted}]Use 'on', 'off', or 'status'[/{theme.muted}]")
            raise typer.Exit(1)


def _show_status() -> None:
    """Show current demo mode status."""
    if is_demo_mode():
        console.print(f"\n[{theme.warning}]Demo mode is ON[/{theme.warning}]")
        console.print(f"[{theme.muted}]Using demo.duckdb with sample data[/{theme.muted}]")
        console.print(f"[{theme.muted}]Run 'tl demo off' to switch to real data[/{theme.muted}]\n")
    else:
        console.print(f"\n[{theme.success}]Demo mode is OFF[/{theme.success}]")
        console.print(f"[{theme.muted}]Using treeline.duckdb with real data[/{theme.muted}]")
        console.print(f"[{theme.muted}]Run 'tl demo on' to try demo mode[/{theme.muted}]\n")


def _delete_demo_database() -> None:
    """Delete existing demo database for fresh start."""
    demo_db_path = get_treeline_dir() / "demo.duckdb"
    if demo_db_path.exists():
        demo_db_path.unlink()
        # Also remove WAL file if it exists
        wal_path = demo_db_path.with_suffix(".duckdb.wal")
        if wal_path.exists():
            wal_path.unlink()


def _enable_demo(get_container: callable, ensure_initialized: callable, scenario: DemoScenarioBase) -> None:
    """Enable demo mode with specified scenario.

    Args:
        get_container: Factory function to get the DI container
        ensure_initialized: Function to initialize the database
        scenario: The scenario implementation to set up
    """
    # Always delete existing demo database for fresh scenario
    _delete_demo_database()

    set_demo_mode(True)

    # Reset container to pick up new database
    from treeline.cli import reset_container
    reset_container()

    console.print(f"\n[{theme.success}]Demo mode enabled[/{theme.success}]")
    console.print(f"[{theme.muted}]Scenario: {scenario.name} - {scenario.description}[/{theme.muted}]")

    # Initialize demo database (runs migrations)
    ensure_initialized()

    # Delegate to scenario for setup - no if/else needed
    scenario.setup(get_container)

    console.print(f"\n[{theme.muted}]Run 'tl demo off' to return to real data[/{theme.muted}]\n")


def _disable_demo() -> None:
    """Disable demo mode."""
    if not is_demo_mode():
        console.print(f"[{theme.muted}]Demo mode is already disabled[/{theme.muted}]\n")
        return

    set_demo_mode(False)
    console.print(f"\n[{theme.success}]Demo mode disabled[/{theme.success}]")
    console.print(f"[{theme.muted}]Now using treeline.duckdb with real data[/{theme.muted}]")
    console.print(f"[{theme.muted}]Run 'tl status' to see your data[/{theme.muted}]\n")
