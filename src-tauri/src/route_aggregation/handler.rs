//! Request handlers — entry points for Axum routes.
//!
//! Each handler extracts the request, delegates to the forwarder, and returns
//! the upstream response (with SSE passthrough for streaming).

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::forwarder;
use super::router::AppState;
use super::RouteGroup;

/// Handler for Claude Code messages endpoint: POST /v1/messages
pub async fn handle_claude_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let config = state.config.read().await.clone();

    match forwarder::forward(RouteGroup::ClaudeCode, body, &headers, &config, &state.provider_router).await {
        Ok(resp) => resp,
        Err(forwarder::ForwardError::NoAvailableProvider) => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, "没有可用的供应商")
        }
        Err(forwarder::ForwardError::AllProvidersFailed) => {
            error_response(StatusCode::BAD_GATEWAY, "所有供应商均请求失败")
        }
        Err(forwarder::ForwardError::CloakingError(msg)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &msg)
        }
        Err(forwarder::ForwardError::RequestError(msg)) => {
            error_response(StatusCode::BAD_GATEWAY, &msg)
        }
    }
}

/// Handler for Codex Responses API: POST /v1/responses
/// Codex CLI uses the Responses API format, not Chat Completions.
pub async fn handle_codex_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let config = state.config.read().await.clone();

    match forwarder::forward(RouteGroup::Codex, body, &headers, &config, &state.provider_router).await {
        Ok(resp) => resp,
        Err(forwarder::ForwardError::NoAvailableProvider) => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, "没有可用的供应商")
        }
        Err(forwarder::ForwardError::AllProvidersFailed) => {
            error_response(StatusCode::BAD_GATEWAY, "所有供应商均请求失败")
        }
        Err(forwarder::ForwardError::CloakingError(msg)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &msg)
        }
        Err(forwarder::ForwardError::RequestError(msg)) => {
            error_response(StatusCode::BAD_GATEWAY, &msg)
        }
    }
}

/// Handler for listing models: GET /v1/models
pub async fn handle_list_models(
    State(_state): State<AppState>,
) -> Response {
    let models = serde_json::json!({
        "object": "list",
        "data": [
            {"id": "claude-sonnet-4-20250514", "object": "model", "owned_by": "anthropic"},
            {"id": "claude-haiku-4-20250506", "object": "model", "owned_by": "anthropic"},
        ]
    });
    (StatusCode::OK, Json(models)).into_response()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "type": "route_aggregation_error",
            "message": message,
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
