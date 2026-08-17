# Treeline Plugin Development

This is a Treeline plugin template. Treeline is a local-first personal finance app.

## Key Files

| File | Purpose |
|------|---------|
| `manifest.json` | Plugin metadata (id, name, version, permissions) |
| `src/index.ts` | Plugin entry point - registers views and commands |
| `src/*View.svelte` | Svelte 5 components for your UI |
| `package.json` | Dependencies (includes `@treeline-money/plugin-sdk`) |

## Quick Commands

```bash
npm install          # Install dependencies
npm run build        # Build to dist/index.js
tl plugin install .  # Install locally for testing (first time only)
npm run dev          # Watch mode - builds to ~/.treeline/plugins/<id>/ with hot-reload
```

## Dev Workflow (Hot-Reload)

1. `tl plugin install .` — one-time install to register the plugin
2. Enable hot-reload in Treeline: Settings > Plugin Hot-Reload > On
3. `npm run dev` — starts vite in watch mode, builds directly to the installed plugin directory
4. Edit source files — Treeline reloads the plugin automatically (no restart)

Note: Migrations are skipped during hot-reload. Restart the app after adding new migrations.

## SDK Import

All types are imported from the npm package:

```typescript
import type { Plugin, PluginContext, PluginSDK } from "@treeline-money/plugin-sdk";
```

Views receive `sdk` via props:

```svelte
<script lang="ts">
  import type { PluginSDK } from "@treeline-money/plugin-sdk";

  interface Props {
    sdk: PluginSDK;
  }
  let { sdk }: Props = $props();
</script>
```

## SDK Quick Reference

| Method | What it does |
|--------|--------------|
| `sdk.sql(sql, params?)` | Read data (returns array of objects) |
| `sdk.query(sql, params?)` | Read data (returns raw row arrays) |
| `sdk.execute(sql, params?)` | Write to your plugin's tables only |
| `sdk.toast.success/error/info/warning(msg, desc?)` | Show notifications |
| `sdk.openView(viewId, props?)` | Navigate to another view |
| `sdk.onDataRefresh(callback)` | React when data changes (sync/import) |
| `sdk.emitDataRefresh()` | Notify other views that data changed |
| `sdk.updateBadge(count)` | Set badge count on sidebar item |
| `sdk.theme.current()` | Get "light" or "dark" |
| `sdk.theme.subscribe(callback)` | React to theme changes |
| `sdk.settings.get/set()` | Persist plugin settings |
| `sdk.state.read/write()` | Ephemeral runtime state |
| `sdk.modKey` | "Cmd" on Mac, "Ctrl" on Windows |
| `sdk.formatShortcut(shortcut)` | Format "mod+p" to platform display |
| `sdk.currency.format(amount)` | Format as currency (e.g., "$1,234.56") |
| `sdk.currency.formatCompact(amount)` | Compact format (e.g., "$1.2M") |
| `sdk.currency.getUserCurrency()` | Get user's currency code |

## Database Schema

Plugins query these views (not the underlying `sys_*` tables). Declare needed views in `permissions.read`.

### `transactions` view columns

| Column | Type | Description |
|--------|------|-------------|
| `transaction_id` | VARCHAR | Primary key (NOT `id`) |
| `account_id` | VARCHAR | Foreign key to accounts |
| `amount` | DECIMAL(15,2) | Amount (negative = expense, positive = income) |
| `description` | VARCHAR | Transaction description |
| `posted_date` | DATE | Date posted to account |
| `transaction_date` | DATE | Date of transaction |
| `tags` | VARCHAR[] | Array of tag strings |
| `source` | VARCHAR | Origin: 'simplefin', 'csv_import', 'manual', etc. |
| `account_name` | VARCHAR | Account display name (joined) |
| `account_type` | VARCHAR | Account type (joined) |
| `currency` | VARCHAR | ISO 4217 currency code (joined) |
| `institution_name` | VARCHAR | Bank/provider name (joined) |

### `accounts` view columns

| Column | Type | Description |
|--------|------|-------------|
| `account_id` | VARCHAR | Primary key (NOT `id`) |
| `name` | VARCHAR | Account display name |
| `account_type` | VARCHAR | Type: depository, investment, credit, loan |
| `currency` | VARCHAR | ISO 4217 currency code |
| `balance` | DECIMAL(15,2) | Latest synced balance |
| `institution_name` | VARCHAR | Bank/provider name |
| `classification` | VARCHAR | 'asset' or 'liability' |
| `is_manual` | BOOLEAN | True if manually created |

### `balance_snapshots` view columns

| Column | Type | Description |
|--------|------|-------------|
| `snapshot_id` | VARCHAR | Primary key |
| `account_id` | VARCHAR | Foreign key to accounts |
| `balance` | DECIMAL(15,2) | Balance at snapshot time |
| `snapshot_time` | TIMESTAMP | When recorded |
| `source` | VARCHAR | 'sync', 'manual', 'backfill' |
| `account_name` | VARCHAR | Account display name |

### Common query examples

