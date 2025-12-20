"""Service for database operations."""

from typing import Any, Dict

from treeline.abstractions import Repository
from treeline.app.backup_service import BackupService
from treeline.domain import Result, Fail


class DbService:
    """Service for database operations."""

    def __init__(self, repository: Repository):
        self.repository = repository

    async def initialize_db(self) -> Result:
        """Initialize database directory and schema."""
        db_result = await self.repository.ensure_db_exists()
        if not db_result.success:
            return db_result

        return await self.repository.ensure_schema_upgraded()

    async def execute_query(self, sql: str) -> Result:
        cleaned_sql = self._clean_and_validate_sql(sql)
        return await self.repository.execute_query(cleaned_sql)

    async def execute_write_query(self, sql: str) -> Result:
        cleaned_sql = self._clean_and_validate_sql(sql)
        return await self.repository.execute_write_query(cleaned_sql)

    def _clean_and_validate_sql(self, sql: str) -> str:
        # TODO: Implement SQL cleaning and validation
        return sql

    async def compact(
        self, backup_service: BackupService | None = None
    ) -> Result[Dict[str, Any]]:
        """Compact the database to reclaim space.

        Args:
            backup_service: Optional BackupService to create a safety backup first.
                           If provided, a backup is created before compaction.

        Returns:
            Result containing dict with:
              - "original_size": int - size in bytes before compaction
              - "compacted_size": int - size in bytes after compaction
              - "backup_name": str | None - name of safety backup if created
        """
        backup_name = None

        # Create safety backup if backup_service provided
        if backup_service:
            backup_result = await backup_service.backup()
            if not backup_result.success:
                return Fail(f"Failed to create safety backup: {backup_result.error}")
            backup_name = backup_result.data.name

        # Perform compaction
        compact_result = await self.repository.compact()
        if not compact_result.success:
            return compact_result

        # Add backup name to result
        result_data = compact_result.data
        result_data["backup_name"] = backup_name

        return compact_result
