# CLIProxyAPI Claude Cloaking 完整移植方案

> 目标：将 CLIProxyAPI 当前 Claude Code cloaking 的有效行为，按 AgentBuddy 的 Rust 路由聚合架构重新实现，并通过可复现的请求/响应夹具验证。
>
> 当前状态：AgentBuddy 已有基础 cloaking 实现，但不是 CLIProxyAPI 的完整行为等价实现。本方案用于追踪后续移植进度。

## 1. 范围与非目标

### 1.1 本次范围

- Claude Messages 同协议透传链路中的请求 cloaking。
- Claude Code 客户端识别、请求头和设备指纹。
- billing header 与 CCH 签名。
- 系统提示词布局、调用方 system block 重排及 legacy model 兼容。
- OAuth 身份、`user_id`、工具名称和敏感词处理。
- prompt cache、context management、`count_tokens` 等 Claude 请求级策略。
- 配置开关、错误分类、日志脱敏和回归测试。

### 1.2 明确不直接移植的内容

- CLIProxyAPI 的 Go 执行器、HTTP 客户端、路由器和认证生命周期。
- 上游专属的 KV 存储实现；只移植设备档案所需的行为，存储方式使用 AgentBuddy 现有配置/缓存能力。
- 与 Claude cloaking 无关的协议转换、供应商路由和 passthrough 逻辑。
- 没有被 AgentBuddy 路由聚合使用的 Antigravity 或其他客户端专属分支。

## 2. 当前基线

| 项目 | 当前情况 | 完成目标 |
|------|----------|----------|
| Claude 请求头 | 已有基础注入，版本基线已更新 | 与目标 CLIProxyAPI 提交的有效字段、顺序和覆盖规则一致 |
| 设备指纹 | 已有进程内 TTL 缓存 | 补齐版本解析、平台映射、候选 profile 校验和稳定化 |
| billing/CCH | 已有简化 hash 生成 | 按上游最终 body 规则精确计算，并支持 placeholder 替换 |
| system prompt | 已有固定片段和 reminder | 支持字符串/数组、严格/非严格模式和 system turn 放置策略 |
| `user_id` | 已有随机生成 | 对齐 OAuth/非 OAuth 的生成、缓存和反向映射行为 |
| 工具重命名 | 已有基础映射 | 对齐上游映射、嵌套位置和响应恢复规则 |
| 敏感词混淆 | 已有递归 JSON 字符串处理 | 改为请求语义范围明确、可配置、可重复测试的处理器 |
| cache/context 策略 | 尚未完整实现 | 补齐 cache control、context management 和限制规则 |
| `count_tokens` | 仅有通用透传能力 | 对齐 Claude Code 的最小 wire shape 和 system relocation |
| 回归验证 | 仅有模块级 Rust 测试 | 增加固定夹具、属性测试和上游差异检查 |

## 3. 分阶段实施

### Phase 0：冻结基线与建立夹具（预计 1–2 人日）

- [ ] 固定目标 CLIProxyAPI 提交、Claude Code 版本和本地配置样例。
- [ ] 从上游整理相关文件及依赖关系：
  - `claude_executor_cloaking.go`
  - `claude_signing.go`
  - `helps/claude_device_profile.go`
  - `helps/claude_system_prompt.go`
  - `helps/cloak_obfuscate.go`
  - Claude/Codex 请求头与身份辅助函数。
- [ ] 建立最小请求夹具：普通请求、工具请求、数组 system、字符串 system、无 system、流式请求、`count_tokens`。
- [ ] 记录每个夹具的输入 JSON、关键输出 JSON、关键请求头和错误结果。
- [ ] 明确哪些字段允许随机化，测试比较时使用结构化断言而非完整字符串比较。

验收标准：同一夹具可以在 Rust 测试中稳定重放；随机 UUID、时间和 hash 均有明确比较策略。

### Phase 1：请求身份与设备指纹（预计 2–3 人日）

