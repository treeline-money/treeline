# Treeline Integration Strategy Review — March 2026

## Executive Summary

The AI agent ecosystem has converged around two complementary standards: **MCP** (tool execution) and **SKILL.md** (agent knowledge). Treeline's existing OpenClaw skill is well-positioned — the SKILL.md format is now supported by 20+ platforms. The highest-impact next step is building an **MCP server** that wraps the CLI, which would instantly make Treeline accessible from Claude Desktop, ChatGPT Desktop, Claude Code, Cursor, and any MCP-compatible client.

---

## The Two Standards That Matter

### MCP — The "Hands"

MCP (Model Context Protocol) is the universal tool protocol. Originally developed by Anthropic (Nov 2024), it was donated to the Agentic AI Foundation under the Linux Foundation in December 2025, co-founded by Anthropic, Block, and OpenAI.

**Who supports it:**
- Claude Desktop (local STDIO + remote HTTP)
- Claude Code (local STDIO + remote HTTP)
- ChatGPT Desktop (via mcp.run gateway, remote HTTP on web)
- Cursor, Windsurf, VS Code (Copilot)
- Any client implementing the JSON-RPC 2.0 spec

**What it provides:**
- **Tools** — functions the LLM can invoke (e.g., `query`, `status`, `sync`)
- **Resources** — read-only data/context
- **Prompts** — pre-written prompt templates

**Context cost:** Heavy (~23-50K tokens per server for tool definitions). Mitigated by Claude Code's "Tool Search" feature which lazily loads only relevant tools.

### SKILL.md — The "Brain"

SKILL.md teaches agents *how* to use tools — SQL patterns, question mapping, response formatting. It's the knowledge layer.

