//! Request forwarder — sends requests to upstream providers with failover,
//! header injection, and SSE passthrough.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use futures::StreamExt;

use super::cloaking;
use super::config::RouteAggregationConfig;
use super::provider_router::ProviderRouter;
use super::{RouteGroup, RouteProvider};

/// Errors that can occur during forwarding.
#[derive(Debug)]
pub enum ForwardError {
    NoAvailableProvider,
    AllProvidersFailed,
    CloakingError(String),
    RequestError(String),
}

/// Forward a request through the route group's provider pool with failover.
pub async fn forward(
    group: RouteGroup,
    body: serde_json::Value,
    client_headers: &HeaderMap,
    config: &RouteAggregationConfig,
    router: &Arc<ProviderRouter>,
) -> Result<Response, ForwardError> {
    // select_providers already filters by circuit breaker state
    let providers = router.select_providers(group, config.auto_failover).await;

    if providers.is_empty() {
        return Err(ForwardError::NoAvailableProvider);
    }

    let max_attempts = (config.max_retries + 1).min(providers.len() as u32);

    let mut last_error = String::new();

    for (index, provider) in providers.iter().take(max_attempts as usize).enumerate() {
        // 1. Apply cloaking (Claude Code rectifier or Codex client simulation)
        let (modified_body, mut cloaked_headers) = match group {
            RouteGroup::ClaudeCode => {
                cloaking::claude_cloaking::apply_cloaking(&body, client_headers, config)
                    .map_err(ForwardError::CloakingError)?
            }
            RouteGroup::Codex => {
                cloaking::codex_cloaking::apply_cloaking(&body, client_headers, config)
                    .map_err(ForwardError::CloakingError)?
            }
        };

        // 2. Build auth headers from decrypted API key
        let auth_headers = build_auth_headers(provider, group);
        for (name, value) in &auth_headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                cloaked_headers.insert(n, v);
            }
        }

        // 3. Scrub proxy fingerprint headers
        cloaking::header_scrub::scrub_proxy_headers(&mut cloaked_headers);

        // 4. Build upstream URL
        let upstream_url = build_upstream_url(provider, group);

        // 5. Check if this is a streaming request
        let is_stream = modified_body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 6. Send request
        match send_request(&upstream_url, &cloaked_headers, &modified_body, config, is_stream).await {
            Ok(resp) => {
                router.record_success(&provider.id, group).await;
                return Ok(resp);
            }
            Err(e) => {
                last_error = e.clone();
                router.record_failure(&provider.id, group, &e).await;
                eprintln!(
                    "[route-aggregation] provider {} attempt {} failed: {}",
                    provider.name,
                    index + 1,
                    e
                );
                continue;
            }
        }
    }

    if last_error.is_empty() {
        Err(ForwardError::AllProvidersFailed)
    } else {
        Err(ForwardError::RequestError(last_error))
    }
}

/// Build authentication headers based on provider type and route group.
fn build_auth_headers(provider: &RouteProvider, group: RouteGroup) -> Vec<(String, String)> {
    let mut headers = Vec::new();

    if provider.api_key.is_empty() {
        return headers;
    }

    match group {
        RouteGroup::ClaudeCode => {
            if provider.provider_type == crate::ai_provider::TYPE_ANTHROPIC {
                headers.push(("x-api-key".to_string(), provider.api_key.clone()));
                headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
            } else {
                headers.push((
                    "authorization".to_string(),
                    format!("Bearer {}", provider.api_key),
                ));
            }
        }
        RouteGroup::Codex => {
            headers.push((
                "authorization".to_string(),
                format!("Bearer {}", provider.api_key),
            ));
        }
    }

    headers
}

/// Build the upstream URL for the given provider and group.
fn build_upstream_url(provider: &RouteProvider, group: RouteGroup) -> String {
    let base = provider.base_url.trim_end_matches('/');
    match group {
        RouteGroup::ClaudeCode => format!("{}/v1/messages", base),
        RouteGroup::Codex => format!("{}/v1/responses", base),
    }
}

