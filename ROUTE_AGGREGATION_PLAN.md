# 路由聚合功能实施计划

> **版本**: 1.0  
> **创建日期**: 2026-07-31  
> **状态**: 代码实现完成，待集成测试  
> **关联待办**: waitTODO.txt 第14项「本地路由聚合+故障转移」  
> **参考项目**: [cc-switch](https://github.com/farion1231/cc-switch) (Rust/Tauri)、[CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) (Go)

---

## 1. 功能概述

在 AgentBuddy 中实现本地路由代理聚合功能，将已添加的 AI 供应商聚合为统一的本地代理端点。支持 Claude Code 路由（含整流器伪装）和 Codex 路由（含客户端模拟伪装），两条路由可独立开关，开启后自动聚合对应类型的供应商，提供故障转移能力。

用户只需在 CLI 客户端中配置本地代理地址（如 `127.0.0.1:16888`），即可通过 AgentBuddy 代理请求到多个供应商，享受自动故障转移和请求伪装能力。

---

## 2. 架构设计

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                     AgentBuddy 桌面应用                       │
│  ┌─────────────┐  ┌──────────────────────────────────────┐  │
│  │  前端 UI     │  │         Rust 后端 (Tauri 2)           │  │
│  │             │  │  ┌────────────────────────────────┐  │  │
│  │ 路由聚合页面  │──▶│  │     RouteAggregator 模块      │  │  │
│  │ - CC 开关    │  │  │  ┌──────────┐ ┌─────────────┐ │  │  │
│  │ - Codex 开关 │  │  │  │CC 路由组   │ │Codex 路由组  │ │  │  │
│  │ - 供应商列表  │  │  │  │(整流器)    │ │(客户端模拟)  │ │  │  │
│  │ - 故障转移    │  │  │  └─────┬─────┘ └──────┬──────┘ │  │  │
│  │             │  │  │        │  故障转移      │        │  │  │
│  │             │  │  │        ▼               ▼        │  │  │
│  │             │  │  │  ┌────────────────────────────┐ │  │  │
│  │             │  │  │  │  CircuitBreaker (熔断器)    │ │  │  │
│  │             │  │  │  └────────────────────────────┘ │  │  │
│  │             │  │  │        │               │        │  │  │
│  │             │  │  │  ┌─────▼───────────────▼────┐  │  │  │
│  │             │  │  │  │   Axum HTTP Server       │  │  │  │
│  │             │  │  │  │   127.0.0.1:16888        │  │  │  │
│  │             │  │  │  └─────────────────────────┘  │  │  │
│  └─────────────┘  │  └────────────────────────────────┘  │  │
│                   └──────────────────────────────────────┘  │
└──────────────────────────┬──────────────────────────────────┘
                           │
                    ┌──────┴──────┐
                    │ CLI 客户端   │
                    │ Claude Code │
                    │ Codex CLI   │
                    └─────────────┘
```

### 2.2 核心设计决策

| 决策项 | 选择 | 理由 |
|--------|------|------|
| HTTP 服务器框架 | **axum 0.8 + hyper 1.x** | Tauri 2 传递依赖已有 tower 0.5 / hyper 1.x / tower-http 0.6；axum 0.8 依赖 tower 0.5，版本完全对齐，避免双版本 tower 冲突 |
| 异步运行时 | **tokio** (复用 Tauri 内置) | 无需引入额外运行时 |
| 路由模式 | **单服务器 + 路径分组** | 一个端口 16888，通过路径区分 CC/Codex 路由组，两组可独立开关 |
| 供应商筛选 | **按类型自动归类** | Claude Code 路由组 ← `anthropic` + `universal` 类型供应商；Codex 路由组 ← `openai` + `universal` 类型供应商 |
| 整流器实现 | **Rust 移植 CLIProxyAPI** | 将 Go 版 cloaking 逻辑移植为 Rust，保持功能对等 |
| TLS 指纹伪装 | **暂不实现 uTLS** | Rust 生态无成熟 uTLS 库；cc-switch 也未做 TLS 指纹；后续可评估 `boring`/自定义 |
| 熔断器 | **三态熔断 (Closed/Open/HalfOpen)** | 直接采用 cc-switch 的成熟设计 |
| 配置存储 | **SQLite + config.json 双层** | 运行时状态（熔断器、健康度）存内存；持久配置（开关、端口、供应商排序）存 SQLite |

### 2.3 路由组与供应商映射

```
Claude Code 路由组 (路径: /v1/messages, /claude/v1/messages)
  ├── Provider A (type=anthropic, baseUrl=https://api.anthropic.com)
  ├── Provider B (type=universal, baseUrl=https://relay.example.com)
  └── Provider C (type=universal, baseUrl=https://api.another.com)
      ↓ 整流器处理
      ├── 注入 Claude Code 系统提示
      ├── 伪造 billing header + CCH 签名
      ├── 注入 Stainless SDK 头部
      ├── 伪造 user_id
      └── OAuth 工具名重映射

Codex 路由组 (路径: /v1/chat/completions, /v1/responses, /v1/models)
  ├── Provider B (type=universal, openaiBaseUrl=https://relay.example.com/v1)
  └── Provider D (type=openai, baseUrl=https://api.openai.com)
      ↓ 客户端模拟处理
      ├── 注入 codex-tui User-Agent
      ├── 注入 Originator: codex-tui
      ├── 注入 session_id
      └── 身份混淆 (prompt_cache_key 等)
```

**universal 类型供应商会同时出现在两个路由组中**，可以各自独立开关其在每个路由组中的参与。

---

## 3. 技术方案详情

### 3.1 Rust 后端新增模块

#### 3.1.1 模块结构

```
src-tauri/src/
├── lib.rs                          ← 注册新命令 + setup 启动代理
├── config.rs                       ← 扩展 RouteAggregationConfig
├── db.rs                           ← 新增 route_aggregation 表
├── ai_provider.rs                  ← 现有，无需改动
├── http_client.rs                  ← 现有，代理服务器内部不复用此模块
└── route_aggregation/              ← 【新增】路由聚合模块
    ├── mod.rs                      ← 模块入口 + 公共类型
    ├── server.rs                   ← Axum HTTP 服务器生命周期管理
    ├── router.rs                   ← 路由注册 + 请求分发
    ├── handler.rs                  ← 请求处理器（入口）
    ├── forwarder.rs                ← 请求转发器（故障转移 + Header 注入）
    ├── provider_router.rs          ← 供应商选择 + 熔断器管理
    ├── circuit_breaker.rs          ← 三态熔断器实现
    ├── types.rs                    ← 数据结构定义
    ├── config.rs                   ← 路由聚合配置结构
    ├── cloaking/                   ← 【伪装/整流子模块】
    │   ├── mod.rs
    │   ├── claude_cloaking.rs      ← Claude Code 整流器主逻辑
    │   ├── codex_cloaking.rs       ← Codex 客户端模拟主逻辑
    │   ├── claude_system_prompt.rs ← Claude Code 系统提示常量
    │   ├── claude_billing.rs       ← Billing header 伪造 + CCH 签名
    │   ├── claude_headers.rs       ← Claude 请求头注入
    │   ├── codex_headers.rs        ← Codex 请求头注入
    │   ├── tool_remap.rs           ← OAuth 工具名重映射
    │   ├── obfuscate.rs            ← 敏感词零宽空格混淆
    │   ├── device_profile.rs       ← 设备指纹稳定化
    │   └── header_scrub.rs         ← 代理指纹头清除
    └── async_client.rs             ← 异步 HTTP 客户端 (tokio + reqwest async)
```

#### 3.1.2 核心数据结构

```rust
// route_aggregation/types.rs

/// 路由组类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteGroup {
    ClaudeCode,  // /v1/messages
    Codex,       // /v1/chat/completions, /v1/responses
}

/// 路由聚合运行时状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAggregationStatus {
    pub server_running: bool,
    pub listen_address: String,
    pub listen_port: u16,
    pub claude_code: GroupStatus,
    pub codex: GroupStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupStatus {
    pub enabled: bool,
    pub active_providers: Vec<ProviderRouteStatus>,
    pub total_providers: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRouteStatus {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,           // 是否参与此路由组
    pub circuit_state: String,   // "closed" | "open" | "half_open"
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
    pub request_count: u64,
    pub success_count: u64,
}

/// 路由聚合持久配置（SQLite）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAggregationConfig {
    pub claude_code_enabled: bool,
    pub codex_enabled: bool,
    pub listen_address: String,       // 默认 "127.0.0.1"
    pub listen_port: u16,             // 默认 16888
    pub auto_failover: bool,          // 默认 true
    pub max_retries: u32,             // 默认 3
    pub stream_first_byte_timeout: u64, // 秒，默认 60
    pub stream_idle_timeout: u64,       // 秒，默认 120
    pub non_stream_total_timeout: u64,  // 秒，默认 600
    pub cloaking_mode: CloakingMode,    // 默认 Auto
    pub claude_code_version: String,    // 默认 "2.1.63"
    pub codex_version: String,          // 默认 "0.146.0"
}

/// 供应商在路由组中的开关
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRouteToggle {
    pub provider_id: String,
    pub group: RouteGroup,
    pub enabled: bool,
    pub sort_order: i32,
}
```

#### 3.1.3 Axum 服务器生命周期

```rust
// route_aggregation/server.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use axum::Router;

pub struct RouteAggregationServer {
    /// 服务器句柄，用于优雅关闭
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// 运行时状态（内存，供 Tauri 命令读取）
    status: Arc<RwLock<RouteAggregationStatus>>,
    /// 配置快照
    config: Arc<RwLock<RouteAggregationConfig>>,
    /// 供应商路由器
    router: Arc<provider_router::ProviderRouter>,
}

impl RouteAggregationServer {
    /// 启动本地代理服务器
    pub async fn start(config: RouteAggregationConfig) -> Result<Self, String> {
        let addr = format!("{}:{}", config.listen_address, config.listen_port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("绑定地址 {} 失败: {}", addr, e))?;
        
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let app = build_router(config.clone());
        
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("代理服务器异常退出");
        });
        
        Ok(Self { /* ... */ })
    }
    
    /// 热更新配置（不停机切换供应商列表、调整超时等）
    pub async fn apply_config(&self, config: RouteAggregationConfig) {
        // 更新配置快照 + 通知 provider_router 刷新供应商池
    }
    
    /// 优雅关闭
    pub async fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn build_router(config: RouteAggregationConfig) -> Router {
    use axum::routing::post;
    let mut router = Router::new();
    
    if config.claude_code_enabled {
        router = router
            .route("/v1/messages", post(handler::handle_claude_messages))
            .route("/claude/v1/messages", post(handler::handle_claude_messages));
    }
    
    if config.codex_enabled {
        router = router
            .route("/v1/chat/completions", post(handler::handle_codex_chat))
            .route("/v1/responses", post(handler::handle_codex_responses))
            .route("/v1/models", axum::routing::get(handler::handle_list_models));
    }
    
    router
}
```

#### 3.1.4 请求转发器（故障转移核心）

```rust
// route_aggregation/forwarder.rs

pub async fn forward_with_retry(
    group: RouteGroup,
    request: ForwardRequest,
    config: &RouteAggregationConfig,
    router: &ProviderRouter,
) -> Result<ForwardResponse, ForwardError> {
    let providers = router.select_providers(group, config.auto_failover);
    
    if providers.is_empty() {
        return Err(ForwardError::NoAvailableProvider);
    }
    
    let max_attempts = (config.max_retries + 1).min(providers.len() as u32);
    
    for (index, provider) in providers.iter().take(max_attempts as usize).enumerate() {
        // 1. 熔断器检查
        if !router.can_attempt(provider, group) {
            continue;  // 供应商已熔断，跳过
        }
        
        // 2. 应用整流器/伪装
        let (modified_body, modified_headers) = match group {
            RouteGroup::ClaudeCode => {
                cloaking::claude_cloaking::apply_cloaking(
                    &request.body, &request.headers, config
                )?
            }
            RouteGroup::Codex => {
                cloaking::codex_cloaking::apply_cloaking(
                    &request.body, &request.headers, config
                )?
            }
        };
        
        // 3. 构建上游 URL
        let upstream_url = build_upstream_url(provider, group, &request.path);
        
        // 4. 发送请求
        match async_client::send_request(
            &upstream_url, &modified_headers, &modified_body,
            config.stream_first_byte_timeout, config.stream_idle_timeout,
            config.non_stream_total_timeout, request.stream,
        ).await {
            Ok(resp) => {
                router.record_success(provider, group);
                return Ok(resp);
            }
            Err(e) => {
                router.record_failure(provider, group, &e);
                log::warn!("供应商 {} 请求失败，尝试下一个: {}", provider.name, e);
                continue;
            }
        }
    }
    
    Err(ForwardError::AllProvidersFailed)
}
```

#### 3.1.5 熔断器实现

```rust
// route_aggregation/circuit_breaker.rs

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,   // 正常放行
    Open,     // 熔断中，拒绝请求
    HalfOpen, // 半开，仅允许探测
}

pub struct CircuitBreaker {
    state: Mutex<CircuitState>,
    consecutive_failures: AtomicU32,
    success_count: AtomicU32,
    opened_at: Mutex<Option<Instant>>,
    total_requests: AtomicU64,
    total_failures: AtomicU64,
    
    // 可调参数
    failure_threshold: u32,        // 默认 4
    success_threshold: u32,        // 默认 2
    timeout: Duration,             // 默认 60s
    error_rate_threshold: f64,     // 默认 0.6
    min_requests: u64,             // 默认 10
}

impl CircuitBreaker {
    pub async fn can_attempt(&self) -> bool {
        let state = *self.state.lock().await;
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // 检查是否已过冷却期
                let opened_at = self.opened_at.lock().await;
                if let Some(t) = *opened_at {
                    if t.elapsed() >= self.timeout {
                        // 转入半开状态
                        *self.state.lock().await = CircuitState::HalfOpen;
                        self.success_count.store(0, Ordering::Relaxed);
                        return true; // 允许探测请求
                    }
                }
                false
            }
            CircuitState::HalfOpen => {
                // 半开状态仅允许 1 个探测请求
                let in_flight = self.consecutive_failures.load(Ordering::Relaxed);
                in_flight == 0
            }
        }
    }
    
    pub async fn record_success(&self) {
        let state = *self.state.lock().await;
        match state {
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if successes >= self.success_threshold {
                    *self.state.lock().await = CircuitState::Closed;
                    self.consecutive_failures.store(0, Ordering::Relaxed);
                }
            }
            CircuitState::Closed => {
                self.consecutive_failures.store(0, Ordering::Relaxed);
            }
            _ => {}
        }
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }
    
    pub async fn record_failure(&self, _error: &str) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        self.total_failures.fetch_add(1, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        
        let state = *self.state.lock().await;
        match state {
            CircuitState::HalfOpen => {
                // 探测失败，重新熔断
                *self.state.lock().await = CircuitState::Open;
                *self.opened_at.lock().await = Some(Instant::now());
            }
            CircuitState::Closed => {
                if failures >= self.failure_threshold {
                    *self.state.lock().await = CircuitState::Open;
                    *self.opened_at.lock().await = Some(Instant::now());
                }
                // 或基于错误率
                let total = self.total_requests.load(Ordering::Relaxed);
                if total >= self.min_requests {
                    let fail_rate = self.total_failures.load(Ordering::Relaxed) as f64 / total as f64;
                    if fail_rate >= self.error_rate_threshold {
                        *self.state.lock().await = CircuitState::Open;
                        *self.opened_at.lock().await = Some(Instant::now());
                    }
                }
            }
            _ => {}
        }
    }
}
```

### 3.2 Claude Code 整流器（Cloaking）实现

参考 CLIProxyAPI 的 `claude_executor_cloaking.go`，在 Rust 中移植以下功能：

#### 3.2.1 系统提示注入 (`claude_system_prompt.rs`)

```rust
// route_aggregation/cloaking/claude_system_prompt.rs

