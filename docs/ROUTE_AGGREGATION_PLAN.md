# AgentBuddy 路由聚合完整实现方案

> 目标：在 AgentBuddy 内完整实现 CLIProxyAPI 的翻译矩阵（Anthropic / OpenAI Chat Completions / OpenAI Responses / Codex native ↔ Gemini 等），每个模块在文件头标注对应的 CLIProxyAPI commit SHA，后续定期手动 cherry-pick 上游修复。
>
> 决策依据：用户 2026-08-12 拍板"在 AgentBuddy 里实现 + 后续手动同步 CLIProxyAPI 修复"，不采用 sidecar 模式。CLIProxyAPI 近 9 天 ~1.3 天一次 commit，~60% bug fix + ~30% new feature + ~10% perf，是高频迭代项目，必须建立可追溯的同步机制。

---

## 1. 现状盘点

| 项目 | 状态 |
|------|------|
| `src-tauri/src/route_aggregation/` | 已存在：`circuit_breaker.rs` / `cloaking/` / `forwarder.rs` / `handler.rs` / `provider_router.rs` / `router.rs` / `server.rs` / `config.rs` / `types.rs` / `mod.rs` |
| `translator/` 子目录 | ❌ 尚未创建（本次新建） |
| Google Generative AI Provider 类型 | ✅ 已完成（`TYPE_GOOGLE_GENERATIVE_AI` 常量 + 默认 base URL + 模型解析 + 档位清空） |
| 路由聚合路由池自动刷新 | ✅ 已修复（`spawn_route_aggregation_pool_refresh`，写操作后异步刷 pool） |
| 非可路由 Provider 类型灰显 | ✅ 已完成（`isRouteableProviderType` 白名单 + 红色徽标 + opacity） |
| Google 主题适配 | ✅ 已完成（22 个 `[data-theme]` 块各加 `--seed-google-fg`，TypeBadge 改用 color-mix） |

---

## 2. CLIProxyAPI 翻译矩阵全景

来源：`internal/translator/init.go`（commit 拉到 2026-08-11）。

注册表 8 个 source × 5 个 target，实际有效 **23 个 pair**。每个 pair 物理上是一个子目录，包含请求/响应/流式三个 translator。

| Source │ ① Gemini | ② Claude | ③ OpenAI Chat | ④ OpenAI Responses | ⑤ Interactions | ⑥ Codex native |
|--------|-----------|-----------|----------------|--------------------|----------------|----------------|
| antigravity | `antigravity/gemini/` | `antigravity/claude/` | `antigravity/openai/chat-completions/` | `antigravity/openai/responses/` | `antigravity/interactions/` | — |
| claude | `claude/gemini/` | — | `claude/openai/chat-completions/` | `claude/openai/responses/` | `claude/interactions/` | — |
| codex | `codex/gemini/` | `codex/claude/` | `codex/openai/chat-completions/` | `codex/openai/responses/` | `codex/interactions/` | — |
| gemini | `gemini/gemini/` | `gemini/claude/` | `gemini/openai/chat-completions/` | `gemini/openai/responses/` | `gemini/interactions/` | — |
| interactions | — | `interactions/claude/` | — | — | — | — |
| openai | `openai/gemini/` | `openai/claude/` | `openai/openai/chat-completions/` | `openai/openai/responses/` | `openai/interactions/{chat-completions,responses}/` | — |

注册接口（`internal/translator/translator/translator.go`）：

```go
Register(from, to string, request TranslateRequestFunc, response TranslateResponse)
Request(from, to, model string, rawJSON []byte, stream bool) []byte
NeedConvert(from, to string) bool
Response(from, to, ctx, model, originalReq, req, rawResp []byte, param *any) [][]byte
ResponseNonStream(...) []byte
```

### 命名约定陷阱 ⚠️

CLIProxyAPI 目录命名是 **请求源 → 目标协议**（即上游用什么协议发请求、我们翻译成什么协议发给下游），但 **响应函数名是目标→源**：

- `internal/translator/gemini/claude/` 物理上是 `ConvertClaudeRequestToGemini`（把 Claude 请求转 Gemini）+ `ConvertGeminiResponseToClaude`（把 Gemini 响应转回 Claude）。
- 所以一个完整的"客户端 Claude ↔ 下游 Gemini"对话，需要 **2 个 pair 同时注册**：一个是请求方向（`gemini/claude/`），另一个是请求方向的逆（`claude/gemini/`），响应函数都注册到对应 pair 上。

AgentBuddy 实现时，按 **`translator/<source>_<target>/`** 命名（不分请求/响应方向），内含 `request.rs` + `response_stream.rs` + `response_non_stream.rs` 三个文件，**对外只暴露一个 `Translatable` trait**，请求/响应方向由注册表 key 决定。

---

## 3. 上游同步机制

### 3.1 文件头注释模板

每个 Rust 翻译器文件第 1-3 行必须包含：

```rust
//! Translator: <source> → <target>
//!
//! CLIProxyAPI aligned: <short_sha> - <commit message>
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/<full_sha>
//! Last verified: <YYYY-MM-DD>
```