/// Send the request to the upstream provider.
async fn send_request(
    url: &str,
    headers: &HeaderMap,
    body: &serde_json::Value,
    config: &RouteAggregationConfig,
    is_stream: bool,
) -> Result<Response, String> {
    let client = build_proxied_client(config)?;

    let mut req = client
        .post(url)
        .header("content-type", "application/json")
        .header(
            "accept",
            if is_stream { "text/event-stream" } else { "application/json" },
        );

    // Copy cloaked headers (skip ones we already set)
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("content-type")
            || name_str.eq_ignore_ascii_case("accept")
            || name_str.eq_ignore_ascii_case("content-length")
            || name_str.eq_ignore_ascii_case("host")
            || name_str.eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        req = req.header(name, value);
    }

    let body_str = serde_json::to_string(body)
        .map_err(|e| format!("序列化请求体失败: {e}"))?;
    let req = req.body(body_str);

    let response = req
        .send()
        .await
        .map_err(|e| format!("上游请求失败: {e}"))?;

    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);

    let mut response_builder = Response::builder().status(status);

    // Copy key response headers
    for key in &["content-type", "cache-control", "connection", "x-request-id"] {
        if let Some(val) = response.headers().get(*key) {
            response_builder = response_builder.header(*key, val);
        }
    }

    if is_stream {
        let stream = response
            .bytes_stream()
            .map(|result| {
                result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            });

        response_builder
            .body(Body::from_stream(stream))
            .map_err(|e| format!("构建流式响应失败: {e}"))
    } else {
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| format!("读取响应体失败: {e}"))?;
        response_builder
            .body(Body::from(body_bytes))
            .map_err(|e| format!("构建响应失败: {e}"))
    }
}

/// Build a reqwest async client with outbound proxy applied.
fn build_proxied_client(config: &RouteAggregationConfig) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.non_stream_total_timeout));

    let network = crate::config::load_network_settings().unwrap_or_default();
    use crate::config::{ProxyMode, ProxyProtocol};

    match network.proxy.mode {
        ProxyMode::None => {
            builder = builder.no_proxy();
        }
        ProxyMode::System => {
            #[cfg(target_os = "macos")]
            {
                if let Ok(output) = std::process::Command::new("scutil")
                    .arg("--proxy")
                    .output()
                {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(host) = parse_scutil_proxy(&stdout, "HTTP") {
                        if let Ok(proxy) = reqwest::Proxy::all(&host) {
                            builder = builder.proxy(proxy);
                        }
                    }
                }
            }
        }
        ProxyMode::Custom => {
            let p = &network.proxy;
            if !p.host.is_empty() && p.port > 0 {
                let scheme = if p.protocol == ProxyProtocol::Socks5 { "socks5" } else { "http" };
                let url = if p.username.is_empty() {
                    format!("{}://{}:{}", scheme, p.host, p.port)
                } else {
                    format!("{}://{}:{}@{}:{}", scheme, p.username, p.password, p.host, p.port)
                };
                if let Ok(proxy) = reqwest::Proxy::all(&url) {
                    builder = builder.proxy(proxy);
                }
            }
        }
    }

    builder.build().map_err(|e| format!("构建 HTTP 客户端失败: {e}"))
}

#[cfg(target_os = "macos")]
fn parse_scutil_proxy(scutil_output: &str, proxy_type: &str) -> Option<String> {
    let key = format!("{}Enable", proxy_type);
    let mut enabled = false;
    let mut host = String::new();
    let mut port = 0u16;

    for line in scutil_output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&key) {
            enabled = trimmed.contains("1");
        }
        if trimmed.starts_with(&format!("{}Proxy", proxy_type)) {
            host = trimmed
                .split(':')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
        }
        if trimmed.starts_with(&format!("{}Port", proxy_type)) {
            port = trimmed
                .split(':')
                .nth(1)
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }

    if enabled && !host.is_empty() && port > 0 {
        Some(format!("http://{}:{}", host, port))
    } else {
        None
    }
}
