"""Reusable helpers for Treeline e2e tests."""

from pathlib import Path

from selenium.webdriver.common.by import By
from selenium.webdriver.remote.webdriver import WebDriver
from selenium.webdriver.support import expected_conditions as EC
from selenium.webdriver.support.ui import WebDriverWait

import time

SCREENSHOT_DIR = Path(__file__).resolve().parent / "screenshots"


def screenshot(driver: WebDriver, name: str, delay_seconds: float = 0):
    """Save a screenshot with the given name."""
    SCREENSHOT_DIR.mkdir(exist_ok=True)
    filepath = SCREENSHOT_DIR / f"{name}.png"
    
    time.sleep(delay_seconds)
    driver.save_screenshot(str(filepath))
    print(f"  Screenshot: {filepath}")


def dismiss_modals(driver: WebDriver, timeout=3):
    """Dismiss any modal overlay (e.g. What's New)."""
    try:
        wait = WebDriverWait(driver, timeout)
        modal = wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".modal-overlay")))
        btn = modal.find_element(By.CSS_SELECTOR, ".modal-actions button")
        btn.click()
        WebDriverWait(driver, 3).until(EC.invisibility_of_element(modal))
    except Exception:
        pass  # No modal


def navigate_to(driver: WebDriver, view_id: str, timeout=10):
    """Click a sidebar item and wait for its tab to appear."""
    btn = driver.find_element(By.CSS_SELECTOR, f'[data-testid="sidebar-{view_id}"]')
    btn.click()
    wait = WebDriverWait(driver, timeout)
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, f'[data-testid="tab-{view_id}"]')))


def testid(driver: WebDriver, test_id: str):
    """Find an element by data-testid."""
    return driver.find_element(By.CSS_SELECTOR, f'[data-testid="{test_id}"]')
