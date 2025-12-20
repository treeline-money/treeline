"""CSV file provider for importing transactions."""

import csv
import re
from datetime import date, datetime, timezone
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any, Dict, List
from uuid import uuid4

from treeline.abstractions import DataAggregationProvider
from treeline.domain import Account, BalanceSnapshot, Fail, Ok, Result, Transaction


class CSVProvider(DataAggregationProvider):
    """CSV file implementation for data aggregation."""

    @property
    def can_get_accounts(self) -> bool:
        return False  # Accounts must be manually mapped

    @property
    def can_get_transactions(self) -> bool:
        return True

    @property
    def can_get_balances(self) -> bool:
        return False

    async def get_accounts(
        self,
        provider_account_ids: List[str] = [],
        provider_settings: Dict[str, Any] | None = None,
    ) -> Result[List[Account]]:
        """CSV provider does not support getting accounts."""
        return Fail("CSV provider does not support getting accounts")

    async def get_transactions(
        self,
        start_date: datetime,
        end_date: datetime,
        provider_account_ids: List[str] = [],
        provider_settings: Dict[str, Any] | None = None,
    ) -> Result[List[Transaction]]:
        """Parse CSV file and return transactions."""
        if not provider_settings:
            return Fail("provider_settings is required")

        file_path = provider_settings.get("file_path")
        if not file_path:
            return Fail("file_path is required in provider_settings")

        column_mapping = provider_settings.get("column_mapping")
        if not column_mapping:
            return Fail("column_mapping is required in provider_settings")

        date_format = provider_settings.get("date_format", "auto")
        flip_signs = provider_settings.get("flip_signs", False)
        debit_negative = provider_settings.get("debit_negative", False)

        # Check if file exists
        path = Path(file_path)
        if not path.exists():
            return Fail(f"File not found: {file_path}")

        try:
            with open(path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                transactions = []

                for row in reader:
                    # Parse transaction from CSV row
                    tx_result = self._parse_transaction_row(
                        row, column_mapping, date_format, debit_negative
                    )
                    if not tx_result.success:
                        # Skip invalid rows but continue processing
                        continue

                    tx = tx_result.data

                    # Apply sign flip if requested
                    if flip_signs:
                        tx = tx.model_copy(update={"amount": -tx.amount})

                    transactions.append(tx)

                return Ok(transactions)

        except Exception as e:
            return Fail(f"Failed to parse CSV file: {str(e)}")

    async def get_balances(
        self,
        provider_account_ids: List[str] = [],
        provider_settings: Dict[str, Any] | None = None,
    ) -> Result[List[BalanceSnapshot]]:
        """CSV provider does not support getting balances."""
        return Fail("CSV provider does not support getting balances")

    def _parse_transaction_row(
        self,
        row: Dict[str, str],
        column_mapping: Dict[str, str],
        date_format: str,
        debit_negative: bool = False,
    ) -> Result[Transaction]:
        """Parse a single CSV row into a Transaction."""
        try:
            # Get column names from mapping
            date_col = column_mapping.get("date")
            description_col = column_mapping.get("description")
            amount_col = column_mapping.get("amount")
            debit_col = column_mapping.get("debit")
            credit_col = column_mapping.get("credit")
            posted_date_col = column_mapping.get("posted_date")

            if not date_col:
                return Fail("date column is required in column_mapping")

            # Must have either amount OR (debit and credit)
            has_amount = bool(amount_col)
            has_debit_credit = bool(debit_col or credit_col)

            if not has_amount and not has_debit_credit:
                return Fail(
                    "amount column (or debit/credit columns) required in column_mapping"
                )

            # Parse date
            date_str = row.get(date_col, "").strip()
            if not date_str:
                return Fail("Missing date value")

            transaction_date = self._parse_date(date_str, date_format)
            if not transaction_date:
                return Fail(f"Failed to parse date: {date_str}")

            # Parse posted_date if provided
            if posted_date_col:
                posted_date_str = row.get(posted_date_col, "").strip()
                if posted_date_str:
                    posted_date = self._parse_date(posted_date_str, date_format)
                    if not posted_date:
                        posted_date = transaction_date
                else:
                    posted_date = transaction_date
            else:
                posted_date = transaction_date

            # Parse amount - handle both single amount and debit/credit columns
            if has_amount:
                amount_str = row.get(amount_col, "").strip()
                if not amount_str:
                    return Fail("Missing amount value")

                amount = self._parse_amount(amount_str)
                if amount is None:
                    return Fail(f"Failed to parse amount: {amount_str}")
            else:
                # Handle debit/credit columns
                # Use whichever column has a value (they're mutually exclusive in most CSVs)
                debit_str = row.get(debit_col, "").strip() if debit_col else ""
                credit_str = row.get(credit_col, "").strip() if credit_col else ""

                # Skip row if both are empty
                if not debit_str and not credit_str:
                    return Fail("Both debit and credit are empty")

                # Parse values
                debit_amt = self._parse_amount(debit_str) if debit_str else None
                credit_amt = self._parse_amount(credit_str) if credit_str else None

                if debit_amt is not None and credit_amt is not None:
                    # Both have values - this is unusual but handle it
                    # Take the non-zero one, or use the larger absolute value
                    if abs(debit_amt) > abs(credit_amt):
                        amount = debit_amt  # Preserve sign from CSV
                    else:
                        amount = credit_amt  # Preserve sign from CSV
                elif debit_amt is not None:
                    # Only debit has value - preserve sign from CSV, then apply debit_negative
                    amount = debit_amt
                    if debit_negative and amount > 0:
                        amount = -amount
                elif credit_amt is not None:
                    # Only credit has value - preserve sign from CSV
                    amount = credit_amt
                else:
                    return Fail(
                        f"Failed to parse debit/credit: {debit_str}/{credit_str}"
                    )

            # Parse description and clean it
            description = ""
            if description_col:
                raw_description = row.get(description_col, "").strip()
                description = self._clean_description(raw_description)

            # Create transaction
            # Note: account_id will be set by ImportService when mapping to target account
            transaction = Transaction(
                id=uuid4(),
                account_id=uuid4(),  # Placeholder, will be replaced by ImportService
                amount=amount,
                description=description,
                transaction_date=transaction_date,
                posted_date=posted_date,
                created_at=datetime.now(timezone.utc),
                updated_at=datetime.now(timezone.utc),
            )

            return Ok(transaction)

        except Exception as e:
            return Fail(f"Failed to parse transaction row: {str(e)}")

    def _parse_date(self, date_str: str, date_format: str) -> date | None:
        """Parse date string and return date object (no timezone)."""
        if not date_str:
            return None

        # Try auto-detection if format is "auto"
        if date_format == "auto":
            formats = [
                "%Y-%m-%d",  # 2024-10-01
                "%m/%d/%Y",  # 10/01/2024
                "%d/%m/%Y",  # 01/10/2024
                "%Y/%m/%d",  # 2024/10/01
                "%m-%d-%Y",  # 10-01-2024
                "%d-%m-%Y",  # 01-10-2024
            ]
        else:
            # Map common format names to strftime formats
            format_map = {
                "YYYY-MM-DD": "%Y-%m-%d",
                "MM/DD/YYYY": "%m/%d/%Y",
                "DD/MM/YYYY": "%d/%m/%Y",
                "YYYY/MM/DD": "%Y/%m/%d",
            }
            fmt = format_map.get(date_format)
            if not fmt:
                return None
            formats = [fmt]

        # Try each format
        for fmt in formats:
            try:
                dt = datetime.strptime(date_str, fmt)
                # Return date object, not datetime (no timezone conversion)
                return dt.date()
            except ValueError:
                continue

        return None

    def _parse_amount(self, amount_str: str) -> Decimal | None:
        """Parse amount string, handling $ signs and commas."""
        if not amount_str:
            return None

        try:
            # Remove currency symbols and commas
            cleaned = amount_str.strip()
            cleaned = cleaned.replace("$", "")
            cleaned = cleaned.replace(",", "")
            cleaned = cleaned.replace(" ", "")

            # Handle parentheses notation for negative numbers: (100.00) -> -100.00
            if cleaned.startswith("(") and cleaned.endswith(")"):
                cleaned = "-" + cleaned[1:-1]

            return Decimal(cleaned)
        except (InvalidOperation, ValueError):
            return None

    def _clean_description(self, description: str) -> str:
        """Remove CSV noise from descriptions before storing.

        Removes:
        - Literal "null" strings
        - Card number masks (XXXXXXXXXXXX1234)
        - Extra whitespace
        """
        if not description:
            return ""

        cleaned = description

        # Remove literal "null" strings (case insensitive)
        cleaned = re.sub(r"\bnull\b", "", cleaned, flags=re.IGNORECASE)

        # Remove card number masks (XXXXXXXXXXXX followed by digits)
        cleaned = re.sub(r"x{10,}\d+", "", cleaned, flags=re.IGNORECASE)

        # Clean up extra whitespace (collapse multiple spaces, trim)
        cleaned = re.sub(r"\s+", " ", cleaned).strip()

        return cleaned

    def detect_columns(self, file_path: str) -> Result[Dict[str, str]]:
        """Auto-detect column mapping from CSV headers.

        Returns best-guess mapping for date, amount, and description columns.
        """
        try:
            with open(file_path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                headers = reader.fieldnames or []

            # Expanded patterns for column detection with fuzzy matching
            date_patterns = [
                "date",
                "transaction date",
                "trans date",
                "txn date",
                "txndate",
                "posted",
                "post date",
                "dt",  # Abbreviated
            ]
            desc_patterns = [
                "description",
                "desc",  # Abbreviated
                "memo",
                "payee",
                "merchant",
                "details",
                "narration",  # International
            ]
            amount_patterns = [
                "amount",
                "amt",  # Abbreviated
                "total",
                "transaction amount",
            ]
            debit_patterns = [
                "debit",
                "dr",  # International abbreviation
                "withdrawal",
                "debit amount",
            ]
            credit_patterns = [
                "credit",
                "cr",  # International abbreviation
                "deposit",
                "credit amount",
            ]

            detected = {}

            # Find date column
            for header in headers:
                header_lower = header.lower().strip()
                if any(pattern in header_lower for pattern in date_patterns):
                    detected["date"] = header
                    break

            # Find amount column (prefer single amount column) - with fuzzy matching
            for header in headers:
                header_lower = header.lower().strip()
                # Remove common suffixes like currency codes for fuzzy matching
                header_clean = re.sub(r"\s+(usd|eur|gbp|cad|aud)$", "", header_lower)
                if any(pattern in header_clean for pattern in amount_patterns):
                    detected["amount"] = header
                    break

            # If no 'amount' found, check for debit/credit
            if "amount" not in detected:
                debit_col = None
                credit_col = None
                for header in headers:
                    header_lower = header.lower().strip()
                    if any(pattern in header_lower for pattern in debit_patterns):
                        debit_col = header
                    if any(pattern in header_lower for pattern in credit_patterns):
                        credit_col = header

                if debit_col or credit_col:
                    detected["debit"] = debit_col
                    detected["credit"] = credit_col

            # Find description column with fallback
            for header in headers:
                header_lower = header.lower().strip()
                # Skip if this is a date column
                if detected.get("date") and header == detected["date"]:
                    continue
                if any(pattern in header_lower for pattern in desc_patterns):
                    detected["description"] = header
                    break

            # If no description found, try fallback columns
            if "description" not in detected:
                fallback_patterns = ["name", "type", "ref", "reference", "category"]
                for header in headers:
                    header_lower = header.lower().strip()
                    # Skip if this is a date column
                    if detected.get("date") and header == detected["date"]:
                        continue
                    if any(pattern in header_lower for pattern in fallback_patterns):
                        detected["description"] = header
                        break

            return Ok(detected)

        except Exception as e:
            return Fail(f"Failed to detect columns: {str(e)}")

    def should_negate_debits(
        self, file_path: str, debit_col: str, credit_col: str
    ) -> Result[bool]:
        """Detect if debit values should be negated (unsigned debit/credit convention).

        Returns True if debits appear to be unsigned positive values that should be negated.
        Returns False if debits are already signed (negative) or mixed.
        """
        try:
            with open(file_path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)

                debit_values = []
                for i, row in enumerate(reader):
                    if i >= 10:  # Sample first 10 rows
                        break

                    debit_str = row.get(debit_col, "").strip()
                    if debit_str:
                        debit_amt = self._parse_amount(debit_str)
                        if debit_amt is not None:
                            debit_values.append(debit_amt)

                # If we have debit values and they're all positive, suggest negating
                if len(debit_values) >= 2:  # Need at least 2 samples
                    all_positive = all(amt > 0 for amt in debit_values)
                    return Ok(all_positive)

                # Not enough data to determine
                return Ok(False)

        except Exception as e:
            return Fail(f"Failed to analyze debit convention: {str(e)}")

    def preview_transactions(
        self,
        file_path: str,
        column_mapping: Dict[str, str],
        date_format: str = "auto",
        limit: int = 5,
        flip_signs: bool = False,
        debit_negative: bool = False,
    ) -> Result[List[Transaction]]:
        """Preview first N transactions from CSV with given mapping.

        This is used to show the user what will be imported before committing.
        """
        try:
            with open(file_path, "r", encoding="utf-8") as f:
                reader = csv.DictReader(f)
                transactions = []

                for i, row in enumerate(reader):
                    if i >= limit:
                        break

                    # Parse transaction
                    tx_result = self._parse_transaction_row(
                        row, column_mapping, date_format, debit_negative
                    )

                    if tx_result.success and tx_result.data:
                        tx = tx_result.data

                        # Apply sign flip if requested
                        if flip_signs:
                            tx = tx.model_copy(update={"amount": -tx.amount})

                        transactions.append(tx)

                return Ok(transactions)

        except Exception as e:
            return Fail(f"Failed to preview transactions: {str(e)}")
