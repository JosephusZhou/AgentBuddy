# CLAUDE.md

AgentBuddy 是一个 Tauri 2 桌面应用，用于发现本机 AI coding agent，并管理 MCP、Skills、
多环境、模型配置、供应商、项目配置和备份。

## 技术栈与命令

- 前端：React 18、TypeScript、Vite 6、Tailwind 3，代码在 `src/`。
- 后端：Rust、Tauri 2，代码在 `src-tauri/`。
- 包管理：pnpm。主要平台：macOS、Windows。界面文案为中文。
- 应用数据：macOS/Linux 使用 `~/.agentbuddy/`；Windows 优先使用
  `%LOCALAPPDATA%\AgentBuddy`，同时兼容旧的 `~/.agentbuddy/`。

版本发布：当前版本为 `0.1.6`。更新版本号时需要同步修改以下 5 个文件：
`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、
`src-tauri/Cargo.lock` 和 `README.md`。

```bash
pnpm install
pnpm dev                 # Vite + Tauri
pnpm dev:renderer        # 仅前端
pnpm typecheck
pnpm build               # 前端 + Tauri 打包
pnpm build:renderer      # 仅前端构建

cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib route_aggregation
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

当前没有 ESLint、Prettier 或前端测试脚本。构建需要 Node、pnpm、Rust 和对应平台的 Tauri
系统依赖。

## 代码入口

```text
src/
  App.tsx                         视图状态与页面路由
  components/pages/               功能页面与 Tauri API helper
  components/ui.tsx               页面无关的共享 UI
  index.css                       主题和全局样式

src-tauri/src/
  lib.rs                          Tauri command 注册、启动初始化
  agents.rs                       Agent 注册表：身份、路径、MCP 方言、Skills 根
  sniff.rs                        Agent 安装发现和 shim 过滤
  mcp_config.rs                   MCP 多方言读写、扫描和连通性探测
  skills.rs                       Skills 库、导入、更新、导出和分发
  claude_env.rs / codex_env.rs    CLAUDE_CONFIG_DIR / CODEX_HOME 多环境
  opencode_config.rs              OpenCode provider/model/auth
  pi_model_config.rs              Pi / Oh-My-Pi 模型和 auth
  agent_model_config.rs           OpenCode / Pi / Oh-My-Pi 的统一模型分派
  project_config.rs               项目级 AI 骨架、MCP 和共享 Skills
  ai_provider.rs                  AI 供应商、加密 API Key 和 custom models
  db.rs                            SQLite 持久化
  config.rs                        app config.json、主题、代理和 secretsKey
  crypto.rs                        AES-256-GCM + HKDF 密钥字段加密
  http_client.rs                   统一代理下的 reqwest client
  webdav.rs / backup.rs            WebDAV 连接、备份打包上传和远端恢复
  route_aggregation/               同协议路由聚合、故障转移和 cloaking

docs/SYNC_PLAYBOOK.md              CLIProxyAPI 行为同步手册
docs/cli_proxy_api_sync_state.json CLIProxyAPI 同步状态
scripts/check_upstream_sync.py     上游漂移检查
AGENT_MCP_SKILLS_MAP.md            MCP/Skills 路径、方言和字段规范
```

## 前后端边界

页面通过动态导入调用 Tauri：

```ts
const { invoke } = await import("@tauri-apps/api/core");
await invoke("command_name", { ... });
```

公共 DTO 使用 Serde `camelCase`，Rust 的 `type` 字段按 JSON 键 `type` 序列化。所有 command
最终在 `src-tauri/src/lib.rs` 注册；新增或修改 command 时同步前端 API helper 和类型。

安全边界：

- `secretsKey` 只留在 Rust；`get_app_config` 不返回它。
- API Key、Token、密码不得进入日志、文档、提交或示例配置；列表 DTO 只返回存在性标志，
  明文通过按需 command 读取。
- Agent 打开配置的 command 只接受注册表中的 `name` 和 `ConfigFileKind`，不接受前端任意路径。
- 路径和平台分支集中在 `platform.rs`、`agents.rs` 及各自的路径解析层，业务逻辑不要散落
  `cfg!(windows)`。

## Agent、MCP 与 Skills

`agents.rs` 是 Agent 身份、发现路径、MCP 方言/路径和 Skills 根的运行时单一数据源。当前
稳定的 `name` 为：

```text
codex, claude-code, claude-desktop, opencode, antigravity,
codebuddy-cn, workbuddy, pi, oh-my-pi
```

`kiro` 和国际版 `codebuddy` 已移除。新增 Agent 或修改路径时，先改 `agents.rs`，必要时
扩展 `McpPath` 和 `mcp_config.rs`，再同步前端项目配置列表。

`AGENT_MCP_SKILLS_MAP.md` 是跨 Agent 的详细规范来源，记录 5 种 MCP 方言、全局/项目路径、
字段映射、Skills 根、原子写策略和已知限制。它不是运行时代码，但不能在没有迁移这些规范
并更新 `mcp_config.rs` / `skills.rs` 引用前删除。

MCP 写入的共同不变量：按 server title 合并、保留无关字段、临时文件加 rename 原子写入；
OpenCode JSONC 用 json5 读取后写为标准 JSON；扫描以磁盘状态更新 `appliedAgents`；部分
成功只记录实际成功的 Agent；`test_mcp_connection` 只探测，不写配置。

多环境规则：

