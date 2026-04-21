---
name: ui-dev
description: Drive the desktop app via WebDriver for UI work — launch, screenshot, click, iterate. Use when modifying Svelte/CSS so changes are verified visually, not assumed.
allowed-tools: Bash, Read, Glob, Grep, Edit, Write
---

# UI Development

The Treeline desktop app is Tauri + Svelte 5. Vite's HMR reloads the frontend on save, so after `launch` you can iterate without relaunching.

## Commands

All from `desktop/e2e/`:

```bash
uv run python explore.py launch                           # Start (once)
uv run python explore.py screenshot /tmp/ui.png           # Save PNG, then Read it
uv run python explore.py click '[data-testid="sidebar-budget"]'
uv run python explore.py eval 'return document.title'    # Arbitrary JS
uv run python explore.py html 'aside.sidebar'            # outerHTML
uv run python explore.py kill                             # Stop
```

Sandbox at `~/.treeline-e2e-sandbox` — seeded once with CSVs + first-party plugins.

## Rules

- Screenshot before/after any visual change, Read the PNG. Don't claim done without looking.
- Keep the app running across prompts in the same session — cold launch is slow.
- Stable selectors: `[data-testid="sidebar-<viewId>"]`, `[data-testid="tab-<viewId>"]`, `.tab-content.active`.
- To force a conditional state (banners, modals), use `eval` for quick checks or add an env-guarded dev mechanism — never ship it.
