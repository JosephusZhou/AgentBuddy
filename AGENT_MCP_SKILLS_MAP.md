# AgentBuddy — 各 Agent MCP / Skills 配置方案

> **范围**：macOS 全局配置（与当前 `sniff.rs` 一致）  
> **日期**：2026-07-14  
> **目的**：为「应用到 Agent / 删除时同步改配置文件 / Skills 管理」提供准确落盘规格  
> **本阶段**：仅研究与方案；**不实现**写盘代码
>
> **变更（2026-07-27）**：`kiro` 与 `codebuddy`（国际版）支持已移除，仅保留
> `codebuddy-cn`（CodeBuddy CN，独占 `~/.codebuddy`）。下文 §10/§11 为移除前的
> 历史校验与来源记录，予以保留。

---

## 1. 安全与写作约定

1. 本文只记录**路径、字段名、schema、合并策略**。
2. **禁止**写入 token、API Key、密码、本机完整密钥配置。
3. Agent 标识统一使用 `sniff.rs` 中的 `name` 字段（如 `claude-code`）。
4. 「全局应用」默认写**用户级**配置；项目级路径仅作兼容说明。

---

## 2. 与嗅探的对齐

| # | `name` | `display_name` | 嗅探 config root |
|---|--------|----------------|------------------|
| 1 | `codex` | Codex | `~/.codex` |
| 2 | `claude-code` | Claude Code | `~/.claude` |
| 3 | `claude-desktop` | Claude Desktop | `~/Library/Application Support/Claude*`（含 `claude_desktop_config.json`） |
| 4 | `opencode` | OpenCode | `~/.config/opencode` |
| 5 | `deveco-code` | DevEco Code | `~/.config/deveco` |
| 6 | `antigravity` | Antigravity | `~/.gemini` |
| 7 | `codebuddy-cn` | CodeBuddy CN | `~/.codebuddy` |
| 8 | `workbuddy` | WorkBuddy | `~/.workbuddy` |

---

## 3. 统一方言 Taxonomy

后续适配器建议按 `dialect` 分发，而不是按 10 个 if-else 复制逻辑。

### 3.1 MCP 方言

| dialect id | 顶层键 | 文件形态 | 代表 Agent |
|------------|--------|----------|------------|
| `toml.mcp_servers` | `[mcp_servers.<name>]` | TOML | Codex |
| `json.mcpServers` | `mcpServers` | JSON | Claude Code / Desktop / CodeBuddy CN / WorkBuddy |
| `json.mcp` | `mcp` | JSON / JSONC | OpenCode / DevEco |
| `json.gemini_mixed` | `mcpServers`（字段混用） | JSON | Antigravity（Gemini 系） |

### 3.2 Skills 方言

| dialect id | 约定 |
|------------|------|
| `dir.SKILL.md` | `<root>/skills/<skill-name>/SKILL.md` |
| `dir.agents_skills` | 官方新路径 `~/.agents/skills`（Codex）；本机仍见 `~/.codex/skills` |
| `marketplace` | 市场目录与用户 skills 目录分离（如 CodeBuddy CN `skills-marketplace`） |

### 3.3 写策略枚举（实现时复用）

| strategy | 含义 |
|----------|------|
| `merge-object-key` | JSON 对象内按 server name 合并/覆盖 |
| `merge-toml-table` | TOML 表合并 |
| `jsonc-aware-merge` | 支持注释与尾逗号 |
| `create-if-missing` | 文件/目录不存在则建最小骨架 |
| `scope-user-global` | 只写用户全局，不写项目 |
| `shared-root-multi-agent` | 多 Agent 共用同一文件（机制保留；codebuddy 国际版移除后当前无实例） |
| `skill-copy-or-symlink` | skills 目录复制或软链 |

---

## 4. 总览对照表