例：

```rust
//! Translator: Gemini → Anthropic Messages
//!
//! CLIProxyAPI aligned: 189776a - fix(claude): validate legacy-model system turns before sending
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/189776aab1fc7523229633a830850b8375079849
//! Last verified: 2026-08-12
```

### 3.2 同步检查脚本

新增 `scripts/check_upstream_sync.py`（不依赖外部 Python 包，仅用 `urllib` + `subprocess`）：

- 输入：`docs/cli_proxy_api_sync_state.json`（每个 pair 一个 `{short_sha, full_sha, last_verified_date, last_diff_summary}`）
- 每周 / 每月 / 手动跑：
  1. `git ls-remote https://github.com/router-for-me/CLIProxyAPI.git refs/heads/main` 拿最新 main commit
  2. 对每个 pair，从 `init.go` 注册历史或 git log 找出对应 pair 目录最新的 commit
  3. 若上游最新 commit ≠ 已记录 commit：
     - 距上次同步 ≤ 14 天：warning
     - 距上次同步 > 14 天：error（CI 阻断，PR 红 ✗）
  4. 输出 `docs/cli_proxy_api_sync_state.json.new`，CI 提示人工 review 后 `mv` 落地
- CI 工作流：`.github/workflows/upstream-sync-check.yml`，每周一 9:00 UTC 跑一次（artifacts 上传 report）

### 3.3 同步 playbook

新增 `docs/SYNC_PLAYBOOK.md`，流程：

1. `python3 scripts/check_upstream_sync.py --pair gemini/claude` 定位需要同步的 pair
2. `git log --oneline 189776a..upstream/main -- internal/translator/gemini/claude/` 看 diff 列表
3. 对每个 fix commit，按"修改点映射表"逐个 cherry-pick 到 AgentBuddy 对应 Rust 文件
4. 更新文件头 `CLIProxyAPI aligned: <new_short_sha>` + `Last verified` 日期
5. 跑 `cargo test translator -- --nocapture` 验证
6. 提交 PR，标题前缀 `[sync] gemini/claude`

---

## 4. 目录结构（最终态）

```
src-tauri/src/route_aggregation/
├── mod.rs                          # 模块入口 + ProviderRouter 整合
├── forwarder.rs                    # 现有转发逻辑（保持）
├── provider_router.rs              # 现有路由池（保持）
├── handler.rs                      # 现有 HTTP handler（保持）
├── cloaking/                       # 现有请求伪装（保持）
├── circuit_breaker.rs              # 现有熔断（保持）
├── server.rs / router.rs / config.rs / types.rs
│
└── translator/                     # ← 新建
    ├── mod.rs                      # registry HashMap<(Format, Format), Box<dyn Translatable>>
    ├── translatable.rs             # trait 定义（5 个方法）
    ├── params.rs                   # StreamParams 流式状态机
    ├── formats.rs                  # enum Format { Anthropic, OpenAiChat, OpenAiResponses, Gemini, CodexNative, Interactions, Antigravity }
    │
    ├── common/                     # 跨 pair 共享工具
    │   ├── mod.rs
    │   ├── tool_name.rs            # sanitize + recover（hash-suffix 冲突检测）
    │   ├── id_map.rs               # input_id 确定性 ID 生成
    │   ├── schema.rs               # object schema normalize（auto properties）
    │   ├── thinking.rs             # thinkingBudget / thinkingLevel 双向映射 + ModeNone 清除
    │   ├── multimodal.rs           # inline_data 编解码（base64 / mime_type）
    │   ├── sse.rs                  # SSE chunk parser（`event:`, `data:`, `[DONE]` 处理）
    │   └── http.rs                 # Google endpoint 探测（是否走 OpenAI 兼容端点 passthrough）
    │
    ├── gemini_claude/              # Gemini → Anthropic Messages（请求源是 Gemini，下游给 Claude）
    ├── claude_gemini/               # Anthropic Messages → Gemini
    ├── gemini_openai_chat/         # Gemini → OpenAI Chat Completions
    ├── openai_gemini/              # OpenAI Chat → Gemini
    ├── gemini_openai_responses/    # Gemini → OpenAI Responses（Codex 客户端路径）
    ├── openai_openai_responses/    # OpenAI Responses 内部转发（passthrough 微调）
    │
    ├── codex_claude/              # Codex native → Anthropic（暂缓，等真有 Codex→Claude 需求再做）
    ├── codex_gemini/              # Codex native → Gemini
    ├── codex_openai_responses/    # Codex native → OpenAI Responses
    │
    ├── antigravity_claude/         # Antigravity 客户端路径（暂缓）
    ├── antigravity_gemini/
    ├── antigravity_openai_chat/
    ├── antigravity_openai_responses/
    │
    ├── interactions_claude/        # Interactions API ↔ Claude（暂缓）
    │
    ├── openai_claude/              # OpenAI Chat → Anthropic（暂缓）
    ├── claude_openai_chat/         # Anthropic → OpenAI Chat（暂缓）
    └── openai_interactions/        # OpenAI → Interactions（暂缓）

docs/
├── ROUTE_AGGREGATION_PLAN.md      # 本文档
├── SYNC_PLAYBOOK.md                # 上游同步操作手册
└── cli_proxy_api_sync_state.json   # 同步状态记录

scripts/
└── check_upstream_sync.py          # 同步检查脚本
```

