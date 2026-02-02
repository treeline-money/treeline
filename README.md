# Treeline

**Local-first personal finance**

Your financial data stays on your computer in a DuckDB database. No cloud accounts, no subscriptions, no data harvesting.

[Download](https://treeline.money/download) · [Documentation](https://docs.treeline.money) · [Discord](https://discord.gg/EcNvBnSft5)

> **Beta**: Treeline is in active development. Back up your data and expect breaking changes.

## Quick Start

Download the desktop app from [treeline.money/download](https://treeline.money/download), or install the CLI:

```bash
# macOS / Linux
curl -fsSL https://treeline.money/install.sh | sh

# Windows (PowerShell)
irm https://treeline.money/install.ps1 | iex
```

See the [Getting Started guide](https://docs.treeline.money/getting-started/installation/) for details.

## Repository Structure

| Directory | Description |
|-----------|-------------|
| `desktop/` | Desktop app (Tauri + Svelte) |
| `core/` | Rust core library |
| `cli/` | Rust CLI (`tl` command) |
| `sdk/` | TypeScript SDK for plugins ([npm](https://www.npmjs.com/package/@treeline-money/plugin-sdk)) |
| `template/` | Starter template for new plugins |
| `docs/` | Documentation site ([docs.treeline.money](https://docs.treeline.money)) |
| `plugins.json` | Registry of community plugins |

## Documentation

- [Installation](https://docs.treeline.money/getting-started/installation/)
- [CLI Reference](https://docs.treeline.money/cli/)
- [Building Plugins](https://docs.treeline.money/plugins/creating-plugins/)
- [Database Schema](https://docs.treeline.money/reference/database-schema/)

## Contributing

See the [Contributing guide](https://docs.treeline.money/contributing/) for development setup.

## License

MIT
