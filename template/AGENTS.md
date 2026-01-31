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
npm run dev          # Watch mode (rebuild on changes)
tl plugin install .  # Install locally for testing
```

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
| `sdk.query(sql)` | Read data (SELECT queries) |
| `sdk.execute(sql)` | Write to your plugin's tables only |
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

## Database Access

- **Read declared tables**: Declare in `manifest.json` permissions.read
- **Write to own schema**: Plugins automatically have write access to `plugin_{id}.*`
- **Schema naming**: Each plugin gets a schema like `plugin_hello_world`

## Common Patterns

### Create a table for your plugin data
```typescript
// Create schema first (required before creating tables)
await sdk.execute(`CREATE SCHEMA IF NOT EXISTS plugin_my_plugin`);

await sdk.execute(`
  CREATE TABLE IF NOT EXISTS plugin_my_plugin.data (
    id VARCHAR PRIMARY KEY,
    value INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
  )
`);
```

### Subscribe to theme changes
```typescript
let theme = $state(sdk.theme.current());
sdk.theme.subscribe(t => theme = t);
```

### Show loading state
```typescript
let isLoading = $state(true);
try {
  const data = await sdk.query("SELECT ...");
  // use data
} finally {
  isLoading = false;
}
```

### Format currency
```typescript
const formatted = sdk.currency.format(1234.56); // "$1,234.56"
const compact = sdk.currency.formatCompact(1234567); // "$1.2M"
```

## Icons

Use Lucide icon names for sidebar items and views:

```typescript
icon: "target"   // Preferred - icon name
icon: "gift"     // Also works
```

**Available icons:** `target`, `repeat`, `shield`, `wallet`, `credit-card`, `chart`, `tag`, `tags`, `database`, `refresh`, `link`, `zap`, `calendar`, `file-text`, `settings`, `plus`, `search`, `check`, `x`, `alert-triangle`, `info`, `help-circle`, `activity`, `gift`, `piggy-bank`

## Don't Do

- Don't write to tables not in your permissions (will throw error)
- Don't forget dark mode support (test with both themes)
- Don't bundle heavy dependencies (keep plugins lightweight)
- Don't use `sdk.execute()` for SELECT queries (use `sdk.query()`)

## Releasing

```bash
./scripts/release.sh 0.1.0   # Tags and pushes, GitHub Action creates release
```

## Full Documentation

See https://github.com/treeline-money/treeline#building-plugins