- [ ] 抽象 `ClaudeClientProfile`，统一承载版本、User-Agent、Stainless 版本、OS 和架构。
- [ ] 实现 Claude Code User-Agent 解析和版本比较。
- [ ] 实现 OS/架构映射及非法值回退。
- [ ] 实现候选 profile 与基线 profile 的完整校验。
- [ ] 将当前进程内缓存改造成按 profile/auth scope 隔离的稳定缓存。
- [ ] 明确 TTL、版本升级和并发读写行为。
- [ ] 对齐 Claude 请求头覆盖优先级，避免透传第三方指纹头。

验收标准：同一稳定 profile 在 TTL 内保持一致；版本升级会生成新 profile；非法或第三方头不会泄漏到上游。

### Phase 2：system prompt 与请求形状（预计 3–4 人日）

- [ ] 支持 top-level system 的字符串、文本数组和非法 block 分类。
- [ ] 实现 billing block、Claude agent block 与调用方 system block 的正确顺序。
- [ ] 实现严格模式和非严格模式。
- [ ] 将调用方 system 内容注入首个 user message 时保留文本边界和 tool result 顺序。
- [ ] 实现现代模型的 mid-conversation system message 放置。
- [ ] 实现 legacy model 的兼容判断和请求级错误。
- [ ] 实现 `count_tokens` 的最小 Claude Code 请求形状及 system relocation。

验收标准：所有 Phase 0 system 夹具的结构化输出与上游参考结果一致；非法 system block 返回稳定、可识别的请求级错误。

### Phase 3：billing header 与 CCH 精确签名（预计 2–3 人日）

- [ ] 对齐 fingerprint salt、版本 build hash 和 entrypoint/workload 字段。
- [ ] 实现 billing placeholder 注入，不重复插入 system block。
- [ ] 按上游规则生成 CCH 的未签名 body：只替换 CCH 数字，不重新序列化无关 JSON。
- [ ] 实现 JSON 扫描、字符串清空、dispatch-only 字段排除和字段顺序保持。
- [ ] 覆盖短 body、无 system、空 system、非法 JSON 和已有 CCH 等边界情况。
- [ ] 使用上游已知输入输出向量验证 xxHash64 seed 和截断规则。

验收标准：固定输入的 CCH 与参考向量完全一致；重复处理不会产生多个 billing block 或改变非目标字段。

### Phase 4：身份、工具和敏感内容策略（预计 2–3 人日）

- [ ] 对齐 OAuth 与非 OAuth 的 `user_id` 生成策略。
- [ ] 明确 user/session/account 标识的缓存范围及响应恢复需求。
- [ ] 完善 OAuth tool rename map，覆盖 tools、tool use、tool result 等位置。
- [ ] 实现可配置敏感词 matcher：过滤空词、最长词优先、大小写处理和重复混淆保护。
- [ ] 仅在 Claude 请求语义允许的 system/message 文本范围内处理敏感词。
- [ ] 保证中文、组合字符和非 ASCII 前缀不会造成 UTF-8 边界错误。

验收标准：请求和响应中的工具名可以按映射往返；敏感词处理不修改字段名、数字、图片或工具参数结构。

### Phase 5：cache control 与 context management（预计 3–5 人日）

- [ ] 对齐 tools、system、messages 的 cache breakpoint 顺序。
- [ ] 实现已有 cache control 的保留和 TTL 规范化。
- [ ] 实现最大 cache block 数限制及 deferred tool 排除。
- [ ] 实现无 system、字符串 system、空 system 的 cache fallback。
- [ ] 实现 context management 注入条件、thinking 能力判断和重复注入保护。
- [ ] 对齐流式/非流式及失败重试时的请求状态处理。

验收标准：缓存断点数量、顺序和 TTL 满足上游规则；同一请求重复执行不会累加策略字段。

### Phase 6：集成、差异测试和发布门禁（预计 2–4 人日）

