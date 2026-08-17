# AgentBuddy — 各 Agent MCP / Skills 配置方案

> **范围**：macOS / Windows 全局配置 + 项目级配置（与当前代码一致）
> **修订日期**：2026-08-14（v3）
> **目的**：作为 `agents.rs` / `mcp_config.rs` / `skills.rs` / `project_config.rs`
> 的**人类可读 mirror**，说明每个 Agent 的路径、方言、读写策略。
>
> **状态**：**全部已实现**——本文不是方案/研究稿，而是当前落盘代码的规格说明。
> 改 agents 路径或方言时，**先改 `agents.rs`**，再同步本文。
>
> **修订摘要（相对 v2 2026-07-14）**：
> - 新增 Agent：`pi` / `oh-my-pi`（can1357 系列）
> - 移除 Agent：`kiro`（不再支持）、`codebuddy` 国际版（仅保留 `codebuddy-cn`）
> - McpDialect 收紧：删除 `kiro` 对应的 `JsonMcpServers` 兜底，更新方言枚举
> - 项目级 MCP 写入：经 `project_config::init_project_config` 的 `AGENT_SPECS`
>   落地（不再分散在适配器代码里）
> - §10 本机校验摘要删除（机器相关，迁入测试 fixtures）

---

## 1. 安全与写作约定

1. 本文只记录**路径、字段名、schema、合并策略**。
2. **禁止**写入 token、API Key、密码、本机完整密钥配置。
3. Agent 标识统一使用 `agents.rs` 中的 `name` 字段（如 `claude-code`）。
4. 「全局应用」默认写**用户级**配置；项目级路径见 §8。
5. 文件写入统一走 **temp file + rename**（原子写），解析保留原 JSON 其它顶层键。

---

## 2. 与嗅探的对齐

| # | `name` | `display_name` | 嗅探 config root | `scan_app_support` |
|---|--------|----------------|------------------|--------------------|
| 1 | `codex` | Codex | `~/.codex` | false |
| 2 | `claude-code` | Claude Code | `~/.claude`（MCP 主存 `~/.claude.json`，**不**在 `~/.claude/` 内） | false |
| 3 | `claude-desktop` | Claude Desktop | `~/Library/Application Support/Claude*`（扫描含 `claude_desktop_config.json` 的目录） | true |
| 4 | `opencode` | OpenCode | `~/.config/opencode` | false |
| 5 | `antigravity` | Antigravity | `~/.gemini` | false |
| 6 | `codebuddy-cn` | CodeBuddy CN | `~/.codebuddy`（独占，国际版已移除） | false |
| 7 | `workbuddy` | WorkBuddy | `~/.workbuddy` | false |
| 8 | `pi` | Pi | `~/.pi` | false |
| 9 | `oh-my-pi` | Oh-My-Pi | `~/.omp`（CLI `omp`，Pi 的全功能 fork） | false |

规则（与 `sniff.rs` / `agents.rs` 一致）：

- `found == true` 仅当 App 或 CLI 二者至少其一存在；config dir 单独存在不算
- CLI 解析：静态 `bin_paths` + `PATH`（`search_names`）；App 路径排在 CLI 前
- PATH / cache 解析会过滤 `cmux` 等 temp-dir 下的 shim 包装
- Codex CLI + ChatGPT App **共享** `~/.codex`
- Claude Desktop config 用 `scan_app_support` 在 `~/Library/Application Support` 扫 `claude_desktop_config.json`
- `codebuddy-cn` 标记 `shared_root: Some("codebuddy-shared")`（保留机制以备未来加回国际版）

---

## 3. 统一方言 Taxonomy

后续适配器按 `McpDialect` / `McpPath` 分发（见 `agents.rs`），不再有 if-else 复制逻辑。

### 3.1 MCP 方言（`agents::McpDialect`）

| variant | 顶层键 | 文件形态 | 代表 Agent |
|---------|--------|----------|------------|
| `TomlMcpServers` | `[mcp_servers.*]` | TOML | `codex` |
| `ClaudeJsonUser` | `mcpServers` | JSON | `claude-code`（独立方言，因为文件位置特殊：`~/.claude.json` 而非 `~/.claude/*.json`） |
| `JsonMcpServers` | `mcpServers` | JSON / JSONC（按 spec） | `claude-desktop` / `codebuddy-cn` / `workbuddy` / `pi` / `oh-my-pi` |
| `JsonMcp` | `mcp`（**不是** `mcpServers`） | JSON / JSONC（`json5` 解析） | `opencode` |
| `JsonGeminiMixed` | `mcpServers`，远程用 `httpUrl`（非 `url`） | JSON | `antigravity` |