/// Claude Code v2.1.63 的系统提示各段内容
/// 来源: CLIProxyAPI internal/runtime/executor/helps/claude_system_prompt.go
pub const CLAUDE_CODE_AGENT_IDENTIFIER: &str = 
    "You are Claude Code, Anthropic's official CLI for Claude.";

pub const CLAUDE_CODE_INTRO: &str = /* ... 完整的 intro 段落 ... */;
pub const CLAUDE_CODE_SYSTEM: &str = /* ... 完整的 system 段落 ... */;
pub const CLAUDE_CODE_DOING_TASKS: &str = /* ... 完整的 doing tasks 段落 ... */;
pub const CLAUDE_CODE_TONE_AND_STYLE: &str = /* ... 完整的 tone and style 段落 ... */;

/// 构建与真实 Claude Code 完全相同的 system 数组结构
pub fn build_system_array(original_system: &str, cloak_mode: CloakingMode) -> Vec<Value> {
    // system[0]: billing header (无 cache_control)
    // system[1]: agent identifier
    // system[2]: 核心系统提示
    // 非严格模式下，原始系统指令移到第一条用户消息的 <system-reminder> 中
}
```

#### 3.2.2 Billing Header 伪造 + CCH 签名 (`claude_billing.rs`)

```rust
// route_aggregation/cloaking/claude_billing.rs

