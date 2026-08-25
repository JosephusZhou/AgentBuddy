//! Claude Code rectifier — main cloaking logic.
//! Reference: CLIProxyAPI claude_executor_cloaking.go
//!
//! Applies: system prompt injection, billing header forging, request header
//! injection, fake user_id generation, OAuth tool name remapping, and sensitive
//! word obfuscation.

use super::claude_billing;
use super::claude_cache;
use super::claude_context;
use super::claude_headers;
use super::claude_identity;
use super::claude_system_prompt;
use super::header_scrub;
use super::obfuscate;
use super::tool_remap;
use crate::route_aggregation::config::RouteAggregationConfig;
use crate::route_aggregation::CloakingMode;
use axum::http::HeaderMap;

/// Apply Claude Code cloaking to the request body and headers.
///
/// Returns (modified_body, modified_headers).
pub fn apply_cloaking(
    body: &serde_json::Value,
    _client_headers: &HeaderMap,
    config: &RouteAggregationConfig,
) -> Result<(serde_json::Value, HeaderMap), String> {
    let mut modified_body = body.clone();
    let mut headers = HeaderMap::new();

    // Determine whether to cloak
    let user_agent = _client_headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // 真实 Claude Code 客户端本身就是目标指纹：即使 mode=always 也不做二次伪装。
    // 用过时的配置版本重新伪造（UA / anthropic-beta / x-stainless 全部降级）只会
    // 让请求偏离"直连形态"，被校验客户端一致性的中转拒绝。改为净透传客户端头，
    // 仅由 forwarder 替换鉴权头。
    if !should_cloak(config, user_agent) || is_genuine_claude_cli(user_agent) {
        return Ok((
            modified_body,
            header_scrub::passthrough_client_headers(_client_headers),
        ));
    }

    // 1. Normalize Claude Code system shape and relocate caller instructions.
    let billing_header = format!(
        "x-anthropic-billing-header: {}",
        claude_billing::generate_billing_header(
            &config.claude_code_version,
            &claude_billing::billing_message_text(body),
        )
    );
    claude_system_prompt::apply_system_policy(
        &mut modified_body,
        config.claude_strict_mode,
        &billing_header,
    )?;

    // 2. Inject fake user_id into metadata
    claude_identity::inject_user_id(&mut modified_body, "default");

    // 3. OAuth tool name remapping
    tool_remap::remap_tool_names_in_request(&mut modified_body);

    // 4. Sensitive word obfuscation
    obfuscate::obfuscate_claude_body(&mut modified_body, &config.claude_sensitive_words);

    let context_injected = config.claude_context_management
        && claude_context::ensure_context_management(&mut modified_body);
    claude_context::remove_auto_context_management(&mut modified_body, context_injected);
    claude_cache::normalize(
        &mut modified_body,
        usize::from(config.claude_cache_max_blocks.max(1)),
        None,
    );

    // 5. Generate billing header
    let (billing_header, _signed_body) =
        claude_billing::finalize_body_with_cch(&mut modified_body, &config.claude_code_version)?;
    if let Ok(name) = axum::http::HeaderName::from_bytes("x-anthropic-billing-header".as_bytes()) {
        if let Ok(value) = axum::http::HeaderValue::from_bytes(billing_header.as_bytes()) {
            headers.insert(name, value);
        }
    }

    // 6. Inject Claude Code client headers
    let session_id = uuid::Uuid::new_v4().to_string();
    claude_headers::inject_claude_headers(&mut headers, &config.claude_code_version, &session_id);
    // 伪造头集合同样过一遍 scrub（当前集合不含代理追踪头，保持与 forwarder
    // 旧统一行为等价的防御性清理）。
    header_scrub::scrub_proxy_headers(&mut headers);

    Ok((modified_body, headers))
}

/// Apply the measured minimal shape used by Claude Code's count_tokens call.
pub fn apply_count_tokens_cloaking(
    body: &serde_json::Value,
    client_headers: &HeaderMap,
    config: &RouteAggregationConfig,
) -> Result<(serde_json::Value, HeaderMap), String> {
    let user_agent = client_headers
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !should_cloak(config, user_agent) || is_genuine_claude_cli(user_agent) {
        return Ok((
            body.clone(),
            header_scrub::passthrough_client_headers(client_headers),
        ));
    }
    let mut modified_body = body.clone();
    claude_system_prompt::relocate_for_count_tokens(&mut modified_body, config.claude_strict_mode)?;
    claude_identity::inject_user_id(&mut modified_body, "default");
    tool_remap::remap_tool_names_in_request(&mut modified_body);
    obfuscate::obfuscate_claude_body(&mut modified_body, &config.claude_sensitive_words);
    let mut headers = HeaderMap::new();
    let session_id = uuid::Uuid::new_v4().to_string();
    claude_headers::inject_claude_headers(&mut headers, &config.claude_code_version, &session_id);
    header_scrub::scrub_proxy_headers(&mut headers);
    Ok((modified_body, headers))
}

fn should_cloak(config: &RouteAggregationConfig, user_agent: &str) -> bool {
    match config.cloaking_mode {
        CloakingMode::Always => true,
        CloakingMode::Never => false,
        CloakingMode::Auto => !user_agent.starts_with("claude-cli"),
    }
}

fn is_genuine_claude_cli(user_agent: &str) -> bool {
    user_agent.trim().starts_with("claude-cli")
}