### 3.2 MCP 路径（`agents::McpPath`）

| variant | 含义 | 用到 |
|---------|------|------|
| `Fixed(&'static str)` | 相对用户主目录的固定路径，如 `.codex/config.toml` | `codex` / `claude-code` / `antigravity` / `workbuddy` / `pi` / `oh-my-pi` |
| `OpencodeConfig` | `~/.config/opencode/opencode.{jsonc,json}`（存在优先，缺省 `.json`） | `opencode` |
| `CodebuddyMcp` | `~/.codebuddy/{.mcp.json,mcp.json}`（存在优先，缺省 `.mcp.json`） | `codebuddy-cn` |
| `ClaudeDesktopScan` | `~/Library/Application Support` 下扫 `claude_desktop_config.json` | `claude-desktop` |

### 3.3 Skills 根（来自 `agents::skills_roots`）

| Agent | Skills roots | 是否支持 |
|-------|--------------|----------|
| `codex` | `~/.codex/skills`, `~/.agents/skills` | ✅ |
| `claude-code` | `~/.claude/skills` | ✅ |
| `claude-desktop` | — | ❌（云端 VM，无本地目录；客观限制） |
| `opencode` | `~/.config/opencode/skills` | ✅ |
| `antigravity` | `~/.gemini/skills`, `~/.gemini/antigravity-cli/skills` | ✅ |
| `codebuddy-cn` | `~/.codebuddy/skills` | ✅ |
| `workbuddy` | `~/.workbuddy/skills` | ✅ |
| `pi` | `~/.pi/agent/skills` | ✅ |
| `oh-my-pi` | `~/.omp/agent/skills` | ✅ |

### 3.4 写策略（已实现于 `mcp_config.rs`）

| 行为 | 说明 |
|------|------|
| 合并方式 | JSON 对象按 server `title` 合并；TOML 表按 `mcp_servers.<title>` 合并 |
| 其它键 | 保留（不动文件其它顶层字段，如 Claude Code `~/.claude.json` 的大状态文件） |
| 原子写 | temp file + rename（避免半截写入） |
| 共享根去重 | `shared_root` 相同的 agent 一次写入（如未来加回 `codebuddy` 国际版时自动复用 `codebuddy-cn` 写盘） |
| JSONC | OpenCode 用 `json5` 解析；重写为 pretty JSON（注释不保留） |
| Sniff 合并 | 磁盘优先（`appliedAgents` 反映磁盘实际状态）；`__agentbuddy` 前缀的内置冒烟 server 忽略 |

---

## 4. 总览对照表

| sniff `name` | MCP 文件 | 方言 | Skills 根 |
|--------------|----------|------|-----------|
| `codex` | `~/.codex/config.toml` | `TomlMcpServers` | `~/.codex/skills` + `~/.agents/skills` |
| `claude-code` | `~/.claude.json` 顶层 `mcpServers` | `ClaudeJsonUser` | `~/.claude/skills` |
| `claude-desktop` | `~/Library/Application Support/Claude*/claude_desktop_config.json` | `JsonMcpServers` | — |
| `opencode` | `~/.config/opencode/opencode.{jsonc,json}` | `JsonMcp` | `~/.config/opencode/skills` |
| `antigravity` | `~/.gemini/settings.json` | `JsonGeminiMixed` | `~/.gemini/skills` + `~/.gemini/antigravity-cli/skills` |
| `codebuddy-cn` | `~/.codebuddy/{.mcp.json,mcp.json}`（存在优先） | `JsonMcpServers` | `~/.codebuddy/skills` |
| `workbuddy` | `~/.workbuddy/.mcp.json` | `JsonMcpServers` | `~/.workbuddy/skills` |
| `pi` | `~/.pi/agent/mcp.json` | `JsonMcpServers` | `~/.pi/agent/skills` |
| `oh-my-pi` | `~/.omp/agent/mcp.json` | `JsonMcpServers` | `~/.omp/agent/skills` |

