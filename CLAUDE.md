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
  pi_model_config.rs     Pi / Oh-My-Pi model config backends (~/.pi/agent/models.json / ~/.omp/agent/models.yml + auth.json)
  agent_model_config.rs  Common dispatch for agent model config (OpenCode / Pi / Oh-My-Pi) — `ModelConfigAgent` enum + parse/id
  agent_open.rs          Open-in-shell helpers for Agent 管理 card (config dir / MCP file / settings file — agent+kind only, no arbitrary paths)
  ai_provider.rs         AI provider registry (anthropic/openai) + encrypted API key storage + custom model list (`custom_models_json` 是对外可见模型的唯一来源)
  project_config.rs      Project-dir AI skeleton init (Full / Symlink modes) + optional project-level MCP (per-agent files, merge by title) and shared skills install into `.agents/skills` (copy/symlink)
  db.rs                  SQLite persistence (agents, mcp_servers, webdav_connections, skills, claude_environments, codex_environments, ai_providers, provider_route_toggle)
  config.rs              App config.json (theme; backup; network proxy; secretsKey private)
  http_client.rs         Shared reqwest client wired to `apply_proxy` from config.rs (WebDAV / Skills / MCP probe / OpenCode catalog / agent_open external url)
  crypto.rs              AES-256-GCM + HKDF for secret fields
  webdav.rs              WebDAV connections + probe + MKCOL/PUT upload
  backup.rs              Backup units probe, zip/abenc pack, multi-WebDAV upload (restore is future)

  route_agregation/     Passthrough local proxy aggregating AI providers (A→A + OR→OR; Phase 5 / 2026-08-13)
    mod.rs               Module surface (passthrough-only architecture note)
    server.rs            Axum HTTP server lifecycle (start/stop/config)
    router.rs           Route registration + request dispatch
    handler.rs           Request entry points (Claude messages / Codex responses)
    forwarder.rs        Request forwarding with failover + header injection + SSE passthrough
    provider_router.rs  Provider selection + circuit breaker management
    circuit_breaker.rs   Three-state circuit breaker (Closed/Open/HalfOpen)
    cloaking/            Claude Code rectifier + Codex CLI simulation (CLIProxyAPI aligned — see docs/SYNC_PLAYBOOK.md §2)
      claude_cloaking.rs claude_billing.rs claude_headers.rs claude_system_prompt.rs
      device_profile.rs obfuscate.rs tool_remap.rs header_scrub.rs
      codex_cloaking.rs codex_headers.rs
    config.rs            RouteAggregationConfig load/save (stored in config.json)
    types.rs             Data structures (DTOs for Tauri commands)
    log.rs / logfile.rs  In-memory ring buffer + on-disk tail-friendly log

