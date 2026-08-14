//! Route aggregation (local proxy server): aggregates multiple AI providers
//! behind a single local endpoint with automatic failover and request cloaking.
//!
//! 仅 passthrough：A→A（Claude Code → Anthropic 兼容 provider）+ OR→OR
//! （Codex CLI → OpenAI Responses 兼容 provider）。路由聚合不做协议翻译。
//!
//! Architecture:
//! - `server` — Axum HTTP server lifecycle (start/stop/config)
//! - `router` — Route registration + request dispatch
//! - `handler` — Request entry points (Claude messages / Codex responses)
//! - `forwarder` — Request forwarding with failover + header injection + SSE passthrough
//! - `provider_router` — Provider selection + circuit breaker management
//! - `circuit_breaker` — Three-state circuit breaker (Closed/Open/HalfOpen)
//! - `cloaking` — Claude Code rectifier + Codex client simulation
//!   （仿照 CLIProxyAPI `internal/runtime/executor/`，由 check_upstream_sync.py 的
//!   client_fingerprint_anchors 跟踪同步状态；详见 docs/SYNC_PLAYBOOK.md §2）
//! - `config` — RouteAggregationConfig load/save (stored in config.json)
//! - `types` — Data structures (DTOs for Tauri commands)
//! - `log` / `logfile` — In-memory ring buffer + on-disk tail-friendly log

pub mod circuit_breaker;
pub mod cloaking;
pub mod config;
pub mod forwarder;
pub mod handler;
pub mod log;
pub mod logfile;
pub mod provider_router;
pub mod router;
pub mod server;
pub mod types;

use std::sync::Arc;
use tokio::sync::RwLock;

pub use types::*;

// Re-export key types used across modules and in lib.rs/config.rs.
pub use config::RouteAggregationConfig;
pub use circuit_breaker::CircuitBreakerSnapshot;
pub use log::{LogEntry, LogStore};
pub use logfile::LogFile;

/// Global route aggregation state, shared across Tauri commands via `app.manage()`.
pub struct RouteAggregationState {
    /// Server instance (present when running, None when stopped).
    pub server: RwLock<Option<Arc<server::RouteAggregationServer>>>,
    /// Config snapshot — shared with Axum handlers via AppState so that runtime
    /// config updates (e.g. toggling a route group) are visible without restart.
    pub config: Arc<RwLock<RouteAggregationConfig>>,
    /// Provider router (holds circuit breaker state, survives server stop).
    pub provider_router: Arc<provider_router::ProviderRouter>,
    /// In-memory ring buffer of inbound/upstream request logs. Used by the UI's
    /// "进出日志" list.
    pub log_store: LogStore,
}

impl RouteAggregationState {
    pub fn new(config: RouteAggregationConfig) -> Self {
        Self {
            server: RwLock::new(None),
            config: Arc::new(RwLock::new(config)),
            provider_router: Arc::new(provider_router::ProviderRouter::new()),
            log_store: LogStore::new(),
        }
    }

    /// Build a status snapshot for the frontend.
    pub async fn get_status(&self) -> RouteAggregationStatus {
        let config = self.config.read().await;
        let server_running = self.server.read().await.is_some();
        let providers = self.provider_router.get_merged_statuses().await;

        RouteAggregationStatus {
            server_running,
            listen_address: config.listen_address.clone(),
            listen_port: config.listen_port,
            providers,
        }
    }
}
