//! Request handlers — entry points for Axum routes.
//!
//! The aggregated endpoint always serves both API formats (Claude messages
//! and Codex responses) while the server is running. Each handler
//! authenticates the request against the configured API key list, then
//! delegates to the forwarder and returns the upstream response
//! (with SSE passthrough for streaming).

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::forwarder;
use super::router::AppState;
use super::RouteGroup;

/// Extract the Bearer token from the Authorization header.
/// Returns None if the header is missing or not a Bearer token.
fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Authenticate a request against the configured API key list.
/// Returns Ok(()) if the token matches any key, or if no key is configured
/// (no-auth mode). Returns Err(Response) with 401 otherwise.
fn authenticate(headers: &HeaderMap, api_keys: &[String]) -> Result<(), Response> {
    if api_keys.is_empty() {
        // No API key configured — open access
        return Ok(());
    }
    match extract_bearer_token(headers) {
        Some(token) if api_keys.iter().any(|k| *k == token) => Ok(()),
        _ => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "无效或缺失的 API Key",
        )),
    }
}

/// Handler for Claude Code messages endpoint: POST /v1/messages
pub async fn handle_claude_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let config = state.config.read().await.clone();

    if let Err(resp) = authenticate(&headers, &config.api_keys) {
        return resp;
    }

    match forwarder::forward(
        RouteGroup::ClaudeCode,
        body,
        &headers,
        &config,
        &state.provider_router,
        &state.translator_registry,
    )
    .await
    {
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
        Err(forwarder::ForwardError::UnsupportedTranslation(msg)) => {
            error_response(StatusCode::NOT_IMPLEMENTED, &msg)
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

    if let Err(resp) = authenticate(&headers, &config.api_keys) {
        return resp;
    }

    match forwarder::forward(
        RouteGroup::Codex,
        body,
        &headers,
        &config,
        &state.provider_router,
        &state.translator_registry,
    )
    .await
    {
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
        Err(forwarder::ForwardError::UnsupportedTranslation(msg)) => {
            error_response(StatusCode::NOT_IMPLEMENTED, &msg)
        }
    }
}

/// Handler for listing models: GET /v1/models
///
/// Authenticates against the API key list and returns the union of all
/// checked providers' models. A provider with a custom model list uses it
/// directly; otherwise models are fetched from its remote /v1/models API.
pub async fn handle_list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let config = state.config.read().await.clone();

    if let Err(resp) = authenticate(&headers, &config.api_keys) {
        return resp;
    }

    let infos = state.provider_router.get_enabled_provider_model_infos().await;

    // Collect model IDs on a blocking thread (remote fetches are blocking).
    let ids: Vec<String> = tauri::async_runtime::spawn_blocking(move || {
        let mut model_set = std::collections::BTreeSet::new();
        for (_id, name, model_ids, base_url, api_key) in &infos {
            if !model_ids.is_empty() {
                // Custom model list takes precedence over remote fetching.
                for mid in model_ids {
                    model_set.insert(mid.clone());
                }
            } else {
                let key = if api_key.is_empty() { None } else { Some(api_key.clone()) };
                match crate::claude_env::fetch_remote_models(base_url.clone(), key, None) {
                    Ok(result) => {
                        for mid in result.model_ids {
                            model_set.insert(mid);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[route-aggregation] /v1/models fetch from {} failed: {}",
                            name, e
                        );
                    }
                }
            }
        }
        model_set.into_iter().collect::<Vec<String>>()
    })
    .await
    .unwrap_or_default();

    // Build the response
    let data: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "object": "model",
                "owned_by": "route-aggregation",
            })
        })
        .collect();

    let body = serde_json::json!({
        "object": "list",
        "data": data,
    });
    (StatusCode::OK, Json(body)).into_response()
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
