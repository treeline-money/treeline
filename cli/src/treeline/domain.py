"""Domain model definitions."""

from __future__ import annotations

from datetime import date, datetime, timezone
from decimal import Decimal
from typing import Any, Dict, Generic, Mapping, Type, TypeVar

from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator


class User(BaseModel):
    """Represents an authenticated user."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    id: str
    email: str


def _ensure_tzinfo(value: datetime) -> datetime:
    if value.tzinfo is None or value.utcoffset() is None:
        msg = "datetime must be timezone-aware"
        raise ValueError(msg)
    return value.astimezone(timezone.utc)


class Account(BaseModel):
    """Represents a financial account owned by the user."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    id: UUID
    name: str = Field(min_length=1)
    nickname: str | None = None
    account_type: str | None = None
    currency: str = Field(default="USD")
    external_ids: Dict[str, str] = Field(default_factory=dict)
    balance: Decimal | None = None
    institution_name: str | None = None
    institution_url: str | None = None
    institution_domain: str | None = None
    created_at: datetime
    updated_at: datetime

    @field_validator("name")
    @classmethod
    def _validate_name(cls, value: str) -> str:
        if not value:
            msg = "account name cannot be empty"
            raise ValueError(msg)
        return value

    @field_validator("external_ids", mode="before")
    @classmethod
    def _normalize_external_ids(cls, value: object) -> Dict[str, str]:
        if value is None:
            return {}
        if isinstance(value, Mapping):
            normalized = {str(key): str(val) for key, val in value.items()}
            return normalized
        msg = "external_ids must be a mapping"
        raise TypeError(msg)

    @field_validator("currency")
    @classmethod
    def _normalize_currency(cls, value: str) -> str:
        normalized = value.strip().upper()
        if not normalized:
            msg = "currency cannot be empty"
            raise ValueError(msg)
        return normalized

    @field_validator("created_at")
    @classmethod
    def _require_timezone(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)

    @field_validator("updated_at")
    @classmethod
    def _require_timezone_updated(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)


