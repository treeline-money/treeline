# UI Development Guidelines

## Tech Stack
- **Framework**: Svelte 5 with runes ($state, $derived, $effect)
- **Desktop**: Tauri (Rust backend)
- **Build**: Vite

## Running the App
```bash
npm run tauri:dev   # Development with hot reload
npm run build       # Production build
```

## Styling Conventions

### CSS Variables
Always use CSS variables from the theme:
- `--bg-primary`, `--bg-secondary`, `--bg-tertiary` - Background colors
- `--text-primary`, `--text-secondary`, `--text-muted` - Text colors
- `--border-primary` - Border color
- `--accent-primary` - Primary accent (blue)
- `--accent-success` - Success (green)
- `--accent-danger` - Danger (red)
- `--accent-warning` - Warning (orange)
- `--spacing-xs`, `--spacing-sm`, `--spacing-md`, `--spacing-lg`, `--spacing-xl` - Spacing
- `--font-mono` - Monospace font

### Form Select Dropdowns
All `<select>` elements in modals/forms should use this styling pattern:
```css
select {
  padding: 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-primary);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 13px;
  appearance: none;
  -webkit-appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  padding-right: 28px;
  cursor: pointer;
}

select:focus {
  outline: none;
  border-color: var(--accent-primary);
}

select option {
  background: var(--bg-secondary);
  color: var(--text-primary);
  padding: 8px;
}
```

### Buttons
- `.btn.primary` - Primary action (accent background, white text)
- `.btn.secondary` - Secondary action (tertiary background, border)
- `.btn.danger` - Destructive action (danger color)
- `.btn.text` - Text-only button

### Modal Structure
Modals use the shared `Modal` component from `../../shared`:
```svelte
<Modal open={showModal} title="Title" onclose={closeModal} width="500px">
  <!-- Content -->

  {#snippet actions()}
    <button class="btn secondary" onclick={close}>Cancel</button>
    <button class="btn primary" onclick={save}>Save</button>
  {/snippet}
</Modal>
```

## Component Patterns

### State Management
Use Svelte 5 runes:
```typescript
let value = $state<string>("");
let computed = $derived(value.toUpperCase());
let complexComputed = $derived.by(() => { /* logic */ });
```

### Keyboard Navigation
Most views support vim-style navigation:
- `j/k` or arrows - Navigate up/down
- `Enter` - Select/open
- `e` - Edit
- `d` - Delete
- `a` - Add new

## Database Access
Use `executeQuery` from SDK for DuckDB queries:
```typescript
import { executeQuery } from "../../sdk";

// Read-only (default)
const result = await executeQuery("SELECT * FROM transactions");

// Write operations
await executeQuery("UPDATE sys_transactions SET ...", { readonly: false });
```

**Important**: Use `sys_transactions` (base table) for UPDATE/INSERT, not `transactions` (view).

## Logging

The app logs structured events to `~/.treeline/logs.duckdb` for troubleshooting. Use the `logger` from the SDK.

### Frontend Logging

```typescript
import { logger } from "../sdk";

// Log page/view navigation (already done automatically in ContentArea.svelte)
logger.page("accounts");

// Log user actions
logger.action("settings_opened", "command");
logger.action("sync_clicked", "statusbar");

// Log errors (message is auto-sanitized)
logger.error("query_error", error.message);
```

### When to Log

**DO log:**
- Key user actions (sync, backup, import, major settings changes)
- Errors in catch blocks (use `logger.error`)
- Operation completion in Tauri commands (sync, backup, restore)

**DON'T log:**
- Every button click or minor interaction
- User data (transactions, accounts, amounts, descriptions)
- Queries or SQL statements (may contain user data)

### Backend Logging (Rust)

In Tauri commands, access `LoggingState`:

```rust
#[tauri::command]
fn my_command(logging_state: State<LoggingState>) -> Result<(), String> {
    // Log event
    if let Ok(guard) = logging_state.logger.lock() {
        if let Some(logger) = guard.as_ref() {
            let _ = logger.log(LogEvent::new("my_event_started"));
        }
    }

    // ... do work ...

    // Log completion with context
    if let Ok(guard) = logging_state.logger.lock() {
        if let Some(logger) = guard.as_ref() {
            let _ = logger.log(
                LogEvent::new("my_event_completed")
                    .with_integration("simplefin")  // optional
            );
        }
    }
    Ok(())
}
```

### Privacy (CRITICAL)

Error messages are sanitized on the frontend, but be careful in Rust code:
- Never log transaction descriptions or amounts
- Never log account names or IDs
- Never log file paths with usernames
- Only log event names, integration names, and generic error types

## UX Philosophy

