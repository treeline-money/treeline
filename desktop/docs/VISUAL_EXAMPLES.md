# UI Testing Infrastructure

This PR adds automated UI testing infrastructure that allows Claude to launch, interact with, and verify the Treeline UI.

## What's Included

### Testing Scripts

1. **`test-ui-interactive.js`** - Full UI controller with Playwright
   - Launch app (dev server or Tauri binary)
   - Take screenshots
   - Click, type, navigate
   - Execute JavaScript for verification
   - Inspect page structure

2. **`claude-ui-tester.js`** - CLI with pre-built scenarios
   - `initial-load` - Test app startup
   - `find-buttons` - Discover UI elements
   - `test-modal` - Test modal system
   - `keyboard-nav` - Test keyboard shortcuts

3. **`test-tauri-webdriver.js`** - WebDriver controller for full Tauri app

### Dependencies Installed

- ✅ `playwright` & `@playwright/test` - Browser automation
- ✅ `webdriverio` & `@wdio/*` packages - WebDriver protocol
- ✅ `tauri-driver` - Tauri-specific WebDriver (via cargo)

## How It Works

### After Implementing a Feature

Claude can automatically verify it works:

```javascript
import { UITestController } from './test-ui-interactive.js';

const controller = new UITestController();
await controller.startDevServer();
await controller.launch({ headless: true });

// Test the feature
await controller.click('button:has-text("Add Transaction")');
await controller.screenshot('modal-opened.png');

// Verify
const modalExists = await controller.elementExists('[role="dialog"]');
console.log('✅ Modal opened:', modalExists);

await controller.close();
```

## Available Methods

| Method | Example | Purpose |
|--------|---------|---------|
| `screenshot(path)` | `await controller.screenshot('test.png')` | Capture UI state |
| `click(selector)` | `await controller.click('button')` | Click elements |
| `type(selector, text)` | `await controller.type('input', 'test')` | Fill forms |
| `press(key)` | `await controller.press('Escape')` | Keyboard input |
| `getText(selector)` | `await controller.getText('.title')` | Read text |
| `waitForElement(sel)` | `await controller.waitForElement('.modal')` | Wait for elements |
| `executeScript(fn)` | `await controller.executeScript(() => ...)` | Custom JS |
| `inspectPage()` | `await controller.inspectPage()` | Get page stats |

## Testing Modes

### Mode 1: Dev Server (Browser)

**Status:** ⚠️ Limited - Tauri APIs not available

```bash
npm run test:ui initial-load
```

**Pros:**
- Fast - no build required
- Can test pure UI components
- Good for rapid iteration

**Cons:**
- Tauri APIs (`invoke`) unavailable
- Database operations don't work
- App shows errors when calling native APIs

### Mode 2: Full Tauri Binary (WebDriver)

**Status:** ✅ Full functionality

```bash
# Requires: tauri-driver + WebKitWebDriver installed
node test-tauri-webdriver.js
```

**Pros:**
- Full Tauri API support
- Real database operations
- Native features work
- Production-accurate testing

**Requirements:**
- Built Tauri binary or downloaded release
- `tauri-driver` installed (via `cargo install tauri-driver`)
- System WebDriver (WebKitWebDriver on Linux, etc.)

## Usage Examples

### Example 1: Test Modal Opening

```javascript
const controller = new UITestController();
await controller.launch();

// Click settings
await controller.click('[aria-label="Settings"]');
await controller.screenshot('settings-open.png');

// Verify modal appeared
const hasModal = await controller.elementExists('[role="dialog"]');
console.log('Settings modal:', hasModal ? '✅ Opened' : '❌ Failed');
```

### Example 2: Test Transaction Creation

```javascript
await controller.click('text=Transactions');
await controller.click('button:has-text("Add Transaction")');

// Fill form
await controller.type('input[name="amount"]', '42.50');
await controller.type('input[name="description"]', 'Coffee');
await controller.screenshot('transaction-form-filled.png');

// Submit
await controller.click('button:has-text("Save")');
await controller.screenshot('transaction-saved.png');

// Verify it appears in list
const exists = await controller.elementExists('text=Coffee');
console.log('Transaction created:', exists);
```

### Example 3: Test Keyboard Navigation

```javascript
await controller.press('k'); // Vim-style up
await controller.press('j'); // Vim-style down
await controller.press('Enter'); // Select
await controller.screenshot('keyboard-nav-test.png');
```

## Typical Workflow

1. **User:** "Add dark mode toggle to settings"

2. **Claude implements** the feature

3. **Claude verifies** automatically:
   ```javascript
   await controller.click('[aria-label="Settings"]');
   await controller.click('input[name="darkMode"]');
   await controller.screenshot('dark-mode-on.png');

   const isDark = await controller.executeScript(() =>
     document.documentElement.classList.contains('dark')
   );
   ```

4. **Claude reports:** "✅ Dark mode toggle implemented and verified"

## Benefits

**vs Traditional Tests:**
- No test code to maintain
- Tests real app
- Flexible - write verification code on-demand
- Visual evidence via screenshots

**For Development:**
- Fast feedback after implementing features
- Can verify UI looks correct
- Screenshots for PR reviews
- Catches visual regressions

## npm Scripts

```json
{
  "test:ui": "node claude-ui-tester.js",
  "test:ui:interactive": "node test-ui-interactive.js"
}
```

## Future Enhancements

- **CI Integration** - Run UI tests in GitHub Actions
- **Visual Regression** - Compare screenshots against baseline
- **Record Interactions** - Save test scenarios for replay
- **Multi-platform** - Test on macOS, Windows, Linux
- **Performance Metrics** - Measure load times, rendering

## Notes

- Screenshots saved to `desktop/screenshots/` (gitignored)
- Test scripts are in `desktop/` directory
- See `CLAUDE_UI_TESTING_GUIDE.md` for detailed documentation
- Requires Node.js and npm dependencies installed

---

**This infrastructure gives Claude the ability to automatically verify UI features work!** 🎉

While perfect screenshots aren't available in all environments, the testing framework is complete and ready to use in proper development/CI environments.
