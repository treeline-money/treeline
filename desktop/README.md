# Treeline Desktop

A Tauri desktop application for Treeline personal finance. Built with Svelte 5 and a plugin-based architecture.

> **Full Documentation**: [treeline.money/docs](https://treeline.money/docs)

## Quick Start

```bash
cd desktop
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

**Core plugins** (built into the app): `status`, `query`, `tagging`

**External plugins** are loaded from `~/.treeline/plugins/` at startup.

See the [Plugin SDK documentation](https://treeline.money/docs/plugins) for the full API reference.

## Project Structure

```
desktop/
├── src/
│   ├── lib/
│   │   ├── core/           # Shell components (Sidebar, TabBar, etc.)
│   │   ├── sdk/            # Plugin SDK types
│   │   └── plugins/        # Core plugins (status, query, tagging)
│   ├── App.svelte
│   └── main.ts
│
├── src-tauri/              # Rust backend
│   ├── src/lib.rs          # Tauri commands
│   └── tauri.conf.json
│
└── package.json
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `⌘K` | Open command palette |
| `⌘1-9` | Switch to tab 1-9 |
| `⌘W` | Close current tab |

## Theme System

Themes use CSS variables. Built-in themes: `dark` (default), `light`, `nord`.

Click the theme button in the status bar to cycle through themes.

## Further Reading

- [Full Documentation](https://treeline.money/docs)
- [Plugin Development Guide](https://treeline.money/docs/plugins)
- [CLI Reference](https://treeline.money/docs/cli)