**目录组织原则**：

- 不拆 source/target 嵌套（避免 CLIProxyAPI 那种命名陷阱），改用平铺 `<source>_<target>/`
- 每个 pair 一个目录，内含 `request.rs` + `response_stream.rs` + `response_non_stream.rs` + `mod.rs`
- 跨 pair 共享的逻辑放 `common/`
- 命名按字母排序：`claude_gemini` 在前，`gemini_openai_chat` 在后（请求源在先）

---

## 5. 7 阶段实施计划

### Phase 0 — 基础设施（必须最先做）

**目标**：搭建 translator 框架 + 流式状态机 + 测试脚手架，无任何具体 translator。

| 子任务 | Rust 文件 | 参考 CLIProxyAPI commit |
|--------|-----------|-------------------------|
| Translator registry 雏形 | `translator/mod.rs` + `translatable.rs` | 接口定义参考 `internal/translator/translator/translator.go`（任意 main commit） |
| 格式枚举 | `translator/formats.rs` | 同上 |
| 流式状态机（StreamParams） | `translator/params.rs` | `internal/translator/gemini/claude/gemini_claude_response.go`（任意 main commit） |
| SSE chunk 解析 | `translator/common/sse.rs` | `fix(auth): repair force-mapped Responses SSE framing` `150e7f0dc50e3d3a0f7c4e552cc402ae105eb2a0` (2026-06-29) |
| Tool name sanitize（基础版） | `translator/common/tool_name.rs` | `fix(codex): make input ID sanitization collision-resistant and deterministic` `db143aebac93f9be136ba3d18bd75381d61a2750` (2026-08-11) |
| Schema normalize | `translator/common/schema.rs` | `feat(translator): add test and logic to ensure object schemas include properties field` `c13dbcc24e1373e353338d90bdb38b8e4722e22b` (2026-06-18) |
| 测试脚手架 | `tests/translator_registry.rs` + `tests/streaming_state_machine.rs` | — |

**验收**：
- `cargo test translator` 全绿
- `cargo doc --no-deps` 生成的 doc 里每个模块文件头显示 CLIProxyAPI aligned 行
- 翻译器注册表可以动态 add/remove（用于 HMR-like 热更新）

---

### Phase 1 — 核心链路：Anthropic Messages ↔ Gemini

**目标**：第一个端到端可用的 pair，覆盖基础文本 + system + 多轮对话 + tool call + 图片。

| 子任务 | Rust 文件 | 参考 CLIProxyAPI commit |
|--------|-----------|-------------------------|
| Claude → Gemini 请求转换 | `translator/claude_gemini/request.rs` | `internal/translator/claude/gemini/`（main） |
| Gemini → Claude 流式响应 | `translator/claude_gemini/response_stream.rs` | `internal/translator/gemini/claude/gemini_claude_response.go`（main） |
| Gemini → Claude 非流式响应 | `translator/claude_gemini/response_non_stream.rs` | `ConvertGeminiResponseToClaudeNonStream`（main） |
| 反向：Gemini → Claude 请求 | `translator/gemini_claude/request.rs` | `internal/translator/gemini/claude/gemini_claude_request.go`（main） |
| 反向：Claude → Gemini 响应 | `translator/gemini_claude/response.rs` | `ConvertClaudeResponseToGemini`（main） |
| Thinking mode 双向映射 | `translator/common/thinking.rs` | `feat(thinking): remove thinkingConfig for ModeNone with zero budget` `ac8fb9706fb84bedfbd1f813738680fdc6767115` (2026-06-18) |
| 图片 inline_data | `translator/common/multimodal.rs` | 同 main commit |
| ProviderRouter 接入 | `provider_router.rs` 增加 `translatable: Arc<TranslatorRegistry>` | — |
| handler 调度逻辑 | `handler.rs` 注入 `provider.base_url` 判断 → 走 passthrough / translator | — |

**字段映射表**（来自 `gemini/claude/gemini_claude_request.go`）：