/// Claude Code 使用的盐值 (来源: CLIProxyAPI)
const FINGERPRINT_SALT: &str = "59cf53e54c78";

/// CCH 签名种子 (来源: CLIProxyAPI)
const CCH_SEED: u64 = 0x6E52736AC806831E;

/// 生成 x-anthropic-billing-header
/// 格式: cc_version=<ver>.<buildHash>; cc_entrypoint=<ep>; cch=<hash>; [cc_workload=<wl>;]
pub fn generate_billing_header(version: &str, body: &str) -> String {
    let fingerprint = compute_fingerprint(body, version);
    let cch = compute_cch(body);
    format!(
        "cc_version={version}.abc123; cc_entrypoint=cli; cch={cch:05x};"
    )
}

/// 计算指纹: SHA256(salt + messageText[4] + messageText[7] + messageText[20] + version)[:3]
fn compute_fingerprint(body: &str, version: &str) -> String {
    use sha2::{Sha256, Digest};
    // 从系统消息文本中提取特定位置字符
    // ...
}

/// CCH 签名: xxHash64(billing_header, seed=CCH_SEED) 取前5个hex字符
fn compute_cch(body: &str) -> u64 {
    // 使用 xxhash-rust 计算
    // ...
}
```

#### 3.2.3 请求头注入 (`claude_headers.rs`)

```rust
// route_aggregation/cloaking/claude_headers.rs

/// 注入完整的 Claude Code 客户端请求头
/// 来源: CLIProxyAPI applyClaudeHeaders 函数
pub fn inject_claude_headers(
    headers: &mut HeaderMap,
    version: &str,
    session_id: &str,
) {
    // Anthropic-Beta: claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,...
    headers.insert("anthropic-beta", 
        "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14".parse().unwrap());
    
    // Anthropic-Version: 2023-06-01
    headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
    
    // X-App: cli
    headers.insert("x-app", "cli".parse().unwrap());
    
    // Stainless SDK 头部
    headers.insert("x-stainless-retry-count", "0".parse().unwrap());
    headers.insert("x-stainless-runtime", "node".parse().unwrap());
    headers.insert("x-stainless-lang", "js".parse().unwrap());
    headers.insert("x-stainless-timeout", "600".parse().unwrap());
    headers.insert("x-stainless-package-version", "0.74.0".parse().unwrap());
    headers.insert("x-stainless-runtime-version", "v24.3.0".parse().unwrap());
    headers.insert("x-stainless-os", "MacOS".parse().unwrap());
    headers.insert("x-stainless-arch", "arm64".parse().unwrap());
    
    // Claude Code 会话标识
    headers.insert("x-claude-code-session-id", session_id.parse().unwrap());
    headers.insert("x-client-request-id", &uuid::Uuid::new_v4().to_string().parse().unwrap());
    
    // User-Agent: claude-cli/2.1.63 (external, cli)
    let ua = format!("claude-cli/{} (external, cli)", version);
    headers.insert("user-agent", ua.parse().unwrap());
}
```

#### 3.2.4 伪造用户 ID (`claude_cloaking.rs` 中)

```rust
/// 生成 Claude Code 格式的 user_id
/// 格式: user_[64-hex-chars]_account_[UUID-v4]_session_[UUID-v4]
fn generate_fake_user_id() -> String {
    let mut hex_bytes = [0u8; 32];
    rand::thread_rng().fill(&mut hex_bytes);
    let hex_part = hex::encode(hex_bytes);
    let account_uuid = uuid::Uuid::new_v4();
    let session_uuid = uuid::Uuid::new_v4();
    format!("user_{hex_part}_account_{account_uuid}_session_{session_uuid}")
}
```

#### 3.2.5 OAuth 工具名重映射 (`tool_remap.rs`)

```rust
// route_aggregation/cloaking/tool_remap.rs

/// 第三方工具名 → Claude Code 官方工具名
/// 来源: CLIProxyAPI oauthToolRenameMap
pub static OAUTH_TOOL_RENAME_MAP: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("read", "Read"),
    ("write", "Write"),
    ("edit", "Edit"),
    ("glob", "Glob"),
    ("grep", "Grep"),
    ("task", "Task"),
    ("webfetch", "WebFetch"),
    ("websearch", "WebSearch"),
    ("todowrite", "TodoWrite"),
    ("bashinput", "BashInput"),
    ("notebookedit", "NotebookEdit"),
    // ... 完整列表
];