- [ ] 将各阶段逻辑接入 `claude_cloaking.rs`，保持 `forwarder.rs` 的 passthrough 边界不变。
- [ ] 将错误分为请求级错误、配置错误和上游错误，避免错误重试污染请求。
- [ ] 增加固定夹具测试、属性测试、并发缓存测试和 Unicode 边界测试。
- [ ] 增加 Claude Code/Codex 头部回归测试，防止两个协议的身份字段串线。
- [ ] 运行 `cargo test --lib route_aggregation`、Clippy 和构建检查。
- [ ] 更新 `docs/cli_proxy_api_sync_state.json`，只在对应行为真正完成后推进 anchor。
- [ ] 做一次人工端到端请求，核对请求头、请求体、流式响应和日志脱敏。

验收标准：测试、构建和端到端检查全部通过；同步状态中的每个 anchor 都有对应代码或明确的本地差异说明。

## 4. 建议的代码拆分

```text
src-tauri/src/route_aggregation/cloaking/
├── claude_cloaking.rs       # 编排与模式选择
├── claude_profile.rs        # 客户端识别和设备指纹
├── claude_headers.rs        # 请求头覆盖和身份头
├── claude_system_prompt.rs  # system block / system turn 布局
├── claude_billing.rs        # billing header 和 CCH
├── claude_cache.rs          # cache control 策略
├── claude_context.rs        # context management
├── claude_identity.rs       # user_id、OAuth 身份和映射状态
├── obfuscate.rs             # 可配置敏感词处理
└── tool_remap.rs            # 工具名双向映射
```

拆分时保持每个模块只负责一种请求变换；`claude_cloaking.rs` 只编排顺序、传递上下文和汇总错误，避免继续膨胀成单体函数。

## 5. 关键风险与应对

| 风险 | 影响 | 应对 |
|------|------|------|
| 上游频繁改变请求形状 | 高 | 固定提交 + 夹具回放 + 每次升级先跑差异测试 |
| CCH 依赖原始 JSON 字节布局 | 高 | 禁止无意的 serde 重序列化，使用字节级扫描和向量测试 |
| OAuth/KV 语义无法一对一映射 | 高 | 先抽象行为接口，使用 AgentBuddy 本地缓存实现，明确降级策略 |
| system turn 与模型能力耦合 | 高 | 建立模型能力表，错误必须是请求级且可解释 |
| 随机 UUID、日期、时区造成测试抖动 | 中 | 注入 clock/id provider，默认实现只在生产层生成 |
| 将上游专属逻辑误用于 passthrough | 中 | 保持协议边界，只在 Claude Messages 路由调用 cloaking |
| 同步状态虚报完全对齐 | 高 | anchor 只在行为实现和测试完成后更新；保留差异说明 |

## 6. 进度记录

更新时间：2026-08-15

- [x] Phase 0：已有基础同步状态和上游提交核对；完整夹具待补齐。
- [ ] Phase 1：请求身份与设备指纹
- [ ] Phase 2：system prompt 与请求形状
- [ ] Phase 3：billing header 与 CCH 精确签名
- [ ] Phase 4：身份、工具和敏感内容策略
- [ ] Phase 5：cache control 与 context management
- [ ] Phase 6：集成、差异测试和发布门禁

### 当前已落地的基础同步

- Claude Code 基线：`2.1.220 / 0.94.0 / v26.3.0`。
- CCH seed 与 Codex `Session-Id` 头名称已同步。
- 上游 `helps/claude_device_profile.go` 和 `helps/cloak_obfuscate.go` 路径已修正。
- 这些改动不等同于完整 Claude cloaking 行为移植，后续阶段完成前不得标记为完整对齐。

## 7. 参考资料

- `docs/SYNC_PLAYBOOK.md`
- `docs/cli_proxy_api_sync_state.json`
- `scripts/check_upstream_sync.py`
- CLIProxyAPI：`router-for-me/CLIProxyAPI`
