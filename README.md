# Treeline

**Local-first personal finance**

Your financial data stays on your computer. No cloud accounts, no subscriptions, no data harvesting.

> **Early Stage Software**: Treeline is in active development. Back up your data and expect breaking changes.

## What's Here

| Directory | Description |
|-----------|-------------|
| `desktop/` | Desktop app (Tauri + Svelte) |
| `core/` | Rust core library |
| `cli/` | Rust CLI |
| `sdk/` | TypeScript SDK for building plugins ([npm](https://www.npmjs.com/package/@treeline-money/plugin-sdk)) |
| `template/` | Starter template for new plugins |
| `plugins.json` | Registry of community plugins |

## Installing the CLI

Build from source (requires [Rust](https://rustup.rs/)):

```bash
git clone https://github.com/treeline-money/treeline.git
cd treeline
cargo build --release

# Add to your PATH or create an alias
alias tl="$(pwd)/target/release/tl"
```

Verify installation:

```bash
tl --version
```

## Quick Start

Try Treeline with sample data:

```bash
tl demo on
tl status
```

```
Financial Data Status

+-------------------+------+
| Accounts          | 6    |
| Transactions      | 365  |
| Balance Snapshots | 1080 |
| Integrations      | 1    |
+-------------------+------+

Date range: 2025-07-10 to 2026-01-04
```

Query your data with SQL:

```bash
tl query "SELECT posted_date, amount, description FROM transactions LIMIT 5"
```

```
+-------------+--------+------------------------+
| posted_date | amount | description            |
+==============================================+
| 2026-01-04  | -85.23 | WHOLE FOODS MARKET     |
| 2026-01-04  | -5.65  | STARBUCKS              |
| 2026-01-04  | -12.50 | CHIPOTLE MEXICAN GRILL |
| 2026-01-04  | -55.00 | SHELL OIL              |
| 2026-01-04  | -32.99 | AMAZON.COM             |
+-------------+--------+------------------------+
```

Turn off demo mode when done:

```bash
tl demo off
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `tl status` | Show account summary |
| `tl query <sql>` | Execute SQL queries |
| `tl sync` | Sync from connected integrations |
| `tl tag <tags> --ids <ids>` | Apply tags to transactions |
| `tl backup create` | Create a database backup |
| `tl backup list` | List available backups |
| `tl backup restore <file>` | Restore from a backup |
| `tl compact` | Compact the database |
| `tl doctor` | Run database health checks |
| `tl encrypt` | Encrypt the database |
| `tl decrypt` | Decrypt the database |
| `tl demo on/off` | Toggle demo mode with sample data |
| `tl plugin list` | List installed plugins |
| `tl plugin install <source>` | Install a plugin (local path or GitHub URL) |
| `tl plugin uninstall <id>` | Uninstall a plugin |
| `tl plugin new <name>` | Create a new plugin from template |

Run `tl <command> --help` for detailed options.

---

# Building Plugins

Plugins extend Treeline with custom views, commands, and functionality. They're built with TypeScript and Svelte 5, and run inside the [Treeline desktop app](https://github.com/treeline-money/treeline/releases).

> **Note**: The CLI is for data management and plugin development. To use plugins, you need the desktop app.

## Create a Plugin

**Prerequisites**: Node.js 18+ and npm

```bash
tl plugin new my-plugin
cd my-plugin
npm install
```

This creates a ready-to-build plugin with your plugin name already configured in `manifest.json`. Rename `HelloWorldView.svelte` to match your plugin.

## Project Structure

```
my-plugin/
├── manifest.json          # Plugin metadata and permissions
├── src/
│   ├── index.ts           # Entry point - registers views and commands
│   └── MyPluginView.svelte # Svelte 5 component for your UI
├── dist/
│   └── index.js           # Built plugin (generated)
├── vite.config.ts
├── package.json
└── tsconfig.json
```

## manifest.json

Defines your plugin's identity and permissions:

```json
{
  "id": "my-plugin",
  "name": "My Plugin",
  "version": "0.1.0",
  "description": "What your plugin does",
  "author": "Your Name",
  "icon": "target",
  "main": "index.js",
  "permissions": {
    "read": ["transactions", "accounts"],
    "schemaName": "plugin_my_plugin"
  }
}
```

**Permissions:**
- `read` - Core tables your plugin can SELECT from (e.g., `transactions`, `accounts`)
- `schemaName` - Your plugin's database schema (auto-created, defaults to `plugin_<id>`)

Your plugin automatically has full read/write access to its own schema.

> **Note**: The manifest is defined in both `manifest.json` (for the app to discover your plugin) and in your code's `plugin.manifest` (for runtime). Keep them in sync.

## Entry Point (index.ts)

Register your views, sidebar items, and commands:

```typescript
import type { Plugin, PluginContext, PluginSDK, PluginMigration } from "@treeline-money/plugin-sdk";
import MyPluginView from "./MyPluginView.svelte";
import { mount, unmount } from "svelte";

const migrations: PluginMigration[] = [
  {
    version: 1,
    name: "create_data_table",
    up: `
      CREATE TABLE IF NOT EXISTS plugin_my_plugin.data (
        id VARCHAR PRIMARY KEY,
        value INTEGER,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      )
    `,
  },
];

export const plugin: Plugin = {
  manifest: {
    id: "my-plugin",
    name: "My Plugin",
    version: "0.1.0",
    description: "What your plugin does",
    author: "Your Name",
    permissions: {
      read: ["transactions", "accounts"],
      schemaName: "plugin_my_plugin",
    },
  },

  migrations,

  activate(context: PluginContext) {
    // Register the view
    context.registerView({
      id: "my-plugin-view",
      name: "My Plugin",
      icon: "target",
      mount: (target: HTMLElement, props: { sdk: PluginSDK }) => {
        const instance = mount(MyPluginView, { target, props });
        return () => unmount(instance);
      },
    });

    // Add to sidebar
    context.registerSidebarItem({
      sectionId: "main",
      id: "my-plugin",
      label: "My Plugin",
      icon: "target",
      viewId: "my-plugin-view",
    });

    // Register a command (appears in command palette)
    context.registerCommand({
      id: "my-plugin.open",
      name: "Open My Plugin",
      execute: () => context.openView("my-plugin-view"),
    });
  },
};
```

## Svelte Views

Views receive the SDK via props:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import type { PluginSDK } from "@treeline-money/plugin-sdk";

  interface Props {
    sdk: PluginSDK;
  }
  let { sdk }: Props = $props();

  let transactions = $state<any[]>([]);
  let isLoading = $state(true);

  onMount(async () => {
    await loadData();

    // Re-load when data changes (sync, import, etc.)
    sdk.onDataRefresh(() => loadData());
  });

  async function loadData() {
    isLoading = true;
    try {
      transactions = await sdk.query(
        "SELECT * FROM transactions ORDER BY posted_date DESC LIMIT 10"
      );
    } catch (e) {
      sdk.toast.error("Failed to load", e instanceof Error ? e.message : String(e));
    } finally {
      isLoading = false;
    }
  }
</script>

<div class="container">
  <h1>My Plugin</h1>
  {#if isLoading}
    <p>Loading...</p>
  {:else}
    <ul>
      {#each transactions as tx}
        <li>{tx.posted_date}: {sdk.currency.format(tx.amount)} - {tx.description}</li>
      {/each}
    </ul>
  {/if}
</div>
```

## SDK Reference

The `sdk` object provides everything your plugin needs:

### Database

```typescript
// Read data (parameterized queries recommended)
const rows = await sdk.query<Transaction>(
  "SELECT * FROM transactions WHERE amount > ?",
  [100]
);

// Write to your plugin's tables
const schema = sdk.getSchemaName(); // "plugin_my_plugin"
await sdk.execute(
  `INSERT INTO ${schema}.data (id, value) VALUES (?, ?)`,
  [crypto.randomUUID(), 42]
);
```

### Notifications

```typescript
sdk.toast.success("Saved!", "Your changes have been saved");
sdk.toast.error("Error", "Something went wrong");
sdk.toast.info("Tip", "You can do this...");
sdk.toast.warning("Warning", "This action cannot be undone");
```

### Navigation

```typescript
sdk.openView("budget");              // Open a view
sdk.openView("query", { sql: "..." }); // Open with props
```

### Data Events

```typescript
// React to data changes
const unsubscribe = sdk.onDataRefresh(() => {
  loadData();
});

// Notify other views that you changed data
sdk.emitDataRefresh();
```

### Theme

```typescript
const theme = sdk.theme.current(); // "light" | "dark"

sdk.theme.subscribe((newTheme) => {
  // React to theme changes
});
```

### Currency Formatting

```typescript
sdk.currency.format(1234.56);        // "$1,234.56"
sdk.currency.formatCompact(1234567); // "$1.2M"
sdk.currency.formatAmount(1234.56);  // "1,234.56" (no symbol)
sdk.currency.getSymbol("EUR");       // "€"
sdk.currency.getUserCurrency();      // "USD"
```

### Settings & State

```typescript
// Persistent settings (survives restarts)
const settings = await sdk.settings.get<{ showHidden: boolean }>();
await sdk.settings.set({ showHidden: true });

// Ephemeral state (runtime only)
const state = await sdk.state.read<MyState>();
await sdk.state.write({ count: 5 });
```

### UI Helpers

```typescript
sdk.modKey;                   // "Cmd" on Mac, "Ctrl" on Windows
sdk.formatShortcut("mod+p");  // "⌘P" on Mac, "Ctrl+P" on Windows
sdk.updateBadge(5);           // Show badge count on sidebar item
```

## Database Migrations

Migrations run automatically when your plugin loads. Define them as an array ordered by version:

```typescript
const migrations: PluginMigration[] = [
  {
    version: 1,
    name: "initial_schema",
    up: `
      CREATE TABLE plugin_my_plugin.items (
        id VARCHAR PRIMARY KEY,
        name VARCHAR NOT NULL
      )
    `,
  },
  {
    version: 2,
    name: "add_priority",
    up: `ALTER TABLE plugin_my_plugin.items ADD COLUMN priority INTEGER DEFAULT 0`,
  },
];
```

The app tracks which migrations have run in `plugin_<id>.schema_migrations`.

## Available Tables

Plugins can read these core tables (when declared in `permissions.read`):

| Table | Description |
|-------|-------------|
| `transactions` | Enriched transaction view with account info |
| `accounts` | Account information with current balances |
| `balance_snapshots` | Historical balance data |

Query the schema for column details:

```sql
SELECT column_name, data_type
FROM information_schema.columns
WHERE table_name = 'transactions'
```

## Icons

Use Lucide icon names for sidebar items and views:

```typescript
icon: "target"      // Recommended: icon names
icon: "piggy-bank"
icon: "credit-card"
```

Available: `target`, `repeat`, `shield`, `wallet`, `credit-card`, `chart`, `tag`, `tags`, `calendar`, `settings`, `search`, `plus`, `check`, `x`, `alert-triangle`, `info`, `activity`, `gift`, `piggy-bank`, and more.

## Build and Test

```bash
# Build once
npm run build

# Watch mode for development
npm run dev
```

Install your plugin locally:

```bash
tl plugin install .
# Restart the Treeline app to load changes
```

## Release Your Plugin

1. Create a GitHub repository for your plugin
2. Include the GitHub Actions workflow (in `template/.github/workflows/release.yml`)
3. Run the release script:

```bash
./scripts/release.sh 0.1.0
```

This tags the release and pushes to GitHub. The workflow automatically builds and publishes the release with `manifest.json` and `dist/index.js`.

## Submit to Community Plugins

Once your plugin has a GitHub release, open a PR to [treeline](https://github.com/treeline-money/treeline) adding your plugin to `plugins.json`:

```json
{
  "id": "my-plugin",
  "name": "My Plugin",
  "description": "What it does",
  "author": "Your Name",
  "repo": "https://github.com/you/my-plugin"
}
```

Users can then install your plugin from Settings > Plugins in the app.

---

## License

MIT