/// 在请求 body 中将工具名重映射为官方名称
pub fn remap_tool_names_in_request(body: &mut Value) { /* ... */ }
/// 在响应 body 中将工具名还原
pub fn reverse_remap_tool_names_in_response(body: &mut Value) { /* ... */ }
```

### 3.3 Codex 客户端模拟实现

参考 CLIProxyAPI 的 `codex_executor_request.go`：

#### 3.3.1 Codex 请求头注入 (`codex_headers.rs`)

```rust
// route_aggregation/cloaking/codex_headers.rs

/// 注入 Codex CLI (codex-tui) 客户端请求头
/// 来源: CLIProxyAPI applyCodexHeadersFromSources 函数
pub fn inject_codex_headers(
    headers: &mut HeaderMap,
    version: &str,
    account_id: Option<&str>,
    session_id: &str,
) {
    // User-Agent: codex-tui/0.146.0 (Mac OS 26.5.0; arm64) iTerm.app/3.6.10 (codex-tui; 0.146.0)
    let ua = format!(
        "codex-tui/{} (Mac OS 26.5.0; arm64) iTerm.app/3.6.10 (codex-tui; {})",
        version, version
    );
    headers.insert("user-agent", ua.parse().unwrap());
    
    // Originator: codex-tui
    headers.insert("originator", "codex-tui".parse().unwrap());
    
    // Session-Id
    headers.insert("session_id", session_id.parse().unwrap());
    
    // ChatGPT-Account-Id (如果有)
    if let Some(id) = account_id {
        headers.insert("chatgpt-account-id", id.parse().unwrap());
    }
    
    // Content-Type
    headers.insert("content-type", "application/json".parse().unwrap());
}
```

#### 3.3.2 Codex 身份混淆 (`codex_cloaking.rs` 中)

```rust
/// 混淆 Codex 身份标识，防止多账户关联检测
/// 来源: CLIProxyAPI applyCodexIdentityConfuseBody 函数
pub fn confuse_codex_identity(body: &mut Value) {
    // prompt_cache_key → 用 auth_id 派生的 UUID 替换
    // client_metadata.x-codex-installation-id → 替换
    // client_metadata.x-codex-turn-metadata → 替换 turn_id 和 window_id
    // client_metadata.x-codex-window-id → 替换
}
```

### 3.4 代理指纹头清除 (`header_scrub.rs`)

```rust
// route_aggregation/cloaking/header_scrub.rs

/// 删除所有可能暴露代理基础设施的头部
/// 来源: CLIProxyAPI ScrubProxyAndFingerprintHeaders
pub fn scrub_proxy_headers(headers: &mut HeaderMap) {
    // 代理追踪头
    headers.remove("x-forwarded-for");
    headers.remove("x-forwarded-host");
    headers.remove("x-forwarded-proto");
    headers.remove("x-real-ip");
    headers.remove("via");
    headers.remove("forwarded");
    
    // 客户端身份头（由 cloaking 重新注入）
    headers.remove("x-stainless-retry-count");
    headers.remove("x-stainless-runtime");
    headers.remove("x-stainless-lang");
    headers.remove("x-stainless-timeout");
    headers.remove("x-stainless-package-version");
    headers.remove("x-stainless-runtime-version");
    headers.remove("x-stainless-os");
    headers.remove("x-stainless-arch");
    headers.remove("referer");
    
    // 浏览器指纹头
    headers.remove("sec-ch-ua");
    headers.remove("sec-ch-ua-mobile");
    headers.remove("sec-ch-ua-platform");
    headers.remove("sec-fetch-mode");
    headers.remove("sec-fetch-site");
    headers.remove("sec-fetch-dest");
    
    // 编码协商（防止 zstd 等指纹不匹配）
    headers.remove("accept-encoding");
}
```

### 3.5 数据库变更

新增 `provider_route_toggle` 表（仅此一张表，路由聚合配置本身存 config.json，见 3.12 节）：

```sql
-- 供应商在路由组中的开关与排序
CREATE TABLE IF NOT EXISTS provider_route_toggle (
    provider_id TEXT NOT NULL,
    route_group TEXT NOT NULL,  -- 'claude_code' | 'codex'
    enabled INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider_id, route_group)
);
```

**注意**：`route_aggregation` 配览表（原计划中的单行配置表）已移除，配置统一存入 `config.json` 的 `routeAggregation` 字段，与 `NetworkSettings`、`BackupSettings` 模式一致。SQLite 仅保留 `provider_route_toggle`（数据记录型，随供应商增删而变化）。

### 3.6 Tauri 命令清单

在 `lib.rs` 中注册以下新命令：

```rust
// 路由聚合生命周期
#[tauri::command]
async fn get_route_aggregation_status() -> Result<RouteAggregationStatus, String>

#[tauri::command]
async fn get_route_aggregation_config() -> Result<RouteAggregationConfig, String>

#[tauri::command]
async fn update_route_aggregation_config(config: RouteAggregationConfig) -> Result<RouteAggregationStatus, String>

#[tauri::command]
async fn start_route_aggregation(app: AppHandle) -> Result<RouteAggregationStatus, String>

#[tauri::command]
async fn stop_route_aggregation() -> Result<(), String>

// 供应商路由组开关
#[tauri::command]
async fn get_provider_route_toggles(group: String) -> Result<Vec<ProviderRouteToggle>, String>

#[tauri::command]
async fn toggle_provider_route(provider_id: String, group: String, enabled: bool) -> Result<(), String>

#[tauri::command]
async fn reorder_provider_routes(ids: Vec<String>, group: String) -> Result<(), String>

// 熔断器管理
#[tauri::command]
async fn reset_circuit_breaker(provider_id: String, group: String) -> Result<(), String>

#[tauri::command]
async fn get_circuit_breaker_status(group: String) -> Result<Vec<CircuitBreakerStatus>, String>
```

### 3.7 前端实现

#### 3.7.1 新增页面与导航

**`App.tsx`** — 添加新的 MainView 类型：
```typescript
export type MainView =
  | "agent-sniff" | "ai-providers" | "backup-manage"
  | "opencode-config" | "project-config" | "skills-manage"
  | "mcp-manage" | "claude-env" | "codex-env"
  | "route-aggregation";  // ← 新增
```

**`Sidebar.tsx`** — 在 AI 供应商下方添加菜单项：
```tsx
<button
  className={`menu-item ${mainView === "route-aggregation" ? "active" : ""}`}
  onClick={() => onNavigateMain("route-aggregation")}
>
  <Network size={18} strokeWidth={1.8} />
  <span className="menu-label">路由聚合</span>
</button>
```

#### 3.7.2 路由聚合页面组件结构

```
src/components/pages/
└── RouteAggregation.tsx          ← 主页面
    └── route-aggregation/
        ├── api.ts                ← Tauri invoke 封装
        └── types.ts              ← TypeScript 类型定义
