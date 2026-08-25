//! Codex client simulation — main cloaking logic.
//! Reference: CLIProxyAPI codex_executor_request.go

use axum::http::HeaderMap;

use super::codex_headers;
use super::header_scrub;
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
    // 与 Claude 路径同理：真实 Codex CLI（codex-tui/*）本身就是目标指纹，
    // 即使 mode=always 也净透传客户端头，仅由 forwarder 替换鉴权。
    if !should_cloak(config, user_agent) || is_genuine_codex_cli(user_agent) {
        return Ok((
            modified_body,
            header_scrub::passthrough_client_headers(client_headers),
        ));
    }

    // 1. Inject Codex client headers
    let session_id = uuid::Uuid::new_v4().to_string();
    codex_headers::inject_codex_headers(&mut headers, &config.codex_version, None, &session_id);
    header_scrub::scrub_proxy_headers(&mut headers);

    // 2. Identity confusion — replace identifiers to prevent multi-account correlation
    confuse_codex_identity(&mut modified_body);

    Ok((modified_body, headers))
}

fn should_cloak(config: &RouteAggregationConfig, user_agent: &str) -> bool {
    match config.cloaking_mode {
        CloakingMode::Always => true,
        CloakingMode::Never => false,
        CloakingMode::Auto => {
            // Auto: cloak if the client UA is not already codex-tui
            !user_agent.starts_with("codex-tui")
        }
    }
}

/// UA 判定是否为真实 Codex CLI 客户端（`codex-tui/*`）。
///
/// 与 Claude 路径的 `is_genuine_claude_cli` 对称：真实客户端本身就是目标
/// 指纹，命中时即使 mode=always 也跳过伪装、净透传客户端头。
fn is_genuine_codex_cli(user_agent: &str) -> bool {
    user_agent.trim().starts_with("codex-tui")
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

#[cfg(test)]
mod passthrough_tests {
    use super::apply_cloaking;
    use crate::route_aggregation::config::RouteAggregationConfig;
    use crate::route_aggregation::CloakingMode;
    use axum::http::{HeaderMap, HeaderValue};

    fn hv(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    #[test]
    fn genuine_codex_tui_passes_through_even_in_always_mode() {
        let mut config = RouteAggregationConfig::default();
        config.cloaking_mode = CloakingMode::Always;

        let mut client = HeaderMap::new();
        client.insert(
            "user-agent",
            hv("codex-tui/0.148.0 (Mac OS 26.5.0; arm64) iTerm.app"),
        );
        client.insert("originator", hv("codex-tui"));
        client.insert("authorization", hv("Bearer sk-local-route-key"));

        let body = serde_json::json!({ "model": "gpt-5", "prompt_cache_key": "old" });
        let (out_body, out_headers) = apply_cloaking(&body, &client, &config).unwrap();

        assert_eq!(out_body["prompt_cache_key"], "old");
        assert!(out_headers
            .get("user-agent")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("codex-tui/0.148.0"));
        assert_eq!(out_headers.get("originator").unwrap(), "codex-tui");
        assert!(out_headers.get("authorization").is_none());
        // 伪造头不应出现
        assert!(out_headers.get("session-id").is_none());
    }

    #[test]
    fn non_codex_client_gets_injected_headers() {
        let config = RouteAggregationConfig::default(); // auto
        let mut client = HeaderMap::new();
        client.insert("user-agent", hv("python-requests/2.0"));

        let body = serde_json::json!({ "model": "gpt-5", "prompt_cache_key": "old" });
        let (_, out_headers) = apply_cloaking(&body, &client, &config).unwrap();

        assert_eq!(out_headers.get("originator").unwrap(), "codex-tui");
        assert!(out_headers.get("session-id").is_some());
    }
}
