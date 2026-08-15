# CLIProxyAPI 上游同步操作手册

> **适用范围**：AgentBuddy 路由聚合中仿照 CLIProxyAPI 的客户端行为同步，包括请求伪装（cloaking）以及必要的响应恢复。
>
> **当前范围（2026-08-15）**：
>
> | 同步面 | 状态 |
> |--------|------|
> | **Claude Code cloaking**（请求形状、客户端指纹、身份和请求级策略） | ✅ 跟踪上游提交与客户端版本 |
> | **Codex CLI cloaking**（请求头和身份字段模拟） | ✅ 跟踪上游提交与客户端版本 |
>
> AgentBuddy 没有引入 CLIProxyAPI 的 Go 运行时、库或可执行文件。路由聚合的
> HTTP 服务、供应商选择、故障转移、熔断、日志和配置管理为 AgentBuddy 自有实现，
> 不按 CLIProxyAPI 提交逐文件同步；其中 `forwarder.rs` 只对 cloaking 接入边界、
> SSE 分帧和工具名响应恢复做回归核对。

---

## 0. 架构决策（2026-08-15）

当前路由聚合维护两种协议、三个同协议转发入口：Claude Messages、Claude
`count_tokens` 和 OpenAI Responses。

| # | 决策 | 理由 |
|---|------|------|
| 1 | **同协议透传** | 请求体、响应体和 SSE 不做协议转换 |
| 2 | **CLI 入站仅支持两种协议** | `/v1/messages` 与 `/v1/messages/count_tokens` 对应 Claude，`/v1/responses` 对应 Codex |
| 3 | **Cloaking 跟踪客户端版本** | Claude Code / Codex CLI 版本变化时重新核对上游提交和本地夹具 |

任何与以上决策冲突的 PR 都需先开 issue 重新讨论，并同步更新本节与 sync_state.json。

---

## 1. 同步检查

### 1.1 本地手动检查

```bash
# 需要 GITHUB_TOKEN 环境变量避免 API 限流（公网匿名 60 req/h）
export GITHUB_TOKEN=<your_token>
python3 scripts/check_upstream_sync.py
```

退出码：
- `0` — 无 error（Cloaking 各 file 都在 SLA 内，或 anchor 全部为空）
- `1` — 至少一个 Cloaking file 超 14 天未同步（CI 阻断）
- `2` — sync state 文件缺失（首次运行）

输出示例（v6 schema、2 种协议 / 3 个入口）：

```
=== Passthrough 链路覆盖 (3 paths) ===
  • Anthropic Messages → Anthropic Messages (A → A)
  • Anthropic count_tokens → Anthropic count_tokens (A → A)
  • OpenAI Responses → OpenAI Responses (OR → OR)

=== Cloaking 客户端指纹 (2 clients) ===
    📦 Claude Code 客户端指纹 (config: claude_code_version = 2.1.220, 11 files)
    ⚠ cloaking/claude_billing.rs (上游 2d 内有新 commit)
    ✗ cloaking/device_profile.rs (上游 135d 未同步，超 SLA 14d)
    ...
  📦 Codex CLI 客户端指纹 (config: codex_version = 0.146.0, 2 files)
    ⚠ cloaking/codex_cloaking.rs (上游 12d 内有新 commit)
    ...

新 sync state 已写到 docs/cli_proxy_api_sync_state.json.new
```

### 1.2 CI 集成

`.github/workflows/upstream-sync-check.yml` 在以下时机跑：

- 每周一 9:00 UTC（定时）
- 手动 `workflow_dispatch`（按需）
- cloaking/ 或 sync state 改动时（path filter）

CI 行为：
- 跑 `python3 scripts/check_upstream_sync.py`
- 退出码 1 → 失败（红 ✗）；0 → 成功（绿 ✓）
- artifacts 上传 `docs/cli_proxy_api_sync_state.json.new` 供 review

---

## 2. Cloaking 同步（高频）

### 2.1 何时需要重对 cloaking

- `src-tauri/src/route_aggregation/config.rs` 的 `claude_code_version` 或 `codex_version` 升级
- `claude_strict_mode`、`claude_sensitive_words`、`claude_cache_max_blocks` 或
  `claude_context_management` 的默认行为变化
- 上游 CLIProxyAPI `internal/runtime/executor/` 目录有新 commit（CI 自动报警）
- 用户报"cloaking 失效"或"被服务端识别为非官方客户端"

### 2.2 同步流程

1. **确认上游 commit**：
   ```bash
   gh api 'repos/router-for-me/CLIProxyAPI/commits?path=internal/runtime/executor/claude_executor_cloaking.go&per_page=5' \
     | jq -r '.[] | "  \(.sha[0:7])  \(.commit.author.date)  \(.commit.message | split("\n")[0])"'
   ```

2. **拉 diff 看具体改动**：
   ```bash
   gh api 'repos/router-for-me/CLIProxyAPI/commits/<NEW_FULL_SHA>' | jq -r '.files[].filename'
   ```

3. **映射到本地文件**（参考 `cli_proxy_api_sync_state.json` 的 `client_fingerprint_anchors`）。
   下表的 local 路径均相对于 `src-tauri/src/route_aggregation/`：
   | upstream | local |
   |-----------|-------|
   | `internal/runtime/executor/claude_executor_cloaking.go` | `cloaking/claude_cloaking.rs` / `claude_headers.rs` / `tool_remap.rs` |
   | `internal/runtime/executor/claude_executor_cloaking.go` | `cloaking/claude_cache.rs` / `claude_context.rs` / `claude_identity.rs` |
   | `internal/runtime/executor/claude_signing.go` | `cloaking/claude_billing.rs` |
   | `internal/runtime/executor/helps/claude_system_prompt.go` | `cloaking/claude_system_prompt.rs` |
   | `internal/runtime/executor/helps/claude_device_profile.go` | `cloaking/device_profile.rs` |
   | `internal/runtime/executor/helps/cloak_obfuscate.go` | `cloaking/obfuscate.rs` |
   | `internal/runtime/executor/codex_executor_request.go` | `cloaking/codex_cloaking.rs` / `codex_headers.rs` |
   | `internal/runtime/executor/claude_executor_cloaking.go` | `cloaking/header_scrub.rs` |

   `forwarder.rs` 中的 SSE 分帧、非流式响应工具名恢复和供应商重试是本地集成逻辑，
   不作为 CLIProxyAPI 文件镜像；但上游工具名协议变化时必须回归验证。

