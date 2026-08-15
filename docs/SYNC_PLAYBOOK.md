# CLIProxyAPI 上游同步操作手册

> **适用范围**：AgentBuddy 路由聚合中的客户端指纹（cloaking）同步。
>
> **Phase 5（2026-08-13）当前范围**：
>
> | 同步面 | 状态 |
> |--------|------|
> | **Cloaking**（客户端指纹：Claude Code / Codex CLI 模拟） | ✅ 跟踪客户端版本与上游提交 |
>
> 路由聚合 passthrough 核心（`handler.rs` / `forwarder.rs` /
> `provider_router.rs` / `circuit_breaker.rs` / `log.rs` / `logfile.rs` /
> `server.rs` / `router.rs` / `config.rs` / `types.rs` / `mod.rs`）为
> AgentBuddy 原创，不在同步范围内。

---

## 0. 架构决策（2026-08-13 Phase 5 拍板）

当前路由聚合仅维护两条同协议透传链路：Claude Messages 和 OpenAI Responses。

| # | 决策 | 理由 |
|---|------|------|
| 1 | **同协议透传** | 请求体、响应体和 SSE 不做协议转换 |
| 2 | **CLI 入站仅支持两种协议** | `/v1/messages` 与 `/v1/responses` 分别对应 Claude 和 Codex |
| 3 | **Cloaking 跟踪客户端版本** | Claude Code / Codex CLI 版本变化时重新核对上游提交 |

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

输出示例（v5 schema、passthrough + cloaking only）：

```
=== Passthrough 链路覆盖 (2 paths) ===
  • Anthropic Messages → Anthropic Messages (A → A)
  • OpenAI Responses → OpenAI Responses (OR → OR)

=== Cloaking 客户端指纹 (2 clients) ===
    📦 Claude Code 客户端指纹 (config: claude_code_version = 2.1.220, 8 files)
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

3. **映射到本地文件**（参考 `cli_proxy_api_sync_state.json` 的 `client_fingerprint_anchors`）：
   | upstream | local |
   |-----------|-------|
   | `internal/runtime/executor/claude_executor_cloaking.go` | `cloaking/claude_cloaking.rs` / `claude_headers.rs` / `tool_remap.rs` |
   | `internal/runtime/executor/claude_signing.go` | `cloaking/claude_billing.rs` |
   | `internal/runtime/executor/helps/claude_system_prompt.go` | `cloaking/claude_system_prompt.rs` |
| `internal/runtime/executor/helps/claude_device_profile.go` | `cloaking/device_profile.rs` |
| `internal/runtime/executor/helps/cloak_obfuscate.go` | `cloaking/obfuscate.rs` |
   | `internal/runtime/executor/codex_executor_request.go` | `cloaking/codex_cloaking.rs` / `codex_headers.rs` |

4. **同步常量与算法**（典型改动点）：
   - `claude_billing.rs` 的 `FINGERPRINT_SALT` 常量
   - `claude_headers.rs` 的 header 名 + value 模板
   - `claude_system_prompt.rs` 的 segment 字符串
   - `device_profile.rs` 的字段集合
   - `obfuscate.rs` 的敏感词列表
   - `tool_remap.rs` 的 `OAUTH_TOOL_RENAME_MAP`

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
   cd src-tauri && cargo test --lib route_aggregation::cloaking
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

如果 AgentBuddy 已有 CLIProxyAPI 没有的功能（如 cloaking 额外的零宽空格混淆变体），同步时只 cherry-pick CLIProxyAPI 同步部分，自有部分保留。

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