| sniff `name` | MCP 目标文件 | MCP 方言 | Skills 目录 | 置信度 |
|--------------|--------------|----------|-------------|--------|
| `codex` | `~/.codex/config.toml` | `toml.mcp_servers` | `~/.codex/skills` + `~/.agents/skills` | 高 |
| `claude-code` | 用户：`~/.claude.json` 顶层 `mcpServers`；项目：`.mcp.json` | `json.mcpServers` | `~/.claude/skills/<name>/SKILL.md` | 高 |
| `claude-desktop` | `~/Library/Application Support/Claude/claude_desktop_config.json` | `json.mcpServers` | 无明确全局 Skills（缺口） | MCP 高 / Skills 低 |
| `opencode` | `~/.config/opencode/opencode.json`（或 `.jsonc`） | `json.mcp` | `~/.config/opencode/skills/` | 高 |
| `deveco-code` | `~/.config/deveco/deveco.jsonc` | `json.mcp` + JSONC | `~/.config/deveco/skills/` | 中高 |
| `antigravity` | 主：`~/.gemini/settings.json`；兼容：`~/.gemini/config/mcp_config.json` | `json.gemini_mixed` | `~/.gemini/skills/`（迁移路径另有 `antigravity-cli/skills`） | 中 |
| `codebuddy-cn` | 优先 `~/.codebuddy/.mcp.json`，兼容 `mcp.json` | `json.mcpServers` | `~/.codebuddy/skills/`（可缺省；有 marketplace） | 中高 |
| `workbuddy` | `~/.workbuddy/.mcp.json` | `json.mcpServers` | `~/.workbuddy/skills/` | 中高 |

置信度说明：

- **高**：官方文档 + 本机路径/结构互证  
- **中高**：官方/产品文档明确，本机可能尚未创建该文件  
- **中**：产品处于迁移期，路径存在多版本  

---

## 5. 分 Agent 详解

每节统一模板：**配置根 → MCP → Skills → 读写策略 → 注意点**。

### 5.1 Codex (`codex`)

**配置根**：`~/.codex`（CLI 与 ChatGPT App 共享）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.codex/config.toml` |
| 方言 | `toml.mcp_servers` |
| 结构 | `[mcp_servers.<server-name>]` |
| stdio 字段 | `type = "stdio"`，`command`，`args`，可选 `enabled`、`cwd`、`startup_timeout_sec`；环境变量为 `[mcp_servers.<name>.env]` 子表 |
| http 字段 | `type = "http"`，`url`（及官方文档中的 header/auth 相关键） |
| CLI | `codex mcp add` / `codex mcp list` |

示例骨架（无密钥）：

```toml
[mcp_servers.example]
type = "stdio"
command = "npx"
args = ["-y", "some-mcp-package"]
enabled = true

[mcp_servers.example.env]
SOME_ENV = "value"

