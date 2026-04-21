"""Transaction notes round-trip: type, save, indicator, popup, dismiss."""

from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.support import expected_conditions as EC
from selenium.webdriver.support.ui import WebDriverWait

from helpers import dismiss_modals, navigate_to, screenshot

# Match test_smoke.py's SEED — the session-scoped treeline_dir fixture picks the
# first module's SEED it finds, so mismatched SEEDs across test modules would
# cause whichever is collected first to silently deprive the other.
SEED = {
    "accounts": [
        {"file": "checking.csv", "name": "Test Checking", "type": "depository"},
        {"file": "credit_card.csv", "name": "Test Credit Card", "type": "credit"},
    ],
    "plugins": ["budget", "goals", "subscriptions", "cashflow", "emergency-fund"],
}

NOTE_TEXT = "Tried a new burger spot\nOrdered the special"


def test_notes_round_trip(driver):
    # CI runners (especially headed WebKit on Linux) can be noticeably slower
    # than local dev. This test is collected before test_smoke alphabetically,
    # so we can't rely on test_smoke's warmup — wait for the sidebar ourselves.
    WebDriverWait(driver, 30).until(
        EC.presence_of_element_located((By.CSS_SELECTOR, "aside.sidebar"))
    )
    dismiss_modals(driver)
    navigate_to(driver, "transactions")

    wait = WebDriverWait(driver, 20)
    row = wait.until(
        EC.element_to_be_clickable((By.CSS_SELECTOR, '[role="option"]'))
    )

    # Open the edit modal via double-click (matches the in-app interaction).
    driver.execute_script(
        "arguments[0].dispatchEvent(new MouseEvent('dblclick', {bubbles: true}))", row
    )

    notes_field = wait.until(
        EC.visibility_of_element_located((By.CSS_SELECTOR, "#modal-notes"))
    )
    notes_field.clear()
    notes_field.send_keys(NOTE_TEXT)

    save_btn = wait.until(
        EC.element_to_be_clickable((By.CSS_SELECTOR, ".modal-actions .btn.primary"))
    )
    save_btn.click()

    # Modal closes, row now shows the notes indicator.
    wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, "#modal-notes")))
    indicator = wait.until(
        EC.element_to_be_clickable((By.CSS_SELECTOR, ".notes-indicator"))
    )
    screenshot(driver, "notes-01-indicator-on-row", delay_seconds=0.3)

    # Click opens the popup with the full multi-line note.
    indicator.click()
    popup = wait.until(
        EC.visibility_of_element_located((By.CSS_SELECTOR, ".notes-popup"))
    )
    assert NOTE_TEXT in popup.text, f"popup missing note text: {popup.text!r}"
    screenshot(driver, "notes-02-popup-open", delay_seconds=0.3)

    # Escape dismisses the popup.
    ActionChains(driver).send_keys(Keys.ESCAPE).perform()
    wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, ".notes-popup")))

    # Reopening the modal shows the persisted notes.
    driver.execute_script(
        "arguments[0].dispatchEvent(new MouseEvent('dblclick', {bubbles: true}))", row
    )
    notes_field = wait.until(
        EC.visibility_of_element_located((By.CSS_SELECTOR, "#modal-notes"))
    )
    assert notes_field.get_attribute("value") == NOTE_TEXT, (
        f"notes did not round-trip: {notes_field.get_attribute('value')!r}"
    )