AGENT_MCP_SKILLS_MAP.md  Canonical path/schema/dialect spec for MCP & Skills (mirror of agents.rs + mcp_config.rs + project_config.rs)
docs/SYNC_PLAYBOOK.md     CLIProxyAPI upstream sync playbook (cloaking 客户端指纹)
docs/cli_proxy_api_sync_state.json  Canonical sync state (passthrough_paths + client_fingerprint_anchors)
scripts/check_upstream_sync.py     Sync check script (exits 1 on drift > 14 days)
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
| Agents | `sniff_agents`, `get_cached_agents`, `add_agent_manual`, `agent_open_targets`, `open_agent_config_file`, `reveal_agent_config_dir` |
| Agent Model Config | `get_agent_model_config`, `get_agent_detail`, `get_agent_config_stats`, `set_agent_model_defaults`, `upsert_agent_provider`, `delete_agent_provider`, `get_agent_provider_secret`, `set_agent_provider_secret`, `upsert_agent_model`, `delete_agent_model`, `reveal_agent_model_config` |
| MCP | `apply_mcp_to_agents`, `remove_mcp_from_agents`, `sniff_mcp_servers`, `get_mcp_servers`, `save_mcp_servers`, `delete_mcp_server`, `test_mcp_connection` |
| App | `get_app_config`, `set_theme`, `set_window_appearance`, `get_network_settings`, `update_network_settings` |
| Skills | `list_skills`, `sniff_skills`, `preview_sniff_skills`, `import_sniffed_skills`, `check_skill_updates`, `check_skill_local_duplicate`, `add_skill_local`, `pick_and_add_skill_local`, `pick_skill_folder_path`, `add_skill_github`, `add_skill_gitcode`, `update_skill`, `update_skills_batch`, `export_skill_to_dir`, `check_export_duplicates`, `open_external_url`, `delete_skill`, `apply_skill_to_agents`, `batch_delete_skills`, `batch_export_skills_to_dir`, `batch_apply_skills_to_agents`, `batch_set_skill_tag`, `preview_cc_switch_skills`, `migrate_cc_switch_skills`, `sync_claude_env_skills`, `sync_codex_env_skills` |
| Claude Env | `list_claude_environments`, `sniff_claude_environments`, `import_claude_environment`, `clone_claude_environment`, `upsert_claude_environment`, `delete_claude_environment`, `install_claude_env_alias`, `remove_claude_env_alias`, `remove_all_claude_env_aliases`, `get_claude_env_shell_status`, `reveal_claude_env_dir`, `open_claude_env_settings`, `get_claude_env_secret`, `sync_claude_env_mcp`, `sync_all_claude_env_mcp`, `get_claude_env_mcp_status`, `fetch_claude_env_remote_models` |
| Codex Env | `list_codex_environments`, `sniff_codex_environments`, `import_codex_environment`, `clone_codex_environment`, `upsert_codex_environment`, `delete_codex_environment`, `install_codex_env_alias`, `remove_codex_env_alias`, `remove_all_codex_env_aliases`, `get_codex_env_shell_status`, `reveal_codex_env_dir`, `open_codex_env_config`, `get_codex_env_secret`, `sync_codex_env_mcp`, `sync_codex_env_mcp`, `sync_codex_env_skills`, `fetch_codex_env_remote_models` |
| OpenCode / Pi / Oh-My-Pi provider | `get_opencode_config`, `set_opencode_defaults`, `upsert_opencode_provider`, `delete_opencode_provider`, `upsert_opencode_model`, `delete_opencode_model`, `get_opencode_provider_secret`, `set_opencode_provider_secret`, `fetch_models_dev_catalog`, `probe_models_endpoint`, `reveal_opencode_config` |
| AI Providers | `list_ai_providers`, `upsert_ai_provider`, `delete_ai_provider`, `reorder_ai_providers`, `get_ai_provider_secret`, `get_ai_provider_secrets` |
| Project Config | `pick_project_folder`, `check_project_config_exists`, `init_project_config` |
| WebDAV | `get_webdav_connections`, `upsert_webdav_connection`, `delete_webdav_connection`, `test_webdav_connection`, `test_webdav_connection_draft` |
| Backup | `list_backup_units`, `list_remote_backups`, `restore_remote_backup`, `get_backup_settings`, `update_backup_settings`, `run_backup_upload` |
| Route Aggregation | `get_route_aggregation_config`, `update_route_aggregation_config`, `get_route_aggregation_status`, `get_route_aggregation_logs`, `clear_route_aggregation_logs`, `get_route_aggregation_log_file_path`, `reveal_route_aggregation_log_file`, `start_route_aggregation`, `stop_route_aggregation`, `toggle_provider_route`, `reset_circuit_breaker`, `get_route_provider_models`, `add_route_aggregation_api_key`, `delete_route_aggregation_api_key`, `regenerate_route_aggregation_api_key` |

`get_app_config` never returns `secretsKey`. Master key stays Rust-only via `config::load_secrets_key()`.
Agent open commands (`open_agent_config_file` / `reveal_agent_config_dir` / `agent_open_targets`)
accept only `name` + `ConfigFileKind` (`mcp` | `settings`) — never an arbitrary path from the UI — to keep
shell invocation constrained to the registry.

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

`codex`, `claude-code`, `claude-desktop`, `opencode`, `antigravity`, `codebuddy-cn`, `workbuddy`, `pi`, `oh-my-pi`

(removed: `kiro`, `codebuddy` 国际版 — 2026-08 收窄)

Rules:

- `found == true` only if an install path (App and/or CLI) exists; config dir alone is not enough
- CLI is resolved from static `bin_paths` + `PATH` (`search_names`); App paths sorted before CLI paths
- PATH/cache results filter out CLI shims (e.g. cmux under `$TMPDIR/cmux-cli-shims/`, other temp-dir wrappers) via `sniff::is_shim_path`; `get_cached_agents` also strips shim install paths on read so stale cache self-heals
- Codex CLI + ChatGPT App share `~/.codex`
- `codebuddy-cn` 标记 `shared_root: Some("codebuddy-shared")`（机制保留；国际版移除后当前无共享实例，未来加回时自动复用写盘）
- Claude Desktop config is scanned under `~/Library/Application Support/Claude*` for `claude_desktop_config.json` (`scan_app_support: true` / `McpPath::ClaudeDesktopScan`)
- `pi` / `oh-my-pi` 走 `McpDialect::JsonMcpServers`；MCP 文件 `~/.pi/agent/mcp.json` / `~/.omp/agent/mcp.json`；模型配置统一走 `agent_model_config::ModelConfigAgent` dispatcher

When adding a new agent or changing MCP/skills paths, update **`agents.rs` first** (and only extend `McpPath` / writers in `mcp_config` if a new path shape is needed). Keep `AGENT_MCP_SKILLS_MAP.md` in sync.

### MCP multi-dialect writer (`mcp_config.rs`)

Implements apply/remove/sniff against each agent’s user-global config. Spec source of truth: **`AGENT_MCP_SKILLS_MAP.md`**. Dialects (defined on `agents::McpDialect`):

| Dialect | Agents | File shape |
|---------|--------|------------|
| `TomlMcpServers` | codex | `~/.codex/config.toml` → `[mcp_servers.*]` |
| `ClaudeJsonUser` | claude-code | `~/.claude.json` 顶层 `mcpServers`（独立方言；文件位置特殊：不在 `~/.claude/` 目录下） |
| `JsonMcpServers` | claude-desktop, codebuddy-cn, workbuddy, pi, oh-my-pi | 顶层 `mcpServers` |
| `JsonMcp` | opencode | 顶层 `mcp`（JSON/JSONC） |
| `JsonGeminiMixed` | antigravity | `~/.gemini/settings.json` `mcpServers`，远程用 `httpUrl` |

Important behaviors:

- `dedupe_write_targets`: `shared_root` 相同的 agent 一次写入（当前仅 `codebuddy-cn` 标记）
- Writes are atomic (temp file + rename)
- JSONC (OpenCode) is parsed with `json5`; rewrite is pretty JSON (comments not preserved)
- Sniff merges disk entries into SQLite by title (case-insensitive); disk is source of truth for `appliedAgents`
- Internal smoke titles prefixed `__agentbuddy` are ignored on sniff
- `test_mcp_connection`: runtime probe only (stdio spawns the process + sends `initialize`; http/sse POSTs `initialize`); never writes config
- Frontend records `appliedAgents` from the **actually-succeeded** agents in the apply batch (partial failures no longer mark all as applied); the MCP list is DB-only (localStorage mirror removed)

### Local persistence

- **SQLite** `~/.agentbuddy/agents.db` tables: `agents`, `mcp_servers`, `webdav_connections`, `skills` (skill source metadata: local/github/gitcode, repo, commit refs), `claude_environments` (multi CLAUDE_CONFIG_DIR profiles), `codex_environments` (multi CODEX_HOME profiles), `ai_providers` (AI 供应商库：类型 anthropic/openai、base_url、加密 API Key、默认模型、Anthropic 档位模型 `models_json` {haiku,sonnet,opus,fable}；列表 DTO 只回 `hasApiKey`，明文经 `get_ai_provider_secret` 按需读取；本期仅管理、不下发到 Agent/环境), `provider_route_toggle` (route_aggregation provider × route_group 启停 + sort_order 持久化)
- **Skills files** `~/.agentbuddy/skills/<id>/SKILL.md` (content on disk; provenance in SQLite `skills`)
- **Config** `~/.agentbuddy/config.json`: public `theme`, `backup`, `network` (proxy mode: none|system|custom; custom supports http/socks5); private `secretsKey` (base64 32-byte master key). Outbound HTTP (WebDAV / Skills / MCP probe / OpenCode catalog / agent_open external url) goes through `http_client::apply_proxy`.
- WebDAV passwords: per-row salt/nonce/cipher via `crypto` (HKDF info `agentbuddy/webdav/v1`); never returned to UI
- route_aggregation 在内存中维护 `RouteAggregationState`（provider_router / config / log_store），route_group × provider 的启用状态持久化到 `provider_route_toggle`