| Anthropic | Gemini |
|-----------|--------|
| `system` (string / array) | `systemInstruction.parts[].text` |
| `messages[].role: user` | `contents[].role: user` |
| `messages[].role: assistant` | `contents[].role: model` ⚠️ |
| `content: text` | `parts[].text` |
| `content: tool_use` | `parts[].functionCall` |
| `content: tool_result` | `parts[].functionResponse` |
| `content: image` (base64) | `parts[].inline_data` |
| `content: thinking` | 丢弃（除非兼容模式保留空签名） |
| `max_tokens` | `generationConfig.maxOutputTokens` |
| `temperature / top_p / top_k` | `generationConfig.{temperature, topP, topK}` |
| `thinking.budget_tokens` | `generationConfig.thinkingConfig.thinkingBudget` |
| `thinking.type: adaptive` | `thinkingConfig.thinkingLevel` |
| `tool_choice: auto/none/any/tool` | `toolConfig.functionCallingConfig.mode: AUTO/NONE/ANY` |
| `tools[].input_schema` | `tools[].functionDeclarations[].parametersJsonSchema` |
| Stop reason: `tool_use` → `tool_use` / `MAX_TOKENS` → `max_tokens` / 其它 → `end_turn` |

**Anthropic SSE 事件序列**（流式响应组装）：
```
message_start → content_block_start → content_block_delta → content_block_stop → message_delta → message_stop
```
对应 `StreamParams.ResponseType ∈ {0=none, 1=text, 2=thinking, 3=tool_use}`，`ResponseIndex` 维护当前 content_block 索引。

**验收**：
- Claude Code CLI 用 `ANTHROPIC_BASE_URL=http://localhost:<port>` 指向 AgentBuddy 路由聚合，能成功对话 gemini-2.5-pro
- 流式输出断网/超时场景下 SSE 事件能正确闭合（不会丢 `message_stop`）
- Tool call name 经过 sanitize → 还原 测试通过

---

### Phase 2 — OpenAI Chat Completions ↔ Gemini（passthrough 优先）

**目标**：处理 OpenAI Chat 客户端（Cline、Continue、Cursor 部分模式、Codex CLI 非 Responses 路径）。

| 子任务 | Rust 文件 | 参考 CLIProxyAPI commit |
|--------|-----------|-------------------------|
| Google OpenAI 兼容端点探测 | `translator/common/http.rs` | — |
| OpenAI Chat → Gemini（自建翻译） | `translator/openai_gemini/request.rs` + `response.rs` | `internal/translator/openai/gemini/`（main） |
| Gemini → OpenAI Chat（自建翻译） | `translator/gemini_openai_chat/request.rs` + `response.rs` | `internal/translator/gemini/openai/chat-completions/`（main） |
| **passthrough 路径** | `handler.rs` 探测 `provider.base_url == "https://generativelanguage.googleapis.com"` 且路径匹配 `/v1beta/openai/v1/chat/completions`，跳过翻译直接转发 | — |

**决策**：**passthrough 优先**。Google 官方 OpenAI 兼容端点已经处理了工具调用、图片、流式，无需我们自建翻译器。只有当 base URL 是第三方代理（如 OpenRouter → Gemini）时才走自建翻译。

**验收**：
- Cline 配置 `apiBase = http://localhost:<port>/openai` 指向 Google provider（base URL `generativelanguage.googleapis.com`），passthrough 工作
- 第三方代理 base URL 走自建翻译，结果一致

---

### Phase 3 — 流式 SSE 状态机 + Tool name 冲突检测

**目标**：把流式响应组装的状态机打磨稳定，工具名 sanitize 升级到 hash-suffix 冲突检测。

| 子任务 | Rust 文件 | 参考 CLIProxyAPI commit |
|--------|-----------|-------------------------|
| collision-resistant ID 生成 | `translator/common/id_map.rs` | `fix(codex): make input ID sanitization collision-resistant and deterministic` `db143aebac93f9be136ba3d18bd75381d61a2750` (2026-08-11) |
| 状态机 fallback（item ID 不匹配） | `translator/params.rs` | `fix(codex): fall back to current tool-call state when item IDs don't match` `e9c44ae256c5ebbc3960bcda6f64ea603b8c6b35` (2026-08-11) |
| Claude thinking redacted_thinking 双向映射 | `translator/claude_gemini/response_stream.rs` 增量 | `fix(claude): rebuild the Responses reasoning chain` `9b1142399c2985788bd9be27497689f746aca6d1` (2026-08-03) |
| Claude caller system → legacy model 处理 | `translator/claude_gemini/request.rs` | `fix(claude): validate legacy-model system turns before sending` `189776aab1fc7523229633a830850b8375079849` (2026-08-11) |
| Claude clear_thinking strategy | `translator/claude_gemini/request.rs` | `fix(claude): drop auto context_management without eligible thinking` `8638f28db55624e7829157c2f629fb426caf7204` (2026-08-11) |

**验收**：
- 模拟 Gemini 流式 chunk 序列（tool name 仅在首 chunk，后续 chunk 只发 delta），验证 sanitize + recover 不丢不串
- 哈希冲突场景（两个工具 sanitize 后名字相同）：hash-suffix 算法让它们 deterministic 且不互相覆盖
- `cargo bench translator` 流式吞吐 > 5000 chunks/sec

---

### Phase 4 — OpenAI Responses API ↔ Gemini（Codex CLI 路径）

**目标**：处理 Codex CLI（OpenAI Responses 协议）。