#[cfg(test)]
mod passthrough_tests {
    use super::{apply_cloaking, apply_count_tokens_cloaking};
    use crate::route_aggregation::config::RouteAggregationConfig;
    use crate::route_aggregation::CloakingMode;
    use axum::http::{HeaderMap, HeaderValue};

    fn hv(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    fn genuine_client_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", hv("claude-cli/2.1.237 (external, cli)"));
        headers.insert("authorization", hv("Bearer sk-local-route-key"));
        headers.insert("anthropic-beta", hv("effort-2025-11-24,fallback-credit-2026-06-01"));
        headers.insert("x-app", hv("cli"));
        headers.insert("x-stainless-lang", hv("js"));
        headers.insert("x-stainless-package-version", hv("0.112.1"));
        headers.insert("via", hv("1.1 local-proxy"));
        headers
    }

    fn always_config() -> RouteAggregationConfig {
        let mut config = RouteAggregationConfig::default();
        config.cloaking_mode = CloakingMode::Always;
        config
    }

    #[test]
    fn genuine_claude_cli_passes_through_even_in_always_mode() {
        let body: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/claude/basic.json"))
                .unwrap();
        let client = genuine_client_headers();

        let (out_body, out_headers) = apply_cloaking(&body, &client, &always_config()).unwrap();

        // body 不做任何改写；客户端指纹原样透传（仅 auth / 代理追踪头被剥离，
        // 鉴权由 forwarder 按供应商密钥重新注入）。
        assert_eq!(out_body, body);
        assert_eq!(
            out_headers.get("user-agent").unwrap(),
            "claude-cli/2.1.237 (external, cli)"
        );
        assert_eq!(out_headers.get("x-stainless-package-version").unwrap(), "0.112.1");
        assert!(out_headers.get("anthropic-beta").is_some());
        assert!(out_headers.get("authorization").is_none());
        assert!(out_headers.get("via").is_none());
        // 伪造头不应出现
        assert!(out_headers.get("x-anthropic-billing-header").is_none());
    }

    #[test]
    fn non_cc_client_is_fully_cloaked_in_always_mode() {
        let body: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/claude/basic.json"))
                .unwrap();
        let mut client = HeaderMap::new();
        client.insert("user-agent", hv("curl/8.4.0"));

        let config = always_config();
        let (_, out_headers) = apply_cloaking(&body, &client, &config).unwrap();

        // 非 CC 客户端仍走全量伪装：注入按配置版本生成的 Claude Code 头。
        assert!(out_headers
            .get("user-agent")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(&format!("claude-cli/{}", config.claude_code_version)));
        assert!(out_headers.get("x-anthropic-billing-header").is_some());
    }

    #[test]
    fn never_mode_still_passes_client_headers() {
        let body = serde_json::json!({ "model": "claude-opus-5", "messages": [] });
        let mut config = RouteAggregationConfig::default();
        config.cloaking_mode = CloakingMode::Never;
        let client = genuine_client_headers();

        let (out_body, out_headers) = apply_cloaking(&body, &client, &config).unwrap();
        assert_eq!(out_body, body);
        assert_eq!(
            out_headers.get("user-agent").unwrap(),
            "claude-cli/2.1.237 (external, cli)"
        );
    }

    #[test]
    fn count_tokens_genuine_client_passes_through_in_always_mode() {
        let body: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/claude/count_tokens.json"))
                .unwrap();
        let client = genuine_client_headers();

        let (out_body, out_headers) =
            apply_count_tokens_cloaking(&body, &client, &always_config()).unwrap();
        assert_eq!(out_body, body);
        assert_eq!(
            out_headers.get("user-agent").unwrap(),
            "claude-cli/2.1.237 (external, cli)"
        );
        assert!(out_headers.get("authorization").is_none());
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::apply_count_tokens_cloaking;
    use crate::route_aggregation::config::RouteAggregationConfig;
    use crate::route_aggregation::CloakingMode;
    use axum::http::HeaderMap;

    const FIXTURES: &[&str] = &[
        include_str!("../../../tests/fixtures/claude/basic.json"),
        include_str!("../../../tests/fixtures/claude/tools.json"),
        include_str!("../../../tests/fixtures/claude/system_array.json"),
        include_str!("../../../tests/fixtures/claude/system_string.json"),
        include_str!("../../../tests/fixtures/claude/no_system.json"),
        include_str!("../../../tests/fixtures/claude/streaming.json"),
        include_str!("../../../tests/fixtures/claude/count_tokens.json"),
    ];

    #[test]
    fn phase_zero_fixtures_are_valid_claude_requests() {
        for fixture in FIXTURES {
            let body: serde_json::Value = serde_json::from_str(fixture).unwrap();
            assert!(body.get("model").and_then(|v| v.as_str()).is_some());
            assert!(body.get("messages").and_then(|v| v.as_array()).is_some());
        }
    }

    #[test]
    fn count_tokens_cloaking_relocates_system_and_injects_headers() {
        let body: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude/count_tokens.json"
        ))
        .unwrap();
        let mut body = body;
        body["model"] = serde_json::Value::String("claude-opus-5".into());
        let mut config = RouteAggregationConfig::default();
        config.cloaking_mode = CloakingMode::Always;
        let (cloaked, headers) =
            apply_count_tokens_cloaking(&body, &HeaderMap::new(), &config).unwrap();
        assert!(cloaked.get("system").is_none());
        assert_eq!(cloaked["messages"][1]["role"], "system");
        assert!(headers.get("user-agent").is_some());
    }
}
