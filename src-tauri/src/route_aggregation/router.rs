//! Route registration — builds the Axum Router.
//!
//! All routes are always registered regardless of which groups are enabled.
//! Enabling/disabling a route group is handled at runtime by the handler
//! (checking the shared config), so toggling a group does NOT require a
//! server restart.

use std::sync::Arc;

use axum::{routing::get, routing::post, Router};
use tokio::sync::RwLock;

use super::config::RouteAggregationConfig;
use super::handler;
use super::log::LogStore;
use super::provider_router::ProviderRouter;

/// Shared state passed to all Axum handlers.
///
/// `config` is the same `Arc<RwLock<Config>>` used by `RouteAggregationState`,
/// so runtime config updates are immediately visible to handlers without
/// needing to restart the server.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<RouteAggregationConfig>>,
    pub provider_router: Arc<ProviderRouter>,
    pub log_store: LogStore,
}

/// Build the Axum router.
///
/// All routes are always registered. Whether a group accepts requests is
/// decided at runtime by the handler reading the shared config — this allows
/// toggling groups without restarting the server.
pub fn build_router(
    config: Arc<RwLock<RouteAggregationConfig>>,
    provider_router: Arc<ProviderRouter>,
    log_store: LogStore,
) -> Router {
    let state = AppState {
        config,
        provider_router,
        log_store,
    };

    Router::new()
        // Claude Code: POST /v1/messages (and /claude/v1/messages alias)
        .route("/v1/messages", post(handler::handle_claude_messages))
        .route("/claude/v1/messages", post(handler::handle_claude_messages))
        .route(
            "/v1/messages/count_tokens",
            post(handler::handle_claude_count_tokens),
        )
        // Codex: POST /v1/responses (Responses API, not Chat Completions)
        .route("/v1/responses", post(handler::handle_codex_responses))
        // /v1/models is always available when the server is running
        .route("/v1/models", get(handler::handle_list_models))
        .with_state(state)
}
