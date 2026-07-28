# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

**AgentBuddy** (npm/Cargo package `agent-buddy`, Rust lib `agent_buddy_lib`, Tauri productName `AgentBuddy`) is a desktop app (macOS + Windows) that discovers local AI coding agents and manages their shared configuration — especially MCP servers and Skills, plus backup upload to WebDAV.

Stack:

- **Frontend**: React 18 + TypeScript + Vite 6 + Tailwind 3 (`src/`)
- **Backend**: Tauri 2 + Rust (`src-tauri/`)
- **Package manager**: pnpm (see `pnpm-lock.yaml` / `pnpm-workspace.yaml`)
- **App data**: `~/.agentbuddy/` on macOS/Linux; Windows prefers `%LOCALAPPDATA%\AgentBuddy` while still reading legacy `~/.agentbuddy` (`config.json`, SQLite `agents.db`, skills library). Platform helpers live in `src-tauri/src/platform.rs`.

Primary platforms: **macOS** and **Windows** (see `WINDOWS_ADAPTATION_PLAN.md`). UI copy is Chinese.

## Common commands

```bash
# Install JS deps
pnpm install

# Full desktop app (Vite on :3000 + Rust/Tauri)
pnpm dev

# Renderer only (browser preview; Tauri `invoke` will fail gracefully)
pnpm dev:renderer

# Production build (renderer → dist, then Tauri bundle)
pnpm build
pnpm build:renderer   # Vite only

# Typecheck frontend
pnpm typecheck

# Rust unit tests (crypto, config, sniff, mcp_config, skills, claude_env, codex_env, …)
cd src-tauri && cargo test

# Run a single Rust test by name filter
cd src-tauri && cargo test encrypt_decrypt
```

There is no ESLint/Prettier script and no frontend test runner in `package.json` yet.

Requirements: Node + pnpm, Rust toolchain, Tauri 2 system deps (macOS / Windows).

## Architecture

```
src/                     React UI
  App.tsx                Mode (main | settings) + view routing
  components/Sidebar.tsx Navigation
  components/ui.tsx      Shared UI primitives (page-neutral)
  components/Toast.tsx   Toast feedback
  components/pages/      Feature pages (invoke Tauri commands)
    claude-env/          Claude Env types + API helpers
    codex-env/           Codex Env types + API helpers
    opencode-config/     OpenCode provider/model types + API helpers
    project-config/      Project AI config types + API helpers
    skills/              Skills types, API, controls, icons
  lib/theme.ts           Theme load/save via invoke
  index.css              Global styles + design tokens (data-theme)

src-tauri/src/
  main.rs                Binary entry → agent_buddy_lib::run()
  lib.rs                 Registers all #[tauri::command] handlers + setup
  platform.rs            Cross-platform paths, open/reveal, permissions, symlink/copy, folder picker
  agents.rs              Single source of truth: AgentSpec (identity, sniff paths, MCP dialect/path, skills roots)
  sniff.rs               Discover agents from agents::agents(); PATH/PATHEXT + shim filtering
  mcp_config.rs          Read/write/sniff MCP configs via agents registry dialects
  skills.rs              Skills library under app data/skills + sniff/GitHub/GitCode import
  claude_env.rs          Multi Claude Code envs via CLAUDE_CONFIG_DIR + shell aliases (zsh/bash/fish/PowerShell)
  codex_env.rs           Multi Codex CLI envs via CODEX_HOME + shell aliases (zsh/bash/fish/PowerShell)
  opencode_config.rs     OpenCode ~/.config/opencode provider/model + auth.json + Models.dev
  project_config.rs      Project-dir AI skeleton init (Full / Symlink modes) + optional project-level MCP (per-agent files, merge by title) and shared skills install into `.agents/skills` (copy/symlink)
  db.rs                  SQLite persistence (agents, mcp_servers, webdav, skills, claude_environments, codex_environments)
  config.rs              App config.json (theme; secretsKey private)
  crypto.rs              AES-256-GCM + HKDF for secret fields
  webdav.rs              WebDAV connections + probe + MKCOL/PUT upload
  backup.rs              Backup units probe, zip/abenc pack, multi-WebDAV upload

AGENT_MCP_SKILLS_MAP.md  Canonical path/schema/dialect spec for MCP & Skills
```

### Frontend ↔ backend boundary

Pages call Tauri with dynamic import:

```ts
const { invoke } = await import("@tauri-apps/api/core");
await invoke("command_name", { ... });
```

Serde uses `rename_all = "camelCase"` on public DTOs so Rust fields map to frontend camelCase. Transport type field is JSON key `"type"` (`#[serde(rename = "type")]` on Rust side).

Registered commands (see `lib.rs`):

