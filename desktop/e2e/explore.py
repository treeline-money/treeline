"""
Interactive harness for driving the desktop app via WebDriver.

Intended for UI polish work: launch the app once, then iterate on CSS/Svelte
changes while poking the running instance for screenshots and DOM state.

Usage:
    # Start once (backgrounds the app, writes PID file)
    uv run python explore.py launch

    # Take a screenshot (saves PNG you can then read with the Read tool)
    uv run python explore.py screenshot /tmp/ui.png

    # Click an element
    uv run python explore.py click '[data-testid="sidebar-budget"]'

    # Run arbitrary JS (return values are printed)
    uv run python explore.py eval 'return document.title'

    # Dump outerHTML of an element (or document.body if no selector)
    uv run python explore.py html 'aside.sidebar'

    # Stop
    uv run python explore.py kill

Sandbox TREELINE_DIR defaults to ~/.treeline-e2e-sandbox — seeded once with
CSVs + community plugins (same SEED as test_smoke.py).
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

from selenium import webdriver
from selenium.webdriver.common.by import By

from conftest import (
    DESKTOP_DIR,
    WEBDRIVER_PORT,
    WEBDRIVER_URL,
    seed_from_config,
    wait_for_webdriver,
)

SANDBOX_DIR = Path(os.environ.get("TREELINE_EXPLORE_DIR", str(Path.home() / ".treeline-e2e-sandbox")))
PID_FILE = SANDBOX_DIR / ".explore.pid"
LOG_FILE = SANDBOX_DIR / ".explore.log"
SEEDED_MARKER = SANDBOX_DIR / ".seeded"

DEFAULT_SEED = {
    "accounts": [
        {"file": "checking.csv", "name": "Test Checking", "type": "depository"},
        {"file": "credit_card.csv", "name": "Test Credit Card", "type": "credit"},
    ],
    "plugins": ["budget", "goals", "subscriptions", "cashflow", "emergency-fund"],
}


def webdriver_is_up() -> bool:
    import urllib.error
    import urllib.request

    try:
        urllib.request.urlopen(f"{WEBDRIVER_URL}/status", timeout=1)
        return True
    except (urllib.error.URLError, ConnectionError, OSError):
        return False


def ensure_sandbox() -> None:
    SANDBOX_DIR.mkdir(parents=True, exist_ok=True)
    if SEEDED_MARKER.exists():
        return
    print(f"Seeding sandbox at {SANDBOX_DIR} (one-time)...")
    seed_from_config(str(SANDBOX_DIR), DEFAULT_SEED)
    SEEDED_MARKER.touch()


def cmd_launch(args) -> None:
    if webdriver_is_up():
        print(f"App is already running (WebDriver responding on {WEBDRIVER_URL})")
        print("Run `explore.py kill` first if you want to restart.")
        return

    ensure_sandbox()

    env = {
        **os.environ,
        "TREELINE_DIR": str(SANDBOX_DIR),
        "TREELINE_DISABLE_UPDATE_CHECKS": "1",
    }

    log = open(LOG_FILE, "w")
    proc = subprocess.Popen(
        ["npx", "tauri", "dev", "--features", "e2e-testing"],
        cwd=DESKTOP_DIR,
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    PID_FILE.write_text(str(proc.pid))

    print(f"Launching (PID {proc.pid}, logs at {LOG_FILE})...")
    try:
        wait_for_webdriver(timeout=300)
        print(f"Ready — WebDriver listening on {WEBDRIVER_URL}")
        print(f"Sandbox: {SANDBOX_DIR}")
    except TimeoutError:
        print("WebDriver did not come up in time — check logs", file=sys.stderr)
        sys.exit(1)


def cmd_kill(args) -> None:
    if not PID_FILE.exists():
        print("No PID file — nothing to kill.")
        return
    pid = int(PID_FILE.read_text().strip())
    try:
        os.killpg(os.getpgid(pid), signal.SIGTERM)
        time.sleep(2)
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
        except ProcessLookupError:
            pass
    except ProcessLookupError:
        print("Process already gone.")
    PID_FILE.unlink(missing_ok=True)
    print("Killed.")


def cmd_status(args) -> None:
    up = webdriver_is_up()
    pid_exists = PID_FILE.exists()
    print(f"WebDriver: {'UP' if up else 'down'} ({WEBDRIVER_URL})")
    if pid_exists:
        print(f"PID file:  {PID_FILE.read_text().strip()}")
    print(f"Sandbox:   {SANDBOX_DIR}")
    print(f"Logs:      {LOG_FILE}")


def with_driver():
    if not webdriver_is_up():
        print("App isn't running. Launch it first: explore.py launch", file=sys.stderr)
        sys.exit(1)
    options = webdriver.ChromeOptions()
    drv = webdriver.Remote(command_executor=WEBDRIVER_URL, options=options)
    drv.implicitly_wait(3)
    return drv


def cmd_screenshot(args) -> None:
    drv = with_driver()
    try:
        out = Path(args.path).expanduser().resolve()
        out.parent.mkdir(parents=True, exist_ok=True)
        drv.save_screenshot(str(out))
        print(out)
    finally:
        drv.quit()


def cmd_click(args) -> None:
    drv = with_driver()
    try:
        elem = drv.find_element(By.CSS_SELECTOR, args.selector)
        elem.click()
        print(f"Clicked {args.selector}")
    finally:
        drv.quit()


def cmd_eval(args) -> None:
    drv = with_driver()
    try:
        result = drv.execute_script(args.js)
        if result is None:
            print("(undefined / no return value)")
        else:
            try:
                print(json.dumps(result, indent=2, default=str))
            except TypeError:
                print(repr(result))
    finally:
        drv.quit()


def cmd_html(args) -> None:
    drv = with_driver()
    try:
        if args.selector:
            elem = drv.find_element(By.CSS_SELECTOR, args.selector)
            print(elem.get_attribute("outerHTML"))
        else:
            print(drv.find_element(By.TAG_NAME, "body").get_attribute("outerHTML"))
    finally:
        drv.quit()


def main() -> None:
    parser = argparse.ArgumentParser(description="Drive the Treeline desktop app via WebDriver.")
    sub = parser.add_subparsers(dest="cmd", required=True)

    sub.add_parser("launch", help="Start the app in the background (reuses sandbox)")
    sub.add_parser("kill", help="Stop the running app")
    sub.add_parser("status", help="Show app + sandbox status")

    p_screenshot = sub.add_parser("screenshot", help="Save a PNG screenshot")
    p_screenshot.add_argument("path", help="Output PNG path")

    p_click = sub.add_parser("click", help="Click an element by CSS selector")
    p_click.add_argument("selector")

    p_eval = sub.add_parser("eval", help="Run JS in the webview (use `return` to get a value)")
    p_eval.add_argument("js")

    p_html = sub.add_parser("html", help="Dump outerHTML of an element (or body)")
    p_html.add_argument("selector", nargs="?", default=None)

    args = parser.parse_args()

    dispatch = {
        "launch": cmd_launch,
        "kill": cmd_kill,
        "status": cmd_status,
        "screenshot": cmd_screenshot,
        "click": cmd_click,
        "eval": cmd_eval,
        "html": cmd_html,
    }
    dispatch[args.cmd](args)


if __name__ == "__main__":
    main()