[mcp_servers.remote-example]
type = "http"
url = "https://example.com/mcp"
```

**Skills**

| 项 | 值 |
|----|-----|
| 本机常见 | `~/.codex/skills/<skill>/SKILL.md` |
| 官方亦述 | `~/.agents/skills`、仓库 `.agents/skills` |
| 配置引用 | `config.toml` 中可有 `[[skills.config]]`（path / enabled） |
| 提示词 | 全局 `~/.codex/AGENTS.md`（不是 Skill，但是配置根一部分） |

**推荐写策略**

1. 读：解析 `config.toml`  
2. 写：`merge-toml-table` 合并 `mcp_servers.<title>`  
3. 删：删除对应 table（含 `.env` 子表）  
4. Skills：优先 `~/.codex/skills/`；若需兼容官方路径，可读 `~/.agents/skills`  

**来源**

- [Codex config basics](https://developers.openai.com/codex/config-basic)  
- [Codex MCP](https://developers.openai.com/codex/extend/mcp)  
- [Codex Skills](https://developers.openai.com/codex/skills)  
- [AGENTS.md](https://developers.openai.com/codex/guides/agents-md)  

---

### 5.2 Claude Code (`claude-code`)

**配置根**：`~/.claude`（settings / skills / agents 等）  
**MCP 状态文件**：`~/.claude.json`（注意：不在 `~/.claude/` 目录内）

**MCP**

| Scope | 路径 | 说明 |
|-------|------|------|
| User（全局，推荐应用目标） | `~/.claude.json` 顶层 `mcpServers` | 所有项目可见 |
| Local（单项目、个人） | `~/.claude.json` → `projects["/abs/path"].mcpServers` | `claude mcp add` 默认 scope |
| Project（团队共享） | 项目根 `.mcp.json` | git 可提交；需信任/批准 |

| 项 | 值 |
|----|-----|
| 方言 | `json.mcpServers` |
| **不是**主存储 | `~/.claude/settings.json`（权限/hooks/插件；可含 MCP **策略**键，但不是 server 定义主位置） |
| stdio | `command` + `args` + 可选 `env`；可带 `type` |
| http/sse | `type: "http"` / `"sse"` + `url` + 可选 `headers` |
| CLI | `claude mcp add [--scope user|project|local] ...` |

用户全局示例骨架：

```json
{
  "mcpServers": {
    "example": {
      "command": "npx",
      "args": ["-y", "some-mcp-package"],
      "env": {}
    },
    "remote-example": {
      "type": "http",
      "url": "https://example.com/mcp"
    }
  }
}
```

**Skills**

| 项 | 值 |
|----|-----|
| 用户 | `~/.claude/skills/<skill-name>/SKILL.md` |
| 项目 | `.claude/skills/<skill-name>/SKILL.md` |
| 格式 | 目录 + 必需 `SKILL.md`（YAML frontmatter + Markdown） |

**推荐写策略**

1. AgentBuddy「应用到 Agent」默认写 **user scope**：`~/.claude.json` 顶层 `mcpServers`  
2. **不要**默认写项目 `.mcp.json`（避免污染仓库）  
3. 合并：`merge-object-key`；文件是大状态文件，只改 `mcpServers` 键，保留其它顶层字段  
4. 删除：从 `mcpServers` 移除对应 name  
5. Skills：`~/.claude/skills/` + `skill-copy-or-symlink`  

**来源**

- [Claude Code MCP](https://code.claude.com/docs/en/mcp)  
- [Claude Code Settings](https://code.claude.com/docs/en/settings)  
- [Claude Code Skills](https://code.claude.com/docs/en/skills)  

---

### 5.3 Claude Desktop (`claude-desktop`)

**配置根**：`~/Library/Application Support/Claude`（及 `Claude-*` 变体，嗅探已扫描含 `claude_desktop_config.json` 的目录）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `…/Claude/claude_desktop_config.json` |
| 方言 | `json.mcpServers` |
| 结构 | 顶层 `mcpServers`；stdio：`command` / `args` / `env` |
| 生效 | 通常需**完全退出并重启** Desktop |

示例骨架：

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}
```

**Skills**

