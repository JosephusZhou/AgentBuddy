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
    let should_cloak = should_cloak(config, user_agent);

    if !should_cloak {
        // Still inject auth headers but don't do full cloaking
        return Ok((modified_body, headers));
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
    if !should_cloak(config, user_agent) {
        return Ok((body.clone(), HeaderMap::new()));
    }
    let mut modified_body = body.clone();
    claude_system_prompt::relocate_for_count_tokens(&mut modified_body, config.claude_strict_mode)?;
    claude_identity::inject_user_id(&mut modified_body, "default");
    tool_remap::remap_tool_names_in_request(&mut modified_body);
    obfuscate::obfuscate_claude_body(&mut modified_body, &config.claude_sensitive_words);
    let mut headers = HeaderMap::new();
    let session_id = uuid::Uuid::new_v4().to_string();
    claude_headers::inject_claude_headers(&mut headers, &config.claude_code_version, &session_id);
    Ok((modified_body, headers))
}

fn should_cloak(config: &RouteAggregationConfig, user_agent: &str) -> bool {
    match config.cloaking_mode {
        CloakingMode::Always => true,
        CloakingMode::Never => false,
        CloakingMode::Auto => !user_agent.starts_with("claude-cli"),
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