- Claude 使用 `CLAUDE_CONFIG_DIR`；默认 MCP 在 `~/.claude.json` 顶层 `mcpServers`，自定义环境
  的 MCP 同步到 `$CLAUDE_CONFIG_DIR/.claude.json`。
- Codex 使用 `CODEX_HOME`；默认 MCP 在 `~/.codex/config.toml` 的 `[mcp_servers]`，自定义
  环境同步时只替换该表并保留其它配置。
- 项目级配置由 `project_config.rs` 的 8 个 `AGENT_SPECS` 决定；全局 MCP 应用不会默认写项目。
- Skills 库位于 `~/.agentbuddy/skills/<id>/SKILL.md`，项目级统一安装到 `.agents/skills`，
  再按 Full/Symlink 模式供选中的 Agent 使用。Claude Desktop 当前不支持本地 Skills。

## 数据与模型规则

- SQLite 数据库：`~/.agentbuddy/agents.db`，包含 agents、MCP、Skills、WebDAV、Claude/Codex
  environments、AI providers 和 `provider_route_toggle`。
- `config.json` 保存主题、备份、网络代理和私有 `secretsKey`；外部 HTTP（Skills、WebDAV、
  MCP probe、Models.dev、OpenCode catalog 等）统一经过代理配置。
- 已保存 AI 供应商的 `custom_models_json` 是运行时可见模型的唯一来源；路由池、provider
  状态、`GET /v1/models` 和 `get_route_provider_models` 都不得自动请求供应商模型列表。
- 编辑表单可显式执行一次远端模型拉取作为候选；未关联供应商的临时环境配置也允许远端拉取。

## 路由聚合

`src-tauri/src/route_aggregation/` 提供本地 Axum 代理，维护两种协议、三个入口：

| 入口 | 协议 | 行为 |
|------|------|------|
| `POST /v1/messages` | Anthropic Messages | 同协议转发，Claude cloaking，工具名响应恢复 |
| `POST /v1/messages/count_tokens` | Anthropic `count_tokens` | 专用 system relocation 和请求级 cloaking |
| `POST /v1/responses` | OpenAI Responses | 同协议转发，Codex CLI cloaking |

另有 `GET /v1/models`，只返回启用供应商的自定义模型并集，不向远端查询。

路由聚合只做同协议转发，不做 Claude/OpenAI 协议转换。供应商类型仅接受 Anthropic、OpenAI、
Universal；provider pool 按启用状态和自定义模型过滤，失败时按配置执行 failover，并通过
每个 provider × route group 的 Closed/Open/HalfOpen 熔断器保护上游。

默认后端配置：`127.0.0.1:16888`、自动 failover、最多 3 次重试、Claude `2.1.220`、Codex
`0.146.0`、cloaking `auto`。监听端点使用 `Authorization: Bearer <API Key>`，主 API Key 首次启动自动生成，
配置中的 Key 经加密落盘。

cloaking 模块按客户端行为维护：

- Claude：请求头/设备 profile、system prompt、billing/CCH、user_id、工具名、敏感词、cache、
  context 和 `count_tokens`。
- Codex：请求头、`Session-Id` 和请求身份字段。
- `forwarder.rs` 负责本地集成，包括 SSE 背压、非流式/SSE 工具名恢复、请求重试和代理；这些
  不是 CLIProxyAPI 文件镜像。

CLIProxyAPI 只提供行为同步参考，不是运行时依赖。上游变更按
[`docs/SYNC_PLAYBOOK.md`](docs/SYNC_PLAYBOOK.md) 和
[`docs/cli_proxy_api_sync_state.json`](docs/cli_proxy_api_sync_state.json) 处理；客户端指纹
漂移超过 14 天时同步检查脚本返回失败。

## 前端视图

主界面包含 Agent 发现、MCP 管理、Skills 管理、Claude 环境、Codex 环境、OpenCode 配置、AI
供应商、路由聚合、项目 AI 配置和备份管理；设置界面包含偏好、网络代理和 WebDAV。

## 修改规则

1. 先读相关模块、测试和对应规范文档；不要只按文件名猜接口。
2. 跨 Rust/TypeScript 的字段改动必须同时更新 DTO、API helper 和页面类型。
3. 修改 Agent/MCP/Skills 路径时，遵循 `agents.rs` → 写入器/项目配置 → `AGENT_MCP_SKILLS_MAP.md`
   → 前端镜像的顺序。
4. 修改路由聚合或 cloaking 时，至少运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib route_aggregation`。
5. 保留用户已有配置和未相关工作区改动；不要读取或输出密钥、密码、Token 等敏感信息。

## 首要阅读文件

1. `src-tauri/src/lib.rs`：command 注册和启动初始化
2. `src-tauri/src/agents.rs`：Agent 注册表
3. `src-tauri/src/mcp_config.rs` + `AGENT_MCP_SKILLS_MAP.md`：MCP 方言和路径
4. `src-tauri/src/project_config.rs`：项目级骨架、MCP 和 Skills
5. `src-tauri/src/ai_provider.rs` + `agent_model_config.rs`：供应商和模型
6. `src-tauri/src/claude_env.rs` + `codex_env.rs`：多环境及同步
7. `src-tauri/src/route_aggregation/` + `docs/SYNC_PLAYBOOK.md`：本地路由和 cloaking
8. `src/App.tsx` + 对应页面：前端视图与交互