| Area | Commands |
|------|----------|
| Agents | `sniff_agents`, `get_cached_agents`, `add_agent_manual` |
| MCP | `apply_mcp_to_agents`, `remove_mcp_from_agents`, `sniff_mcp_servers`, `get_mcp_servers`, `save_mcp_servers`, `delete_mcp_server`, `test_mcp_connection` |
| App | `get_app_config`, `set_theme`, `get_network_settings`, `update_network_settings` |
| Skills | `list_skills`, `sniff_skills`, `preview_sniff_skills`, `import_sniffed_skills`, `check_skill_updates`, `add_skill_local`, `pick_and_add_skill_local`, `add_skill_github`, `add_skill_gitcode`, `update_skill`, `export_skill_to_dir`, `open_external_url`, `delete_skill`, `apply_skill_to_agents`, `batch_delete_skills`, `batch_export_skills_to_dir`, `batch_apply_skills_to_agents`, `batch_set_skill_tag`, `preview_cc_switch_skills`, `migrate_cc_switch_skills` |
| Claude Env | `list_claude_environments`, `sniff_claude_environments`, `import_claude_environment`, `clone_claude_environment`, `upsert_claude_environment`, `delete_claude_environment`, `install_claude_env_alias`, `remove_claude_env_alias`, `remove_all_claude_env_aliases`, `get_claude_env_shell_status`, `reveal_claude_env_dir`, `open_claude_env_settings`, `get_claude_env_secret`, `sync_claude_env_mcp`, `sync_all_claude_env_mcp`, `get_claude_env_mcp_status` |
| Codex Env | `list_codex_environments`, `sniff_codex_environments`, `import_codex_environment`, `clone_codex_environment`, `upsert_codex_environment`, `delete_codex_environment`, `install_codex_env_alias`, `remove_codex_env_alias`, `remove_all_codex_env_aliases`, `get_codex_env_shell_status`, `reveal_codex_env_dir`, `open_codex_env_config`, `get_codex_env_secret`, `sync_codex_env_mcp`, `sync_all_codex_env_mcp` |
| OpenCode | `get_opencode_config`, `set_opencode_defaults`, `upsert_opencode_provider`, `delete_opencode_provider`, `upsert_opencode_model`, `delete_opencode_model`, `get_opencode_provider_secret`, `set_opencode_provider_secret`, `fetch_models_dev_catalog`, `probe_opencode_models_endpoint`, `reveal_opencode_config` |
| Project Config | `pick_project_folder`, `check_project_config_exists`, `init_project_config` |
| WebDAV | `get_webdav_connections`, `upsert_webdav_connection`, `delete_webdav_connection`, `test_webdav_connection`, `test_webdav_connection_draft` |
| Backup | `list_backup_units`, `get_backup_settings`, `update_backup_settings`, `run_backup_upload` |

`get_app_config` never returns `secretsKey`. Master key stays Rust-only via `config::load_secrets_key()`.

### Claude multi-env MCP paths (`claude_env.rs`)

Claude Code isolates config roots via `CLAUDE_CONFIG_DIR`, but **MCP user-scope storage is path-asymmetric**:

| Launch | Top-level `mcpServers` file |
|--------|-----------------------------|
| Default `claude` (no env var) | `~/.claude.json` (home root, **not** under `~/.claude/`) |
| `CLAUDE_CONFIG_DIR=$DIR claude` | `$DIR/.claude.json` |

AgentBuddy MCP manage still writes only the default home-root file. Custom envs share MCP via explicit sync:

- Source of truth: `~/.claude.json` top-level `mcpServers` only (does **not** touch `projects[*].mcpServers`)
- Target: each non-default env’s `$config_dir/.claude.json` — replace `mcpServers`, preserve other keys; atomic write
- UI: per-env “同步 MCP”, “同步 MCP 到全部”, clone checkbox `syncMcp` (default true)
- Default env is a no-op (already uses the global file)

Per non-default env, managed `settings.json` env keys are `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL` (plus the companion default-model keys written with the same custom value: `ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS,FABLE}_MODEL` and `ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS,FABLE}_MODEL_NAME`). Clearing the custom model deletes the whole set; the list/edit DTO still only reads the primary `ANTHROPIC_MODEL`. On every non-default **save** (and after clone), if `ANTHROPIC_MODEL` is set but any companion is missing or differs, the full set is backfilled from the primary even when the edit form did not change the model field. The token is **never returned in the list DTO** (only a `hasApiKey` flag); the edit dialog fetches it on demand via `get_claude_env_secret`. Shell aliases follow `$SHELL`: zsh→`~/.zshrc`, bash→`~/.bash_profile`/`~/.bashrc`, fish→`~/.config/fish/config.fish` (fish uses an `env`-prefixed alias body).

