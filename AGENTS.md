# Treeline - Development Guide

This is the main Treeline repository containing the desktop app, core library, CLI, and plugin SDK.

## Directory Structure

```
treeline/
├── desktop/                  # Desktop app (Tauri + Svelte)
│   ├── src/                  # Svelte frontend
│   ├── src-tauri/            # Tauri + Rust backend
│   └── AGENTS.md             # UI development guidelines
├── core/                     # Rust core library (treeline-core)
├── cli/                      # Rust CLI (tl command)
├── sdk/                      # Plugin SDK for npm
├── template/                 # Plugin template
├── mobile/                   # Future mobile app
├── plugins.json              # Plugin registry
└── Cargo.toml                # Workspace manifest
```

## Versioning

Treeline uses **CalVer** (Calendar Versioning) with unified versions across all components.

### Format: `YY.M.DDRR`

| Component | Example | Description |
|-----------|---------|-------------|
| YY | 26 | Year (2026) |
| M | 1 | Month (1-12, no leading zero) |
| DDRR | 3001 | Day (01-31) + Release number (01-99) |

**Examples:**
- `26.1.3001` - Jan 30, 2026, release 1
- `26.1.3002` - Jan 30, 2026, release 2
- `26.12.3115` - Dec 31, 2026, release 15

### Unified Versioning

All components share the same version at release time:
- Desktop app
- SDK (`@treeline-money/plugin-sdk`)
- CLI (`tl`)
- Core library

Git tags use the version directly (no prefix): `26.1.3001`

## Development

### Running the Desktop App

```bash
cd desktop
npm install
npm run tauri:dev
```

### Building the CLI

```bash
cargo build --release
./target/release/tl --help
```

### Building the SDK

```bash
cd sdk
npm install
npm run build
```

### Testing a Plugin Locally

```bash
cd /path/to/plugin
npm run build

# Install using CLI (restart app after):
./target/release/tl plugin install .

# Or install via Settings > Plugins in the app
```

## Releasing

The release process has two phases: **Build** (automatic) and **Release** (manual).

### Phase 1: Build (Automatic)

Every push to `main` triggers CI that builds:
- Desktop binaries for macOS, Linux, Windows
- CLI binaries for all platforms
- SDK npm tarball

Artifacts are uploaded to GitHub Actions and available for 90 days.

### Phase 2: Release (Manual)

When ready to release:

```bash
/release
```

Or trigger via GitHub Actions "Release" workflow with CalVer version.

This:
1. Downloads artifacts from latest CI run
2. Updates version files, commits, tags
3. Creates releases in both `treeline` and `treeline-releases` (dual-publish)
4. Publishes SDK to npm
5. Creates `latest-staging.json` (RC - not visible to users yet)

### Phase 3: Promote (Manual)

After testing the RC:

```bash
/promote
```

This copies `latest-staging.json` to `latest.json`, making the release visible to auto-updater.

## Auto-Updater

The desktop app checks for updates from:
```
https://github.com/treeline-money/treeline/releases/latest/download/latest.json
```

During transition, releases are dual-published to `treeline-releases` for backwards compatibility with existing users.

### Testing RC Updates

```bash
# Enable staging updates
touch ~/.treeline/use-staging-updates

# Open app, check for updates
# Disable when done
rm ~/.treeline/use-staging-updates
```

## Key Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace manifest (version, dependencies) |
| `desktop/src-tauri/tauri.conf.json` | Tauri config (version, updater endpoint) |
| `desktop/src-tauri/src/lib.rs` | Tauri commands + CalVer comparator |
| `sdk/package.json` | SDK npm package |
| `plugins.json` | Plugin registry |
| `.github/workflows/ci.yml` | CI builds |
| `.github/workflows/release.yml` | Release workflow |

## Architecture

### Desktop App

The desktop app is a Tauri application with:
- Svelte 5 frontend (runes, components)
- Rust backend (Tauri commands)
- DuckDB database (local-first)

The Rust backend uses `treeline-core` as a path dependency for direct library calls.

### Plugin System

Everything is a plugin (accounts, budget, transactions). Plugins are:
- Built with Svelte 5 and TypeScript
- Sandboxed with permission system
- Distributed via GitHub releases

See `desktop/AGENTS.md` for detailed UI development guidelines.

### Core Library

`treeline-core` provides:
- Database adapters (DuckDB)
- Integration adapters (SimpleFIN)
- Services (sync, backup, encryption, import, plugins)
- Domain models (Account, Transaction)

**Hexagonal architecture:** The layers are adapters → services → domain. Consumers (CLI, desktop) must only call services, never adapters/repository directly. `TreelineContext` exposes services (`ctx.import_service`, `ctx.sync_service`, etc.) as the public API. If you need new functionality, add it to the appropriate service — don't reach into `ctx.repository` from the CLI or desktop.

### CLI

The CLI (`tl`) provides command-line access to core functionality:
- `tl status` - Account summary
- `tl sync` - Sync from integrations
- `tl import` - Import transactions from CSV
- `tl query <sql>` - Execute SQL
- `tl plugin` - Manage plugins
- `tl backup` - Backup/restore

**CLI commands are thin wrappers.** Each command handles argument parsing, output formatting, and logging. All business logic belongs in core services. CLI commands must never call `ctx.repository` directly — always go through a service.

## Logging

Both CLI and desktop app log to `~/.treeline/logs.duckdb`.

**Privacy:** Never log user data (transactions, accounts, amounts). Only log:
- Event names
- Integration names
- Sanitized error messages

View logs:
```bash
./target/release/tl logs list --limit 20
```

## Testing

```bash
# Run all Rust tests
cargo test

# Run frontend type check
cd desktop && npm run check

# Build frontend
cd desktop && npm run build
```

## Principles

- **Local First**: All data stays on user's computer
- **Plugin System**: Everything is a plugin
- **Open Source**: Desktop app and core are now open source
- **AI Native**: AGENTS.md for AI-assisted development
