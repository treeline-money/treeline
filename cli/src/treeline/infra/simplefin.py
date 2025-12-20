"""SimpleFIN infrastructure implementation."""

import base64
from datetime import datetime, timezone
from decimal import Decimal
from types import MappingProxyType
from typing import Any, Dict, List
from urllib.parse import urlparse
from uuid import UUID, uuid4

import httpx

from treeline.abstractions import DataAggregationProvider, IntegrationProvider
from treeline.domain import Account, BalanceSnapshot, Fail, Ok, Result, Transaction
from treeline.utils import get_logger


class SimpleFINProvider(DataAggregationProvider, IntegrationProvider):
    """SimpleFIN implementation for data aggregation."""

    @property
    def can_get_accounts(self) -> bool:
        return True

    @property
    def can_get_transactions(self) -> bool:
        return True

    @property
    def can_get_balances(self) -> bool:
        return True

    async def get_accounts(
        self,
        provider_account_ids: List[str] = [],
        provider_settings: Dict[str, Any] = {},
    ) -> Result[List[Account]]:
        """Get accounts from SimpleFIN."""
        access_url = provider_settings.get("accessUrl")
        if not access_url:
            return Fail("accessUrl is required for SimpleFIN")

        # Parse and validate access URL
        parse_result = self._parse_access_url(access_url)
        if not parse_result.success:
            return parse_result

        url_parts = parse_result.data

        try:
            async with httpx.AsyncClient() as client:
                response = await client.get(
                    f"{url_parts['clean_url']}/accounts",
                    auth=(url_parts["username"], url_parts["password"]),
                    timeout=30.0,
                )

                # Handle specific HTTP error codes with actionable messages
                if response.status_code == 403:
                    return Fail(
                        "SimpleFIN authentication failed. Your access token may be invalid or revoked. "
                        "Please reset your SimpleFIN credentials at https://beta-bridge.simplefin.org/"
                    )
                if response.status_code == 402:
                    return Fail(
                        "SimpleFIN subscription payment required. "
                        "Please check your SimpleFIN account at https://beta-bridge.simplefin.org/"
                    )
                if response.status_code != 200:
                    return Fail(f"SimpleFIN API error: HTTP {response.status_code}")

                data = response.json()

                # Check for API-level errors (e.g., "You must reauthenticate")
                # These are warnings/errors from SimpleFIN about individual connections
                api_errors = data.get("errors", [])
                if api_errors:
                    logger = get_logger("infra.simplefin")
                    logger.warning(f"SimpleFIN returned errors: {api_errors}")

                accounts = []

                for acc_data in data.get("accounts", []):
                    # Filter by account IDs if specified
                    if (
                        provider_account_ids
                        and acc_data["id"] not in provider_account_ids
                    ):
                        continue

                    # Extract balance if present
                    balance = None
                    if "balance" in acc_data and acc_data["balance"] is not None:
                        balance = Decimal(str(acc_data["balance"]))

                    account = Account(
                        id=uuid4(),
                        name=acc_data["name"],
                        currency=acc_data.get("currency", "USD"),
                        external_ids=MappingProxyType({"simplefin": acc_data["id"]}),
                        balance=balance,
                        institution_name=acc_data.get("org", {}).get("name"),
                        institution_url=acc_data.get("org", {}).get("url"),
                        institution_domain=acc_data.get("org", {}).get("domain"),
                        created_at=datetime.now(timezone.utc),
                        updated_at=datetime.now(timezone.utc),
                    )
                    accounts.append(account)

                # Return accounts along with any API errors/warnings
                return Ok({"accounts": accounts, "errors": api_errors})

        except httpx.TimeoutException as e:
            logger = get_logger("infra.simplefin")
            logger.error(f"Timeout fetching SimpleFIN accounts: {e}", exc_info=True)
            return Fail(
                "Failed to fetch SimpleFIN accounts: Connection timed out after 30 seconds"
            )
        except httpx.ConnectError as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Connection error fetching SimpleFIN accounts: {e}", exc_info=True
            )
            return Fail(
                "Failed to fetch SimpleFIN accounts: Unable to connect to SimpleFIN servers"
            )
        except Exception as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Unexpected error fetching SimpleFIN accounts: {e}", exc_info=True
            )
            return Fail(
                f"Failed to fetch SimpleFIN accounts: {type(e).__name__}: {str(e)}"
            )

    async def get_transactions(
        self,
        start_date: datetime,
        end_date: datetime,
        provider_account_ids: List[str] = [],
        provider_settings: Dict[str, Any] = {},
    ) -> Result[List[Transaction]]:
        """Get transactions from SimpleFIN."""
        access_url = provider_settings.get("accessUrl")
        if not access_url:
            return Fail("accessUrl is required for SimpleFIN")

        parse_result = self._parse_access_url(access_url)
        if not parse_result.success:
            return parse_result

        url_parts = parse_result.data

        try:
            # Build query parameters
            # Note: SimpleFIN uses 'account' (singular) specified multiple times, not comma-separated
            params: list[tuple[str, str]] = []
            if start_date:
                params.append(("start-date", str(int(start_date.timestamp()))))
            if end_date:
                params.append(("end-date", str(int(end_date.timestamp()))))
            if provider_account_ids:
                for acc_id in provider_account_ids:
                    params.append(("account", acc_id))

            async with httpx.AsyncClient() as client:
                response = await client.get(
                    f"{url_parts['clean_url']}/accounts",
                    auth=(url_parts["username"], url_parts["password"]),
                    params=params,
                    timeout=30.0,
                )

                # Handle specific HTTP error codes with actionable messages
                if response.status_code == 403:
                    return Fail(
                        "SimpleFIN authentication failed. Your access token may be invalid or revoked. "
                        "Please reset your SimpleFIN credentials at https://beta-bridge.simplefin.org/"
                    )
                if response.status_code == 402:
                    return Fail(
                        "SimpleFIN subscription payment required. "
                        "Please check your SimpleFIN account at https://beta-bridge.simplefin.org/"
                    )
                if response.status_code != 200:
                    return Fail(f"SimpleFIN API error: HTTP {response.status_code}")

                data = response.json()

                # Check for API-level errors (e.g., "You must reauthenticate")
                # These are warnings/errors from SimpleFIN about individual connections
                api_errors = data.get("errors", [])
                if api_errors:
                    logger = get_logger("infra.simplefin")
                    logger.warning(f"SimpleFIN returned errors: {api_errors}")

                # Return list of tuples: (simplefin_account_id, transaction)
                # This allows service layer to map accounts without polluting external_ids
                transactions_with_accounts = []

                for acc_data in data.get("accounts", []):
                    simplefin_account_id = acc_data["id"]
                    for tx_data in acc_data.get("transactions", []):
                        transaction = Transaction(
                            id=uuid4(),
                            account_id=UUID(
                                int=0
                            ),  # Placeholder, will be mapped by service
                            external_ids=MappingProxyType(
                                {
                                    "simplefin": tx_data["id"],
                                }
                            ),
                            amount=Decimal(str(tx_data["amount"])),
                            description=tx_data.get("description", ""),
                            transaction_date=datetime.fromtimestamp(
                                tx_data["posted"], tz=timezone.utc
                            ),
                            posted_date=datetime.fromtimestamp(
                                tx_data["posted"], tz=timezone.utc
                            ),
                            tags=tuple([tx_data["extra"]["category"]])
                            if tx_data.get("extra", {}).get("category")
                            else tuple(),
                            created_at=datetime.now(timezone.utc),
                            updated_at=datetime.now(timezone.utc),
                        )
                        transactions_with_accounts.append(
                            (simplefin_account_id, transaction)
                        )

                # Return transactions along with any API errors/warnings
                return Ok(
                    {"transactions": transactions_with_accounts, "errors": api_errors}
                )

        except httpx.TimeoutException as e:
            logger = get_logger("infra.simplefin")
            logger.error(f"Timeout fetching SimpleFIN transactions: {e}", exc_info=True)
            return Fail(
                "Failed to fetch SimpleFIN transactions: Connection timed out after 30 seconds"
            )
        except httpx.ConnectError as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Connection error fetching SimpleFIN transactions: {e}", exc_info=True
            )
            return Fail(
                "Failed to fetch SimpleFIN transactions: Unable to connect to SimpleFIN servers"
            )
        except Exception as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Unexpected error fetching SimpleFIN transactions: {e}", exc_info=True
            )
            return Fail(
                f"Failed to fetch SimpleFIN transactions: {type(e).__name__}: {str(e)}"
            )

    async def get_balances(
        self,
        provider_account_ids: List[str] = [],
        provider_settings: Dict[str, Any] = {},
    ) -> Result[List[BalanceSnapshot]]:
        """Get balance snapshots from SimpleFIN.

        NOTE: This method is deprecated. Balances are now returned as part of the
        Account model in get_accounts() and balance snapshots are created automatically
        by the sync service.
        """
        return Fail("get_balances is deprecated - balances are synced via get_accounts")

    async def create_integration(
        self, integration_name: str, integration_options: Dict[str, Any]
    ) -> Result[Dict[str, str]]:
        """Set up SimpleFIN integration by exchanging setup token for access URL."""
        setup_token = integration_options.get("setupToken")
        if not setup_token:
            return Fail("setupToken is required for SimpleFIN integration")

        try:
            # Decode Base64 setup token to get claim URL
            try:
                claim_url = base64.b64decode(setup_token).decode("utf-8")
            except Exception:
                return Fail("Invalid setup token format")

            # Exchange setup token for access URL
            async with httpx.AsyncClient() as client:
                response = await client.post(claim_url, timeout=30.0)

                if response.status_code != 200:
                    return Fail("Failed to verify SimpleFIN token")

                access_url = response.text

                if not access_url:
                    return Fail("No access URL received from SimpleFIN")

                return Ok({"accessUrl": access_url})

        except httpx.TimeoutException as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Timeout during SimpleFIN integration setup: {e}", exc_info=True
            )
            return Fail("Integration setup failed: Connection timed out")
        except httpx.ConnectError as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Connection error during SimpleFIN integration setup: {e}",
                exc_info=True,
            )
            return Fail(
                "Integration setup failed: Unable to connect to SimpleFIN servers"
            )
        except Exception as e:
            logger = get_logger("infra.simplefin")
            logger.error(
                f"Unexpected error during SimpleFIN integration setup: {e}",
                exc_info=True,
            )
            return Fail(f"Integration setup failed: {type(e).__name__}: {str(e)}")

    def _parse_access_url(self, access_url: str) -> Result[Dict[str, str]]:
        """Parse and validate SimpleFIN access URL."""
        if not access_url:
            return Fail("accessUrl is required")

        try:
            parsed = urlparse(access_url)
        except Exception:
            return Fail("Invalid URL format")

        # Validate HTTPS
        if parsed.scheme != "https":
            return Fail("accessUrl must use HTTPS")

        # Validate domain
        if not parsed.hostname or not parsed.hostname.endswith("simplefin.org"):
            return Fail("accessUrl must be from simplefin.org domain")

        # Validate credentials
        if not parsed.username or not parsed.password:
            return Fail("accessUrl must contain username and password")

        clean_url = f"{parsed.scheme}://{parsed.hostname}{parsed.path}"

        return Ok(
            {
                "clean_url": clean_url,
                "username": parsed.username,
                "password": parsed.password,
            }
        )
