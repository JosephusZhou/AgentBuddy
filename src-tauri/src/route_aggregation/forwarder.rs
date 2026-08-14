//! Request forwarder — sends requests to upstream providers with failover,
//! header injection, same-protocol passthrough, and SSE passthrough.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use futures::StreamExt;

use super::cloaking;
use super::config::RouteAggregationConfig;
use super::provider_router::ProviderRouter;
use super::{ProviderFormat, RouteGroup, RouteProvider};

/// Errors that can occur during forwarding.
#[derive(Debug)]
pub enum ForwardError {
    NoAvailableProvider,
    AllProvidersFailed,
    CloakingError(String),
    RequestError(String),
}

/// Result of a successful forward — the final response plus a body preview
/// captured for the inbound/outbound log.
///
/// `body_preview` is the upstream response body, capped at
/// `super::log::BODY_PREVIEW_MAX_BYTES`. For streaming responses we skip
/// reading the body so we don't break backpressure; `body_preview` will be
/// empty and `body_truncated` is meaningless (always `false` in that case).
#[derive(Debug)]
pub struct ForwardResult {
    pub response: Response,
    pub body_preview: Vec<u8>,
    pub body_truncated: bool,
    /// Id of the provider that successfully served this request. Used by the
    /// handler to fill the `provider_id` / `provider_name` fields on the
    /// inbound/outbound log entry.
    pub provider_id: String,
    pub provider_name: String,
    /// Effective upstream URL the request was sent to. Filled by forwarder so
    /// the log entry can surface "which exact endpoint served the request".
    pub upstream_url: String,
}

/// 把客户端入口（RouteGroup）映射为协议格式。
///
/// Phase 5+：路由聚合只支持两种入站协议，每个 group 对应唯一一个 ProviderFormat；
/// `format_for_group` 与 `format_for_provider_type` 总是在 group 一致时返回相同结果。
#[allow(dead_code)] // 公开 API，外部 caller 可能依赖；Forwarder 内部不再使用
pub fn format_for_group(group: RouteGroup) -> ProviderFormat {
    match group {
        // Claude Code 客户端 → Anthropic Messages 协议
        RouteGroup::ClaudeCode => ProviderFormat::Anthropic,
        // Codex CLI 客户端 → OpenAI Responses 协议
        RouteGroup::Codex => ProviderFormat::OpenAiResponses,
    }
}

/// 把上游 provider 类型映射为协议格式。
///
/// `group` 是客户端入口，决定同一 provider_type 在不同入站组下使用哪种 ProviderFormat：
/// - universal（同时支持 Anthropic + OpenAI Responses）：ClaudeCode 组走 Anthropic
///   （`/v1/messages`），Codex 组走 OpenAiResponses（`/v1/responses`）。两组下都不需要
///   协议转换，直接 passthrough。
/// - openai 类型：Codex 组走 OpenAiResponses（用户拍板决策 2026-08-13：去掉 Chat 兼容，
///   外部 OpenAI client 全部走 Responses 协议）。
/// - anthropic 类型：固定 Anthropic（ClaudeCode 组的同方言 passthrough）。
/// - 其它类型：兜底为 OpenAiResponses；正常路径不会命中。
pub fn format_for_provider_type(provider_type: &str, group: RouteGroup) -> ProviderFormat {
    match provider_type {
        crate::ai_provider::TYPE_ANTHROPIC => ProviderFormat::Anthropic,
        crate::ai_provider::TYPE_UNIVERSAL => match group {
            RouteGroup::ClaudeCode => ProviderFormat::Anthropic,
            RouteGroup::Codex => ProviderFormat::OpenAiResponses,
        },
        crate::ai_provider::TYPE_OPENAI => ProviderFormat::OpenAiResponses,
        _ => ProviderFormat::OpenAiResponses,
    }
}