```

#### 3.7.3 页面布局设计

```
┌─────────────────────────────────────────────────────────┐
│  路由聚合                                                │
│  ┌─────────────────────────────────────────────────┐    │
│  │  ⚙ 基本配置                                      │    │
│  │  监听地址: [127.0.0.1]  端口: [16888]             │    │
│  │  自动故障转移: [● 开启]                          │    │
│  │  最大重试次数: [3]                                │    │
│  │  伪装模式: [自动 ▼]                               │    │
│  └─────────────────────────────────────────────────┘    │
│                                                          │
│  ┌─────────────────────────────────────────────────┐    │
│  │  Claude Code 路由                    [● 开启]   │    │
│  │  CC 版本: [2.1.63]   整流器: [● 开启]            │    │
│  │  ┌───────────────────────────────────────────┐   │    │
│  │  │ ✓ Provider A (anthropic)  ● 正常  请求 234 │   │    │
│  │  │ ✓ Provider B (universal)  ● 正常  请求  89 │   │    │
│  │  │ ✗ Provider C (universal)  ○ 熔断  请求  12 │   │    │
│  │  │                          [重置熔断器]       │   │    │
│  │  └───────────────────────────────────────────┘   │    │
│  │  代理地址: http://127.0.0.1:16888/v1/messages     │    │
│  └─────────────────────────────────────────────────┘    │
│                                                          │
│  ┌─────────────────────────────────────────────────┐    │
│  │  Codex 路由                          [○ 关闭]   │    │
│  │  Codex 版本: [0.146.0]  客户端模拟: [● 开启]     │    │
│  │  ┌───────────────────────────────────────────┐   │    │
│  │  │ ✓ Provider B (universal)  ● 正常          │   │    │
│  │  │ ✓ Provider D (openai)     ● 正常          │   │    │
│  │  └───────────────────────────────────────────┘   │    │
│  │  代理地址: http://127.0.0.1:16888/v1/chat/...     │    │
│  └─────────────────────────────────────────────────┘    │
│                                                          │
│  ┌─────────────────────────────────────────────────┐    │
│  │  使用说明                                         │    │
│  │  Claude Code: 设置 ANTHROPIC_BASE_URL=...        │    │
│  │  Codex CLI: 设置 OPENAI_BASE_URL=...             │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

#### 3.7.4 前端 API 封装

```typescript
// src/components/pages/route-aggregation/api.ts

export async function getStatus(): Promise<RouteAggregationStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_status");
}

export async function getConfig(): Promise<RouteAggregationConfig> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_config");
}

export async function updateConfig(config: RouteAggregationConfig): Promise<RouteAggregationStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("update_route_aggregation_config", { config });
}

export async function startServer(): Promise<RouteAggregationStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("start_route_aggregation");
}

export async function stopServer(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("stop_route_aggregation");
}

export async function getProviderToggles(group: RouteGroup): Promise<ProviderRouteToggle[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_provider_route_toggles", { group });
}

export async function toggleProviderRoute(
  providerId: string, group: RouteGroup, enabled: boolean
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("toggle_provider_route", { providerId, group, enabled });
}

export async function reorderProviderRoutes(
  ids: string[], group: RouteGroup
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reorder_provider_routes", { ids, group });
}

export async function resetCircuitBreaker(
  providerId: string, group: RouteGroup
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reset_circuit_breaker", { providerId, group });
}
```

### 3.8 Cargo.toml 依赖变更

```toml
[dependencies]
# === 现有依赖保持不变 ===
# tauri, serde, serde_json, toml_edit, json5, rusqlite, dirs, 
# aes-gcm, hkdf, sha2, rand, base64, reqwest, zip, walkdir, chrono

# === 新增依赖 ===
# HTTP 服务器 — axum 0.8 依赖 tower 0.5，与 Tauri 2 传递依赖版本对齐
# (axum 0.7 依赖 tower 0.4，会导致双版本 tower 共存 + trait 不匹配)
axum = { version = "0.8", features = ["ws"] }
hyper = { version = "1", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors"] }

# 异步运行时 (Tauri 2 已传递依赖 tokio，但需显式声明 features)
tokio = { version = "1", features = ["full"] }

# Stream trait 扩展 (SSE 透传需要 StreamExt::map)
futures = "0.3"

# HTTP 客户端 (异步版，与现有 reqwest blocking 分开)
# 注意: reqwest 已有 0.12，但当前只启用 blocking feature；需新增 async feature
# 方案: 在现有 reqwest 上追加 "multipart" feature，或新建一个 async client
# 推荐: 直接使用 reqwest async (axum 内部已用 hyper)

# UUID 生成
uuid = { version = "1", features = ["v4"] }

# xxHash (CCH 签名)
xxhash-rust = { version = "0.8", features = ["xxh64"] }

# Hex 编码
hex = "0.4"

# 日志 — 使用现有 eprintln! 模式，不额外引入 env_logger

# JSON 操作 (已用 serde_json，无需额外)
```

**关键依赖说明**:

1. **`axum = "0.8"`（非 0.7）**：axum 0.7 依赖 tower 0.4，与 Tauri 2 已有的 tower 0.5 不兼容会导致双版本共存和 trait 不匹配。axum 0.8 正确依赖 tower 0.5 + hyper 1.x，与 Tauri 2 的传递依赖完全对齐。

2. **`futures = "0.3"`**：SSE 流式透传需要 `futures::StreamExt` 的 `.map()` 方法对 `bytes_stream()` 做转换，否则编译失败。

3. **`reqwest` 的 features 调整**：当前是 `["rustls-tls", "blocking", "json", "socks"]`，需追加 `"stream"` feature：

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "blocking", "json", "socks", "stream"] }
```

reqwest 的 async API 不受 feature flag 控制，`default-features = false` 不影响异步功能。`"stream"` feature 启用 `bytes_stream()` 方法用于 SSE 流式转发。

### 3.9 应用启动集成与 Tauri State 管理

路由聚合服务器实例需要跨多个 Tauri 命令（start/stop/get_status/update_config）共享。现有代码库不使用 `tauri::State`，但路由聚合功能需要引入此模式来管理服务器生命周期。

#### 3.9.1 全局状态定义

```rust
// route_aggregation/mod.rs

use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局路由聚合状态，通过 tauri::State 共享给所有命令
pub struct RouteAggregationState {
    /// 服务器实例（运行时存在，停止时为 None）
    pub server: RwLock<Option<Arc<RouteAggregationServer>>>,
    /// 配置快照
    pub config: RwLock<RouteAggregationConfig>,
    /// 供应商路由器（含熔断器状态，即使服务器停止也保留状态）
    pub router: Arc<provider_router::ProviderRouter>,
}
```

#### 3.9.2 setup 注册

```rust
// lib.rs

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ... 现有所有命令 ...
            // 路由聚合命令
            get_route_aggregation_status,
            get_route_aggregation_config,
            update_route_aggregation_config,
            start_route_aggregation,
            stop_route_aggregation,
            get_provider_route_toggles,
            toggle_provider_route,
            reorder_provider_routes,
            reset_circuit_breaker,
            get_circuit_breaker_status,
        ])
        .setup(|app| {
            // 现有初始化...
            config::ensure_app_config()?;
            db::purge_removed_agents(&["kiro", "codebuddy"])?;
            
            // 路由聚合初始化：创建 DB 表
            db::ensure_route_aggregation_tables()?;
            
            // 注册全局状态
            let config = route_aggregation::config::load_config()?;
            let state = route_aggregation::RouteAggregationState::new(config.clone());
            app.manage(state);
            
            // 如果上次退出时服务器在运行，自动恢复
            if config.claude_code_enabled || config.codex_enabled {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = route_aggregation::server::start_server(app_handle).await {
                        eprintln!("[agent-buddy] 路由聚合服务器自动启动失败: {}", e);
                    }
                });
            }
            
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

#### 3.9.3 命令中访问状态

