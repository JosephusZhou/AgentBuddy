//! Route aggregation (local proxy server): aggregates multiple AI providers
//! behind a single local endpoint with automatic failover and request cloaking.
//!
//! Architecture:
//! - `server` — Axum HTTP server lifecycle (start/stop/config)
//! - `router` — Route registration + request dispatch
//! - `handler` — Request entry points (Claude messages, Codex chat/responses)
//! - `forwarder` — Request forwarding with failover + header injection + SSE passthrough
//! - `provider_router` — Provider selection + circuit breaker management
//! - `circuit_breaker` — Three-state circuit breaker (Closed/Open/HalfOpen)
//! - `cloaking` — Claude Code rectifier + Codex client simulation
//! - `config` — RouteAggregationConfig load/save (stored in config.json)
//! - `types` — Data structures (DTOs for Tauri commands)

pub mod circuit_breaker;
pub mod cloaking;
pub mod config;
pub mod forwarder;
pub mod handler;
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

/// Global route aggregation state, shared across Tauri commands via `app.manage()`.
pub struct RouteAggregationState {
    /// Server instance (present when running, None when stopped).
    pub server: RwLock<Option<Arc<server::RouteAggregationServer>>>,
    /// Config snapshot — shared with Axum handlers via AppState so that runtime
    /// config updates (e.g. toggling a route group) are visible without restart.
    pub config: Arc<RwLock<RouteAggregationConfig>>,
    /// Provider router (holds circuit breaker state, survives server stop).
    pub provider_router: Arc<provider_router::ProviderRouter>,
}

impl RouteAggregationState {
    pub fn new(config: RouteAggregationConfig) -> Self {
        Self {
            server: RwLock::new(None),
            config: Arc::new(RwLock::new(config)),
            provider_router: Arc::new(provider_router::ProviderRouter::new()),
        }
    }

    /// Build a status snapshot for the frontend.
    pub async fn get_status(&self) -> RouteAggregationStatus {
        let config = self.config.read().await;
        let server_running = self.server.read().await.is_some();
        let claude_providers = self
            .provider_router
            .get_provider_statuses(RouteGroup::ClaudeCode)
            .await;
        let codex_providers = self
            .provider_router
            .get_provider_statuses(RouteGroup::Codex)
            .await;

        RouteAggregationStatus {
            server_running,
            listen_address: config.listen_address.clone(),
            listen_port: config.listen_port,
            claude_code: GroupStatus {
                enabled: config.claude_code_enabled,
                active_providers: claude_providers,
                total_providers: 0,
            },
            codex: GroupStatus {
                enabled: config.codex_enabled,
                active_providers: codex_providers,
                total_providers: 0,
            },
        }
    }
}