### UI structure

`App.tsx` switches:

- **Main**: agent-sniff, mcp-manage, skills-manage, claude-env, codex-env, opencode-config, ai-providers, route-agregation, project-config, backup-manage
- **Settings**: preferences, network (proxy), webdav

`Agent 管理` card 调 `agent_open_targets` / `open_agent_config_file` / `reveal_agent_config_dir`（仅传 agent `name` + `ConfigFileKind`，绝不接受前端任意路径；防止 shell 被诱导打开非注册表位置）。

### Project AI config (`project_config.rs`)

Per-repo init for a picked folder: skeleton (Full / Symlink modes) plus two optional selections the UI offers from app-configured data:

- **MCP（可选）**：从 MCP 管理页的列表勾选，写入每个被勾选 agent 的**项目级** MCP 文件（`AGENT_SPECS[].mcp`）：claude-code / codebuddy-cn / workbuddy → 项目根 `.mcp.json`（同路径去重只写一次）；codex → `.codex/config.toml`；opencode → `opencode.json`；antigravity → `.gemini/settings.json`。写入复用 `mcp_config::apply_draft_to_file` 的方言写器，按 server title 合并（保留文件其它键），不受 overwrite 开关阻断、不计入冲突列表。**不支持**项目级的 agent：claude-desktop（桌面应用）、pi / oh-my-pi（暂未在 `AGENT_SPECS` 内，如需后续追加）。
- **Skills（可选）**：从技能库（`skills::library_skill_dir`）勾选，以「软链接 / 完整复制」安装到 `<repo>/.agents/skills/<id>`（overwrite 语义与骨架一致：非空真实目录拒绝删除）。Symlink 模式下 `<config_dir>/skills` 本已链接到 `.agents/skills`；Full 模式下选中 skills 时改为为每个 agent 创建 `<config_dir>/skills → ../.agents/skills` 软链接（跳过创建真实 skills 子目录），实现多 agent 共享。

`AGENT_SPECS` 表（项目级 + 全局唯一权威）：claude-code / codex / opencode / antigravity / codebuddy-cn / workbuddy 6 项；其他 agent 故意不纳入（参见 `project_config.rs` 头部注释）。

### Route Aggregation（`route_agregation/`，Phase 5 2026-08-13）

本地 proxy 聚合多个 AI 供应商，单端点 + 自动 failover + 请求 cloaking。

**仅 passthrough**——A→A（Claude Code → Anthropic 兼容 provider）+ OR→OR（Codex CLI → OpenAI Responses 兼容 provider）。路由聚合不做协议翻译。

| 模块 | 职责 |
|------|------|
| `server` | Axum HTTP server lifecycle (start/stop/config) |
| `router` | Route 注册 + 请求分派 |
| `handler` | 请求入口（Claude `/v1/messages` / Codex `/v1/responses`） |
| `forwarder` | 请求转发 + failover + header 注入 + SSE passthrough |
| `provider_router` | provider 选择 + circuit breaker 管理；`build_pool_from_db` 同步填 `supported_model_ids` |
| `circuit_breaker` | Closed / Open / HalfOpen 三态断路器；`reset_circuit_breaker` 命令手动复位 |
| `cloaking/` | Claude Code rectifier + Codex CLI 模拟（仿照 CLIProxyAPI `internal/runtime/executor/`）；CLAUDE.md / SYNC_PLAYBOOK §2 追踪同步状态 |
| `config` / `types` | 配置 load/save（写入 config.json）+ DTO |
| `log` / `logfile` | 内存 ring buffer + 磁盘 tail-friendly 日志 |

**Auth & access control**：本地监听支持 `add_route_aggregation_api_key` / `delete_route_aggregation_api_key` / `regenerate_route_agregation_api_key`（`crypto` 加密落库）；启动时 Axum 通过 middleware 校验请求头 `X-AgentBuddy-Key`。

**上游同步**：详见 [`docs/SYNC_PLAYBOOK.md`](docs/SYNC_PLAYBOOK.md) + [`docs/cli_proxy_api_sync_state.json`](docs/cli_proxy_api_sync_state.json)（passthrough_paths + client_fingerprint_anchors）。`scripts/check_upstream_sync.py` 每周一跑，cloaking 客户端指纹漂移 >14d 退出码 1。