```rust
// 命令通过 tauri::State 获取共享实例
#[tauri::command]
async fn start_route_aggregation(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<RouteAggregationStatus, String> {
    // 端口冲突预检测
    let config = state.config.read().await.clone();
    let test_addr = format!("{}:{}", config.listen_address, config.listen_port);
    if std::net::TcpListener::bind(&test_addr).is_err() {
        return Err(format!(
            "端口 {} 被占用，请在设置中更改监听端口",
            config.listen_port
        ));
    }
    
    // 启动服务器...
    let server = route_aggregation::server::RouteAggregationServer::start(config).await?;
    *state.server.write().await = Some(Arc::new(server));
    Ok(state.get_status().await)
}

#[tauri::command]
async fn stop_route_aggregation(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<(), String> {
    if let Some(server) = state.server.write().await.take() {
        server.stop().await;
    }
    Ok(())
}

#[tauri::command]
async fn get_route_aggregation_status(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<RouteAggregationStatus, String> {
    Ok(state.get_status().await)
}
```

### 3.10 SSE 流式响应转发

Claude Code 和 Codex 都依赖 SSE（Server-Sent Events）流式响应。转发器需要正确处理流式透传。

**重要**：不能使用 `axum::response::sse::{Event, Sse}` 封装，因为它会对每个 chunk 添加 `data: ` 前缀，导致 SSE 事件格式双重嵌套。正确做法是使用 `axum::body::Body::from_stream` 直接透传原始字节流。

```rust
// route_aggregation/forwarder.rs (流式处理部分)

use axum::body::Body;
use axum::response::Response;
use axum::http::StatusCode;
use futures::StreamExt;

/// 将上游 SSE 流原样透传给客户端（不二次封装 Event/Sse）
pub fn forward_streaming(upstream_response: reqwest::Response) -> Response {
    // 保留上游的 Content-Type 和其他响应头
    let mut response_builder = Response::builder()
        .status(StatusCode::from_u16(upstream_response.status().as_u16())
            .unwrap_or(StatusCode::OK));
    
    // 透传关键响应头
    for key in &["content-type", "cache-control", "connection"] {
        if let Some(val) = upstream_response.headers().get(key) {
            response_builder = response_builder.header(key, val);
        }
    }
    
    // 直接将 reqwest 的 bytes_stream 作为响应 body
    // 不做任何解析/转换，SSE 事件原样透传
    let stream = upstream_response.bytes_stream()
        .map(|result| result.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        }));
    
    response_builder.body(Body::from_stream(stream))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
```

### 3.11 API Key 解密与认证 Header 注入

路由聚合转发器需要从 SQLite 读取供应商的加密 API Key，解密后根据供应商类型注入正确的认证 header：

```rust
// route_aggregation/forwarder.rs (认证注入部分)

use crate::ai_provider::{get_provider_row, AiProviderRow};
use crate::crypto;

/// 根据供应商类型构建认证 header
pub fn build_auth_headers(
    provider: &AiProviderRow,
    group: RouteGroup,
) -> Result<Vec<(String, String)>, String> {
    // 解密 API Key
    let api_key = if provider.api_key_cipher.is_empty() {
        return Err(format!("供应商 {} 未设置 API Key", provider.name));
    } else {
        crypto::decrypt_secret(
            &provider.api_key_salt,
            &provider.api_key_nonce,
            &provider.api_key_cipher,
        )?
    };
    
    let mut headers = Vec::new();
    
    match group {
        RouteGroup::ClaudeCode => {
            // Anthropic 类型用 x-api-key；universal 中转服务可能用 Bearer
            if provider.provider_type == "anthropic" {
                headers.push(("x-api-key".to_string(), api_key));
                headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
            } else {
                // universal 类型中转服务通常用 Bearer
                headers.push(("authorization".to_string(), format!("Bearer {}", api_key)));
            }
        }
        RouteGroup::Codex => {
            // OpenAI / universal 类型用 Bearer
            headers.push(("authorization".to_string(), format!("Bearer {}", api_key)));
        }
    }
    
    Ok(headers)
}

/// 构建上游 URL
pub fn build_upstream_url(
    provider: &AiProviderRow,
    group: RouteGroup,
    request_path: &str,
) -> String {
    let base = match group {
        RouteGroup::ClaudeCode => &provider.base_url,
        RouteGroup::Codex => {
            if provider.provider_type == "universal" {
                // universal 类型用派生的 OpenAI base URL
                &provider.base_url // 实际使用 derive_openai_base_url 结果
            } else {
                &provider.base_url
            }
        }
    };
    
    // 拼接路径，避免双重斜杠
    format!("{}{}", base.trim_end_matches('/'), request_path)
}
```

### 3.12 配置存储统一

**决策**：路由聚合配置统一存储在 `config.json` 的 `routeAggregation` 字段中（与 `NetworkSettings`、`BackupSettings` 模式一致），不使用 SQLite。SQLite 仅存储 `provider_route_toggle` 表（供应商路由组开关，属于数据记录型）。

```rust
// config.rs 扩展

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub theme: String,
    #[serde(default)]
    pub backup: BackupSettings,
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]                      // ← 新增
    pub route_aggregation: RouteAggregationConfig, // ← 新增
}

// RouteAggregationConfig 定义在 route_aggregation/config.rs 中
// 通过 config.rs 的 load_app_config / save_app_config 读写
```

### 3.13 端口冲突预检测

在启动代理服务器前进行端口可用性检测：

```rust
// route_aggregation/server.rs

pub async fn start(config: &RouteAggregationConfig) -> Result<Self, String> {
    let addr = format!("{}:{}", config.listen_address, config.listen_port);
    
    // 端口预检测：尝试绑定，失败则返回友好错误
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| {
            let hint = if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("端口 {} 已被占用，请在路由聚合设置中更改监听端口", config.listen_port)
            } else {
                format!("绑定地址 {} 失败: {}", addr, e)
            };
            hint
        })?;
    
    // ... 后续启动逻辑
}
```

### 3.14 热更新限制说明

Axum 不支持运行时动态增删路由。因此：

- **供应商列表热更新**：支持。通过更新 `ProviderRouter` 内部的供应商池实现，不需要重启服务器。
- **供应商开关热更新**：支持。通过更新 `provider_route_toggle` 配置并通知 `ProviderRouter` 刷新。
- **路由组开关热更新（如从 CC-only 变为 CC+Codex）**：不支持。需要停止并重启服务器。前端在切换路由组开关时应自动执行 stop → 更新配置 → start 流程。
- **超时/重试参数热更新**：支持。通过更新 `RouteAggregationConfig` 配置快照实现。
- **监听地址/端口热更新**：不支持。需要停止并重启服务器。

---

## 4. 实施阶段

### Phase 1: 基础设施 (预计 2-3 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 1.1 | 添加 Cargo.toml 新依赖 | `src-tauri/Cargo.toml` | ✅ |
| 1.2 | 创建 `route_aggregation/` 模块骨架 | `src-tauri/src/route_aggregation/mod.rs` | ✅ |
| 1.3 | 实现数据结构定义 | `route_aggregation/types.rs` | ✅ |
| 1.4 | 实现配置加载/保存 | `route_aggregation/config.rs` | ✅ |
| 1.5 | 新增 SQLite 表 + CRUD | `src-tauri/src/db.rs` | ✅ |
| 1.6 | 扩展 `config.rs` 支持路由聚合配置 | `src-tauri/src/config.rs` | ✅ |
| 1.7 | 在 `lib.rs` 注册新模块 | `src-tauri/src/lib.rs` | ✅ |

