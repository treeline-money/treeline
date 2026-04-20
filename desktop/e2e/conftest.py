import json
import os
import shutil
import signal
import subprocess
import tempfile
import time
from pathlib import Path

import pytest
from selenium import webdriver
from selenium.webdriver.remote.webdriver import WebDriver

WEBDRIVER_PORT = int(os.environ.get("TAURI_WEBDRIVER_PORT", "4445"))
WEBDRIVER_URL = f"http://127.0.0.1:{WEBDRIVER_PORT}"
DESKTOP_DIR = Path(__file__).resolve().parent.parent
E2E_DIR = Path(__file__).resolve().parent
SEEDS_DIR = E2E_DIR / "seeds"
SCREENSHOT_DIR = E2E_DIR / "screenshots"

PLUGIN_URLS = {
    "budget": "https://github.com/treeline-money/plugin-budget",
    "goals": "https://github.com/treeline-money/plugin-goals",
    "subscriptions": "https://github.com/treeline-money/plugin-subscriptions",
    "cashflow": "https://github.com/treeline-money/plugin-cashflow",
    "emergency-fund": "https://github.com/treeline-money/plugin-emergency-fund",
}

# Find the tl CLI binary
TL_BINARY = os.environ.get(
    "TL_BINARY",
    str(DESKTOP_DIR.parent / "target" / "debug" / "tl"),
)


def wait_for_webdriver(timeout=60):
    """Poll until the WebDriver server is ready."""
    import urllib.request
    import urllib.error

    start = time.time()
    while time.time() - start < timeout:
        try:
            urllib.request.urlopen(f"{WEBDRIVER_URL}/status", timeout=2)
            return True
        except (urllib.error.URLError, ConnectionError, OSError):
            time.sleep(1)
    raise TimeoutError(f"WebDriver not ready after {timeout}s")


def run_tl(treeline_dir: str, *args):
    """Run a tl CLI command against the given treeline directory."""
    env = {**os.environ, "TREELINE_DIR": treeline_dir}
    result = subprocess.run(
        [TL_BINARY, *args],
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"tl {' '.join(args)} failed:\n{result.stderr}\n{result.stdout}"
        )
    return result.stdout


def seed_from_config(treeline_dir: str, seed: dict):
    """Seed a treeline directory based on a SEED config dict.

    Keys:
        accounts: list of {"file": "x.csv", "name": "...", "type": "..."}
        sql: list of SQL filenames in seeds/ directory
        plugins: list of plugin short names (e.g. "budget", "goals")
    """
    print(f"Seeding data in {treeline_dir}...")

    # Initialize the database
    run_tl(treeline_dir, "status")
    print("  Initialized database")

    # Skip onboarding and what's-new modals
    settings_path = Path(treeline_dir) / "settings.json"
    settings = {"app": {"hasCompletedOnboarding": True, "lastSeenVersion": "0.0.0-dev"}}
    settings_path.write_text(json.dumps(settings))

    # 1. Import CSV accounts
    for acct in seed.get("accounts", []):
        args = [
            "import", str(SEEDS_DIR / acct["file"]),
            "--account", acct["name"],
            "--create-if-not-exists",
        ]
        if "type" in acct:
            args += ["--account-type", acct["type"]]
        if "currency" in acct:
            args += ["--currency", acct["currency"]]
        run_tl(treeline_dir, *args)
        print(f"  Imported {acct['file']} → {acct['name']}")

    # 2. Run SQL seeds
    for sql_file in seed.get("sql", []):
        sql_path = SEEDS_DIR / sql_file
        sql = sql_path.read_text()
        run_tl(treeline_dir, "query", "--allow-writes", sql)
        print(f"  Ran {sql_file}")

    # 3. Install plugins
    for plugin_name in seed.get("plugins", []):
        url = PLUGIN_URLS.get(plugin_name)
        if not url:
            raise ValueError(f"Unknown plugin: {plugin_name}")
        try:
            run_tl(treeline_dir, "plugin", "install", url)
            print(f"  Installed {plugin_name}")
        except RuntimeError as e:
            print(f"  Warning: Failed to install {plugin_name}: {e}")


def get_seed_config(session):
    """Collect SEED config from test modules in this session."""
    for item in session.items:
        module = item.module
        if hasattr(module, "SEED"):
            return module.SEED
    return {}


@pytest.fixture(scope="session")
def treeline_dir(request):
    """Create and seed a temporary treeline directory."""
    seed = get_seed_config(request.session)
    tmpdir = tempfile.mkdtemp(prefix="treeline-e2e-")
    if seed:
        seed_from_config(tmpdir, seed)
    else:
        # Minimal init even without seed
        run_tl(tmpdir, "status")
        settings_path = Path(tmpdir) / "settings.json"
        settings = {"app": {"hasCompletedOnboarding": True, "lastSeenVersion": "0.0.0-dev"}}
        settings_path.write_text(json.dumps(settings))
    yield tmpdir
    shutil.rmtree(tmpdir, ignore_errors=True)


@pytest.fixture(scope="session")
def app_process(treeline_dir):
    """Launch the Tauri app with the seeded test data."""
    binary = os.environ.get("TAURI_E2E_BINARY")
    use_dev = os.environ.get("TAURI_E2E_DEV") == "1"

    env = {**os.environ, "TREELINE_DIR": treeline_dir}

    if not binary and not use_dev:
        # Check if WebDriver is already running (app launched externally)
        try:
            wait_for_webdriver(timeout=3)
            yield None  # Already running
            return
        except TimeoutError:
            pass
        pytest.exit(
            "No app running. Set TAURI_E2E_BINARY or TAURI_E2E_DEV=1, "
            "or launch the app with e2e-testing feature first.",
            returncode=1,
        )

    if binary:
        proc = subprocess.Popen([binary], env=env, start_new_session=True)
    else:
        proc = subprocess.Popen(
            ["npx", "tauri", "dev", "--features", "e2e-testing"],
            cwd=DESKTOP_DIR,
            env=env,
            start_new_session=True,
        )

    wait_for_webdriver()
    yield proc
    # Give the app a moment to finish gracefully
    time.sleep(5)
    # Kill the entire process group (tauri dev spawns child processes)
    os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)


@pytest.fixture(scope="session")
def driver(app_process) -> WebDriver:
    """Create a Selenium WebDriver connected to the Tauri app."""
    options = webdriver.ChromeOptions()
    drv = webdriver.Remote(command_executor=WEBDRIVER_URL, options=options)
    drv.implicitly_wait(5)
    yield drv
    drv.quit()


@pytest.fixture(autouse=True)
def _screenshot_on_failure(request, driver):
    """Automatically save a screenshot when a test fails."""
    yield
    if hasattr(request.node, "rep_call") and request.node.rep_call.failed:
        SCREENSHOT_DIR.mkdir(exist_ok=True)
        name = request.node.name.replace("[", "_").replace("]", "")
        driver.save_screenshot(str(SCREENSHOT_DIR / f"FAILED_{name}.png"))


@pytest.hookimpl(tryfirst=True, hookwrapper=True)
def pytest_runtest_makereport(item, call):
    """Attach test result to the request node for screenshot-on-failure."""
    outcome = yield
    rep = outcome.get_result()
    setattr(item, f"rep_{rep.when}", rep)