---

## 5. 分 Agent 详解

### 5.1 Codex (`codex`)

**配置根**：`~/.codex`（CLI 与 ChatGPT App 共享）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.codex/config.toml` |
| 方言 | `TomlMcpServers` |
| 结构 | `[mcp_servers.<server-name>]` |
| stdio | `type = "stdio"`，`command`，`args`，可选 `enabled`/`cwd`/`startup_timeout_sec`；env 子表 `[mcp_servers.<name>.env]` |
| http | `type = "http"`，`url`（+ 官方 header/auth 键） |
| CLI | `codex mcp add` / `codex mcp list` |

**Skills**

- 本机：`~/.codex/skills/<skill>/SKILL.md`
- 官方：`~/.agents/skills`、仓库 `.agents/skills`
- `config.toml` 中可有 `[[skills.config]]`（path / enabled）
- 全局 `~/.codex/AGENTS.md`（不是 Skill，但是配置根一部分）

**多环境**：`codex_env.rs` 通过 `CODEX_HOME` 隔离 config root；同步 MCP 时默认 home 的
`~/.codex/config.toml` → 各 `$CODEX_HOME/config.toml`，**替换** `[mcp_servers]`，保留其它键。

### 5.2 Claude Code (`claude-code`)

**配置根**：`~/.claude`（settings / skills / agents）
**MCP 状态文件**：`~/.claude.json`（**不**在 `~/.claude/` 目录内，路径不对称）

**MCP**

| Scope | 路径 | 说明 |
|-------|------|------|
| User（全局，默认应用目标） | `~/.claude.json` 顶层 `mcpServers` | 所有项目可见 |
| Local（单项目、个人） | `~/.claude.json` → `projects["/abs/path"].mcpServers` | `claude mcp add` 默认 scope |
| Project（团队共享） | 项目根 `.mcp.json` | git 可提交；需信任/批准 |

| 项 | 值 |
|----|-----|
| 方言 | `ClaudeJsonUser`（独立 dialect，因为文件位置特殊） |
| stdio | `command` + `args` + 可选 `env`；可带 `type: "stdio"` |
| http/sse | `type: "http"` / `"sse"` + `url` + 可选 `headers` |
| CLI | `claude mcp add [--scope user\|project\|local] ...` |

**Skills**

| 项 | 值 |
|----|-----|
| 用户 | `~/.claude/skills/<skill-name>/SKILL.md` |
| 项目 | `.claude/skills/<skill-name>/SKILL.md` |
| 格式 | 目录 + 必需 `SKILL.md`（YAML frontmatter + Markdown） |

**多环境**：`claude_env.rs` 通过 `CLAUDE_CONFIG_DIR` 隔离 config root；同步 MCP 时默认 home 的
`~/.claude.json` → 各 `$CLAUDE_CONFIG_DIR/.claude.json`，**替换** 顶层 `mcpServers`，保留其它键。
每 env 的 `settings.json` 写 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL`
（+ 配套 default-model 键）。

### 5.3 Claude Desktop (`claude-desktop`)

**配置根**：在 `~/Library/Application Support` 下扫 `Claude*/claude_desktop_config.json`

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `…/Claude/claude_desktop_config.json`（多实例时全部扫描） |
| 方言 | `JsonMcpServers` |
| 结构 | 顶层 `mcpServers`；stdio：`command` / `args` / `env` |
| 生效 | 通常需**完全退出并重启** Desktop |

**Skills**

- Claude Desktop 属于官方 "Claude apps"（claude.ai 桌面壳），Skills 经**应用内 Settings → Features 上传 zip**、绑定账号、**云端 VM 执行**——**没有**本地磁盘 Skills 目录。
- `agents.rs` 中 `skills_supported: false`，前端 `SKILL_UNSUPPORTED_AGENTS` 同步置灰。
- 这是**客观限制**（云端执行而非本地），不是缺口。

### 5.4 OpenCode (`opencode`)