- Claude Desktop 属于官方所称的 **"Claude apps"**（claude.ai 账号的桌面壳），其 Skills 经**应用内 Settings → Features 上传 zip**、绑定账号、在**云端 VM（code execution 容器）**执行——**没有**任何本地磁盘 Skills 目录可供注入。  
- 官方文档明确各 surface 相互隔离：`Claude Code Skills are filesystem-based and separate from claude.ai`；只有 **Claude Code** 从本地目录（如 `~/.claude/skills`，且官方支持指向别处的 symlink）读取。  
- 因此 AgentBuddy「本地软链接指向 skill 目录」的机制对 Desktop **无处落地**：`skills.rs` 中 `agent_skills_targets()` 对 `claude-desktop` 取 `roots: []` + `supported: false`，前端 `SKILL_UNSUPPORTED_AGENTS` 同步置灰。**这是客观限制，非缺口**。  
- 核实来源（2026-07）：[Agent Skills 总览](https://docs.claude.com/en/docs/agents-and-tools/agent-skills/overview)、[Claude Code Skills](https://code.claude.com/docs/en/skills)、[Skills 公告](https://www.anthropic.com/news/skills)。

**推荐写策略**

1. 若 `config_dirs` 有多条 Claude* 路径，优先写含 `claude_desktop_config.json` 的目录；多实例时需 UI 选择或全部写入（产品决策）  
2. 合并 `mcpServers`，保留文件中其它键（如 `preferences`）  
3. `create-if-missing`：最小 `{ "mcpServers": {} }`  

**来源**

- [MCP — Connect local servers](https://modelcontextprotocol.io/docs/develop/connect-local-servers)  

---

### 5.4 OpenCode (`opencode`)

**配置根**：`~/.config/opencode`（**不是** `~/.opencode` 安装目录）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.config/opencode/opencode.json`（或 `opencode.jsonc`） |
| 方言 | `json.mcp` |
| 顶层键 | **`mcp`**（不是 `mcpServers`） |
| local | `"type": "local"`，`command` 常为**字符串数组**（含 args），`environment`，`enabled` |
| remote | `"type": "remote"`，`url`，`headers`，`enabled` |
| schema | `"$schema": "https://opencode.ai/config.json"` |

示例骨架：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "my-local": {
      "type": "local",
      "command": ["npx", "-y", "some-mcp-package"],
      "enabled": true,
      "environment": {}
    },
    "my-remote": {
      "type": "remote",
      "url": "https://example.com/mcp",
      "enabled": true,
      "headers": {}
    }
  }
}
```

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.config/opencode/skills/<name>/SKILL.md` |
| 项目 | `.opencode/skills/` 等（并兼容部分 Claude/Agent 路径） |
| 权限 | 可在 `opencode.json` 的 `skill` 字段做 allow/deny |

**推荐写策略**

1. 写 `mcp` 对象，`merge-object-key`  
2. UI `stdio` → OpenCode `type: local`，`command = [cmd, ...args]`，`env` → `environment`  
3. UI `http`/`sse` → `type: remote` + `url`  
4. Skills：`~/.config/opencode/skills/`  

**来源**

- [OpenCode Config](https://opencode.ai/docs/config/)  
- [OpenCode MCP](https://opencode.ai/docs/mcp-servers/)  
- [OpenCode Skills](https://opencode.ai/docs/skills/)  

---

### 5.5 DevEco Code (`deveco-code`)

**配置根**：`~/.config/deveco`  
**背景**：基于 OpenCode 扩展，配置 schema 兼容 OpenCode。

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.config/deveco/deveco.jsonc` |
| 方言 | `json.mcp` + **JSONC**（注释、尾逗号） |
| 键 | `mcp`，结构同 OpenCode（`local` / `remote`） |
| 项目级 | `.deveco/deveco.jsonc`、`deveco.jsonc`（优先级高于用户） |

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.config/deveco/skills/` |
| 安装 | 社区常用 `npx skills add …` 类流程 |

**推荐写策略**

1. 必须用 **JSONC 安全** 读写（`jsonc-aware-merge`），不能用严格 `JSON.parse` 覆盖写坏注释  
2. 字段映射同 OpenCode  
3. 全局应用写 `~/.config/deveco/deveco.jsonc`  

**来源**

- OpenCode schema（DevEco 声明兼容）  
- 社区/HarmonyOS 文档对 `~/.config/deveco` 路径的说明  

---

### 5.6 Antigravity (`antigravity`)

**配置根**：`~/.gemini`  
**说明**：Gemini CLI → Antigravity 迁移中，路径存在新旧两套；以**本机实测 + 官方迁移文档**双轨兼容。

**MCP**

| 优先级 | 路径 | 说明 |
|--------|------|------|
| 主写（本机已验证） | `~/.gemini/settings.json` → `mcpServers` | stdio：`command`+`args`+`env`；远程：`httpUrl`（非 `url`） |
| 兼容读 | `~/.gemini/config/mcp_config.json` | 迁移文档中的独立 MCP 配置；本机可能为空文件 |
| CLI 设置 | `~/.gemini/antigravity-cli/settings.json` | 模型/权限等，**非**本机 MCP 主存 |

Gemini 混用字段示例骨架：

```json
{
  "mcpServers": {
    "local-example": {
      "command": "npx",
      "args": ["-y", "some-mcp-package"],
      "env": {},
      "timeout": 60000
    },
    "remote-example": {
      "httpUrl": "https://example.com/mcp",
      "timeout": 60000
    }
  }
}
```

**Skills**

| 项 | 值 |
|----|-----|
| 常见全局 | `~/.gemini/skills/` |
| 迁移文档 | 全局 `~/.gemini/antigravity-cli/skills/`；项目 `.agents/skills/` |
| 推荐 | 写/装优先 `~/.gemini/skills/`；读时兼容 antigravity-cli 路径 |

**推荐写策略**

1. **默认写** `~/.gemini/settings.json` 的 `mcpServers`（与本机一致）  
2. 若未来 `mcp_config.json` 成为主流，再切换主写路径，读路径保持双兼容  
3. UI `http`/`sse` → 写 `httpUrl`（字段映射特殊）  
4. headers：Gemini 本机形态未必支持标准 `headers`，实现前需再实测  

**来源**

- [Antigravity CLI features](https://antigravity.google/docs/cli-features)  
- [Gemini CLI → Antigravity migration](https://antigravity.google/docs/gcli-migration)  
- 本机 `~/.gemini/settings.json` 结构校验  

---

### 5.7 CodeBuddy CN（`codebuddy-cn`）

**配置根**：`~/.codebuddy`（原与国际版 CodeBuddy 共享；国际版移除后由 CN 独占）

**MCP**

| 项 | 值 |
|----|-----|
| 推荐 | `~/.codebuddy/.mcp.json` |
| 兼容/本机 | `~/.codebuddy/mcp.json`（旧/现用） |
| 方言 | `json.mcpServers` |
| 项目 | 项目根 `.mcp.json` |
| 支持 | stdio / sse / http；JSONC；`${ENV}` 展开（文档） |
| CLI | `codebuddy mcp add ...` |

示例骨架：

```json
{
  "mcpServers": {
    "example": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "some-mcp-package"],
      "env": {}
    },
    "api": {
      "type": "http",
      "url": "http://localhost:3000/mcp",
      "headers": {}
    }
  }
}
```

**Skills**

| 项 | 值 |
|----|-----|
| 用户 | `~/.codebuddy/skills/<name>/SKILL.md` |
| 项目 | `.codebuddy/skills/` |
| 市场 | `~/.codebuddy/skills-marketplace/`（安装源，不等于用户 skills 根） |

**推荐写策略**

1. 读优先级：`.mcp.json` → `mcp.json`  
2. 写：若 `.mcp.json` 存在则写它，否则写/创建本机已在用的 `mcp.json`（或统一迁移到 `.mcp.json`）  
3. ~~`shared-root-multi-agent`~~：机制保留；国际版移除后当前无共享根实例  
4. Skills：`~/.codebuddy/skills/`（`create-if-missing`）  

**来源**

- [CodeBuddy CLI MCP](https://www.codebuddy.ai/docs/cli/mcp)  
- [CodeBuddy CLI Skills](https://www.codebuddy.ai/docs/cli/skills)  

---

### 5.8 WorkBuddy (`workbuddy`)

**配置根**：`~/.workbuddy`（与 CodeBuddy CN 分离）

**MCP**

| 项 | 值 |
|----|-----|
| 文件 | `~/.workbuddy/.mcp.json`（本机已验证） |
| 方言 | `json.mcpServers` |
| 结构 | 同 Claude 系；可见内置 `connector-proxy` 一类 http 代理项 |
| 注意 | 合并时**保留**产品自带 server，勿整体覆盖文件 |

**Skills**

| 项 | 值 |
|----|-----|
| 全局 | `~/.workbuddy/skills/` |
| 项目 | `.workbuddy/skills/` |
| 格式 | 目录 + `SKILL.md`（社区惯例） |

**推荐写策略**

1. `merge-object-key` 写入 `mcpServers`  
2. 禁止 `write_file` 整文件替换成只有新 server 的内容  
3. Skills：`~/.workbuddy/skills/`  

**来源**

- CodeBuddy CN 生态发布说明中 WorkBuddy 独立目录约定  
- 本机 `~/.workbuddy/.mcp.json` 结构校验  
- 社区 Skills 安装路径惯例  

---

## 6. AgentBuddy 表单 → 各方言字段映射

AgentBuddy UI 当前模型（`McpManage.tsx`）：

- `title`  
- `type`: `stdio` | `http` | `sse`  
- `command` / `args[]` / `env{}`  
- `url` / `headers{}`  

| UI | `json.mcpServers`（Claude/CodeBuddy CN/WorkBuddy） | `json.mcp`（OpenCode/DevEco） | `toml.mcp_servers`（Codex） | `json.gemini_mixed` |
|----|------------------------------------------------------|------------------------------|-----------------------------|---------------------|
| title | 对象键 `mcpServers[title]` | 对象键 `mcp[title]` | 表名 `mcp_servers.title` | `mcpServers[title]` |
| stdio | `command` + `args` + `env`；可选 `type: "stdio"` | `type: "local"` + `command: [cmd, ...args]` + `environment` | `type="stdio"` + `command` + `args` + `[.env]` | `command` + `args` + `env` |
| http | `type: "http"` + `url` + `headers` | `type: "remote"` + `url` + `headers` | `type="http"` + `url`（headers 按官方键） | **`httpUrl`**（非 `url`） |
| sse | `type: "sse"` + `url` + `headers` | 映射为 `remote`（或文档等价） | 若官方仅 http，降级为 http 或跳过并提示 | 优先 `httpUrl`；需实测 |
| enabled | 可选 | 常写 `enabled: true` | `enabled = true` | 无统一字段时省略 |

**Skills 映射（后续 Skills 管理页）**

| UI 动作 | 实现建议 |
|---------|----------|
| 安装到 Agent | 将 skill 目录落到该 Agent 的 `skills/<name>/`，保证含 `SKILL.md` |
| 多 Agent | 复制或硬链/软链（产品决策）；共享 root 只操作一次 |
| Claude Desktop | 跳过或提示不支持 |

---

## 7. 全局应用 / 删除 — 统一算法（方案层）

### 7.1 Apply（保存并应用）

```
输入: McpServerDraft, selectedAgentNames[]
for agent in selectedAgents:
  adapter = resolve_adapter(agent.name)   # by dialect
  path = adapter.global_mcp_path(agent.config_dirs)
  doc = adapter.read(path) or adapter.empty_doc()
  entry = adapter.from_ui(draft)          # 字段映射
  doc = adapter.upsert(doc, draft.title, entry)
  adapter.write_atomic(path, doc)         # 写临时文件再 rename
