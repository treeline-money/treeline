"""Service for backup and restore operations."""

from pathlib import Path
from typing import List

from treeline.abstractions import BackupStorageProvider
from treeline.domain import BackupMetadata, Fail, Result
from treeline.utils import get_logger

logger = get_logger("backup")

DEFAULT_MAX_BACKUPS = 7


class BackupService:
    """Service for backup and restore operations.

    Orchestrates backup operations, handles retention policy, and delegates
    storage operations to the BackupStorageProvider.
    """

    def __init__(
        self,
        storage_provider: BackupStorageProvider,
        db_path: Path,
        max_backups: int = DEFAULT_MAX_BACKUPS,
    ):
        """Initialize backup service.

        Args:
            storage_provider: Provider for backup storage operations
            db_path: Path to the database file to backup/restore
            max_backups: Maximum number of backups to retain (default: 7)
        """
        self.storage = storage_provider
        self.db_path = db_path
        self.max_backups = max_backups

    async def backup(self) -> Result[BackupMetadata]:
        """Create a backup, enforcing retention policy.

        If we're at max_backups, deletes the oldest backup before creating a new one.

        Returns:
            Result containing BackupMetadata for the created backup
        """
        # Check current backup count
        list_result = await self.storage.list_backups()
        if not list_result.success:
            return Fail(f"Failed to list existing backups: {list_result.error}")

        existing_backups = list_result.data or []

        # Enforce retention: delete oldest if at or above max
        if len(existing_backups) >= self.max_backups:
            # Backups are sorted newest first, so oldest is at the end
            backups_to_delete = existing_backups[self.max_backups - 1 :]

            for backup in backups_to_delete:
                delete_result = await self.storage.delete_backup(backup.name)
                if not delete_result.success:
                    logger.warning(
                        f"Failed to delete old backup {backup.name}: {delete_result.error}"
                    )
                else:
                    logger.info(f"Deleted old backup to maintain retention: {backup.name}")

        # Create new backup
        return await self.storage.create_backup(self.db_path)

    async def restore(self, backup_name: str) -> Result[None]:
        """Restore from a backup.

        Overwrites the current database with the backup.

        Args:
            backup_name: Name of the backup to restore

        Returns:
            Result indicating success/failure
        """
        return await self.storage.restore_backup(backup_name, self.db_path)

    async def list_backups(self) -> Result[List[BackupMetadata]]:
        """List all backups.

        Returns:
            Result containing list of BackupMetadata, sorted newest first
        """
        return await self.storage.list_backups()

    async def clear_all(self) -> Result[int]:
        """Delete all backups.

        Returns:
            Result containing count of deleted backups
        """
        return await self.storage.delete_all_backups()