### Progressive Disclosure
Make the common case easy, but don't restrict power users. Examples:
- Auto-tag rules: Simple "contains/starts with" by default, but "Edit SQL" link for power users
- Budget transfers: Single transfer with defaults, but "+ Add another" to split across categories

### Don't Over-Constrain
Surface helpful indicators (e.g., "Remaining: $20") but don't prevent users from doing what they want. Let them over-allocate, under-allocate, etc. - it's their budget.

### Data Patterns
- Round currency amounts to cents (`Math.round(amount * 100) / 100`) to avoid floating point display errors
- Store config as JSON in plugin files via `invoke("write_plugin_config", ...)`

## Plugin Architecture

### Core Principles
- **Everything is a plugin**: Accounts, Budget, Transactions, Query are all plugins
- **Core plugins use the same SDK as community plugins** - they just declare different table permissions
- **Settings is NOT a plugin** - it's core app functionality in `lib/core/`

### Plugin Types

| Type | Location | Table Access | Distribution |
|------|----------|--------------|--------------|
| Core plugins | `lib/plugins/` | Can declare any tables in manifest | Bundled with app |
| External plugins | `~/.treeline/plugins/` | Own schema `plugin_{id}.*` | Separate repos, listed in `plugins.json` |

### Manifest Permissions
Plugins declare their schema and read permissions:
```typescript
manifest: {
  id: "budget",
  permissions: {
    read: ["transactions", "accounts"],
    schemaName: "plugin_budget",
  },
}
```

Plugins automatically have full read/write access to tables in their own schema (e.g., `plugin_budget.categories`). Write access is enforced at runtime in `sdk/public.ts`.

### Plugin SDK (`lib/sdk/public.ts`)
External plugins receive an SDK via props when their views are mounted:
- `sdk.query(sql)` - Read any table
- `sdk.execute(sql)` - Write to allowed tables only
- `sdk.toast.*` - Show notifications
- `sdk.openView()` - Navigate to views
- `sdk.onDataRefresh()` - React to data changes
- `sdk.settings.get/set()` - Plugin-scoped settings
- `sdk.theme.current()` - Theme access

### Creating a Plugin
1. Copy the `template/` from the treeline repo
2. Develop your plugin (edit `src/App.svelte`, update `manifest.json`)
3. Build: `npm run build`
4. Create a GitHub release with `manifest.json` and `dist/index.js` as release assets
5. Open a PR to the [treeline repo](https://github.com/treeline-money/treeline) adding your plugin to `plugins.json`

### Installing Plugins
Users install plugins from the UI: Settings → Plugins → Browse Plugins

Installation downloads `manifest.json` and `index.js` from GitHub release assets to `~/.treeline/plugins/{id}/`.

### Publishing Plugin Releases
When creating a GitHub release, attach these files as assets:
- `manifest.json` - Plugin metadata
- `index.js` - Built plugin code (from `dist/index.js`)

Example release command:
```bash
npm run build
gh release create v1.0.0 manifest.json dist/index.js --title "v1.0.0" --notes "Release notes"
```

### Core vs Internal SDK
- **Public SDK** (`sdk/public.ts`): What community plugins can access via props
- **Internal imports**: Core plugins can import from `sdk/` directly for things like `registry`, `activityStore`, internal operations

When building new features, prefer making them plugins unless they require internal SDK access (sync, CSV import, demo mode, etc.).

### Styling Community Plugins
Community plugins should match the app's look and feel using CSS variables. Reference `lib/sdk/styles.css` for common patterns.

**Key patterns to follow:**

**Layout:**
```css
.view {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  color: var(--text-primary);
}

.header {
  padding: var(--spacing-md) var(--spacing-lg);
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-primary);
}

.content {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-lg);
}
```

**Summary Cards:**
```css
.card {
  background: var(--bg-secondary);
  border: 1px solid var(--border-primary);
  border-radius: var(--radius-md);
  padding: var(--spacing-md);
}

.card-label {
  font-size: 11px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.card-value {
  font-size: 24px;
  font-weight: 600;
  color: var(--text-primary);
}
```

**Data Tables:**
```css
.table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.table th {
  text-align: left;
  padding: var(--spacing-sm) var(--spacing-md);
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  border-bottom: 2px solid var(--border-primary);
}

.table td {
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--border-primary);
}

.table tbody tr:hover {
  background: var(--bg-secondary);
}
```

**Badges:**
```css
.badge {
  display: inline-block;
  padding: 3px 8px;
  background: var(--bg-tertiary);
  color: var(--accent-primary);
  font-size: 10px;
  font-weight: 600;
  border-radius: var(--radius-sm);
  text-transform: uppercase;
}
```

**Loading/Empty States:**
```css
.loading, .empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--text-muted);
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border-primary);
  border-top-color: var(--accent-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
```

See the community plugin repos (plugin-subscriptions, plugin-goals, etc.) for complete examples.
