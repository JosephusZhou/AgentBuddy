//! Codex CLI (codex-tui) request header injection.
//! Reference: CLIProxyAPI applyCodexHeadersFromSources function.

use axum::http::{HeaderMap, HeaderName, HeaderValue};

/// Inject Codex CLI client headers into the request.
///
/// Simulates a real codex-tui request by injecting:
/// - User-Agent: codex-tui/<version> (Mac OS 26.5.0; arm64) iTerm.app/3.6.10
/// - Originator: codex-tui
/// - Session-Id
/// - ChatGPT-Account-Id (if provided)
pub fn inject_codex_headers(
    headers: &mut HeaderMap,
    version: &str,
    account_id: Option<&str>,
    session_id: &str,
) {
    // User-Agent
    let ua = format!(
        "codex-tui/{} (Mac OS 26.5.0; arm64) iTerm.app/3.6.10 (codex-tui; {})",
        version, version
    );
    set_header(headers, "user-agent", &ua);

    // Originator
    set_header(headers, "originator", "codex-tui");

    // Session-Id (the upstream wire name uses a hyphen, not an underscore)
    set_header(headers, "session-id", session_id);

    // ChatGPT-Account-Id (if provided)
    if let Some(id) = account_id {
        set_header(headers, "chatgpt-account-id", id);
    }

    // Content-Type
    set_header(headers, "content-type", "application/json");
}

fn set_header(headers: &mut HeaderMap, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_bytes(value.as_bytes()),
    ) {
        headers.insert(n, v);
    }
}

#[cfg(test)]
mod tests {
    use super::inject_codex_headers;
    use axum::http::HeaderMap;

    #[test]
    fn uses_upstream_session_id_header_name() {
        let mut headers = HeaderMap::new();
        inject_codex_headers(&mut headers, "0.146.0", None, "session-123");

        assert_eq!(headers.get("session-id").unwrap(), "session-123");
        assert!(headers.get("session_id").is_none());
    }
}