| 子任务 | Rust 文件 | 参考 CLIProxyAPI commit |
|--------|-----------|-------------------------|
| Responses → Gemini 请求 | `translator/openai_openai_responses/request.rs` | `internal/translator/openai/openai/responses/`（main） |
| Gemini → Responses 流式 | `translator/gemini_openai_responses/response_stream.rs` | `internal/translator/gemini/openai/responses/`（main） |
| Responses reasoning signatures | `translator/openai_openai_responses/request.rs` | `fix(translator): OpenAI Responses reasoning signatures for Gemini` `3648bc155e8d2c760a6bd788208c271a3eb50010` (2026-06-29) |
| Codex multi-agent v2 tool | `translator/openai_openai_responses/request.rs` | `feat(codex): prepare multi-agent v2 tool definitions at the Responses boundary` `7fe8473766672a9763813210208bd4704b25b6e0` (2026-08-03) |
| custom_tool_call_output 文本/图片分流 | `translator/openai_openai_responses/request.rs` | `fix(openai): preserve structured and stringified custom tool outputs during Responses conversion` `934da2379d6272a704953a02322b666b2a2efa3e` (2026-08-11) |

**验收**：
- Codex CLI 配置 `OPENAI_BASE_URL=http://localhost:<port>/openai` 指向 Google provider，能成功对话
- Responses 流式 thinking signature 能正确回放（多轮 tool call 不丢上下文）

---

### Phase 5 — 多模态补全（图片 / 音频 / 视频 / PDF）

**目标**：覆盖 Claude/OpenAI 多模态输入、Gemini 多模态输出。

| 子任务 | Rust 文件 | 参考 CLIProxyAPI commit |
|--------|-----------|-------------------------|
| base64 图片 → inline_data | `translator/common/multimodal.rs` | `internal/translator/claude/gemini/`（main） |
| URL 图片 → file_data | `translator/common/multimodal.rs` | 同上 |
| 音频（mp3 / wav）→ inline_data | `translator/common/multimodal.rs` | 同上 |
| PDF（document）→ inline_data | `translator/common/multimodal.rs` | 同上 |
| Video（mp4）→ file_data（File API 上传） | `translator/common/multimodal.rs` + File API 集成 | 同上 |
| Gemini 输出图片 → Claude/OpenAI image block | `translator/claude_gemini/response.rs` + `translator/gemini_openai_chat/response.rs` | 同上 |

**验收**：
- Claude Code 上传 1 张本地 PNG，Gemini 解析正确
- Gemini 输出图片（`generateContent` 返回 `inline_data`），转换回 Claude `image` block / OpenAI `image_url`

---

### Phase 6 — 剩余 pair + Codex native 路径（按需）

按用户实际接入的客户端逐项：

| pair | 触发条件 | 对应 CLIProxyAPI 目录 |
|------|----------|----------------------|
| `codex_gemini` | Codex CLI 想直连 Gemini | `internal/translator/codex/gemini/` |
| `codex_openai_responses` | Codex native Responses 路径 | `internal/translator/codex/openai/responses/` |
| `codex_claude` | Codex → Claude（罕见） | `internal/translator/codex/claude/` |
| `antigravity_*` | 接入 Antigravity 客户端 | `internal/translator/antigravity/<target>/` |
| `openai_claude` / `claude_openai_chat` | 用户混用 Anthropic 客户端 + OpenAI provider | `internal/translator/openai/claude/` + `internal/translator/claude/openai/chat-completions/` |
| `interactions_claude` | Interactions API | `internal/translator/interactions/claude/` |

每加一个 pair，**必须同步在 `docs/cli_proxy_api_sync_state.json` 记录对应 commit SHA**。

---

### Phase 7 — 性能 + 监控 + 同步 CI

| 子任务 | 文件 |
|--------|------|
| `serde_json::Value` 缓存复用（避免每次重新 parse 翻译结果） | `translator/common/cache.rs` |
| Payload 零拷贝（`bytes::Bytes` 替代 `String`） | 翻译器签名重构 |
| 翻译时延 metrics（per-pair p50/p99） | `telemetry/translator.rs` + Prometheus exporter |
| 上游同步检查 CI | `.github/workflows/upstream-sync-check.yml` |
| 同步状态文件 | `docs/cli_proxy_api_sync_state.json` |
| 同步 playbook | `docs/SYNC_PLAYBOOK.md` |
| 同步检查脚本 | `scripts/check_upstream_sync.py` |

---

## 6. 风险与回退方案

