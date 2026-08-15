//! Claude Code request header injection.
//! Reference: CLIProxyAPI applyClaudeHeaders function.

use axum::http::{HeaderMap, HeaderName, HeaderValue};

use super::device_profile;

/// Inject Claude Code client headers into the request.
///
/// This simulates a real Claude Code CLI request by injecting:
/// - User-Agent: claude-cli/<version> (external, cli)
/// - Anthropic-Beta: claude-code-20250219,...
/// - Anthropic-Version: 2023-06-01
/// - X-App: cli
/// - X-Stainless-* SDK headers
/// - X-Claude-Code-Session-Id
/// - X-Client-Request-Id
pub fn inject_claude_headers(headers: &mut HeaderMap, version: &str, session_id: &str) {
    let profile = device_profile::get_stable_profile(version);
    // Anthropic-Beta
    set_header(
        headers,
        "anthropic-beta",
        "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14",
    );

    // Anthropic-Version
    set_header(headers, "anthropic-version", "2023-06-01");

    // X-App
    set_header(headers, "x-app", "cli");

    // Stainless SDK headers
    set_header(headers, "x-stainless-retry-count", "0");
    set_header(headers, "x-stainless-runtime", "node");
    set_header(headers, "x-stainless-lang", "js");
    set_header(headers, "x-stainless-timeout", "600");
    set_header(
        headers,
        "x-stainless-package-version",
        &profile.package_version,
    );
    set_header(
        headers,
        "x-stainless-runtime-version",
        &profile.runtime_version,
    );
    set_header(headers, "x-stainless-os", &profile.os);
    set_header(headers, "x-stainless-arch", &profile.arch);

    // Claude Code session ID
    set_header(headers, "x-claude-code-session-id", session_id);

    // Client request ID (new UUID per request)
    let request_id = uuid::Uuid::new_v4().to_string();
    set_header(headers, "x-client-request-id", &request_id);

    // User-Agent
    set_header(headers, "user-agent", &profile.user_agent);
}

fn set_header(headers: &mut HeaderMap, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_bytes(value.as_bytes()),
    ) {
        headers.insert(n, v);
    }
}
