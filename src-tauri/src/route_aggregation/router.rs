//! Route registration — builds the Axum Router based on enabled groups.

use std::sync::Arc;

use axum::{routing::get, routing::post, Router};

use super::config::RouteAggregationConfig;
use super::handler;
use super::provider_router::ProviderRouter;

/// Shared state passed to all Axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<tokio::sync::RwLock<RouteAggregationConfig>>,
    pub provider_router: Arc<ProviderRouter>,
}

/// Build the Axum router with routes for enabled groups.
pub fn build_router(
    config: &RouteAggregationConfig,
    provider_router: Arc<ProviderRouter>,
) -> Router {
    let state = AppState {
        config: Arc::new(tokio::sync::RwLock::new(config.clone())),
        provider_router,
    };

    let mut router = Router::new();

    if config.claude_code_enabled {
        router = router
            .route(
                "/v1/messages",
                post(handler::handle_claude_messages),
            )
            .route(
                "/claude/v1/messages",
                post(handler::handle_claude_messages),
            );
    }

    if config.codex_enabled {
        router = router
            .route(
                "/v1/responses",
                post(handler::handle_codex_responses),
            )
            .route("/v1/models", get(handler::handle_list_models));
    }

    router.with_state(state)
}