| 风险 | 缓解 / 回退 |
|------|--------------|
| Phase 1 工期被低估（`gemini/claude/` 实际 ~14KB Go 代码，含大量边界 case） | 分两期交付：Phase 1a = 文本 + system + tool call + 图片；Phase 1b = thinking + 流式状态机 + 多模态 |
| Google OpenAI 兼容端点 passthrough 与自建翻译行为不一致 | 启动时打印 `effective_translator = "passthrough" | "<pair_name>"` 便于诊断；保留自建翻译作为兜底 |
| CLIProxyAPI 高频修复（~1.3 天一次 commit）我们跟不上 | Phase 7 的 CI 是兜底；优先 cherry-pick `fix(*)` 而不是 `feat(*)` |
| 测试用例难以 mock 流式 chunk 序列 | 用 `tokio::test` + `futures::stream` 构造合成 chunk stream；保存真实 Gemini 响应样本到 `tests/fixtures/gemini_*.json`（脱敏） |
| Translator registry 锁竞争 | 用 `Arc<RwLock<HashMap<...>>>` 读多写少场景；provider 写操作（`spawn_route_aggregation_pool_refresh`）已经异步化 |
| Gemini thinking signature 跨请求丢失（已观察到是 Gemini 原生 bug） | 在 Anthropic response stream 中 `signature_delta` 单独走 event，不混入 `thinking_delta`；保留空签名兜底 |

---

## 7. 关键决策点（用户拍板）

✅ **2026-08-12 全部拍板**：

1. ✅ **pair 命名约定**：`<source>_<target>/` 平铺
2. ✅ **Phase 6 范围**：本次 MVP = Phase 0~4
3. ✅ **同步检查脚本语言**：Python（零依赖、CI 友好）
4. ✅ **Antigravity 路径**：**不实现**
5. ✅ **同步 CI 频率**：每周一 9:00 UTC + 14 天阈值
6. ✅ **passthrough 优先级**：接受简化（Google OpenAI 兼容端点 passthrough）

### Phase 0 落地状态（2026-08-12）

- ✅ `src-tauri/src/route_aggregation/translator/` 子目录创建
- ✅ `mod.rs` — `TranslatorRegistry` 注册表 + re-exports
- ✅ `formats.rs` — `Format` 枚举（7 种）+ 映射 `RouteGroup` 测试
- ✅ `translatable.rs` — `Translatable` trait + `TranslateError`
- ✅ `params.rs` — `StreamParams` 流式状态机 + `ResponseType`
- ✅ `common/sse.rs` — `SseLineBuffer` 跨 chunk 累积 + `extract_data`/`extract_event`
- ✅ `common/tool_name.rs` — `sanitize` / `sanitize_with_occupied`（xxhash64 hash-suffix 冲突检测）/ `recover`
- ✅ `common/schema.rs` — `normalize_object_properties` 递归 + `normalize_tool_schemas`
- ✅ `common/id_map.rs` — `IdOccupancy` 冲突检测占位（Phase 4 接入）
- ✅ `common/thinking.rs` — `ThinkingMode` + Anthropic `thinking` 解析 + Gemini `thinkingConfig` 写入（Phase 5 接入）
- ✅ `common/multimodal.rs` — `InlineData` + MIME 分类占位（Phase 5 接入）
- ✅ `common/http.rs` — Google OpenAI 兼容端点探测（Phase 2 接入 ProviderRouter）
- ✅ `route_aggregation/mod.rs` 加 `pub mod translator;`
- ✅ 测试：60 个 translator 单元测试通过
- ✅ `cargo check` 0 warning、`cargo test --lib` 176 passed

### Phase 1 落地状态（2026-08-12）

- ✅ `translator/claude_gemini/` 子目录创建
  - `mod.rs` — `ClaudeToGeminiTranslator` + re-exports 自由函数
  - `request.rs` — `build_request(model, raw, stream)` 完整字段映射
    - `system` (string/array) → `systemInstruction`
    - `messages[].role: assistant` → `model`
    - `content: text/image/tool_use/tool_result/thinking` → `parts[].*`
    - `tools[].input_schema` → `functionDeclarations[].parametersJsonSchema`
    - `tool_choice: auto/none/any/tool` → `toolConfig.functionCallingConfig.mode`
    - `thinking.budget_tokens/adaptive` → `thinkingConfig.{thinkingBudget|thinkingLevel}`
    - `max_tokens/temperature/top_p/top_k/stop_sequences` → `generationConfig.*`
  - `response_stream.rs` — Gemini SSE chunk → Anthropic SSE（流式状态机）
    - `message_start` / `content_block_start` / `content_block_delta` / `content_block_stop` / `message_delta` / `message_stop`
    - text / thinking / functionCall 三种 part 类型分别处理
    - tool_use stop_reason 在含 functionCall 时强制 `tool_use`
  - `response_non_stream.rs` — Gemini JSON → Anthropic Messages JSON