/// Forward a request through the route group's provider pool with failover.
///
/// 路由聚合只走 A→A / OR→OR 的 passthrough——入站协议 = upstream 协议，
/// body / SSE / header（除 auth）原样转发，仅在 provider 选择层做 failover。
pub async fn forward(
    group: RouteGroup,
    body: serde_json::Value,
    client_headers: &HeaderMap,
    config: &RouteAggregationConfig,
    router: &Arc<ProviderRouter>,
) -> Result<ForwardResult, ForwardError> {
    // select_providers already filters by circuit breaker state
    let providers = router.select_providers(group, config.auto_failover).await;

    if providers.is_empty() {
        return Err(ForwardError::NoAvailableProvider);
    }

    // 按请求 model 过滤 pool：把不提供该 model 的 provider 直接剔除，避免
    // 浪费 round-trip / 触发不可知的错误。ProviderRouter::filter_by_model 内
    // 未配置自定义模型的 provider 保留，以支持用户手动指定模型 ID。
    let request_model = body.get("model").and_then(|v| v.as_str());
    let providers = ProviderRouter::filter_by_model(providers, request_model);

    if providers.is_empty() {
        return Err(ForwardError::NoAvailableProvider);
    }

    let max_attempts = (config.max_retries + 1).min(providers.len() as u32);

    let mut last_error = String::new();

    for (index, provider) in providers.iter().take(max_attempts as usize).enumerate() {
        let target_format = format_for_provider_type(&provider.provider_type, group);


        // 1. Apply cloaking (Claude Code rectifier or Codex client simulation).
        let (cloaked_body, mut cloaked_headers) = match group {
            RouteGroup::ClaudeCode => {
                cloaking::claude_cloaking::apply_cloaking(&body, client_headers, config)
                    .map_err(ForwardError::CloakingError)?
            }
            RouteGroup::Codex => {
                cloaking::codex_cloaking::apply_cloaking(&body, client_headers, config)
                    .map_err(ForwardError::CloakingError)?
            }
        };

        // 2. Effective body = cloaked body（passthrough 不做协议转换）
        let effective_body = cloaked_body.clone();
        let effective_model = cloaked_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 3. Build auth headers from decrypted API key
        let auth_headers = build_auth_headers(provider, target_format);
        for (name, value) in &auth_headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                cloaked_headers.insert(n, v);
            }
        }

        // 4. Scrub proxy fingerprint headers
        cloaking::header_scrub::scrub_proxy_headers(&mut cloaked_headers);

        // 5. Build upstream URL（Phase 5+：无 passthrough 旁路，直接拼 target 路径）
        let is_stream = is_stream(&effective_body);
        let upstream_url = build_upstream_url(
            provider, target_format, &effective_model, is_stream,
        );

        // 6. Send request
        match send_request(&upstream_url, &cloaked_headers, &effective_body, config, is_stream).await {
            Ok((resp, body_preview, body_truncated)) => {
                router.record_success(&provider.id, group).await;
                // passthrough：直接返回上游响应，不做协议转换
                return Ok(ForwardResult {
                    response: resp,
                    body_preview,
                    body_truncated,
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    upstream_url,
                });
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

fn is_stream(body: &serde_json::Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Build authentication headers based on target format.
fn build_auth_headers(provider: &RouteProvider, target: ProviderFormat) -> Vec<(String, String)> {
    let mut headers = Vec::new();

    if provider.api_key.is_empty() {
        return headers;
    }

    match target {
        ProviderFormat::Anthropic => {
            headers.push(("x-api-key".to_string(), provider.api_key.clone()));
            headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
        }
        ProviderFormat::OpenAiResponses => {
            headers.push((
                "authorization".to_string(),
                format!("Bearer {}", provider.api_key),
            ));
        }
    }

    headers
}

/// Build the upstream URL for the given provider and target protocol.
///
/// Phase 5+：路由聚合只支持 A→A / OR→OR 同方言 passthrough。
/// - Anthropic → `{base}/v1/messages`
/// - OpenAI Responses → `{base}/v1/responses`
fn build_upstream_url(
    provider: &RouteProvider,
    target: ProviderFormat,
    _model: &str,
    _is_stream: bool,
) -> String {
    let base = provider.base_url.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);

    match target {
        ProviderFormat::Anthropic => format!("{}/v1/messages", base),
        ProviderFormat::OpenAiResponses => format!("{}/v1/responses", base),
    }
}

/// Send the request to the upstream provider.
///
/// Returns `(response, body_preview, body_truncated)`:
/// - Streaming: `body_preview` is empty (we don't read streaming bodies — that
///   would break backpressure and SSE event boundaries).
/// - Non-streaming: `body_preview` is up to `BODY_PREVIEW_MAX_BYTES` bytes.
async fn send_request(
    url: &str,
    headers: &HeaderMap,
    body: &serde_json::Value,
    config: &RouteAggregationConfig,
    is_stream: bool,
) -> Result<(Response, Vec<u8>, bool), String> {
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

        let response = response_builder
            .body(Body::from_stream(stream))
            .map_err(|e| format!("构建流式响应失败: {e}"))?;
        Ok((response, Vec::new(), false))
    } else {
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| format!("读取响应体失败: {e}"))?;
        let (preview, truncated) = super::log::truncate_for_preview(
            &body_bytes,
            super::log::BODY_PREVIEW_MAX_BYTES,
        );
        let response = response_builder
            .body(Body::from(body_bytes))
            .map_err(|e| format!("构建响应失败: {e}"))?;
        Ok((response, preview.into_bytes(), truncated))
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