class Transaction(BaseModel):
    """A single transaction belonging to an account."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    id: UUID
    account_id: UUID
    external_ids: Dict[str, str] = Field(default_factory=dict)
    amount: Decimal
    description: str | None = None
    transaction_date: date  # Changed from datetime - no timezone needed
    posted_date: date  # Changed from datetime - no timezone needed
    tags: tuple[str, ...] = ()
    created_at: datetime
    updated_at: datetime
    # Soft delete support
    deleted_at: datetime | None = None
    # Split transaction support - children reference parent
    parent_transaction_id: UUID | None = None

    # Note: Zero-amount transactions are valid (transfers, pending, corrections, etc.)
    # so we don't validate against zero amounts

    @field_validator("external_ids", mode="before")
    @classmethod
    def _normalize_external_ids(cls, value: object) -> Dict[str, str]:
        if value is None:
            return {}
        if isinstance(value, Mapping):
            normalized = {str(key): str(val) for key, val in value.items()}
            return normalized
        msg = "external_ids must be a mapping"
        raise TypeError(msg)

    @field_validator("transaction_date", mode="before")
    @classmethod
    def _validate_transaction_date(cls, value: date | datetime) -> date:
        """Accept date or datetime, return date."""
        if isinstance(value, datetime):
            return value.date()
        return value

    @field_validator("posted_date", mode="before")
    @classmethod
    def _validate_posted_date(cls, value: date | datetime) -> date:
        """Accept date or datetime, return date."""
        if isinstance(value, datetime):
            return value.date()
        return value

    @field_validator("created_at")
    @classmethod
    def _validate_created_at(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)

    @field_validator("updated_at")
    @classmethod
    def _validate_updated_at(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)

    @field_validator("tags", mode="before")
    @classmethod
    def _normalize_tags(cls, value: object) -> tuple[str, ...]:
        if value is None:
            return ()

        if isinstance(value, tuple):
            raw = list(value)
        elif isinstance(value, list):
            raw = value
        else:
            msg = "tags must be a list or tuple of strings"
            raise TypeError(msg)

        seen: set[str] = set()
        normalized: list[str] = []
        for item in raw:
            tag = str(item).strip()
            if not tag:
                continue
            if tag not in seen:
                seen.add(tag)
                normalized.append(tag)
        return tuple(normalized)

    @model_validator(mode="after")
    def _generate_fingerprint_if_missing(self) -> "Transaction":
        """Auto-generate fingerprint and store in external_ids if not present."""
        if "fingerprint" not in self.external_ids:
            fingerprint = self._calculate_fingerprint()
            # external_ids is a dict, but the model is frozen, so we need to use object.__setattr__
            ids_dict = dict(self.external_ids)
            ids_dict["fingerprint"] = fingerprint
            object.__setattr__(self, "external_ids", ids_dict)
        return self

    def _calculate_fingerprint(self) -> str:
        """Generate fingerprint hash for deduplication.

        Uses: account_id, transaction_date, amount (with sign), and normalized description.

        Description normalization handles CSV vs SimpleFIN format differences:
        - Removes literal "null" strings (CSV exports)
        - Removes card number masks (XXXXXXXXXXXX1234 - CSV only)
        - Normalizes account/phone numbers to last 4 digits (XXXXXX7070 vs 7208987070)
        - Removes whitespace and special characters
        """
        import hashlib
        import re

        tx_date = self.transaction_date.isoformat()  # Already a date object
        # Keep sign - purchases and refunds are different transactions
        # Special case: normalize negative zero to positive zero for consistency
        amount = self.amount if self.amount != 0 else abs(self.amount)
        amount_normalized = f"{amount:.2f}"

        # Normalize description to handle CSV vs SimpleFIN differences
        desc = (self.description or "").lower()

        # Remove literal "null" strings (common in CSV exports)
        desc_normalized = re.sub(r"\bnull\b", "", desc)

        # Remove card number masks (10+ X's followed by 4 digits) - these only appear in CSV
        desc_normalized = re.sub(r"x{10,}\d{4}", "", desc_normalized)

        # Normalize shorter phone/account numbers (7-12 digits or X's + digits)
        # These appear in both CSV and SimpleFIN, just masked differently
        # Examples: XXXXXX7070 vs 7208987070, XXXX9969 vs 00009969
        def normalize_account_numbers(match):
            text = match.group(0)
            # Extract digits only
            digits = "".join(c for c in text if c.isdigit())
            # Keep last 4 digits if we have at least 4
            if len(digits) >= 4:
                return digits[-4:]
            return text

        desc_normalized = re.sub(
            r"[x0-9]{7,12}", normalize_account_numbers, desc_normalized
        )

        # Remove whitespace
        desc_normalized = re.sub(r"\s+", "", desc_normalized)

        # Remove all special characters, keep only alphanumeric
        desc_normalized = re.sub(r"[^a-z0-9]", "", desc_normalized)

        fingerprint_str = (
            f"{self.account_id}|{tx_date}|{amount_normalized}|{desc_normalized}"
        )
        fingerprint_hash = hashlib.sha256(fingerprint_str.encode()).hexdigest()[:16]

        return fingerprint_hash


class BalanceSnapshot(BaseModel):
    """Represents an account balance captured at a point in time."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    id: UUID
    account_id: UUID
    balance: Decimal
    snapshot_time: datetime  # Naive datetime (local time)
    created_at: datetime  # Timezone-aware (UTC)
    updated_at: datetime  # Timezone-aware (UTC)
    source: str | None = None  # 'sync', 'manual', 'backfill', or None for legacy

    @field_validator("created_at")
    @classmethod
    def _require_timezone_created(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)

    @field_validator("updated_at")
    @classmethod
    def _require_timezone_updated(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)


T = TypeVar("T")