- ✅ 测试：30 个 claude_gemini 单元测试通过
- ✅ `TranslatorRegistry` 加 `Mutex<HashMap>` 支持 `&self.register`（setup 阶段调用）
- ✅ `RouteAggregationState` 加 `translator_registry: Arc<TranslatorRegistry>` + `populate_default_translators()`
- ✅ `AppState` 加 `translator_registry` 字段
- ✅ `lib.rs` 在 setup 调用 `populate_default_translators()`
- ✅ `forwarder.rs` 接受 `translators` 参数，按 (source, target) 决定是否翻译
- ✅ URL 改写：Anthropic passthrough `/{base}/v1/messages`，Gemini 翻译 `/{base}/models/{model}:generateContent` 或 `:streamGenerateContent?alt=sse`
- ✅ Auth header 改写：Anthropic `x-api-key`、Gemini `x-goog-api-key`、OpenAI `Bearer`
- ✅ 流式响应翻译：`futures::stream::unfold` 包装 upstream stream，每 chunk 调 `claude_gemini::translate_response_stream`
- ✅ 非流式响应翻译：`axum::body::to_bytes` 读完整 body → `translate_response_non_stream` → 重组 Response
- ✅ `handler.rs` 加 `UnsupportedTranslation` 错误 → 501 NOT_IMPLEMENTED
- ✅ `Cargo.toml` 加 `bytes = "1"`
- ✅ `cargo check` 0 warning、`cargo test --lib` 206 passed

### Phase 1 待本地验证（用户在 Claude Code 内联调）

- 真实 Gemini API key 联调：Claude Code → AgentBuddy:port → Gemini API
- 流式响应长输出（> 100 chunks）正确闭合 `message_stop`
- tool_use 跨轮 context 保留
- thinking signature 跨轮回放
- 图片 base64 输入 → Gemini → 文字回答
- 工具调用错误处理（Gemini 返回错误时翻译成 Anthropic error）

### Phase 2 落地状态（2026-08-12）

- ✅ `translator/openai_gemini/` + `translator/gemini_openai_chat/`
  - `openai_gemini/request.rs` — OpenAI Chat → Gemini 请求（system / messages / tools / tool_choice / image_url data URL / input_audio）
  - `gemini_openai_chat/response_stream.rs` — Gemini SSE → OpenAI Chat SSE（chatcmpl chunk / tool_calls delta / [DONE]）
  - `gemini_openai_chat/response_non_stream.rs` — Gemini JSON → OpenAI Chat JSON
- ✅ `forwarder.rs` 加 `should_passthrough_google_openai_compat()`：当 provider 是 Google 直接 key + 客户端是 OpenAI Chat 协议时跳过自建翻译
- ✅ 测试：26 个新单元测试（openai_gemini 14 + gemini_openai_chat 11 + 集成 1）
- ✅ 累计 232 passed

### Phase 3 落地状态（2026-08-12）

- ✅ `Translatable::translate_request` 签名加 `&mut StreamParams` 参数（跨方向共享状态）
- ✅ `sanitize_with_occupied` 加反向查（幂等：同一原名多次 sanitize 返回同一 sanitized 名）
- ✅ `claude_gemini/request.rs` 与 `openai_gemini/request.rs` 的 `build_tools` / `build_tool_use_part` 全部改用 `sanitize_with_occupied`，写入 `params.sanitized_name_map`
- ✅ `forwarder.rs` 维护 per-request `StreamParams`：请求翻译阶段写入 tool_name_map，响应翻译阶段读出反查
- ✅ 测试：3 个新冲突检测测试（搜索.web + 搜索-web 冲突 / tool declarations 冲突 / 同名幂等）
- ✅ 累计 235 passed

### Phase 4 落地状态（2026-08-12）

- ✅ `translator/openai_openai_responses/` + `translator/gemini_openai_responses/`
  - `openai_openai_responses/request.rs` — Responses input items → Gemini contents（message / function_call / function_call_output / reasoning / instructions / tools / parallel_tool_calls）
  - `gemini_openai_responses/response_stream.rs` — Gemini SSE → Responses SSE（response.created / response.output_item.added / response.output_text.delta / response.function_call_arguments.delta / response.output_item.done / response.completed）
  - `gemini_openai_responses/response_non_stream.rs` — Gemini JSON → Responses JSON
- ✅ 测试：17 个新单元测试
- ✅ 累计 252 passed

### Phase 5 落地状态（2026-08-12）

- ✅ `multimodal.rs` 增强：`to_anthropic_image()` / `to_openai_image_url(detail)` / `parse_data_url()` / `build_data_url()` / `sniff_image_mime()`
- ✅ `claude_gemini/response_non_stream.rs` — Gemini 输出 inline_data → Anthropic `image` block
- ✅ 测试：6 个新单元测试
- ✅ 累计 258 passed

### Phase 6 跳过

按用户拍板"按需触发"，未实现：
- `codex_*` 系列（Codex native ↔ Gemini / OpenAI 等）
- `antigravity_*` 系列（用户拍板不做）
- `openai_claude` / `claude_openai_chat`（混用 Anthropic 客户端 + OpenAI provider 场景）
- `interactions_claude`（Interactions API）

每个 pair 的"是否实现"取决于用户后续接入的客户端类型。

### Phase 7 落地状态（2026-08-12）

- ✅ `scripts/check_upstream_sync.py` — Python 脚本，零依赖，GitHub API 查询每对 pair 上游最新 commit
- ✅ `docs/cli_proxy_api_sync_state.json` — 5 个已实现 pair 的初始同步状态
- ✅ `docs/SYNC_PLAYBOOK.md` — 同步操作手册（定位变更 / cherry-pick / 更新文件头 / 提 PR）
- ✅ `.github/workflows/upstream-sync-check.yml` — 每周一 9:00 UTC 跑一次；超 SLA 14d 报警
- ✅ 脚本运行验证：5 pairs 检测，1 error（24d 超 SLA）/ 4 warning（5-12d 内新 commit）/ 0 ok
- ✅ 累计 258 passed

