# CLIProxyAPI 上游同步操作手册

> 背景：AgentBuddy 路由聚合翻译层（`src-tauri/src/route_aggregation/translator/`）
> 对齐 CLIProxyAPI 的请求/响应翻译矩阵。每个翻译器文件头注释对应一个
> CLIProxyAPI commit（`CLIProxyAPI aligned: <sha> - <message>`）。本手册
> 描述当 CLIProxyAPI 上游更新后，如何把修复同步到 AgentBuddy。

---

## 1. 同步检查

### 1.1 本地手动检查

```bash
# 需要 GITHUB_TOKEN 环境变量避免 API 限流（公网匿名 60 req/h）
export GITHUB_TOKEN=<your_token>
python3 scripts/check_upstream_sync.py
```

输出示例：

```
=== CLIProxyAPI 上游同步检查 (4 pairs) ===

✓ Anthropic Messages → Google Gemini generateContent
    AgentBuddy: db143ae (2026-08-11)
    Upstream  : db143ae (2026-08-11, 0d 前)
    Status    : ok — 已对齐 upstream HEAD

⚠ OpenAI Chat Completions → Google Gemini generateContent
    AgentBuddy: c13dbcc (2026-06-18)
    Upstream  : 934da23 (2026-08-11, 47d 前)
    Status    : warning — 上游 47d 内有新 commit

✗ OpenAI Responses API → Google Gemini generateContent
    AgentBuddy: (none)
    Upstream  : 7fe8473 (2026-08-03, 21d 前)
    Status    : error — 上游 21d 未同步（超 SLA 14d）
```

退出码：
- `0` — 全部在 SLA 内
- `1` — 至少一个 pair 超 14 天未同步（CI 阻断）
- `2` — sync state 文件缺失（首次运行）

### 1.2 CI 集成

`.github/workflows/upstream-sync-check.yml` 在以下时机跑：

- 每周一 9:00 UTC（定时）
- 手动 `workflow_dispatch`（按需）
- 翻译器文件或 sync state 改动时（path filter）

CI 行为：
- 跑 `python3 scripts/check_upstream_sync.py`
- 退出码 1 → 失败（红 ✗）；0 → 成功（绿 ✓）
- artifacts 上传 `docs/cli_proxy_api_sync_state.json.new` 供 review

---

## 2. 同步流程

### 2.1 定位上游变更

```bash
# 1. 在 CLIProxyAPI 仓库看 pair 目录最近 30 天 commit
gh api 'repos/router-for-me/CLIProxyAPI/commits?path=internal/translator/claude/gemini&since=2026-07-12' \
  | jq -r '.[] | "  \(.sha[0:7])  \(.commit.author.date)  \(.commit.message | split("\n")[0])"'
```

列出：
```
  189776a  2026-08-11T10:42:19Z  fix(claude): validate legacy-model system turns before sending
  8638f28  2026-08-11T11:33:40Z  fix(claude): drop auto context_management without eligible thinking
  db143ae  2026-08-11T14:56:41Z  fix(codex): make input ID sanitization collision-resistant and deterministic
  ...
```

### 2.2 评估影响

逐个 commit 看 diff：

```bash
# 单个 commit 的 diff
gh api 'repos/router-for-me/CLIProxyAPI/commits/189776a' | jq -r '.files[].filename'
```

判断：
- ✅ **必须同步**：`fix(*)` bug fix 涉及 Gemini 模型行为 → 高优先级
- ⚠️ **评估后同步**：`feat(*)` 新功能 → 看 AgentBuddy 是否需要
- ❌ **可忽略**：纯测试文件 / 内部重构 / 已经包含的逻辑

### 2.3 cherry-pick 到 AgentBuddy

CLIProxyAPI 是 Go，AgentBuddy 是 Rust —— 不能直接 cherry-pick 源码。
**等价翻译**流程：

1. 读 CLIProxyAPI 改动（用 `gh api` 或 git clone）
2. 在 AgentBuddy 找到对应 Rust 文件（`src-tauri/src/route_aggregation/translator/<pair>/<file>.rs`）
3. 把 Go 代码逻辑翻译成 Rust（保留语义，可能简化）
4. 跑 `cargo test --lib route_aggregation::translator::<pair>` 验证
5. 跑全量 `cargo test --lib` 防止回归

### 2.4 更新文件头