### Codex multi-env (`codex_env.rs`)

Codex CLI isolates almost all local state via **`CODEX_HOME`** (default `~/.codex`; directory must already exist). AgentBuddy manages multiple homes:

| Launch | Config root |
|--------|-------------|
| Default `codex` | `~/.codex` |
| `CODEX_HOME=$DIR codex` | `$DIR` (`config.toml`, auth, skills, sessions, …) |

MCP for multi-env:

- Source of truth for sync: default `~/.codex/config.toml` → `[mcp_servers]`
- Target: each non-default env’s `$CODEX_HOME/config.toml` — **replace** `[mcp_servers]`, preserve other keys; atomic write
- Global MCP manage page still writes **only** the default `~/.codex/config.toml`
- UI: per-env “同步 MCP”, “同步 MCP 到全部”, clone checkbox `syncMcp` (default false)
- Default env sync is a no-op

Managed fields (non-default envs): `config.toml` → `model` / `model_provider` / provider `base_url` (or top-level `openai_base_url`); Token → `$CODEX_HOME/auth.json` as `{ "OPENAI_API_KEY": "…" }` (merge preserve other keys; empty clears key and deletes file if empty; mode `0o600`). List DTO exposes `hasAuth` only; token via `get_codex_env_secret` (prefers `auth.json`, falls back to legacy `experimental_bearer_token`). Shell marker is **independent** of Claude Env (`# >>> AgentBuddy Codex Env ...`). Clone copies `config.toml` / `AGENTS.md` / `skills` only — never `auth.json` / sessions / sqlite (clone form can write a new Token into the target `auth.json`).

### Agent registry (`agents.rs`) + sniffing (`sniff.rs`)

`agents.rs` is the **single source of truth** for agent identity, install/config discovery paths, MCP dialect/path, and skills roots. `sniff.rs`, `mcp_config.rs`, and `skills.rs` all read from `agents::agents()` / `agents::find()`.

Identifiers (`name`) must stay stable — MCP writers and DB keys use them:

`codex`, `claude-code`, `claude-desktop`, `opencode`, `deveco-code`, `kiro`, `antigravity`, `codebuddy`, `codebuddy-cn`, `workbuddy`

Rules:

- `found == true` only if an install path (App and/or CLI) exists; config dir alone is not enough
- CLI is resolved from static `bin_paths` + `PATH` (`search_names`); App paths sorted before CLI paths
- PATH/cache results filter out CLI shims (e.g. cmux under `$TMPDIR/cmux-cli-shims/`, other temp-dir wrappers) via `sniff::is_shim_path`; `get_cached_agents` also strips shim install paths on read so stale cache self-heals
- Codex CLI + ChatGPT App share `~/.codex`
- CodeBuddy and CodeBuddy CN share `~/.codebuddy` (`shared_root: Some("codebuddy-shared")`)
- Claude Desktop config is scanned under `~/Library/Application Support/Claude*` for `claude_desktop_config.json` (`scan_app_support` / `McpPath::ClaudeDesktopScan`)

When adding a new agent or changing MCP/skills paths, update **`agents.rs` first** (and only extend `McpPath` / writers in `mcp_config` if a new path shape is needed). Keep `AGENT_MCP_SKILLS_MAP.md` in sync.

### MCP multi-dialect writer (`mcp_config.rs`)

Implements apply/remove/sniff against each agent’s user-global config. Spec source of truth: **`AGENT_MCP_SKILLS_MAP.md`**. Dialects (defined on `agents::McpDialect`):

| Dialect | Agents | File shape |
|---------|--------|------------|
| `TomlMcpServers` | codex | `~/.codex/config.toml` → `[mcp_servers.*]` |
| `ClaudeJsonUser` / `JsonMcpServers` | claude-code, claude-desktop, kiro, codebuddy*, workbuddy | top-level `mcpServers` |
| `JsonMcp` | opencode, deveco-code | top-level `mcp` (JSON/JSONC) |
| `JsonGeminiMixed` | antigravity | `~/.gemini/settings.json` `mcpServers` |

Important behaviors:

- `dedupe_write_targets`: codebuddy + codebuddy-cn write once to the shared root (`shared_root`)
- Writes are atomic (temp file + rename)
- JSONC (OpenCode/DevEco) is parsed with `json5`; rewrite is pretty JSON (comments not preserved)
- Sniff merges disk entries into SQLite by title (case-insensitive); disk is source of truth for `appliedAgents`
- Internal smoke titles prefixed `__agentbuddy` are ignored on sniff
- `test_mcp_connection`: runtime probe only (stdio spawns the process + sends `initialize`; http/sse POSTs `initialize`); never writes config
- Frontend records `appliedAgents` from the **actually-succeeded** agents in the apply batch (partial failures no longer mark all as applied); the MCP list is DB-only (localStorage mirror removed)