4. **同步常量与算法**（典型改动点）：
   - `claude_billing.rs` 的 `FINGERPRINT_SALT` 常量
   - `claude_headers.rs` 的 header 名 + value 模板
   - `claude_system_prompt.rs` 的 segment 字符串
   - `device_profile.rs` 的字段集合
   - `obfuscate.rs` 的敏感词列表
   - `tool_remap.rs` 的 `OAUTH_TOOL_RENAME_MAP`
   - `claude_cache.rs` 的 breakpoint 顺序、TTL 和数量上限
   - `claude_context.rs` 的 thinking 能力判断与 `clear_thinking` 类型
   - `claude_identity.rs` 的 user/session/account 作用域和生成格式

5. **更新 sync state** 的 `client_fingerprint_anchors[].files[].last_verified_*`：
   ```json
   {
     "local_path": "cloaking/device_profile.rs",
     "upstream_path": "internal/runtime/executor/helps/claude_device_profile.go",
     "last_verified_short_sha": "<NEW_SHORT_SHA>",
     "last_verified_full_sha": "<NEW_FULL_SHA>",
     "last_verified_date": "<YYYY-MM-DD>",
     "last_verified_note": "<本次同步涵盖的修复>"
   }
   ```

6. **更新 `config.rs` 版本常量**（如新版本号）：
   ```rust
   fn default_claude_code_version() -> String {
       "2.1.220".to_string()  // 当前 CLIProxyAPI Claude Code 指纹基线
   }
   ```

7. **跑测试 + 端到端**：
   ```bash
   cargo test --manifest-path src-tauri/Cargo.toml --lib route_aggregation
   # 启动 AgentBuddy → 开启路由聚合 → 触发一次 Claude Code / Codex CLI 请求 → 查日志确认 cloaking header 注入正常
   ```

8. **提 PR**，标题前缀 `[cloak-sync]`：
   ```bash
   git commit -m "[cloak-sync] claude 2.1.64 + device_profile obfuscate 135d 漂移修复"
   ```

---

## 3. 同步优先级

按 fix 的影响范围排：

| 优先级 | 类型 | 例子 |
|--------|------|------|
| P0 | Cloaking 客户端被服务端识别 | billing salt 不对、system prompt 段缺失 |
| P1 | Claude 协议兼容性（影响 Claude Code 客户端） | `fix(claude):` 系列 |
| P2 | Codex Responses（影响 Codex CLI 客户端） | `fix(codex):` / `fix(responses):` |
| P3 | 内部重构 / 性能优化 | `refactor:` / `perf:` |

P0 立即同步（< 1 周）；P1/P2 < 2 周；P3 攒到一定量同步一次。

### 3.1 不属于定期上游同步的功能

以下功能是 AgentBuddy 自有实现，不应因为 CLIProxyAPI 发布而直接复制或改写：

- `provider_router.rs` 的供应商池、模型过滤和多供应商切换。
- `circuit_breaker.rs` 的熔断状态机。
- `server.rs`、`router.rs`、`handler.rs` 的本地 HTTP 服务和 Tauri 集成。
- `log.rs`、`logfile.rs` 的日志留存、脱敏和预览。
- `forwarder.rs` 的请求重试、代理配置、SSE 背压和响应恢复；仅其 cloaking 接入边界需要随本地行为变更回归。

这些模块仍需要本项目自己的测试和版本维护，但不需要按 CLIProxyAPI commit 定时同步。

---

## 4. 常见问题

### 4.1 upstream 403 Rate Limit

设置 `GITHUB_TOKEN` 环境变量。公网匿名限制 60 req/h，token 提升到 5000 req/h。

### 4.2 CLIProxyAPI 大改 cloaking 接口

如果上游改了 cloaking 的常量布局（盐值字段顺序变了、header 名变了），需要 review 整个 `cloaking/` 目录，不是单文件 cherry-pick：

1. 在 issue 写明影响范围
2. 列出所有涉及文件
3. 按 §2 流程逐文件同步

### 4.3 AgentBuddy 实现超出 CLIProxyAPI

如果 AgentBuddy 已有 CLIProxyAPI 没有的功能（如配置化敏感词、作用域内存缓存、
响应工具名恢复或额外的零宽空格混淆变体），同步时只吸收上游行为变化，保留本地
实现和差异说明，禁止直接 cherry-pick Go 代码或覆盖 AgentBuddy 的路由集成。

### 4.4 sync state 文件冲突

多人同时改 `docs/cli_proxy_api_sync_state.json` 时 git 合并可能冲突。
解决：按时间排序覆盖，CI 输出的 `.json.new` 文件是参考。

---

## 5. 工具

- `scripts/check_upstream_sync.py` — 客户端指纹上游同步检查
- `docs/cli_proxy_api_sync_state.json` — passthrough 链路与客户端指纹状态
- `.github/workflows/upstream-sync-check.yml` — CI 工作流
- `gh api` — GitHub CLI（本地查 upstream）

需要更多帮助，开 issue 标 `sync-playbook` 标签。