```

原则：

1. **只写用户全局**（见上表）  
2. **同名覆盖，异名保留**  
3. **原子写**，避免半截 JSON/TOML  
4. 共享物理根的多个 agent 同时勾选 → **去重 path** 只写一次（机制保留）  
5. Claude Code → 只动 `~/.claude.json` 的 `mcpServers`，不动其它大字段语义  

### 7.2 Delete（列表删除 + 可选同步配置）

```
if removeFromAgentConfigs:
  for agent in server.appliedAgents:
    adapter.remove(agent, server.title)
从 AgentBuddy 本地列表移除 server
```

原则：

1. 仅删除目标 server name  
2. 不删除整个配置文件  
3. WorkBuddy / Claude Desktop 等可能含非用户 server，禁止清空 `mcpServers`  

### 7.3 适配器接口草图（不实现，仅规格）

```ts
interface McpAdapter {
  dialect: string;
  /** 解析全局 MCP 文件路径 */
  resolvePath(configDirs: string[]): string;
  read(path: string): unknown;
  emptyDoc(): unknown;
  fromUi(draft: UiMcpDraft): unknown;
  upsert(doc: unknown, name: string, entry: unknown): unknown;
  remove(doc: unknown, name: string): unknown;
  writeAtomic(path: string, doc: unknown): void;
}
```

建议按 dialect 实现 4 个适配器类，Agent 表只做 path 绑定。

---

## 8. 缺口、风险与决策

| 项 | 风险 | 建议 |
|----|------|------|
| Claude Desktop Skills | 无标准全局 skills 目录 | Skills 页标记不支持；只做 MCP |
| Antigravity 双路径 | 迁移中主路径可能变 | 主写 `settings.json`；读兼容 `mcp_config.json` |
| Gemini `httpUrl` | 与 UI `url` 不一致 | 映射表强制转换；headers 需实测 |
| Codex Skills 双路径 | `~/.codex/skills` vs `~/.agents/skills` | 写本机已存在者；两者都无则写 `~/.codex/skills` |
| DevEco JSONC | 严格 JSON 写坏注释 | 必须 jsonc 库 |
| Claude Code 大文件 | `~/.claude.json` 含大量状态 | 只改 `mcpServers` 键 |
| 项目级 MCP | 写项目会进 git / 需信任 | 全局应用默认不写项目；项目级写入仅在「项目 AI 配置」页由用户显式勾选触发（见 §13） |
| SSE 方言差异 | 各产品支持不一 | 能映射则映射，否则提示跳过 |
| 密钥 | 配置中常含 token | AgentBuddy 永不日志打印 env/headers 值 |

---

## 9. 实现优先级建议（后续任务，非本次）

1. **P0**：`json.mcpServers` 适配器（覆盖 Claude Code/Desktop、CodeBuddy CN、WorkBuddy）  
2. **P0**：`json.mcp` 适配器（OpenCode + DevEco JSONC）  
3. **P1**：`toml.mcp_servers`（Codex）  
4. **P1**：`json.gemini_mixed`（Antigravity）  
5. **P2**：Skills 安装/卸载（`dir.SKILL.md`）  
6. **P2**：删除弹窗勾选 → 真实 `removeFromAgentConfigs`  

依赖后端：建议在 Tauri 侧做文件读写（权限与原子写更安全），前端只传 draft + agent names。

---

## 10. 本机校验摘要（2026-07-14，仅结构）

| 路径 | 状态 |
|------|------|
| `~/.codex/config.toml` | 存在；含 `[mcp_servers.*]` |
| `~/.codex/skills` | 存在；多 skill 子目录 + `SKILL.md` |
| `~/.agents/skills` | 存在（官方路径之一） |
| `~/.claude.json` | 存在；顶层有 `mcpServers` |
| `~/.claude/skills` | 存在 |
| `~/Library/Application Support/Claude/claude_desktop_config.json` | 存在 |
| `~/.config/opencode/opencode.json` | 存在；键为 `mcp` |
| `~/.config/opencode/skills` | 存在 |
| `~/.config/deveco/deveco.jsonc` | 存在；键含 `mcp` |
| `~/.kiro/settings/` | 存在；**尚无** `mcp.json` |
| `~/.gemini/settings.json` | 存在；`mcpServers` + `httpUrl` |
| `~/.gemini/config/mcp_config.json` | 存在但可能为空 |
| `~/.gemini/skills` | 存在 |
| `~/.codebuddy/mcp.json` | 存在 |
| `~/.workbuddy/.mcp.json` | 存在 |
| `~/.workbuddy/skills` | 存在 |

---

## 11. 来源索引

### 官方 / 准官方

- Claude Code MCP / Settings / Skills：https://code.claude.com/docs/en/mcp 、https://code.claude.com/docs/en/settings 、https://code.claude.com/docs/en/skills  
- Claude Desktop MCP：https://modelcontextprotocol.io/docs/develop/connect-local-servers  
- Codex Config / MCP / Skills / AGENTS.md：https://developers.openai.com/codex/config-basic 、https://developers.openai.com/codex/extend/mcp 、https://developers.openai.com/codex/skills 、https://developers.openai.com/codex/guides/agents-md  
- OpenCode Config / MCP / Skills：https://opencode.ai/docs/config/ 、https://opencode.ai/docs/mcp-servers/ 、https://opencode.ai/docs/skills/  
- Kiro MCP / Skills：https://kiro.dev/docs/cli/mcp/ 、https://kiro.dev/docs/skills/  
- Antigravity / 迁移：https://antigravity.google/docs/cli-features 、https://antigravity.google/docs/gcli-migration  
- CodeBuddy MCP / Skills：https://www.codebuddy.ai/docs/cli/mcp 、https://www.codebuddy.ai/docs/cli/skills  

### 工程内对照

- 嗅探定义：[`src-tauri/src/sniff.rs`](src-tauri/src/sniff.rs)  
- MCP UI 模型：[`src/components/pages/McpManage.tsx`](src/components/pages/McpManage.tsx)  

---

## 12. 结论（可执行摘要）

1. **四种 MCP 方言**即可覆盖 10 个 Agent：`toml.mcp_servers`、`json.mcpServers`、`json.mcp`、`json.gemini_mixed`。  
2. **Skills** 主流是 `skills/<name>/SKILL.md`；Claude Desktop 暂不支持；Codex 需兼容双路径。  
3. **全局应用**默认只写用户级文件；Claude Code 写 `~/.claude.json` 顶层 `mcpServers`。  
4. **CodeBuddy / CN 共享** `~/.codebuddy`，写盘必须去重。  
5. **OpenCode / DevEco** 用 `mcp` + `local`/`remote`，不是 `mcpServers`。  
6. **Antigravity** 远程字段优先 `httpUrl`。  
7. 下一步实现按 §9 优先级做适配器，删除勾选接入真实 `remove` 即可闭环 MCP 管理页。

---

## 13. 项目级 MCP / Skills（项目 AI 配置页，2026-07-28 起）

「项目 AI 配置」初始化时，用户可从 AgentBuddy 已配置的 MCP / Skills 中勾选，落盘到所选项目目录（与全局应用互不影响）：

### 13.1 项目级 MCP 目标文件

| sniff `name` | 项目级 MCP 文件（相对项目根） | 方言 |
|--------------|-------------------------------|------|
| `claude-code` | `.mcp.json` | `json.mcpServers` |
| `codebuddy-cn` | `.mcp.json`（与 claude-code 同路径，去重只写一次） | `json.mcpServers` |
| `workbuddy` | `.mcp.json`（同上） | `json.mcpServers` |
| `codex` | `.codex/config.toml` | `toml.mcp_servers` |
| `opencode` | `opencode.json` | `json.mcp` |
| `deveco-code` | `.deveco/deveco.jsonc` | `json.mcp` + JSONC |
| `antigravity` | `.gemini/settings.json` | `json.gemini_mixed` |

写策略：复用全局应用的方言写器（`mcp_config::apply_draft_to_file`），**按 server title 合并**、保留文件其它键、原子写；不参与「覆盖/跳过」确认。

### 13.2 项目级 Skills

- 统一安装到 `<项目>/.agents/skills/<id>`（`skill-copy-or-symlink`：完整复制或软链接，源为 `~/.agentbuddy/skills` 技能库）。
- 多 agent 共享：Symlink 模式下 `<config_dir>/skills` 本已链接至 `.agents/skills`；Full 模式下勾选 skills 时，为每个 agent 创建 `<config_dir>/skills → ../.agents/skills` 相对软链接。
- 安全语义同骨架：非 overwrite 跳过已存在；overwrite 可替换软链接/空目录，**不删除非空真实目录**。
