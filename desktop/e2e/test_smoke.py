"""Smoke test: app loads, all pages render with seeded data."""

from selenium.webdriver.common.by import By
from selenium.webdriver.support import expected_conditions as EC
from selenium.webdriver.support.ui import WebDriverWait

from helpers import dismiss_modals, navigate_to, screenshot

SEED = {
    "accounts": [
        {"file": "checking.csv", "name": "Test Checking", "type": "depository"},
        {"file": "credit_card.csv", "name": "Test Credit Card", "type": "credit"},
    ],
    "plugins": ["budget", "goals", "subscriptions", "cashflow", "emergency-fund"],
}


def test_app_loads(driver):
    # Wait for app to fully load (plugins may take time to initialize)
    wait = WebDriverWait(driver, 30)
    dismiss_modals(driver)
    sidebar = wait.until(
        EC.presence_of_element_located((By.CSS_SELECTOR, "aside.sidebar"))
    )
    assert sidebar.is_displayed()
    screenshot(driver, "01-app-loaded", delay_seconds=0.5)


# -- Core pages --


def test_transactions_page(driver):
    navigate_to(driver, "transactions")
    # Verify transactions are rendered (seeded data should show rows)
    wait = WebDriverWait(driver, 10)
    wait.until(
        EC.presence_of_element_located((By.CSS_SELECTOR, ".tab-content.active"))
    )
    screenshot(driver, "02-transactions", delay_seconds=0.5)


def test_accounts_page(driver):
    navigate_to(driver, "accounts")
    wait = WebDriverWait(driver, 10)
    wait.until(
        EC.presence_of_element_located((By.CSS_SELECTOR, ".tab-content.active"))
    )
    screenshot(driver, "03-accounts", delay_seconds=0.5)


def test_query_page(driver):
    navigate_to(driver, "query")
    wait = WebDriverWait(driver, 10)
    wait.until(
        EC.presence_of_element_located((By.CSS_SELECTOR, ".tab-content.active"))
    )
    screenshot(driver, "04-query", delay_seconds=0.5)


# -- Plugins --


def test_budget_plugin(driver):
    navigate_to(driver, "budget")
    screenshot(driver, "05-budget", delay_seconds=0.5)


def test_goals_plugin(driver):
    navigate_to(driver, "goals")
    screenshot(driver, "06-goals", delay_seconds=0.5)


def test_subscriptions_plugin(driver):
    navigate_to(driver, "subscriptions")
    screenshot(driver, "07-subscriptions", delay_seconds=0.5)


def test_cashflow_plugin(driver):
    navigate_to(driver, "cashflow-view")
    screenshot(driver, "08-cashflow", delay_seconds=0.5)


def test_emergency_fund_plugin(driver):
    navigate_to(driver, "emergency-fund")
    screenshot(driver, "09-emergency-fund", delay_seconds=0.5)