**配置根**：`~/.config/opencode`（**不是** `~/.opencode` 安装目录）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.config/opencode/opencode.json`（或 `opencode.jsonc`） |
| 方言 | `JsonMcp` |
| 顶层键 | **`mcp`**（不是 `mcpServers`） |
| local | `"type": "local"`，`command` 是**字符串数组**（含 args），`environment`，`enabled` |
| remote | `"type": "remote"`，`url`，`headers`，`enabled` |
| schema | `"$schema": "https://opencode.ai/config.json"` |

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.config/opencode/skills/<name>/SKILL.md` |
| 项目 | `.opencode/skills/` 等 |
| 权限 | 可在 `opencode.json` 的 `skill` 字段做 allow/deny |

**模型配置**：`opencode_config.rs` 提供 `get_agent_model_config` / `upsert_agent_model` /
`upsert_agent_provider` 等命令；`~/.config/opencode/opencode.json` 的 `provider` / `model`
条目与 `auth.json`（API Key）独立维护。

### 5.5 Antigravity (`antigravity`)

**配置根**：`~/.gemini`（Gemini CLI → Antigravity 迁移中；以**本机实测 + 官方迁移文档**双轨兼容）

**MCP**

| 优先级 | 路径 | 说明 |
|--------|------|------|
| 主写（本机已验证） | `~/.gemini/settings.json` → `mcpServers` | stdio：`command`+`args`+`env`；远程：`httpUrl`（非 `url`） |
| 兼容读 | `~/.gemini/config/mcp_config.json` | 迁移文档中的独立 MCP 配置；本机可能为空文件 |
| CLI 设置 | `~/.gemini/antigravity-cli/settings.json` | 模型/权限等，**非**本机 MCP 主存 |

**Skills**

| 项 | 值 |
|----|-----|
| 常见全局 | `~/.gemini/skills/` |
| 迁移文档 | `~/.gemini/antigravity-cli/skills/`、`.agents/skills/` |
| 推荐 | 写优先 `~/.gemini/skills/`；读时兼容 antigravity-cli 路径 |

### 5.6 CodeBuddy CN (`codebuddy-cn`)

**配置根**：`~/.codebuddy`（独占，国际版已移除）

**MCP**

| 项 | 值 |
|----|-----|
| 推荐 | `~/.codebuddy/.mcp.json` |
| 兼容/本机 | `~/.codebuddy/mcp.json`（旧/现用） |
| 方言 | `JsonMcpServers` |
| 项目 | 项目根 `.mcp.json` |
| 支持 | stdio / sse / http；JSONC；`${ENV}` 展开 |
| CLI | `codebuddy mcp add ...` |

**Skills**

| 项 | 值 |
|----|-----|
| 用户 | `~/.codebuddy/skills/<name>/SKILL.md` |
| 项目 | `.codebuddy/skills/` |
| 市场 | `~/.codebuddy/skills-marketplace/`（安装源，不等于用户 skills 根） |

`shared_root: Some("codebuddy-shared")` — 当前无国际版共享实例；保留机制以备未来扩展。

### 5.7 WorkBuddy (`workbuddy`)

**配置根**：`~/.workbuddy`（与 CodeBuddy CN 分离）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.workbuddy/.mcp.json`（本机已验证） |
| 方言 | `JsonMcpServers` |
| 注意 | 合并时**保留**产品自带 server（如 `connector-proxy` 一类），禁止整体覆盖文件 |

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.workbuddy/skills/` |
| 项目 | `.workbuddy/skills/` |
| 格式 | 目录 + `SKILL.md` |

### 5.8 Pi (`pi`)

**配置根**：`~/.pi`

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.pi/agent/mcp.json` |
| 方言 | `JsonMcpServers` |
| 结构 | 顶层 `mcpServers`；stdio：`command` + `args` + `env`；远程：`type` + `url` |

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.pi/agent/skills/<name>/SKILL.md` |

**模型配置**（`pi_model_config.rs` 后端）：

| 项 | 值 |
|----|-----|
| 模型文件 | `~/.pi/agent/models.json`（JSON，顶层 `providers`） |
| 密钥文件 | `~/.pi/agent/auth.json`（`{ providerId: { type: "api_key", key } }`） |
| 默认模型 | `~/.pi/agent/settings.json` 的 `defaultProvider` / `defaultModel` |

字段映射到通用 DTO：`contextWindow`→`limitContext`、`maxTokens`→`limitOutput`、
`input`→`modalitiesInput`；未建模字段进 `extraOptions` 原样往返。列表 DTO **永不**回传明文 API Key。

### 5.9 Oh-My-Pi (`oh-my-pi`)