### Phase 2: HTTP 服务器 + 转发 (预计 3-4 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 2.1 | Axum 服务器生命周期管理 | `route_aggregation/server.rs` | ✅ |
| 2.2 | 路由注册 + 请求分发 | `route_aggregation/router.rs` | ✅ |
| 2.3 | 请求处理器（入口 handler） | `route_aggregation/handler.rs` | ✅ |
| 2.4 | 异步 HTTP 客户端封装 | `route_aggregation/async_client.rs` | ⚠️ 功能内联到 forwarder.rs，未拆分独立文件 |
| 2.5 | 基本请求转发（无伪装，无故障转移） | `route_aggregation/forwarder.rs` | ✅ |
| 2.6 | SSE 流式响应透传 | `route_aggregation/forwarder.rs` | ✅ |
| 2.7 | Tauri 命令注册 + 启动集成 | `src-tauri/src/lib.rs` | ✅ |
| 2.8 | 基础功能验证（能转发请求） | 手动测试 | ❌ 待测试 |

### Phase 3: 故障转移 + 熔断器 (预计 2-3 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 3.1 | 三态熔断器实现 | `route_aggregation/circuit_breaker.rs` | ✅ |
| 3.2 | 供应商路由器（选择 + 熔断管理） | `route_aggregation/provider_router.rs` | ✅ |
| 3.3 | 故障转移转发器（多供应商重试） | `route_aggregation/forwarder.rs` | ✅ |
| 3.4 | 熔断器状态查询 + 重置命令 | `lib.rs` + `provider_router.rs` | ✅ |
| 3.5 | 供应商路由组开关 CRUD | `db.rs` + `provider_router.rs` | ✅ |
| 3.6 | 故障转移端到端验证 | 手动测试 | ❌ 待测试 |

### Phase 4: Claude Code 整流器 (预计 3-4 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 4.1 | Claude Code 系统提示常量 | `cloaking/claude_system_prompt.rs` | ✅ |
| 4.2 | 系统提示注入逻辑 | `cloaking/claude_cloaking.rs` | ✅ |
| 4.3 | Billing header 伪造 + CCH 签名 | `cloaking/claude_billing.rs` | ✅ |
| 4.4 | Claude 请求头注入 | `cloaking/claude_headers.rs` | ✅ |
| 4.5 | 伪造用户 ID 生成 | `cloaking/claude_cloaking.rs` | ✅ |
| 4.6 | OAuth 工具名重映射 | `cloaking/tool_remap.rs` | ✅ |
| 4.7 | 敏感词零宽空格混淆 | `cloaking/obfuscate.rs` | ✅ |
| 4.8 | 代理指纹头清除 | `cloaking/header_scrub.rs` | ✅ |
| 4.9 | 设备指纹稳定化 | `cloaking/device_profile.rs` | ⚠️ 已实现但未集成到 forwarder（headers 在 claude_headers.rs 中硬编码） |
| 4.10 | 整流器集成到 forwarder | `forwarder.rs` | ✅ |
| 4.11 | 整流器端到端验证（请求指纹对比） | 抓包验证 | ❌ 待验证 |

### Phase 5: Codex 客户端模拟 (预计 2 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 5.1 | Codex 请求头注入 | `cloaking/codex_headers.rs` | ✅ |
| 5.2 | Codex 身份混淆 | `cloaking/codex_cloaking.rs` | ✅ |
| 5.3 | Codex 整流器集成到 forwarder | `forwarder.rs` | ✅ |
| 5.4 | Codex 模拟端到端验证 | 抓包验证 | ❌ 待验证 |

### Phase 6: 前端 UI (预计 3-4 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 6.1 | 路由聚合 TypeScript 类型定义 | `route-aggregation/types.ts` | ✅ |
| 6.2 | 前端 API 封装 | `route-aggregation/api.ts` | ✅ |
| 6.3 | Sidebar 添加菜单项 | `components/Sidebar.tsx` | ✅ |
| 6.4 | App.tsx 添加视图路由 | `App.tsx` | ✅ |
| 6.5 | 基本配置面板（地址/端口/超时） | `pages/RouteAggregation.tsx` | ✅ |
| 6.6 | Claude Code 路由组面板 | `RouteAggregation.tsx` | ✅ |
| 6.7 | Codex 路由组面板 | `RouteAggregation.tsx` | ✅ |
| 6.8 | 供应商列表 + 开关 + 拖拽排序 | `RouteAggregation.tsx` | ⚠️ 列表+开关已完成，拖拽排序未实现 |
| 6.9 | 熔断器状态展示 + 重置按钮 | `RouteAggregation.tsx` | ✅ |
| 6.10 | 使用说明面板 | `RouteAggregation.tsx` | ✅ |
| 6.11 | 实时状态刷新（轮询 / Tauri 事件） | `RouteAggregation.tsx` | ✅ |

### Phase 7: 集成测试 + 文档 (预计 2 天)

| # | 任务 | 涉及文件 | 状态 |
|---|------|----------|------|
| 7.1 | 端到端测试：Claude Code 通过代理正常请求 | 手动 | ❌ 待测试 |
| 7.2 | 端到端测试：Codex CLI 通过代理正常请求 | 手动 | ❌ 待测试 |
| 7.3 | 故障转移测试：关闭一个供应商，验证自动切换 | 手动 | ❌ 待测试 |
| 7.4 | 整流器验证：对比请求头与真实 CC 请求 | 抓包 | ❌ 待验证 |
| 7.5 | SSE 流式响应验证 | 手动 | ❌ 待测试 |
| 7.6 | 应用重启后自动恢复 | 手动 | ❌ 待测试 |
| 7.7 | 更新 waitTODO.txt，标记第14项完成 | `waitTODO.txt` | ❌ 待更新 |

---

## 5. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Rust 无 uTLS 库，无法做 TLS 指纹伪装 | 部分中转服务可能通过 TLS 指纹检测代理 | cc-switch 也未实现 TLS 伪装且运行良好；大部分中转服务仅检测 HTTP 层；后续可评估 Rust 的 boring/boring-sys 或 FFI 调用 Go uTLS |
| reqwest 同时使用 blocking 和 async 可能冲突 | 编译或运行时问题 | reqwest 0.12 支持同时启用 blocking 和 async feature；路由聚合模块内部仅使用 async API |
| Axum 服务器在 Tauri 进程内可能导致阻塞 | UI 卡顿 | 路由聚合服务器在独立 tokio task 中运行，与 Tauri 的 WebView 线程隔离 |
| Claude Code 系统提示版本过时 | 伪装效果减弱 | 将版本号和提示内容设为可配置，方便后续更新 |
| 供应商 API 格式差异（Anthropic vs OpenAI 兼容） | 非 Anthropic 格式的供应商请求失败 | Claude Code 路由组仅路由到 `anthropic` 和 `universal` 类型供应商；universal 类型需其中转服务支持 Anthropic API 格式 |
| SSE 流式透传可能丢失或损坏事件 | 客户端收到错误响应 | 使用 `Body::from_stream` 直接透传原始字节流，不使用 `Sse` wrapper 二次封装 |
| SQLite 供应商路由组开关与现有 AI 供应商表不同步 | 新增供应商后路由组未更新 | 供应商 upsert 时自动在新表中插入默认开关记录（enabled=true） |
| 端口冲突（16888 被占用） | 代理服务器无法启动 | 启动前 `TcpListener::bind` 预检测，返回友好错误提示并建议更改端口 |
| 路由组开关热切换需重启服务器 | 切换 CC/Codex 开关时有短暂中断 | 前端自动执行 stop → 更新配置 → start 流程；切换过程 < 1 秒 |
| Tauri State 是现有代码库的新模式 | 增加架构复杂度 | 仅路由聚合模块使用 `app.manage(state)` + `tauri::State`；其他模块不受影响 |

