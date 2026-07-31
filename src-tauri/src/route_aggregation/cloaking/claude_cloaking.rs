//! Claude Code rectifier — main cloaking logic.
//! Reference: CLIProxyAPI claude_executor_cloaking.go
//!
//! Applies: system prompt injection, billing header forging, request header
//! injection, fake user_id generation, OAuth tool name remapping, and sensitive
//! word obfuscation.

use axum::http::HeaderMap;
use rand::Rng;

use super::claude_billing;
use super::claude_headers;
use super::claude_system_prompt;
use super::obfuscate;
use super::tool_remap;
use crate::route_aggregation::config::RouteAggregationConfig;
use crate::route_aggregation::CloakingMode;

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
    let should_cloak = match config.cloaking_mode {
        CloakingMode::Always => true,
        CloakingMode::Never => false,
        CloakingMode::Auto => {
            // Auto: cloak if the client UA is not already claude-cli
            !user_agent.starts_with("claude-cli")
        }
    };

    if !should_cloak {
        // Still inject auth headers but don't do full cloaking
        return Ok((modified_body, headers));
    }

    // 1. Inject Claude Code system prompts
    let original_system = body
        .get("system")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    let system_array = claude_system_prompt::build_system_array(original_system.as_deref());
    modified_body["system"] = serde_json::Value::Array(system_array);

    // If original system prompt exists, move it to the first user message
    if let Some(orig) = original_system {
        inject_system_reminder_into_first_message(&mut modified_body, &orig);
    }

    // 2. Inject fake user_id into metadata
    let user_id = generate_fake_user_id();
    if let Some(metadata) = modified_body.get_mut("metadata") {
        metadata["user_id"] = serde_json::Value::String(user_id);
    } else {
        let metadata = serde_json::json!({"user_id": user_id});
        modified_body["metadata"] = metadata;
    }

    // 3. OAuth tool name remapping
    tool_remap::remap_tool_names_in_request(&mut modified_body);

    // 4. Sensitive word obfuscation
    obfuscate::obfuscate_body_strings(&mut modified_body);

    // 5. Generate billing header
    let body_text = serde_json::to_string(&modified_body).unwrap_or_default();
    let billing_header = claude_billing::generate_billing_header(
        &config.claude_code_version,
        &body_text,
    );
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

/// Generate a fake Claude Code format user_id.
/// Format: user_[64-hex-chars]_account_[UUID-v4]_session_[UUID-v4]
fn generate_fake_user_id() -> String {
    let mut hex_bytes = [0u8; 32];
    rand::thread_rng().fill(&mut hex_bytes);
    let hex_part = hex::encode(hex_bytes);
    let account_uuid = uuid::Uuid::new_v4();
    let session_uuid = uuid::Uuid::new_v4();
    format!(
        "user_{}_account_{}_session_{}",
        hex_part, account_uuid, session_uuid
    )
}

/// Inject the original system prompt into the first user message as a
/// <system-reminder> tag, matching CLIProxyAPI's non-strict mode.
fn inject_system_reminder_into_first_message(body: &mut serde_json::Value, system_text: &str) {
    let messages = match body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(m) => m,
        None => return,
    };

    if messages.is_empty() {
        return;
    }

    let first_msg = &mut messages[0];

    // Check if the first message is a user message
    let role = first_msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
    if role != "user" {
        return;
    }

    // Inject system-reminder into the content
    let reminder = format!("<system-reminder>\n{}\n</system-reminder>", system_text);

    if let Some(content) = first_msg.get_mut("content") {
        if let Some(content_str) = content.as_str() {
            // Prepend the reminder
            let new_content = format!("{}\n\n{}", reminder, content_str);
            *content = serde_json::Value::String(new_content);
        } else if let Some(content_arr) = content.as_array_mut() {
            // If content is an array, prepend a text block
            let mut new_block = serde_json::json!({
                "type": "text",
                "text": reminder
            });
            content_arr.insert(0, new_block);
        }
    }
}
