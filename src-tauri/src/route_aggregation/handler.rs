//! Request handlers — entry points for Axum routes.
//!
//! Each handler authenticates the request against the per-group API key,
//! then delegates to the forwarder and returns the upstream response
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

/// Authenticate a request against the given group's API key.
/// Returns Ok(()) if the key matches or no key is configured (no-auth mode).
/// Returns Err(Response) with 401 if authentication fails.
fn authenticate(headers: &HeaderMap, expected_key: &str) -> Result<(), Response> {
    if expected_key.is_empty() {
        // No API key configured for this group — open access
        return Ok(());
    }
    match extract_bearer_token(headers) {
        Some(token) if token == expected_key => Ok(()),
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

    if !config.claude_code_enabled {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "Claude Code 路由未启用");
    }

    if let Err(resp) = authenticate(&headers, &config.claude_code_api_key) {
        return resp;
    }

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

    if !config.codex_enabled {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "Codex 路由未启用");
    }

    if let Err(resp) = authenticate(&headers, &config.codex_api_key) {
        return resp;
    }

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
///
/// Authenticates the request against either group's API key and returns
/// the stored model list for the matching group. If the stored list is
/// empty, falls back to auto-collecting from enabled providers.
pub async fn handle_list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let config = state.config.read().await.clone();

    let cc_key_set = !config.claude_code_api_key.is_empty();
    let codex_key_set = !config.codex_api_key.is_empty();

    // Determine which group's models to return based on the API key
    let group = if cc_key_set || codex_key_set {
        // Auth mode: identify the group from the provided API key
        match extract_bearer_token(&headers).as_deref() {
            Some(key) if cc_key_set && key == config.claude_code_api_key => Some(RouteGroup::ClaudeCode),
            Some(key) if codex_key_set && key == config.codex_api_key => Some(RouteGroup::Codex),
            _ => {
                return error_response(StatusCode::UNAUTHORIZED, "无效或缺失的 API Key");
            }
        }
    } else {
        // No-auth mode: return union of all enabled groups
        None
    };

    // Get the stored model list for the identified group(s)
    // Each entry is (model_id, alias) — both owned to avoid lifetime issues
    // with the auto-collect fallback path.
    let mut entries: Vec<(String, String)> = Vec::new();
    let need_fallback;

    match group {
        Some(RouteGroup::ClaudeCode) => {
            need_fallback = config.claude_code_models.is_empty();
            if !need_fallback {
                entries = config.claude_code_models.iter()
                    .map(|e| (e.id.clone(), e.alias.clone()))
                    .collect();
            }
        }
        Some(RouteGroup::Codex) => {
            need_fallback = config.codex_models.is_empty();
            if !need_fallback {
                entries = config.codex_models.iter()
                    .map(|e| (e.id.clone(), e.alias.clone()))
                    .collect();
            }
        }
        None => {
            // No-auth: merge both groups
            let cc_empty = config.claude_code_models.is_empty();
            let codex_empty = config.codex_models.is_empty();
            if cc_empty && codex_empty {
                need_fallback = true;
            } else {
                need_fallback = false;
                for e in &config.claude_code_models {
                    entries.push((e.id.clone(), e.alias.clone()));
                }
                for e in &config.codex_models {
                    entries.push((e.id.clone(), e.alias.clone()));
                }
            }
        }
    }

    // Fallback: fetch from providers' remote /v1/models APIs if stored list is empty
    if need_fallback {
        let groups_to_fetch: Vec<RouteGroup> = match group {
            Some(g) => vec![g],
            None => {
                let mut gs = Vec::new();
                if config.claude_code_enabled {
                    gs.push(RouteGroup::ClaudeCode);
                }
                if config.codex_enabled {
                    gs.push(RouteGroup::Codex);
                }
                gs
            }
        };

        let mut ids: Vec<String> = Vec::new();
        for g in &groups_to_fetch {
            let provider_infos = state.provider_router.get_enabled_provider_infos(*g).await;
            // Move to blocking thread for network calls
            let infos = provider_infos.clone();
            if let Ok(remote_ids) = tauri::async_runtime::spawn_blocking(move || {
                let mut model_set = std::collections::BTreeSet::new();
                for (_id, _name, base_url, api_key) in &infos {
                    let key = if api_key.is_empty() { None } else { Some(api_key.clone()) };
                    match crate::claude_env::fetch_remote_models(base_url.clone(), key) {
                        Ok(result) => {
                            for mid in result.model_ids {
                                model_set.insert(mid);
                            }
                        }
                        Err(e) => {
                            eprintln!("[route-aggregation] /v1/models fallback fetch failed: {}", e);
                        }
                    }
                }
                model_set.into_iter().collect::<Vec<String>>()
            }).await {
                ids.extend(remote_ids);
            }
        }
        ids.sort();
        ids.dedup();
        entries = ids.into_iter().map(|id| (id, String::new())).collect();
    }

    // Build the response
    let data: Vec<serde_json::Value> = entries
        .iter()
        .map(|(id, alias)| {
            let display = if alias.is_empty() { id.as_str() } else { alias.as_str() };
            serde_json::json!({
                "id": display,
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