每个翻译器 Rust 文件头都标注了对应 commit。同步后必须更新：

```rust
//! Translator: <source> → <target>
//!
//! CLIProxyAPI aligned: <NEW_SHORT_SHA> - <NEW_COMMIT_MESSAGE>
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/<NEW_FULL_SHA>
//! Last verified: <YYYY-MM-DD>
```

新 short sha 从 `git log` / GitHub API 拿前 7 位字符。

### 2.5 更新 sync state

编辑 `docs/cli_proxy_api_sync_state.json`：

```json
{
  "source": "Anthropic",
  "target": "Gemini",
  "source_dir": "internal/translator/claude/gemini",
  "target_dir": "internal/translator/gemini/claude",
  "last_verified_short_sha": "189776a",
  "last_verified_full_sha": "189776aab1fc7523229633a830850b8375079849",
  "last_verified_date": "2026-08-13",
  "last_verified_note": "同步 legacy-model system turn 验证 + auto context_management"
}
```

字段说明：
- `source` / `target`：Format 枚举字符串
- `source_dir` / `target_dir`：CLIProxyAPI 仓库中的对应目录（用于 GitHub API 查询）
- `last_verified_*`：本次同步的 commit 信息
- `last_verified_note`：本次同步涵盖的修复（可选）

### 2.6 提 PR

```bash
git add -A
git commit -m "[sync] claude/gemini — legacy-model system turn + auto context_management"
git push origin sync/claude-gemini-2026-08-13
gh pr create --title "[sync] claude_gemini translator 同步 2026-08-13" \
             --body "$(cat <<'EOF'
## 同步内容
- CLIProxyAPI `189776a` fix(claude): validate legacy-model system turns before sending
- CLIProxyAPI `8638f28` fix(claude): drop auto context_management without eligible thinking

## 影响
- Phase 5: Claude → Gemini 请求翻译时验证 `role: system` in messages 是否在 legacy 模型
- Phase 5: context_management 注入仅在 thinking 启用时保留

## 验证
- [x] `cargo check --lib` 0 warning
- [x] `cargo test --lib` 全量通过
- [x] 新增 4 个单元测试覆盖 legacy-model 场景
EOF
)"
```

---

## 3. 同步优先级

按 fix 的影响范围排：

| 优先级 | 类型 | 例子 |
|--------|------|------|
| P0 | Google Gemini API 行为变更（影响所有 Gemini 调用） | `fix(gemini):` 系列 |
| P1 | Claude 协议兼容性（影响 Claude Code 客户端） | `fix(claude):` 系列 |
| P2 | Codex Responses（影响 Codex CLI 客户端） | `fix(codex):` / `fix(responses):` |
| P3 | 内部重构 / 性能优化 | `refactor:` / `perf:` |
| P4 | 新功能（仅在 AgentBuddy 用户有需求时同步） | `feat(*):` |

P0/P1 立即同步（< 1 周）；P2 < 2 周；P3/P4 攒到一定量同步一次。

---

## 4. 常见问题

### 4.1 upstream 403 Rate Limit

设置 `GITHUB_TOKEN` 环境变量。公网匿名限制 60 req/h，token 提升到 5000 req/h。

### 4.2 CLIProxyAPI 大改接口

如果上游改了 translator 接口签名（trait 变化），需要重写整个 translator，
不是 cherry-pick。Phase 重排：

1. 在 issue 写明影响范围
2. 选 1 个 pair 重写（试点）
3. 跑端到端
4. 推广到其它 pair

### 4.3 AgentBuddy 实现超出 CLIProxyAPI

如果 AgentBuddy 已有 CLIProxyAPI 没有的功能（如 `mod.rs` 里的"passthrough 优先"），
同步时只 cherry-pick CLIProxyAPI 同步部分，自有部分保留。

### 4.4 sync state 文件冲突

多人同时改 `docs/cli_proxy_api_sync_state.json` 时 git 合并可能冲突。
解决：按时间排序覆盖，CI 输出的 `.json.new` 文件是参考。

---

## 5. 工具

- `scripts/check_upstream_sync.py` — 同步检查
- `docs/cli_proxy_api_sync_state.json` — 状态文件
- `.github/workflows/upstream-sync-check.yml` — CI 工作流
- `gh api` — GitHub CLI（本地查 upstream）

需要更多帮助，开 issue 标 `sync-playbook` 标签。
