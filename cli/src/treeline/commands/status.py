"""Status command - show account summary and statistics."""

import asyncio
import json

import typer
from pydantic import BaseModel
from rich.console import Console
from rich.table import Table

from treeline.theme import get_theme

console = Console()
theme = get_theme()


def json_serializer(obj):
    """Custom JSON serializer for Pydantic models and other objects."""
    if isinstance(obj, BaseModel):
        return obj.model_dump(mode="json")
    return str(obj)


def output_json(data: dict) -> None:
    """Output data as JSON."""
    print(json.dumps(data, indent=2, default=json_serializer))


def display_status(status: dict) -> None:
    """Display status using Rich formatting."""
    console.print(f"\n[{theme.ui_header}]📊 Financial Data Status[/{theme.ui_header}]\n")

    # Display summary
    summary_table = Table(show_header=False, box=None, padding=(0, 2))
    summary_table.add_column("Metric", style=theme.info)
    summary_table.add_column("Value", style=theme.ui_value)

    summary_table.add_row("Accounts", str(len(status["accounts"])))
    summary_table.add_row("Transactions", str(status["total_transactions"]))
    summary_table.add_row("Balance Snapshots", str(status["total_snapshots"]))
    summary_table.add_row("Integrations", str(len(status["integrations"])))

    console.print(summary_table)

    # Date range
    if status["earliest_date"] and status["latest_date"]:
        console.print(
            f"\n[{theme.muted}]Date range: {status['earliest_date']} to {status['latest_date']}[/{theme.muted}]"
        )

    # Show integrations
    if status["integrations"]:
        console.print(f"\n[{theme.emphasis}]Connected Integrations:[/{theme.emphasis}]")
        for integration in status["integrations"]:
            console.print(f"  • {integration['integrationName']}")

    console.print()


def register(app: typer.Typer, get_container: callable) -> None:
    """Register the status command with the app."""

    @app.command(name="status")
    def status_command(
        json_output: bool = typer.Option(False, "--json", help="Output as JSON"),
    ) -> None:
        """Show account summary and statistics."""
        container = get_container()
        status_service = container.status_service()

        result = asyncio.run(status_service.get_status())

        if not result.success:
            console.print(f"[{theme.error}]Error: {result.error}[/{theme.error}]")
            raise typer.Exit(1)

        if json_output:
            json_data = {
                "total_accounts": result.data["total_accounts"],
                "total_transactions": result.data["total_transactions"],
                "total_snapshots": result.data["total_snapshots"],
                "total_integrations": result.data["total_integrations"],
                "integration_names": result.data["integration_names"],
                "accounts": [
                    {
                        "id": str(acc.id),
                        "name": acc.name,
                        "institution_name": acc.institution_name,
                    }
                    for acc in result.data["accounts"]
                ],
                "date_range": {
                    "earliest": result.data["earliest_date"],
                    "latest": result.data["latest_date"],
                },
            }
            output_json(json_data)
        else:
            display_status(result.data)