**配置根**：`~/.omp`（CLI 名称 `omp`，Pi 的全功能 fork）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.omp/agent/mcp.json` |
| 方言 | `JsonMcpServers` |

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.omp/agent/skills/<name>/SKILL.md` |

**模型配置**（`pi_model_config.rs` 后端，`omp` 别名接受）：

| 项 | 值 |
|----|-----|
| 模型文件 | `~/.omp/agent/models.yml`（YAML；兼容读 `.yaml` 与旧版 `models.json`） |
| 密钥配置 | `~/.omp/agent/models.yml` / `models.yaml` 中 provider 的 `apiKey`；旧版 `auth.json` 仅用于一次性迁移 |
| 默认模型 | 暂不支持可视化编辑（在 omp 内用 `/model` 或 `omp config` 管理） |

---

## 6. AgentBuddy 表单 → 各方言字段映射

UI 模型（`McpManage.tsx`）：

- `title`
- `type`: `stdio` | `http` | `sse`
- `command` / `args[]` / `env{}`
- `url` / `headers{}`

| UI | `ClaudeJsonUser` / `JsonMcpServers`（Claude Code / Desktop / CodeBuddy CN / WorkBuddy / Pi / Oh-My-Pi） | `JsonMcp`（OpenCode） | `TomlMcpServers`（Codex） | `JsonGeminiMixed`（Antigravity） |
|----|------------------------------------------------------|------------------------------|-----------------------------|---------------------|
| title | `mcpServers[title]` | `mcp[title]` | `mcp_servers.title` | `mcpServers[title]` |
| stdio | `command` + `args` + `env`；可选 `type: "stdio"` | `type: "local"` + `command: [cmd, ...args]` + `environment` | `type="stdio"` + `command` + `args` + `[.env]` | `command` + `args` + `env` |
| http | `type: "http"` + `url` + `headers` | `type: "remote"` + `url` + `headers` | `type="http"` + `url`（headers 按官方键） | **`httpUrl`**（非 `url`） |
| sse | `type: "sse"` + `url` + `headers` | 映射为 `remote`（或文档等价） | 若官方仅 http，降级为 http 或跳过并提示 | 优先 `httpUrl`；需实测 |
| enabled | 可选 | 常写 `enabled: true` | `enabled = true` | 无统一字段时省略 |

---

## 7. 全局应用 / 删除 — 统一算法（已实现）

实现入口：`mcp_config::apply_draft_to_file` / `remove_from_file`，按 `McpDialect` 分派到对应写器。

### 7.1 Apply（保存并应用）

```
输入: McpDraft, selectedAgentNames[]
for agent in selectedAgents:
  spec    = agents::find(agent.name)               # AgentSpec + McpSpec
  path    = spec.mcp.path.resolve(...)             # 固定路径 / scan / 优先选择
  dialect = spec.mcp.dialect
  doc     = read_json_or_toml(path) or empty_doc(dialect)
  entry   = draft_from_ui(dialect, draft)          # 字段映射（见 §6）
  doc     = upsert_by_title(doc, draft.title, entry, dialect)
  atomic_write(path, doc)                          # temp + rename
返回值: List<{agent, path, ok, message}>           # 仅 ok=true 的写 appliedAgents
```

原则：

1. **只写用户全局**（见 §4 总览表）
2. **同名覆盖，异名保留** — 不动文件中其它顶层键
3. **原子写**，避免半截 JSON/TOML
4. 共享物理根的多个 agent 同时勾选 → `dedupe_write_targets` 一次写入
5. Claude Code → 只动 `~/.claude.json` 的 `mcpServers`，不动其它大字段
6. **partial success 透明**：apply 失败的部分不再把该 agent 标记为已应用
7. `test_mcp_connection` 仅为运行时 probe（stdio 拉起进程 + 发送 `initialize`；
   http/sse POST `initialize`），**永不写** 配置文件

### 7.2 Delete（列表删除 + 可选同步配置）

```
if removeFromAgentConfigs:
  for agent in server.appliedAgents:
    spec = agents::find(agent)
    path = spec.mcp.path.resolve(...)
    doc  = read(path)
    doc  = remove_by_title(doc, server.title, dialect)
    atomic_write(path, doc)
从 SQLite 移除 server 行（不再有 localStorage 镜像）
```

