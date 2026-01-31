# Treeline UI

A Tauri desktop application for Treeline personal finance. Built with Svelte 5 and a plugin-based architecture.

## Quick Start

```bash
cd ui
npm install
npm run tauri:dev
```

This launches the desktop app in development mode with hot reload.

## Architecture

The UI is a Tauri v2 app with:
- **Frontend**: Svelte 5 with runes
- **Backend**: Rust with DuckDB for direct database access
- **CLI Integration**: Calls the Treeline CLI via Tauri sidecar for operations like sync

```
┌─────────────────────────────────────────────────────────────┐
│                        Core Shell                           │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────────┐ │
│  │ Sidebar │  │ Tab Bar │  │ Content │  │ Command Palette │ │
│  │         │  │         │  │  Area   │  │     (⌘K)        │ │
│  └─────────┘  └─────────┘  └─────────┘  └─────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                       Plugin System                         │
│  Core: Status, Query, Tagging                               │
│  External: ~/.treeline/plugins/                             │
└─────────────────────────────────────────────────────────────┘
```

### Plugins

The shell is minimal - most functionality comes from plugins. Plugins can:

1. **Register views** - Content areas shown in tabs
2. **Register sidebar items** - Navigation entries
3. **Register commands** - Actions for the command palette (⌘K)
4. **Register status bar items** - Footer widgets

**Core plugins** (built into the app):
- `status` - Financial overview dashboard
- `query` - SQL query editor
- `tagging` - Transaction tagging interface

**External plugins** are loaded from `~/.treeline/plugins/` at startup.

## External Plugins

External plugins extend Treeline without modifying the core app.

### Installing Plugins

Use the CLI to manage plugins:

```bash
# Install from a local directory
tl plugin install /path/to/my-plugin

# Install from GitHub
tl plugin install https://github.com/user/treeline-plugin-example

# List installed plugins
tl plugin list

# Uninstall a plugin
tl plugin uninstall my-plugin
```

The install command automatically builds the plugin if needed (`npm install && npm run build`).

### Creating Plugins

```bash
# Create a new plugin from the template
tl plugin new my-plugin
cd my-plugin

# Develop with watch mode
npm run dev

# Build for installation
npm run build
```

Or copy the `template/` directory manually.

### Plugin Structure

```
my-plugin/
├── manifest.json          # Plugin metadata (id, name, version, etc.)
├── src/
│   ├── index.ts          # Plugin entry point - exports `plugin`
│   └── MyView.svelte     # Svelte components
├── dist/
│   └── index.js          # Built plugin (generated)
└── vite.config.ts
```

### Plugin API

Plugins export a `plugin` object:

```typescript
import type { Plugin, PluginContext } from "./types";
import MyView from "./MyView.svelte";

export const plugin: Plugin = {
  manifest: {
    id: "my-plugin",
    name: "My Plugin",
    version: "1.0.0",
    description: "Does something useful",
    author: "Your Name",
  },

  activate(ctx: PluginContext) {
    // Register a view
    ctx.registerView({
      id: "my-view",
      name: "My View",
      icon: "🔧",
      component: MyView,
    });

    // Add to sidebar
    ctx.registerSidebarItem({
      id: "my-plugin-nav",
      label: "My Plugin",
      icon: "🔧",
      sectionId: "main",
      viewId: "my-view",
    });

    // Add a command (shows in ⌘K palette)
    ctx.registerCommand({
      id: "my-plugin:action",
      name: "Do Something",
      category: "My Plugin",
      execute: () => { /* ... */ },
    });
  },
};
```

See `template/README.md` for full API documentation.

### Plugin Isolation

Each external plugin bundles its own Svelte runtime (~47KB). This ensures compatibility regardless of which Svelte version the core app uses.

## Database Access

The Rust backend provides direct DuckDB access via the `execute_query` Tauri command:

```typescript
import { invoke } from "@tauri-apps/api/core";

const result = await invoke("execute_query", {
  query: "SELECT * FROM transactions LIMIT 10",
  readonly: true,
});
```

Returns JSON with `columns`, `rows`, and `row_count`.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `⌘K` | Open command palette |
| `⌘1-9` | Switch to tab 1-9 |
| `⌘W` | Close current tab |

## Project Structure

```
ui/
├── src/
│   ├── lib/
│   │   ├── core/           # Shell components
│   │   │   ├── Shell.svelte
│   │   │   ├── Sidebar.svelte
│   │   │   ├── TabBar.svelte
│   │   │   ├── ContentArea.svelte
│   │   │   ├── CommandPalette.svelte
│   │   │   └── StatusBar.svelte
│   │   │
│   │   ├── sdk/            # Plugin SDK types
│   │   │   ├── types.ts
│   │   │   ├── registry.ts
│   │   │   └── theme.ts
│   │   │
│   │   └── plugins/        # Core plugins
│   │       ├── status/
│   │       ├── query/
│   │       └── tagging/
│   │
│   ├── App.svelte
│   └── main.ts
│
├── src-tauri/              # Rust backend
│   ├── src/lib.rs          # Tauri commands
│   └── tauri.conf.json
│
└── package.json
```

## Theme System

Themes use CSS variables. Built-in themes:
- `dark` (default)
- `light`
- `nord`

Click the theme button in the status bar to cycle through themes.
