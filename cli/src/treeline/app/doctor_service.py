"""Service for database health checks and diagnostics."""

from dataclasses import dataclass
from datetime import date, timedelta
from typing import TYPE_CHECKING, Any, Literal

from treeline.abstractions import Repository
from treeline.domain import Ok, Result

if TYPE_CHECKING:
    from treeline.app.sync_service import SyncService


@dataclass
class HealthCheck:
    """Result of a single health check."""

    name: str
    status: Literal["pass", "warning", "error"]
    message: str
    details: list[dict[str, Any]] | None = None


@dataclass
class HealthReport:
    """Complete health report with all checks."""

    checks: list[HealthCheck]
    passed: int
    warnings: int
    errors: int


class DoctorService:
    """Service for database health checks and diagnostics."""

    def __init__(self, repository: Repository, sync_service: "SyncService | None" = None):
        self.repository = repository
        self.sync_service = sync_service

    async def run_all_checks(self) -> Result[HealthReport]:
        """Run all health checks and return a complete report."""
        checks = [
            await self._check_orphaned_transactions(),
            await self._check_orphaned_snapshots(),
            await self._check_duplicate_fingerprints(),
            await self._check_date_sanity(),
            await self._check_untagged_transactions(),
            await self._check_budget_double_counting(),
            await self._check_uncategorized_expenses(),
            await self._check_integration_connectivity(),
        ]

        passed = sum(1 for c in checks if c.status == "pass")
        warnings = sum(1 for c in checks if c.status == "warning")
        errors = sum(1 for c in checks if c.status == "error")

        return Ok(
            data=HealthReport(
                checks=checks,
                passed=passed,
                warnings=warnings,
                errors=errors,
            )
        )

    async def _check_orphaned_transactions(self) -> HealthCheck:
        """Check for transactions referencing non-existent accounts."""
        query = """
            SELECT
                t.transaction_id,
                t.account_id,
                t.description,
                t.amount
            FROM sys_transactions t
            LEFT JOIN sys_accounts a ON t.account_id = a.account_id
            WHERE a.account_id IS NULL
            LIMIT 100
        """
        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="orphaned_transactions",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = len(rows)

        if count == 0:
            return HealthCheck(
                name="orphaned_transactions",
                status="pass",
                message="No orphaned transactions found",
            )

        details = [
            {
                "transaction_id": row[0],
                "account_id": row[1],
                "description": row[2],
                "amount": float(row[3]) if row[3] else None,
            }
            for row in rows
        ]

        return HealthCheck(
            name="orphaned_transactions",
            status="error",
            message=f"{count} transaction(s) reference missing accounts",
            details=details,
        )

    async def _check_orphaned_snapshots(self) -> HealthCheck:
        """Check for balance snapshots referencing non-existent accounts."""
        query = """
            SELECT
                s.snapshot_id,
                s.account_id,
                s.balance,
                s.snapshot_time
            FROM sys_balance_snapshots s
            LEFT JOIN sys_accounts a ON s.account_id = a.account_id
            WHERE a.account_id IS NULL
            LIMIT 100
        """
        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="orphaned_snapshots",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = len(rows)

        if count == 0:
            return HealthCheck(
                name="orphaned_snapshots",
                status="pass",
                message="No orphaned snapshots found",
            )

        details = [
            {
                "snapshot_id": row[0],
                "account_id": row[1],
                "balance": float(row[2]) if row[2] else None,
                "snapshot_time": str(row[3]) if row[3] else None,
            }
            for row in rows
        ]

        return HealthCheck(
            name="orphaned_snapshots",
            status="error",
            message=f"{count} snapshot(s) reference missing accounts",
            details=details,
        )

    async def _check_duplicate_fingerprints(self) -> HealthCheck:
        """Check for recent transactions with duplicate fingerprints (potential duplicates).

        Only checks transactions from the last 90 days to avoid flagging
        old duplicates that users have already reviewed and accepted.
        """
        # Look for fingerprints that appear more than once within the same account
        # Exclude soft-deleted transactions from this check
        # Use json_extract_string for DuckDB compatibility
        # Only look at recent transactions (last 90 days)
        cutoff_date = date.today() - timedelta(days=90)
        query = f"""
            SELECT
                json_extract_string(external_ids, '$.fingerprint') as fingerprint,
                account_id,
                COUNT(*) as count
            FROM sys_transactions
            WHERE deleted_at IS NULL
              AND json_extract_string(external_ids, '$.fingerprint') IS NOT NULL
              AND transaction_date >= '{cutoff_date}'
            GROUP BY json_extract_string(external_ids, '$.fingerprint'), account_id
            HAVING COUNT(*) > 1
            LIMIT 50
        """
        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="duplicate_fingerprints",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = len(rows)

        if count == 0:
            return HealthCheck(
                name="duplicate_fingerprints",
                status="pass",
                message="No duplicate fingerprints found",
            )

        # Get details about the duplicates
        details = []
        for row in rows[:10]:  # Limit details to first 10 sets
            fingerprint = row[0]
            account_id = row[1]
            dup_count = row[2]

            # Get the actual transactions
            detail_query = f"""
                SELECT transaction_id, transaction_date, amount, description
                FROM sys_transactions
                WHERE json_extract_string(external_ids, '$.fingerprint') = '{fingerprint}'
                  AND account_id = '{account_id}'
                  AND deleted_at IS NULL
                LIMIT 5
            """
            detail_result = await self.repository.execute_query(detail_query)
            if detail_result.success:
                txns = [
                    {
                        "transaction_id": r[0],
                        "date": str(r[1]) if r[1] else None,
                        "amount": float(r[2]) if r[2] else None,
                        "description": r[3],
                    }
                    for r in detail_result.data.get("rows", [])
                ]
                details.append(
                    {
                        "fingerprint": fingerprint[:16] + "...",
                        "account_id": account_id,
                        "duplicate_count": dup_count,
                        "transactions": txns,
                    }
                )

        return HealthCheck(
            name="duplicate_fingerprints",
            status="warning",
            message=f"{count} set(s) of potential duplicate transactions found",
            details=details,
        )

    async def _check_date_sanity(self) -> HealthCheck:
        """Check for transactions with unreasonable dates."""
        today = date.today()
        far_future = today + timedelta(days=365)  # More than 1 year in future
        ancient_past = date(1970, 1, 1)

        query = f"""
            SELECT
                transaction_id,
                transaction_date,
                description,
                amount
            FROM sys_transactions
            WHERE deleted_at IS NULL
              AND (transaction_date > '{far_future}' OR transaction_date < '{ancient_past}')
            LIMIT 100
        """
        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="date_sanity",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = len(rows)

        if count == 0:
            return HealthCheck(
                name="date_sanity",
                status="pass",
                message="All transaction dates are valid",
            )

        details = [
            {
                "transaction_id": row[0],
                "date": str(row[1]) if row[1] else None,
                "description": row[2],
                "amount": float(row[3]) if row[3] else None,
            }
            for row in rows
        ]

        return HealthCheck(
            name="date_sanity",
            status="error",
            message=f"{count} transaction(s) have unreasonable dates",
            details=details,
        )

    async def _check_untagged_transactions(self) -> HealthCheck:
        """Check for transactions without any tags (informational)."""
        query = """
            SELECT COUNT(*) as untagged_count
            FROM sys_transactions
            WHERE deleted_at IS NULL
              AND (tags IS NULL OR array_length(tags, 1) IS NULL OR array_length(tags, 1) = 0)
        """
        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="untagged_transactions",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = rows[0][0] if rows else 0

        # Also get total count for percentage
        total_query = """
            SELECT COUNT(*) FROM sys_transactions WHERE deleted_at IS NULL
        """
        total_result = await self.repository.execute_query(total_query)
        total = (
            total_result.data.get("rows", [[0]])[0][0]
            if total_result.success
            else 0
        )

        if count == 0:
            return HealthCheck(
                name="untagged_transactions",
                status="pass",
                message="All transactions are tagged",
            )

        percentage = (count / total * 100) if total > 0 else 0

        return HealthCheck(
            name="untagged_transactions",
            status="warning",
            message=f"{count} transaction(s) have no tags ({percentage:.0f}% of total)",
            details=[{"untagged_count": count, "total_count": total}],
        )

    async def _check_budget_double_counting(self) -> HealthCheck:
        """Check for transactions that match multiple budget categories.

        This can lead to double-counting in budget reports. Only checks
        the current month's budget if budget data exists.
        """
        # Check if budget table exists
        table_check = await self.repository.execute_query("""
            SELECT table_name FROM information_schema.tables
            WHERE table_name = 'sys_plugin_budget_categories'
        """)

        if not table_check.success or not table_check.data.get("rows"):
            return HealthCheck(
                name="budget_double_counting",
                status="pass",
                message="No budget configured",
            )

        # Get current month
        current_month = date.today().strftime("%Y-%m")

        # Load categories for the current month
        categories_result = await self.repository.execute_query(f"""
            SELECT category_id, name, tags, require_all
            FROM sys_plugin_budget_categories
            WHERE month = '{current_month}' AND type = 'expense'
        """)

        if not categories_result.success:
            return HealthCheck(
                name="budget_double_counting",
                status="error",
                message=f"Failed to check: {categories_result.error}",
            )

        categories = categories_result.data.get("rows", [])
        if not categories:
            return HealthCheck(
                name="budget_double_counting",
                status="pass",
                message="No budget categories for current month",
            )

        # Build a query to find transactions matching multiple categories
        # For each transaction, count how many categories it matches
        category_conditions = []
        for cat in categories:
            _cat_id = cat[0]
            _cat_name = cat[1]
            tags = cat[2] if cat[2] else []
            require_all = cat[3]

            if not tags:
                continue

            if require_all:
                # All tags must be present
                conditions = " AND ".join(
                    f"list_contains(tags, '{tag}')" for tag in tags
                )
                category_conditions.append(f"CASE WHEN {conditions} THEN 1 ELSE 0 END")
            else:
                # Any tag matches
                tag_list = ", ".join(f"'{tag}'" for tag in tags)
                category_conditions.append(
                    f"CASE WHEN list_has_any(tags, [{tag_list}]) THEN 1 ELSE 0 END"
                )

        if not category_conditions:
            return HealthCheck(
                name="budget_double_counting",
                status="pass",
                message="No tag-based budget categories",
            )

        # Query to find transactions with multiple category matches
        match_sum = " + ".join(category_conditions)
        query = f"""
            SELECT
                transaction_id,
                transaction_date,
                description,
                amount,
                tags,
                ({match_sum}) as category_matches
            FROM sys_transactions
            WHERE deleted_at IS NULL
              AND strftime('%Y-%m', transaction_date) = '{current_month}'
              AND amount < 0
              AND ({match_sum}) > 1
            ORDER BY ({match_sum}) DESC, ABS(amount) DESC
            LIMIT 20
        """

        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="budget_double_counting",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = len(rows)

        if count == 0:
            return HealthCheck(
                name="budget_double_counting",
                status="pass",
                message="No double-counted transactions found",
            )

        # Get total amount being double-counted
        total_amount = sum(abs(float(row[3])) for row in rows if row[3])

        details = [
            {
                "transaction_id": row[0],
                "date": str(row[1]) if row[1] else None,
                "description": (row[2] or "")[:40],
                "amount": float(row[3]) if row[3] else None,
                "tags": row[4] if row[4] else [],
                "category_matches": row[5],
            }
            for row in rows
        ]

        # Get user's currency preference for formatting
        from treeline.app.preferences_service import (
            DEFAULT_CURRENCY,
            PreferencesService,
            format_currency,
        )

        prefs = PreferencesService()
        currency_result = prefs.get_currency()
        currency = currency_result.data if currency_result.success else DEFAULT_CURRENCY

        return HealthCheck(
            name="budget_double_counting",
            status="warning",
            message=f"{count} transaction(s) match multiple budget categories ({format_currency(total_amount, currency, decimal_places=0)} affected)",
            details=details,
        )

    async def _check_uncategorized_expenses(self) -> HealthCheck:
        """Check for expense transactions not included in any budget category.

        This helps identify spending that isn't being tracked in the budget.
        Only checks the current month's budget if budget data exists.
        """
        # Check if budget table exists
        table_check = await self.repository.execute_query("""
            SELECT table_name FROM information_schema.tables
            WHERE table_name = 'sys_plugin_budget_categories'
        """)

        if not table_check.success or not table_check.data.get("rows"):
            return HealthCheck(
                name="uncategorized_expenses",
                status="pass",
                message="No budget configured",
            )

        # Get current month
        current_month = date.today().strftime("%Y-%m")

        # Load expense categories for the current month
        categories_result = await self.repository.execute_query(f"""
            SELECT category_id, name, tags, require_all
            FROM sys_plugin_budget_categories
            WHERE month = '{current_month}' AND type = 'expense'
        """)

        if not categories_result.success:
            return HealthCheck(
                name="uncategorized_expenses",
                status="error",
                message=f"Failed to check: {categories_result.error}",
            )

        categories = categories_result.data.get("rows", [])
        if not categories:
            return HealthCheck(
                name="uncategorized_expenses",
                status="pass",
                message="No budget categories for current month",
            )

        # Build conditions for matching ANY category
        category_conditions = []
        for cat in categories:
            tags = cat[2] if cat[2] else []
            require_all = cat[3]

            if not tags:
                continue

            if require_all:
                # All tags must be present
                conditions = " AND ".join(
                    f"list_contains(tags, '{tag}')" for tag in tags
                )
                category_conditions.append(f"({conditions})")
            else:
                # Any tag matches
                tag_list = ", ".join(f"'{tag}'" for tag in tags)
                category_conditions.append(f"list_has_any(tags, [{tag_list}])")

        if not category_conditions:
            return HealthCheck(
                name="uncategorized_expenses",
                status="pass",
                message="No tag-based budget categories",
            )

        # Query to find expense transactions that match ZERO categories
        match_any = " OR ".join(category_conditions)
        query = f"""
            SELECT
                transaction_id,
                transaction_date,
                description,
                amount,
                tags
            FROM sys_transactions
            WHERE deleted_at IS NULL
              AND strftime('%Y-%m', transaction_date) = '{current_month}'
              AND amount < 0
              AND NOT ({match_any})
            ORDER BY ABS(amount) DESC
            LIMIT 50
        """

        result = await self.repository.execute_query(query)

        if not result.success:
            return HealthCheck(
                name="uncategorized_expenses",
                status="error",
                message=f"Failed to check: {result.error}",
            )

        rows = result.data.get("rows", [])
        count = len(rows)

        if count == 0:
            return HealthCheck(
                name="uncategorized_expenses",
                status="pass",
                message="All expenses are categorized in budget",
            )

        # Get total uncategorized amount
        total_amount = sum(abs(float(row[3])) for row in rows if row[3])

        # Get total expense count for context
        total_query = f"""
            SELECT COUNT(*), SUM(ABS(amount))
            FROM sys_transactions
            WHERE deleted_at IS NULL
              AND strftime('%Y-%m', transaction_date) = '{current_month}'
              AND amount < 0
        """
        total_result = await self.repository.execute_query(total_query)
        total_count = 0
        total_expense_amount = 0
        if total_result.success and total_result.data.get("rows"):
            row = total_result.data["rows"][0]
            total_count = row[0] or 0
            total_expense_amount = float(row[1]) if row[1] else 0

        details = [
            {
                "transaction_id": row[0],
                "date": str(row[1]) if row[1] else None,
                "description": (row[2] or "")[:40],
                "amount": float(row[3]) if row[3] else None,
                "tags": row[4] if row[4] else [],
            }
            for row in rows[:20]  # Limit details
        ]

        # Add summary as first detail
        details.insert(0, {
            "uncategorized_count": count if count < 50 else "50+",
            "uncategorized_amount": total_amount,
            "total_expense_count": total_count,
            "total_expense_amount": total_expense_amount,
        })

        # Get user's currency preference for formatting
        from treeline.app.preferences_service import (
            DEFAULT_CURRENCY,
            PreferencesService,
            format_currency,
        )

        prefs = PreferencesService()
        currency_result = prefs.get_currency()
        currency = currency_result.data if currency_result.success else DEFAULT_CURRENCY

        return HealthCheck(
            name="uncategorized_expenses",
            status="warning",
            message=f"{count}+ expense(s) not in any budget category ({format_currency(total_amount, currency, decimal_places=0)} untracked)",
            details=details,
        )

    async def _check_integration_connectivity(self) -> HealthCheck:
        """Check if configured integrations can connect to their data sources.

        Runs a dry-run sync to verify bank connections are working.
        Skipped if no integrations are configured.
        """
        if not self.sync_service:
            return HealthCheck(
                name="integration_connectivity",
                status="pass",
                message="No sync service available",
            )

        # Run a dry-run sync to check connectivity
        result = await self.sync_service.sync_all_integrations(dry_run=True)

        if not result.success:
            # "No integrations configured" is not an error - just skip the check
            if "No integrations configured" in (result.error or ""):
                return HealthCheck(
                    name="integration_connectivity",
                    status="pass",
                    message="No integrations configured",
                )
            # Other errors are actual problems
            return HealthCheck(
                name="integration_connectivity",
                status="error",
                message=f"Sync check failed: {result.error}",
            )

        # Check for provider warnings (e.g., "You must reauthenticate")
        sync_results = result.data.get("results", [])
        issues = []

        for sync_result in sync_results:
            integration = sync_result.get("integration", "unknown")
            provider_warnings = sync_result.get("provider_warnings", [])
            error = sync_result.get("error")

            if error:
                issues.append({
                    "integration": integration,
                    "issue": "error",
                    "message": error,
                })
            elif provider_warnings:
                for warning in provider_warnings:
                    issues.append({
                        "integration": integration,
                        "issue": "warning",
                        "message": warning,
                    })

        if not issues:
            integration_count = len(sync_results)
            return HealthCheck(
                name="integration_connectivity",
                status="pass",
                message=f"All {integration_count} integration(s) connected",
            )

        return HealthCheck(
            name="integration_connectivity",
            status="warning",
            message=f"{len(issues)} integration issue(s) found",
            details=issues,
        )