Implemented end-to-end today: Agent sniff, MCP manage, Skills manage, Claude Env (multi `CLAUDE_CONFIG_DIR`), Codex Env (multi `CODEX_HOME`), OpenCode provider/model config, Project AI config (per-repo Full/Symlink skeleton under a picked folder; see `project_config.rs` / `PROJECT_AI_CONFIG_IMPROVEMENTS.md`), Preferences (theme, window appearance), WebDAV, Backup manage (pack + multi-WebDAV upload; restore via `restore_remote_backup`), Route Aggregation (passthrough + cloaking). See `BACKUP_MANAGE_PLAN.md` / `WINDOWS_ADAPTATION_PLAN.md`.

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
5. `src-tauri/src/project_config.rs` — project-level skeleton + MCP (AGENT_SPECS table)
6. `src-tauri/src/ai_provider.rs` + `src-tauri/src/opencode_config.rs` + `src-tauri/src/pi_model_config.rs` + `src-tauri/src/agent_model_config.rs` — provider / model config (routed through `agent_model_config::ModelConfigAgent`)
7. `src-tauri/src/claude_env.rs` + `src-tauri/src/codex_env.rs` — multi-env management + MCP / skills sync
8. `src-tauri/src/route_agregation/mod.rs` + `docs/SYNC_PLAYBOOK.md` — local proxy aggregation (passthrough + cloaking)
9. `src/App.tsx` + relevant page under `src/components/pages/`
10. `src-tauri/src/db.rs` — schema and merge rules

### AI 供应商自定义模型列表规则

软件内**已配置** AI 供应商（`ai_providers` 表）的 `custom_models_json` 是其对外可见模型的
**唯一来源**；即使该列表为空，也**不**向供应商远端 /v1/models 或 /models 发起任何请求。
涉及的下游消费者：

- 路由聚合 provider 详情面板的"模型列表"：仅来自 `customModels`
- 路由聚合内存池的 `supported_model_ids`：在 `build_pool_from_db` 中同步填好（自定义列表非空→`Some(list)`，空→`None` 走无过滤 failover）
- 路由聚合本地 `GET /v1/models` handler 的并集：仅取所有启用 provider 的 `customModels` 并集
- 后端命令 `get_route_provider_models`：直接读 DB 返回 `customModels`

#### 例外：AI 供应商编辑页的"填表工具"

AI 供应商编辑/新建弹窗的"从端点拉取"按钮可以远端拉一次模型列表（仅编辑期使用）：
- 命令：`fetch_claude_env_remote_models`（复用，与 Claude 环境共用）
- 用途：拉取结果作为多选面板的候选集，用户勾选后通过 `applyPickerSelection` 把所选
  模型写到 `formCustomModels`，最终由 `invokeUpsert` 落 DB
- 保存后再次打开表单、运行时读取、各下游消费者：仍然**只读** `customModels`，
  **不**再向供应商端点发起任何请求

### "临时配置"路径（不受上述规则约束）

以下场景下用户**不**关联 AI 供应商库，而是手填 baseUrl + apiKey，"模型列表"走远端拉取：

- **Claude 环境**：若未关联 AI 供应商，"默认模型"下拉候选由"拉取列表"按钮调用
  `fetch_claude_env_remote_models` 远端拉取 `v1/models`。
- **Codex 环境**：同上，命令 `fetch_codex_env_remote_models`。
- **OpenCode / Pi / Oh-My-Pi provider**：模型弹窗的"从供应商模型列表选择"按钮调用
  `fetch_claude_env_remote_models`（复用 Claude env 的同一条命令）。

优先级：上述场景下若 env / provider 关联了 AI 供应商，仍优先使用 `customModels`；
仅在"无关联 + 手填 baseUrl"时退回远端拉取。

保留的远端 I/O（不属于上述任何规则约束）：

- `fetch_models_dev_catalog`：Models.dev 全局目录元数据（OpenCode 配置页参考数据）
- `probe_models_endpoint`：未保存 BaseURL 的一次性诊断 probe
- `skills.rs` 的 GitHub/GitCode 提交元数据拉取
- `webdav.rs` 的 WebDAV 备份上传
- `mcp_config.rs::test_mcp_connection` 的 MCP server probe
- `route_aggregation::forwarder` 透传实际的 chat 请求（不是 list 调用）