### Local persistence

- **SQLite** `~/.agentbuddy/agents.db`: tables `agents`, `mcp_servers`, `webdav_connections`, `skills` (skill source metadata: local/github/gitcode, repo, commit refs), `claude_environments` (multi CLAUDE_CONFIG_DIR profiles), `codex_environments` (multi CODEX_HOME profiles)
- **Skills files** `~/.agentbuddy/skills/<id>/SKILL.md` (content on disk; provenance in SQLite `skills`)
- **Config** `~/.agentbuddy/config.json`: public `theme`, `backup`, `network` (proxy mode: none|system|custom; custom supports http/socks5); private `secretsKey` (base64 32-byte master key). Outbound HTTP (WebDAV / Skills / MCP probe / OpenCode catalog) goes through `http_client::apply_proxy`.
- WebDAV passwords: per-row salt/nonce/cipher via `crypto` (HKDF info `agentbuddy/webdav/v1`); never returned to UI

### UI structure

`App.tsx` switches:

- **Main**: agent-sniff, mcp-manage, skills-manage, claude-env, codex-env, opencode-config, backup-manage
- **Settings**: preferences, network (proxy), webdav

### Project AI config (`project_config.rs`)

Per-repo init for a picked folder: skeleton (Full / Symlink modes) plus two optional selections the UI offers from app-configured data:

- **MCP（可选）**：从 MCP 管理页的列表勾选，写入每个被勾选 agent 的**项目级** MCP 文件（`AGENT_SPECS[].mcp`）：claude-code / codebuddy-cn / workbuddy → 项目根 `.mcp.json`（同路径去重只写一次）；codex → `.codex/config.toml`；opencode → `opencode.json`；deveco-code → `.deveco/deveco.jsonc`（JSONC）；antigravity → `.gemini/settings.json`。写入复用 `mcp_config::apply_draft_to_file` 的方言写器，按 server title 合并（保留文件其它键），不受 overwrite 开关阻断、不计入冲突列表。
- **Skills（可选）**：从技能库（`skills::library_skill_dir`）勾选，以「软链接 / 完整复制」安装到 `<repo>/.agents/skills/<id>`（overwrite 语义与骨架一致：非空真实目录拒绝删除）。Symlink 模式下 `<config_dir>/skills` 本已链接到 `.agents/skills`；Full 模式下选中 skills 时改为为每个 agent 创建 `<config_dir>/skills → ../.agents/skills` 软链接（跳过创建真实 skills 子目录），实现多 agent 共享。

Implemented end-to-end today: Agent sniff, MCP manage, Skills manage, Claude Env (multi `CLAUDE_CONFIG_DIR`), Codex Env (multi `CODEX_HOME`), OpenCode provider/model config, Project AI config (per-repo Full/Symlink skeleton under a picked folder; see `project_config.rs` / `PROJECT_AI_CONFIG_IMPROVEMENTS.md`), Preferences (theme), WebDAV, Backup manage (pack + multi-WebDAV upload; restore is future). See `BACKUP_MANAGE_PLAN.md`.

Path alias: `@/*` → `./src/*` (Vite + tsconfig).

Dev window: Overlay title bar; debug builds open DevTools in `setup`.

## Product constraints (from repo notes)

- Support macOS and Windows; keep platform branches in `platform.rs` / agent registry path layer — do not scatter `cfg!(windows)` through business logic
- Prefer user-global config roots; do not write project-level MCP as the default “apply” target
- Never put tokens/API keys into docs, commits, or sample configs
- Agent identity in code and UI is the sniff `name` string (e.g. `claude-code`)
- Reuse existing UI components and styles wherever possible (shared classes in `src/index.css`, design tokens like `--seed-*`, and patterns from sibling pages / `components/ui.tsx`) so the app keeps one consistent visual style. When a widget is used on more than one page, give it a page-neutral class name (e.g. `ui-check`) instead of hardcoding it into a single feature; only add net-new styles when nothing existing fits.

## Key files to read first

1. `src-tauri/src/lib.rs` — command surface
2. `src-tauri/src/agents.rs` — agent registry (identity + paths + MCP/skills mapping)
3. `src-tauri/src/sniff.rs` — how agents are discovered from the registry
4. `src-tauri/src/mcp_config.rs` + `AGENT_MCP_SKILLS_MAP.md` — MCP dialects and paths
5. `src/App.tsx` + relevant page under `src/components/pages/`
6. `src-tauri/src/db.rs` — schema and merge rules