```sql
-- Recent transactions
SELECT description, amount, posted_date, account_name
FROM transactions ORDER BY posted_date DESC LIMIT 20

-- Monthly spending by account
SELECT account_name, SUM(-amount) as spent
FROM transactions
WHERE amount < 0 AND posted_date >= DATE_TRUNC('month', CURRENT_DATE)
GROUP BY account_name ORDER BY spent DESC

-- Transaction count per tag
SELECT UNNEST(tags) as tag, COUNT(*) as cnt
FROM transactions GROUP BY tag ORDER BY cnt DESC
```

## Database Access

- **Read declared tables**: Declare in `manifest.json` permissions.read
- **Write to own schema**: Plugins automatically have write access to `plugin_{id}.*`
- **Schema naming**: Each plugin gets a schema like `plugin_hello_world`

## Doctor Checks

Create a view `plugin_{id}.doctor` in a migration and `tl doctor` (and the MCP
`doctor` tool) will report its rows. Columns: `check_id` (stable snake_case),
`name`, `status` (`pass` / `warning` / `error`), `message`, optional `details`.
Checks show up as `<plugin_id>.<check_id>`. It's SQL only — no network, no JS —
and only the first 50 `details` entries are kept. See `src/index.ts` for a
commented example.

## Styling

Plugins inherit the app's theme automatically via CSS variables. **Never hardcode colors.**

### Layout pattern

```svelte
<div class="tl-view">
  <div class="tl-header">
    <div class="tl-header-left">
      <h1 class="tl-title">My Plugin</h1>
      <p class="tl-subtitle">Description</p>
    </div>
  </div>
  <div class="tl-content">
    <!-- content here -->
  </div>
</div>
```

### Available CSS classes

| Class | Purpose |
|-------|---------|
| `.tl-view` | Root container (inherits theme) |
| `.tl-header` / `.tl-header-left` / `.tl-header-right` | Header bar |
| `.tl-title` / `.tl-subtitle` | Typography |
| `.tl-content` | Scrollable content area |
| `.tl-cards` / `.tl-card` / `.tl-card-label` / `.tl-card-value` | Stat cards |
| `.tl-btn` + `.tl-btn-primary` / `.tl-btn-secondary` / `.tl-btn-danger` | Buttons |
| `.tl-table` | Data table |
| `.tl-input` / `.tl-select` | Form elements |
| `.tl-badge` | Status badge |
| `.tl-empty` | Empty state |
| `.tl-loading` / `.tl-spinner` | Loading state |
| `.tl-muted` / `.tl-positive` / `.tl-negative` | Text colors |

### CSS variables (for custom styles)

```css
var(--bg-primary)       /* Main background */
var(--bg-secondary)     /* Cards, sections */
var(--text-primary)     /* Main text */
var(--text-muted)       /* Labels */
var(--border-primary)   /* Borders */
var(--accent-primary)   /* Accent (green) */
var(--color-positive)   /* Income */
var(--color-negative)   /* Expense */
var(--font-mono)        /* Monospace font */
var(--spacing-sm/md/lg) /* 8/12/16px */
var(--radius-md)        /* 6px */
```

### Styling rules

- DO use `.tl-*` classes and CSS variables
- DO NOT hardcode colors — the theme system handles light/dark
- DO NOT manually toggle `.dark` class — CSS variables handle it
- Theme tracking (`sdk.theme.current()`) is only needed for non-CSS purposes (chart libs, canvas)

## Common Patterns

### Create a table for your plugin data
```typescript
// Use migrations in index.ts instead of manual CREATE TABLE
```

### Show loading state
```typescript
let isLoading = $state(true);
try {
  const data = await sdk.sql("SELECT ...");
} finally {
  isLoading = false;
}
```

### Format currency
```typescript
const formatted = sdk.currency.format(1234.56); // "$1,234.56"
const compact = sdk.currency.formatCompact(1234567); // "$1.2M"
```

### Parameterized queries
```typescript
const results = await sdk.sql(
  "SELECT * FROM transactions WHERE amount > ? AND account_name = ?",
  [100, "Checking"]
);
```

## Icons

Use Lucide icon names for sidebar items and views:

```typescript
icon: "target"   // Preferred - icon name
icon: "gift"     // Also works
```

**Available icons:** `target`, `repeat`, `shield`, `wallet`, `credit-card`, `chart-line`, `tag`, `tags`, `database`, `refresh`, `link`, `zap`, `calendar`, `file-text`, `settings`, `plus`, `search`, `check`, `x`, `alert-triangle`, `info`, `help-circle`, `activity`, `gift`, `piggy-bank`

## Don't Do

- Don't write to tables not in your permissions (will throw error)
- Don't hardcode colors — use CSS variables and `.tl-*` classes
- Don't bundle heavy dependencies (keep plugins lightweight)
- Don't use `sdk.execute()` for SELECT queries (use `sdk.sql()` or `sdk.query()`)
- Don't use `id` as a column name — use `transaction_id`, `account_id`, etc.

## Releasing

```bash
./scripts/release.sh 0.1.0   # Tags and pushes, GitHub Action creates release
```

## Full Documentation

See https://treeline.money/docs/plugins/creating-plugins/