原则：

1. 仅删除目标 server name
2. 不删除整个配置文件
3. WorkBuddy / Claude Desktop 等可能含非用户 server，禁止清空 `mcpServers`

---

## 8. 项目级 MCP / Skills（项目 AI 配置页）

「项目 AI 配置」初始化（`init_project_config`）时，用户可从 AgentBuddy 已配置的 MCP / Skills 中勾选，
落盘到所选项目目录（与全局应用互不影响）。

### 8.1 项目级 MCP 目标文件（来自 `project_config::AGENT_SPECS`）

| sniff `name` | 项目级 MCP 文件（相对项目根） | 方言 |
|--------------|-------------------------------|------|
| `claude-code` | `.mcp.json` | `JsonMcpServers` |
| `codebuddy-cn` | `.mcp.json`（与 claude-code 同路径，去重只写一次） | `JsonMcpServers` |
| `workbuddy` | `.mcp.json`（同上） | `JsonMcpServers` |
| `codex` | `.codex/config.toml` | `TomlMcpServers` |
| `opencode` | `opencode.json` | `JsonMcp` |
| `antigravity` | `.gemini/settings.json` | `JsonGeminiMixed` |
| `claude-desktop` | — | ❌（桌面应用，不做项目级） |
| `pi` | `.pi/agent/mcp.json` | `JsonMcpServers` |
| `oh-my-pi` | `.omp/agent/mcp.json` | `JsonMcpServers` |

**写策略**：复用 `mcp_config::apply_draft_to_file` 的方言写器，**按 server title 合并**、保留文件其它键、
原子写；不参与「覆盖/跳过」确认。

### 8.2 项目级 Skills

- 统一安装到 `<repo>/.agents/skills/<id>`（`skill-copy-or-symlink`：完整复制或软链接，源为
  `~/.agentbuddy/skills` 技能库）。
- 多 agent 共享：
  - **Symlink 模式**：`<config_dir>/skills` 本已链接至 `.agents/skills`（每个被勾选的 agent 都享受）。多级目录使用对应层级的相对路径。
  - **Full 模式**：勾选 skills 时为每个 agent 创建指向项目根 `.agents/skills` 的相对软链接（例如 `.pi/agent/skills → ../../.agents/skills`，跳过创建真实 skills 子目录），实现多 agent 共享。
- 安全语义：非空真实目录拒绝删除；overwrite 可替换软链接/空目录。

### 8.3 不支持项目级

- `claude-desktop` — 桌面应用，不写项目树
- `codebuddy`（国际版）— 已移除支持

---

## 9. 风险与决策（当前状态）

| 项 | 状态 | 处理 |
|----|------|------|
| Claude Desktop Skills | 客观不支持（云端 VM） | Skills 页置灰；只做 MCP |
| Antigravity 双路径 | 迁移期 | 主写 `settings.json`；读兼容 `mcp_config.json` |
| Gemini `httpUrl` | 与 UI `url` 字段名不一致 | `JsonGeminiMixed` 写器强制转换；headers 需实测 |
| Codex Skills 双路径 | `~/.codex/skills` vs `~/.agents/skills` | 写本机已存在者；两者都无则写 `~/.codex/skills` |
| Claude Code 大文件 | `~/.claude.json` 含大量状态字段 | 只改 `mcpServers` 键，独立方言 `ClaudeJsonUser` |
| 项目级 MCP | 写项目会进 git / 需信任 | 全局应用默认不写项目；项目级写入由用户在「项目 AI 配置」页显式勾选触发 |
| SSE 方言差异 | 各产品支持不一 | 能映射则映射，否则提示跳过 |
| 密钥 | 配置中常含 token | AgentBuddy 永不日志打印 env/headers 值；API Key 经 `crypto::encrypt` 落 SQLite |
| OpenCode JSONC | 注释 / 尾逗号 | `json5` 解析；重写 pretty JSON（注释不保留，会抛 warn 给用户） |
| partial failure | apply 批次中部分 agent 失败 | UI 只把 ok=true 的 agent 写进 `appliedAgents` |
| Pi / Oh-My-Pi 模型配置 | YAML 解析（omp）+ JSON 解析（pi） | 字段映射到统一 DTO，列表 DTO 不回传明文 Key |
| 内部冒烟 server | 嗅探时混入 | 嗅探忽略 `__agentbuddy` 前缀 title |

