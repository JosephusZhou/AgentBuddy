//! Codex client simulation — main cloaking logic.
//! Reference: CLIProxyAPI codex_executor_request.go

use axum::http::{HeaderMap, HeaderName, HeaderValue};

use super::codex_headers;
use crate::route_aggregation::config::RouteAggregationConfig;
use crate::route_aggregation::CloakingMode;

/// Apply Codex client simulation cloaking to the request body and headers.
///
/// Returns (modified_body, modified_headers).
pub fn apply_cloaking(
    body: &serde_json::Value,
    client_headers: &HeaderMap,
    config: &RouteAggregationConfig,
) -> Result<(serde_json::Value, HeaderMap), String> {
    let mut modified_body = body.clone();
    let mut headers = HeaderMap::new();

    // Determine whether to cloak
    let user_agent = client_headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let should_cloak = match config.cloaking_mode {
        CloakingMode::Always => true,
        CloakingMode::Never => false,
        CloakingMode::Auto => {
            // Auto: cloak if the client UA is not already codex-tui
            !user_agent.starts_with("codex-tui")
        }
    };

    if !should_cloak {
        return Ok((modified_body, headers));
    }

    // 1. Inject Codex client headers
    let session_id = uuid::Uuid::new_v4().to_string();
    codex_headers::inject_codex_headers(&mut headers, &config.codex_version, None, &session_id);

    // 2. Identity confusion — replace identifiers to prevent multi-account correlation
    confuse_codex_identity(&mut modified_body);

    Ok((modified_body, headers))
}

/// Confuse Codex identity identifiers to prevent multi-account association detection.
/// Reference: CLIProxyAPI applyCodexIdentityConfuseBody
fn confuse_codex_identity(body: &mut serde_json::Value) {
    // Replace prompt_cache_key with a new UUID-derived value
    let new_cache_key = uuid::Uuid::new_v4().to_string();
    if let Some(key) = body.get_mut("prompt_cache_key") {
        *key = serde_json::Value::String(new_cache_key);
    }

    // Replace client_metadata identifiers
    if let Some(metadata) = body.get_mut("client_metadata") {
        // x-codex-installation-id
        let install_id = uuid::Uuid::new_v4().to_string();
        if let Some(id) = metadata.get_mut("x-codex-installation-id") {
            *id = serde_json::Value::String(install_id);
        }

        // x-codex-turn-metadata
        if let Some(turn_meta) = metadata.get_mut("x-codex-turn-metadata") {
            let new_turn_id = uuid::Uuid::new_v4().to_string();
            if let Some(tid) = turn_meta.get_mut("turn_id") {
                *tid = serde_json::Value::String(new_turn_id);
            }
            let new_window_id = uuid::Uuid::new_v4().to_string();
            if let Some(wid) = turn_meta.get_mut("window_id") {
                *wid = serde_json::Value::String(new_window_id);
            }
        }

        // x-codex-window-id
        let window_id = uuid::Uuid::new_v4().to_string();
        if let Some(wid) = metadata.get_mut("x-codex-window-id") {
            *wid = serde_json::Value::String(window_id);
        }
    }
}