### 累计 258 passed, 0 warning

| Phase | 累计测试 | 新增测试 |
|-------|---------|----------|
| Phase 0 | 60 | 60 |
| Phase 1 | 116 | 56（含 30 claude_gemini + 26 common 增量） |
| Phase 2 | 232 | 26 + passthrough hook |
| Phase 3 | 235 | 3 |
| Phase 4 | 252 | 17 |
| Phase 5 | 258 | 6 |
| Phase 6 | 258 | 0 (按需) |
| Phase 7 | 258 | 0 (CI/脚本) |

---

## 8. 参考 commit 索引（按 Phase 整理）

### Phase 0 关键 commit
- `c13dbcc24e1373e353338d90bdb38b8e4722e22b` (2026-06-18) `feat(translator): add test and logic to ensure object schemas include properties field`
- `ac8fb9706fb84bedfbd1f813738680fdc6767115` (2026-06-18) `feat(thinking): remove thinkingConfig for ModeNone with zero budget and no level`

### Phase 1 关键 commit
- `internal/translator/claude/gemini/` 当前 main（请求方向 Claude → Gemini）
- `internal/translator/gemini/claude/gemini_claude_request.go` 当前 main（请求方向 Gemini → Claude）
- `internal/translator/gemini/claude/gemini_claude_response.go` 当前 main（响应方向 Gemini → Claude，含 Params 流式状态机）

### Phase 3 关键 commit
- `db143aebac93f9be136ba3d18bd75381d61a2750` (2026-08-11) `fix(codex): make input ID sanitization collision-resistant and deterministic`
- `e9c44ae256c5ebbc3960bcda6f64ea603b8c6b35` (2026-08-11) `fix(codex): fall back to current tool-call state when item IDs don't match`
- `189776aab1fc7523229633a830850b8375079849` (2026-08-11) `fix(claude): validate legacy-model system turns before sending`
- `8638f28db55624e7829157c2f629fb426caf7204` (2026-08-11) `fix(claude): drop auto context_management without eligible thinking`
- `9b1142399c2985788bd9be27497689f746aca6d1` (2026-08-03) `fix(claude): rebuild the Responses reasoning chain`
- `6f8f11a3249e7e6a04ab917a2efa1b8db26bcd3d` (2026-08-03) `fix(claude): carry caller system inputs into Claude system blocks`

### Phase 4 关键 commit
- `00114bec1b76d985fd33a8a19f91c22ffed88580` (2026-06-29) `Merge: fix(responses): full transcript replay on WS-to-SSE Codex paths`
- `150e7f0dc50e3d3a0f7c4e552cc402ae105eb2a0` (2026-06-29) `fix(auth): repair force-mapped Responses SSE framing for WS forwarder`
- `3648bc155e8d2c760a6bd788208c271a3eb50010` (2026-06-29) `fix(translator): OpenAI Responses reasoning signatures for Gemini`
- `7fe8473766672a9763813210208bd4704b25b6e0` (2026-08-03) `feat(codex): prepare multi-agent v2 tool definitions at the Responses boundary for Codex clients`
- `934da2379d6272a704953a02322b666b2a2efa3e` (2026-08-11) `fix(openai): preserve structured and stringified custom tool outputs during Responses conversion`
- `8cf1d46f065c23cd3bd442b9865ea13a1bb8b24d` (2026-08-03) `fix(usage): account for Claude thinking tokens`

### 同步 anchor（首期基线）
- 当前最新 main commit（截至 2026-08-12）：`934da2379d6272a704953a02322b666b2a2efa3e`
- Phase 0 基线：`ac8fb9706fb84bedfbd1f813738680fdc6767115` (2026-06-18)

---

## 9. 工期估算（个人开发节奏）

| Phase | 估算 | 累计 |
|-------|------|------|
| Phase 0 | 3-5 天 | 5 |
| Phase 1（含端到端联调） | 7-10 天 | 15 |
| Phase 2 | 2-3 天 | 18 |
| Phase 3 | 3-5 天 | 23 |
| Phase 4 | 4-6 天 | 29 |
| Phase 5 | 3-4 天 | 33 |
| Phase 6（按需，每 pair 1-3 天） | — | — |
| Phase 7 | 2-3 天 | — |

MVP（Phase 0~4）总计 **约 4 周**；完整 Phase 5~7 **再加 1 周**。

注意：以上估算是单人节奏，且假设已经熟悉 CLIProxyAPI 代码库。第 1 个 Phase 1（含端到端联调）会偏慢，建议先做 Phase 0 + Phase 1a（Phase 1 的子集：文本 + system + tool call + 图片），跑通 Claude Code → Gemini-2.5-pro 之后再加速。