---

## 10. 来源索引

### 官方 / 准官方

- Claude Code MCP / Settings / Skills：https://code.claude.com/docs/en/mcp 、https://code.claude.com/docs/en/settings 、https://code.claude.com/docs/en/skills
- Claude Desktop MCP：https://modelcontextprotocol.io/docs/develop/connect-local-servers
- Codex Config / MCP / Skills / AGENTS.md：https://developers.openai.com/codex/config-basic 、https://developers.openai.com/codex/extend/mcp 、https://developers.openai.com/codex/skills 、https://developers.openai.com/codex/guides/agents-md
- OpenCode Config / MCP / Skills：https://opencode.ai/docs/config/ 、https://opencode.ai/docs/mcp-servers/ 、https://opencode.ai/docs/skills/
- Antigravity / 迁移：https://antigravity.google/docs/cli-features 、https://antigravity.google/docs/gcli-migration
- CodeBuddy CN MCP / Skills：https://www.codebuddy.ai/docs/cli/mcp 、https://www.codebuddy.ai/docs/cli/skills
- Pi / Oh-My-Pi：https://github.com/badlogic/pi-mono 、https://github.com/can1357/oh-my-pi

### 工程内对照

- 嗅探定义 / Agent 注册表：[`src-tauri/src/agents.rs`](src-tauri/src/agents.rs) （`AgentSpec` / `McpDialect` / `McpPath`）
- 嗅探实现：[`src-tauri/src/sniff.rs`](src-tauri/src/sniff.rs)
- MCP 写器（apply / remove / sniff）：[`src-tauri/src/mcp_config.rs`](src-tauri/src/mcp_config.rs)
- Skills 库：[`src-tauri/src/skills.rs`](src-tauri/src/skills.rs)
- 项目级骨架 + 项目级 MCP：[`src-tauri/src/project_config.rs`](src-tauri/src/project_config.rs) （`AGENT_SPECS`）
- MCP UI 模型：[`src/components/pages/McpManage.tsx`](src/components/pages/McpManage.tsx)
- 项目级 UI：[`src/components/pages/ProjectConfig.tsx`](src/components/pages/ProjectConfig.tsx) +
  [`src/components/pages/project-config/types.ts`](src/components/pages/project-config/types.ts) （`AGENT_PROJECT_INFOS`，与 `project_config.rs` 同步）

---

## 11. 结论（可执行摘要）

1. **5 种 MCP 方言**覆盖 9 个 Agent：`TomlMcpServers` / `ClaudeJsonUser` / `JsonMcpServers` /
   `JsonMcp` / `JsonGeminiMixed`（其中 `ClaudeJsonUser` 为 Claude Code 独立方言，因其文件位置
   `~/.claude.json` 特殊）。
2. **9 个 Agent**（2026-08）：`codex` / `claude-code` / `claude-desktop` / `opencode` /
   `antigravity` / `codebuddy-cn` / `workbuddy` / `pi` / `oh-my-pi`。`kiro` 与 `codebuddy`
   国际版已移除。
3. **Skills** 主流是 `<root>/skills/<name>/SKILL.md`；Claude Desktop 暂不支持（云端 VM）；
   Codex 需兼容 `~/.agents/skills` 与 `~/.codex/skills` 双路径。
4. **全局应用**默认只写用户级文件；Claude Code 写 `~/.claude.json` 顶层 `mcpServers`，
   Antigravity 远程用 `httpUrl` 而非 `url`。
5. **项目级** MCP 由 `project_config::AGENT_SPECS` 决定落地路径；目前支持 8 个 agent
   （claude-code / codebuddy-cn / workbuddy / codex / opencode / antigravity / pi / oh-my-pi）。
6. **OpenCode** 用 `mcp` 顶层键 + `local`/`remote`，不是 `mcpServers`；JSONC 用 `json5`。
7. **Pi / Oh-My-Pi** 用 `JsonMcpServers` 方言；模型配置经通用 `agent_model_config` + 各自
   后端（`opencode_config` / `pi_model_config`）。
8. **改动路径**：改 agents / 方言 → 先改 `agents.rs` → 同步本文件 + `AGENT_PROJECT_INFOS`
   （前端 mirror）。