---

## 6. 与现有功能的关系

### 6.1 复用现有模块

| 现有模块 | 复用方式 |
|----------|----------|
| `ai_provider.rs` | 直接读取供应商列表 + 解密 API Key 作为上游池 |
| `crypto.rs` | 复用 AES-256-GCM 解密供应商 API Key |
| `db.rs` | 新增表挂载到同一 SQLite 数据库 |
| `config.rs` | 路由聚合配置存入 `config.json` 的 `routeAggregation` 字段 |
| `http_client.rs` | 代理服务器内部不复用（需要 async，此模块是 blocking）；但上游代理设置可复用配置 |

### 6.2 不修改现有功能

路由聚合功能完全增量开发，不修改现有的 AI 供应商管理、Claude 环境、Codex 环境、MCP 管理等模块的任何逻辑。

---

## 7. 默认配置

```json
{
  "routeAggregation": {
    "claudeCodeEnabled": false,
    "codexEnabled": false,
    "listenAddress": "127.0.0.1",
    "listenPort": 16888,
    "autoFailover": true,
    "maxRetries": 3,
    "streamFirstByteTimeout": 60,
    "streamIdleTimeout": 120,
    "nonStreamTotalTimeout": 600,
    "cloakingMode": "auto",
    "claudeCodeVersion": "2.1.63",
    "codexVersion": "0.146.0"
  }
}
```

**默认行为**：关闭状态，用户需手动开启。开启后自动聚合对应类型的已添加供应商，所有供应商默认参与（enabled=true），按 `sort_order` 顺序故障转移。

---

## 8. 用户使用流程

1. 在「AI 供应商」页面添加供应商（如 Anthropic 官方、某中转服务等）
2. 打开左侧菜单「路由聚合」
3. 根据需要开启 Claude Code 路由和/或 Codex 路由
4. 系统自动将匹配类型的供应商聚合到路由组中
5. 可在供应商列表中关闭不需要参与聚合的供应商
6. 可拖拽调整供应商顺序（决定故障转移优先级）
7. 开启后，页面显示代理地址（如 `http://127.0.0.1:16888`）
8. 在 Claude Code 中设置 `ANTHROPIC_BASE_URL=http://127.0.0.1:16888`
9. 在 Codex CLI 中设置 `OPENAI_BASE_URL=http://127.0.0.1:16888/v1`
10. 客户端请求会自动经路由聚合代理转发到供应商，享受整流器伪装 + 自动故障转移

---

## 附录 A: 关键参考文件对照

| 功能 | 参考项目 | 参考文件路径 | 对应实现文件 |
|------|----------|-------------|-------------|
| Axum 服务器 | cc-switch | `src-tauri/src/proxy/server.rs` | `route_aggregation/server.rs` |
| 请求转发 | cc-switch | `src-tauri/src/proxy/forwarder.rs` | `route_aggregation/forwarder.rs` |
| 熔断器 | cc-switch | `src-tauri/src/proxy/circuit_breaker.rs` | `route_aggregation/circuit_breaker.rs` |
| 供应商路由 | cc-switch | `src-tauri/src/proxy/provider_router.rs` | `route_aggregation/provider_router.rs` |
| 系统提示注入 | CLIProxyAPI | `internal/runtime/executor/claude_executor_cloaking.go` | `cloaking/claude_cloaking.rs` |
| 系统提示常量 | CLIProxyAPI | `internal/runtime/executor/helps/claude_system_prompt.go` | `cloaking/claude_system_prompt.rs` |
| Billing + CCH | CLIProxyAPI | `internal/runtime/executor/claude_signing.go` | `cloaking/claude_billing.rs` |
| Claude 请求头 | CLIProxyAPI | `internal/runtime/executor/claude_executor_request.go` | `cloaking/claude_headers.rs` |
| 工具名重映射 | CLIProxyAPI | `internal/runtime/executor/claude_executor.go` | `cloaking/tool_remap.rs` |
| 敏感词混淆 | CLIProxyAPI | `internal/runtime/executor/helps/cloak_obfuscate.go` | `cloaking/obfuscate.rs` |
| 代理头清除 | CLIProxyAPI | `internal/misc/header_utils.go` | `cloaking/header_scrub.rs` |
| 设备指纹 | CLIProxyAPI | `internal/runtime/executor/helps/claude_device_profile.go` | `cloaking/device_profile.rs` |
| Codex 请求头 | CLIProxyAPI | `internal/runtime/executor/codex_executor_request.go` | `cloaking/codex_headers.rs` |
| Codex 身份混淆 | CLIProxyAPI | `internal/runtime/executor/codex_executor_request.go` | `cloaking/codex_cloaking.rs` |
| 伪造 user_id | CLIProxyAPI | `internal/runtime/executor/helps/cloak_utils.go` | `cloaking/claude_cloaking.rs` |
| 整流器配置 | cc-switch | `src-tauri/src/proxy/types.rs` (RectifierConfig) | `types.rs` |

## 附录 B: 整流器伪装参数清单

### Claude Code 伪装参数

| 参数 | 默认值 | 来源 |
|------|--------|------|
| claude_code_version | 2.1.63 | CLIProxyAPI |
| stainless_package_version | 0.74.0 | CLIProxyAPI |
| stainless_runtime_version | v24.3.0 | CLIProxyAPI |
| stainless_os | MacOS | CLIProxyAPI |
| stainless_arch | arm64 | CLIProxyAPI |
| anthropic_beta | claude-code-20250219,oauth-2025-04-20,... | CLIProxyAPI |
| anthropic_version | 2023-06-01 | CLIProxyAPI |
| fingerprint_salt | 59cf53e54c78 | CLIProxyAPI |
| cch_seed | 0x6E52736AC806831E | CLIProxyAPI |
| user_agent | claude-cli/2.1.63 (external, cli) | CLIProxyAPI |

### Codex 伪装参数

| 参数 | 默认值 | 来源 |
|------|--------|------|
| codex_version | 0.146.0 | CLIProxyAPI |
| user_agent | codex-tui/0.146.0 (Mac OS 26.5.0; arm64) iTerm.app/3.6.10 | CLIProxyAPI |
| originator | codex-tui | CLIProxyAPI |

## 附录 C: 整流器触发逻辑

```rust
// 来源: CLIProxyAPI ShouldCloak 函数
pub fn should_cloak(cloaking_mode: &CloakingMode, user_agent: &str) -> bool {
    match cloaking_mode {
        CloakingMode::Always => true,
        CloakingMode::Never => false,
        CloakingMode::Auto => {
            // auto 模式：如果客户端 UA 不是 claude-cli 开头，则自动启用伪装
            !user_agent.starts_with("claude-cli")
        }
    }
}
```