class Result(BaseModel, Generic[T]):
    success: bool
    data: T | None = None
    error: str | None = None
    context: Dict[str, Any] | None = None

    def raise_for_error(self, exc_type: Type[Exception] = Exception):
        raise exc_type(self.error or "Error has occurred")


def Ok(data: T | None = None, context: Dict[str, Any] | None = None) -> Result[T]:
    return Result(success=True, data=data, context=context)


def Fail(error: str, context: Dict[str, Any] | None = None) -> Result[T]:
    return Result(success=False, error=error, context=context)


# Analysis Mode Models


class AnalysisSession(BaseModel):
    """State for an analysis mode session."""

    model_config = ConfigDict(extra="forbid")

    sql: str = ""
    results: list[list[Any]] | None = None
    columns: list[str] | None = None
    chart: Any | None = None  # Will be ChartDisplay later
    view_mode: str = "results"  # "results" or "chart" or "wizard" or "save_query" or "save_chart" or "help" or "load_menu" or "browse_query" or "browse_chart"
    scroll_offset: int = 0  # Current scroll position in results (vertical)
    column_offset: int = 0  # Current column offset (horizontal scroll)
    selected_row: int = (
        0  # Currently selected/highlighted row (absolute index in results)
    )

    # Chart wizard state
    wizard_step: str = ""  # "chart_type", "x_column", "y_column", or ""
    wizard_chart_type: str = ""
    wizard_x_column: str = ""
    wizard_y_column: str = ""

    # Chart scrolling
    chart_scroll_offset: int = 0  # Vertical scroll offset for chart view

    # Save state
    save_input_buffer: str = ""  # Buffer for save name input

    # Browser state
    browse_items: list[str] = []  # List of available items to load
    browse_selected_index: int = 0  # Currently selected item in browser

    # Error state
    error_message: str = ""  # Error message to display

    def has_results(self) -> bool:
        """Check if session has query results."""
        return self.results is not None and self.columns is not None

    def has_chart(self) -> bool:
        """Check if session has a chart."""
        return self.chart is not None

    def toggle_view(self) -> None:
        """Toggle between results and chart view."""
        if self.view_mode == "results":
            self.view_mode = "chart"
        else:
            self.view_mode = "results"

    def reset(self) -> None:
        """Reset results and chart, keep SQL."""
        self.results = None
        self.columns = None
        self.chart = None
        self.view_mode = "results"

    def clear(self) -> None:
        """Clear everything."""
        self.sql = ""
        self.results = None
        self.columns = None
        self.chart = None
        self.view_mode = "results"


# Chart Configuration Model


class ChartConfig(BaseModel):
    """Represents a saved chart configuration."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    name: str
    query: str
    chart_type: str  # bar, line, scatter, histogram, boxplot
    x_column: str
    y_column: str
    title: str | None = None
    xlabel: str | None = None
    ylabel: str | None = None
    color: str | None = None
    description: str | None = None


# Backup Models


class BackupMetadata(BaseModel):
    """Metadata for a backup file."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    name: str  # e.g., "treeline-2025-01-15T10-30-00.duckdb"
    created_at: datetime  # When backup was created (timezone-aware UTC)
    size_bytes: int  # File size in bytes

    @field_validator("created_at")
    @classmethod
    def _require_timezone(cls, value: datetime) -> datetime:
        return _ensure_tzinfo(value)


# Encryption Models


class EncryptionMetadata(BaseModel):
    """Encryption configuration metadata stored in encryption.json."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    encrypted: bool
    salt: str  # Base64-encoded random salt
    algorithm: str = "argon2id"
    version: int = 1
    argon2_params: Dict[str, int] = Field(
        default_factory=lambda: {
            "time_cost": 3,
            "memory_cost": 65536,  # 64 MiB
            "parallelism": 4,
            "hash_len": 32,
        }
    )


class EncryptionStatus(BaseModel):
    """Status of database encryption for display."""

    model_config = ConfigDict(frozen=True, str_strip_whitespace=True, extra="forbid")

    encrypted: bool
    algorithm: str | None = None
    version: int | None = None