**Who supports it:**
- Claude Desktop & Claude.ai (upload as zip)
- Claude Code (`.claude/skills/` directory)
- OpenAI Codex CLI
- GitHub Copilot Coding Agent
- Google Gemini CLI
- Cursor (via `.cursor/skills/`)
- OpenClaw (where Treeline's skill already lives)
- SkillsMP, ClawHub marketplaces

**Context cost:** Light (~100 tokens for metadata, <5K when activated). Progressive disclosure — only loaded when relevant.

**Key insight:** Skills orchestrate, MCP executes. They're complementary, not competing.

---

## What Treeline Has Today

| Asset | Status | Notes |
|-------|--------|-------|
| **CLI (`tl`)** | Production | 15+ commands, thin wrappers around core services |
| **OpenClaw SKILL.md** | Production | 563 lines, covers all CLI commands, SQL patterns, question mapping |
| **Desktop app** | Production | Tauri + Svelte 5, plugin system with permission validation |
| **Plugin SDK** | Production | TypeScript SDK with SQL sandboxing |
| **Core library** | Production | Hexagonal architecture, service layer ready for new adapters |
| **MCP server** | Does not exist | — |
| **Desktop Extension** | Does not exist | — |

---

## Integration Surfaces by Platform

### Claude Desktop

| Surface | What It Is | Treeline Fit | Priority |
|---------|-----------|--------------|----------|
| **MCP Server (STDIO)** | Local JSON-RPC server | Primary target — wraps `tl` CLI | **P0** |
| **Desktop Extension (.mcpb)** | One-click installable bundle | Distribution play — bundles tl + MCP server | P1 |
| **Skills** | SKILL.md upload or `.claude/skills/` | Already 90% built — adapt OpenClaw SKILL.md | **P0** |
| **Connectors** | Remote MCP (web/desktop/mobile) | Future — requires hosted infrastructure | P3 |
| **Cowork** | Agentic mode with sub-agents | Gets MCP/skills for free once those exist | Free |

### ChatGPT

| Surface | Treeline Fit | Priority |
|---------|--------------|----------|
| **ChatGPT Desktop + mcp.run** | MCP server works here too | Free (once MCP exists) |
| **ChatGPT web (remote MCP)** | Requires hosted server | P3 |
| **Apps SDK** | Built on MCP — future | P3 |

### Developer Tools

| Surface | Treeline Fit | Priority |
|---------|--------------|----------|
| **Claude Code** | MCP server + `.claude/skills/` | Free (once MCP + skill exist) |
| **Cursor** | MCP server + `.cursor/skills/` | Free |
| **OpenAI Codex** | SKILL.md support built-in | Free (OpenClaw skill works) |
| **GitHub Copilot** | SKILL.md support built-in | Free |
| **Gemini CLI** | SKILL.md support built-in | Free |

---

## Recommended Strategy: Three Layers

### Layer 1: MCP Server (P0 — the tool layer)

A local STDIO MCP server that wraps CLI commands as tools. Once this exists, Treeline works in Claude Desktop, Claude Code, ChatGPT Desktop, Cursor, and any MCP client.

**Proposed tools:**

| Tool | Description | Maps to |
|------|-------------|---------|
| `treeline_status` | Account balances and net worth | `tl status --json` |
| `treeline_query` | Execute read-only SQL | `tl query "..." --json` |
| `treeline_query_write` | Execute SQL with write access | `tl query "..." --allow-writes --json` |
| `treeline_sync` | Pull bank transactions | `tl sync --json` |
| `treeline_sync_preview` | Preview sync without applying | `tl sync --dry-run --json` |
| `treeline_import` | Import CSV transactions | `tl import ... --json` |
| `treeline_import_preview` | Preview CSV import | `tl import ... --dry-run --json` |
| `treeline_tag` | Tag transactions | `tl tag ... --json` |
| `treeline_backup` | Create/list/restore backups | `tl backup ...` |
| `treeline_doctor` | Database health check | `tl doctor --verbose` |
| `treeline_demo` | Toggle demo mode | `tl demo on/off/status` |
| `treeline_schema` | Introspect database schema | New — query `information_schema` |

**Implementation options:**

1. **Shell-out approach** (fastest to build): MCP server spawns `tl` CLI commands as subprocesses. Works immediately with existing CLI. Can be built in Python (FastMCP) or TypeScript.

2. **Native Rust approach** (best performance): MCP server links directly to `treeline-core` as a library. Avoids subprocess overhead. Requires implementing JSON-RPC 2.0 over STDIO in Rust.

3. **Hybrid**: Start with shell-out, migrate to native later.

**Recommendation:** Start with shell-out (Python/FastMCP or TypeScript). Ship fast, iterate. The CLI already handles all the heavy lifting — the MCP server is just a thin adapter.

**Estimated scope:** ~200-400 lines of code for the server. Most complexity is already in the CLI.

### Layer 2: Universal Skill (P0 — the knowledge layer)

Adapt the OpenClaw SKILL.md for distribution across all platforms.

**What needs to change:**

| Platform | Format | Changes from OpenClaw SKILL.md |
|----------|--------|-------------------------------|
| OpenClaw | Already done | None |
| Claude Desktop | Zip upload (SKILL.md + CONTEXT.md template) | Remove OpenClaw-specific metadata from frontmatter, add Claude-compatible frontmatter (`name`, `description`) |
| Claude Code | `.claude/skills/treeline/SKILL.md` | Same content, placed in directory |
| Cursor | `.cursor/skills/treeline/SKILL.md` | Same content, placed in directory |
| GitHub Copilot | `.github/skills/treeline/SKILL.md` | Same content, placed in directory |

**Key insight:** The content is 95% the same across all platforms. Only the frontmatter metadata differs. A build step could generate platform-specific variants from a single source.

**What the skill should NOT do:** The OpenClaw SKILL.md currently includes installation instructions for the CLI binary. When Treeline is accessed via MCP, installation is handled by the MCP server setup (or Desktop Extension). The skill should detect which access method is available and adjust accordingly.

### Layer 3: Desktop Extension (P1 — the distribution layer)

Package the MCP server + `tl` binary as a `.mcpb` extension for one-click install in Claude Desktop.

**What it bundles:**
- The MCP server (from Layer 1)
- The `tl` CLI binary (platform-specific)
- Configuration for STDIO transport

**User experience:** User clicks "Install" in Claude Desktop's extension directory. Claude can immediately query their finances. No terminal, no config files, no Node.js.

**Prerequisite:** MCP server (Layer 1) must exist first.

---

## What NOT to Build

### Custom skill management (`tl skills`)

The strategy doc mentioned a `tl skills` CLI for managing user skills. This is unnecessary — the Agent Skills standard already handles discovery and management across all platforms. Each platform has its own skill directory convention. Adding a Treeline-specific skill CLI would:
- Duplicate functionality that platforms already provide
- Create a non-standard interface users must learn
- Add maintenance burden with no cross-platform benefit

### Remote/hosted MCP server (for now)

A hosted MCP server would enable Claude web/mobile and ChatGPT web access. But it requires:
- Server infrastructure
- Authentication (OAuth)
- Data synchronization (Treeline is local-first)
- Security considerations for financial data in transit

This conflicts with Treeline's local-first principle. Revisit only if there's strong user demand for mobile/web agent access.

### Custom GPT / ChatGPT Action

The old "Custom GPT with Actions" pattern is being superseded by MCP. Building a Custom GPT action would be building for a deprecated surface.

---

## Sequencing

```
Month 1:  MCP Server (shell-out) + Skill adaptation
          → Treeline works in Claude Desktop, Claude Code, ChatGPT Desktop

Month 2:  Desktop Extension (.mcpb)
          → One-click install in Claude Desktop

Month 3:  Evaluate adoption, consider:
          → Native Rust MCP server (if performance matters)
          → Remote MCP server (if mobile/web demand exists)
          → Plugin marketplace integrations
```

---

## Architecture Fit

The MCP server fits cleanly into Treeline's hexagonal architecture:

```
                    ┌─────────────────────┐
                    │    MCP Client        │
                    │  (Claude Desktop,    │
                    │   ChatGPT, etc.)     │
                    └─────────┬───────────┘
                              │ JSON-RPC / STDIO
                    ┌─────────▼───────────┐
                    │    MCP Server        │  ← New adapter
                    │  (thin adapter)      │
                    └─────────┬───────────┘
                              │ subprocess / library call
                    ┌─────────▼───────────┐
                    │    CLI / Core        │  ← Existing
                    │  (services layer)    │
                    └─────────────────────┘
```

The MCP server is just another adapter in the hexagonal architecture — same as the CLI, same as the Tauri desktop app. It calls services, never the repository directly.

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| MCP spec changes | Low | Medium | Spec is stable (Nov 2025 version). Foundation governance reduces churn. |
| SKILL.md format fragmentation | Low | Low | Core format is simple markdown. Platform differences are in frontmatter only. |
| Desktop Extension format changes | Medium | Low | `.mcpb` is new (renamed from `.dxt`). Low investment to rebuild if format changes. |
| Security (SQL injection via MCP) | Medium | High | Use parameterized queries. Leverage existing permission validation from `permissions.rs`. Read-only by default. |
| Encrypted database access | Low | Medium | Document limitation clearly. MCP server should detect and report gracefully. |

---

## Conclusion

Treeline is well-positioned for the AI agent era. The OpenClaw SKILL.md is already one of the best finance skills available, and the CLI provides a clean interface for tool execution. The missing piece is the MCP server — a thin adapter (~300 lines) that would instantly make Treeline accessible from every major AI platform.

The recommended sequence is:
1. **MCP server** (shell-out, wraps CLI) — universal tool access
2. **Skill adaptation** — universal knowledge distribution
3. **Desktop Extension** — one-click install for Claude Desktop

This approach maximizes reach with minimal engineering effort, stays true to Treeline's local-first principle, and builds on the existing CLI investment rather than creating parallel interfaces.
