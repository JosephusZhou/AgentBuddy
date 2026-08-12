//! Request forwarder — sends requests to upstream providers with failover,
//! header injection, multi-protocol translation, and SSE passthrough.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures::StreamExt;

use super::cloaking;
use super::config::RouteAggregationConfig;
use super::provider_router::ProviderRouter;
use super::translator::{
    claude_gemini, common::http, Format, StreamParams, TranslatorRegistry,
};
use super::{RouteGroup, RouteProvider};

/// Errors that can occur during forwarding.
#[derive(Debug)]
pub enum ForwardError {
    NoAvailableProvider,
    AllProvidersFailed,
    CloakingError(String),
    RequestError(String),
    /// 当前 (source, target) 没有注册 translator，且 target != source。
    UnsupportedTranslation(String),
}

/// 把客户端入口（RouteGroup）映射为协议格式。
pub fn format_for_group(group: RouteGroup) -> Format {
    match group {
        // Claude Code 客户端 → Anthropic Messages 协议
        RouteGroup::ClaudeCode => Format::Anthropic,
        // Codex CLI 客户端 → OpenAI Responses 协议（Phase 1 用作 placeholder，
        // 真正的 Responses → Gemini 翻译在 Phase 4 接入）
        RouteGroup::Codex => Format::OpenAiResponses,
    }
}

/// 把上游 provider 类型映射为协议格式。
pub fn format_for_provider_type(provider_type: &str) -> Format {
    match provider_type {
        crate::ai_provider::TYPE_ANTHROPIC => Format::Anthropic,
        crate::ai_provider::TYPE_OPENAI => Format::OpenAiChat,
        crate::ai_provider::TYPE_GOOGLE_GENERATIVE_AI => Format::Gemini,
        // universal 类型在 Codex 组下走 OpenAI Chat，在 ClaudeCode 组下走 Anthropic。
        // 由 ProviderRouter 派生 base_url 时已经区分；这里按 OpenAI 处理是兜底，
        // 实际调用前 ProviderRouter 会按 group 调整 base_url。
        crate::ai_provider::TYPE_UNIVERSAL => Format::OpenAiChat,
        _ => Format::OpenAiChat,
    }
}

/// Forward a request through the route group's provider pool with failover.
pub async fn forward(
    group: RouteGroup,
    body: serde_json::Value,
    client_headers: &HeaderMap,
    config: &RouteAggregationConfig,
    router: &Arc<ProviderRouter>,
    translators: &TranslatorRegistry,
) -> Result<Response, ForwardError> {
    // select_providers already filters by circuit breaker state
    let providers = router.select_providers(group, config.auto_failover).await;

    if providers.is_empty() {
        return Err(ForwardError::NoAvailableProvider);
    }

    let max_attempts = (config.max_retries + 1).min(providers.len() as u32);
    let source_format = format_for_group(group);

    let mut last_error = String::new();

    for (index, provider) in providers.iter().take(max_attempts as usize).enumerate() {
        let target_format = format_for_provider_type(&provider.provider_type);

        // 用户拍板简化（2026-08-12）：当 provider 是 Google 且客户端是 OpenAI Chat
        // 协议时，直接走 Google OpenAI 兼容端点 passthrough，跳过请求翻译。
        // 浏览器 URL 改写为 `/v1beta/openai/v1/chat/completions`，
        // 请求体原样，响应体原样（Google 端点原生返回 OpenAI Chat JSON）。
        let effective_source_format = if should_passthrough_google_openai_compat(
            provider, source_format,
        ) {
            target_format
        } else {
            source_format
        };

        // 1. Apply cloaking (Claude Code rectifier or Codex client simulation)
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

        // 2. 决定是否需要翻译
        let needs_translation = effective_source_format != target_format;

        // 3. 翻译请求 body（如果需要）
        let (effective_body, effective_model, mut stream_params) = if needs_translation {
            match translators.get(effective_source_format, target_format) {
                Some(t) => {
                    let model = cloaked_body
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mut params = StreamParams::default();
                    let translated = t
                        .translate_request(&model, &cloaked_body, is_stream(&cloaked_body), &mut params)
                        .map_err(|e| {
                            ForwardError::RequestError(format!(
                                "翻译请求失败 ({source_format:?}→{target_format:?}): {e}"
                            ))
                        })?;
                    (translated, model, params)
                }
                None => {
                    return Err(ForwardError::UnsupportedTranslation(format!(
                        "未注册 translator：{source_format:?} → {target_format:?}"
                    )));
                }
            }
        } else {
            let model = cloaked_body
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (cloaked_body, model, StreamParams::default())
        };

        // 4. Build auth headers from decrypted API key
        let auth_headers = build_auth_headers(provider, target_format);
        for (name, value) in &auth_headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                cloaked_headers.insert(n, v);
            }
        }

        // 5. Scrub proxy fingerprint headers
        cloaking::header_scrub::scrub_proxy_headers(&mut cloaked_headers);

        // 6. Build upstream URL
        let is_stream = is_stream(&effective_body);
        let upstream_url = if should_passthrough_google_openai_compat(provider, source_format) {
            // passthrough：使用 Google OpenAI 兼容端点 URL
            build_google_openai_compat_url(provider, &effective_model, is_stream)
        } else {
            build_upstream_url(
                provider, effective_source_format, target_format, &effective_model, is_stream,
            )
        };

        // 7. Send request
        match send_request(&upstream_url, &cloaked_headers, &effective_body, config, is_stream).await {
            Ok(resp) => {
                router.record_success(&provider.id, group).await;
                // 8. 翻译响应（如果需要）
                if needs_translation {
                    if let Some(t) = translators.get(effective_source_format, target_format) {
                        return translate_response(
                            resp,
                            t.as_ref(),
                            &effective_model,
                            &body,
                            &effective_body,
                            source_format,
                            target_format,
                            is_stream,
                            &mut stream_params,
                        )
                        .await;
                    }
                }
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

/// 翻译上游响应回客户端协议（passthrough 情况直接返回原 Response）。
async fn translate_response(
    resp: Response,
    _translator: &dyn super::translator::Translatable,
    model: &str,
    orig_req: &serde_json::Value,
    _trans_req: &serde_json::Value,
    source: Format,
    target: Format,
    is_stream: bool,
    params: &mut super::translator::StreamParams,
) -> Result<Response, ForwardError> {
    // Phase 1.7+: 当前只支持 claude→gemini 翻译（target == Format::Gemini &&
    // source == Format::Anthropic）。其它 pair 直接 passthrough 原响应。
    if source == Format::Anthropic && target == Format::Gemini {
        return translate_chrome_to_gemini_response(resp, model, orig_req, _trans_req, is_stream, params)
            .await;
    }
    // 其它 pair：passthrough
    Ok(resp)
}

/// Claude → Gemini 响应翻译：拆响应、调用自由函数、重组成 axum Response。
async fn translate_chrome_to_gemini_response(
    resp: Response,
    model: &str,
    orig_req: &serde_json::Value,
    trans_req: &serde_json::Value,
    is_stream: bool,
    params: &mut super::translator::StreamParams,
) -> Result<Response, ForwardError> {
    let status = resp.status();
    let t = claude_gemini::ClaudeToGeminiTranslator;

    if is_stream {
        // 流式：包装 upstream bytes stream，每 chunk 调 translate_response_stream
        let upstream_stream = resp.into_body().into_data_stream();
        let model = model.to_string();
        let orig_req = orig_req.clone();
        let _trans_req = trans_req.clone();
        // 共享调用方传入的 params（tool_name_map 等已由请求翻译阶段写入）
        let mut shared_params = super::translator::StreamParams::default();
        std::mem::swap(params, &mut shared_params);
        let stream = futures::stream::unfold(
            (upstream_stream, shared_params, Vec::<u8>::new()),
            move |(mut upstream, mut params, mut pending)| {
                let model = model.clone();
                let orig_req = orig_req.clone();
                let _trans_req = _trans_req.clone();
                async move {
                    loop {
                        // 1. 先尝试从 buffer 中取一完整 SSE 行（`\n\n` 结束）
                        if let Some(idx) = find_event_separator(&pending) {
                            let line: Vec<u8> = pending.drain(..idx).collect();
                            pending.drain(..2); // skip "\n\n"
                            let pieces = claude_gemini::translate_response_stream(
                                &t, &model, &orig_req, &line, &mut params,
                            ).unwrap_or_default();
                            if !pieces.is_empty() {
                                let mut bytes_buf: Vec<u8> = Vec::with_capacity(
                                    pieces.iter().map(|p| p.len()).sum()
                                );
                                for p in pieces {
                                    bytes_buf.extend_from_slice(&p);
                                }
                                return Some((Ok(Bytes::from(bytes_buf)), (upstream, params, pending)));
                            }
                            continue;
                        }
                        // 2. 从 upstream 读下一个 chunk
                        match upstream.next().await {
                            Some(Ok(chunk)) => {
                                pending.extend_from_slice(&chunk);
                                continue;
                            }
                            Some(Err(e)) => {
                                return Some((
                                    Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
                                    (upstream, params, pending),
                                ));
                            }
                            None => {
                                // flush 残余
                                if !pending.is_empty() {
                                    let tail = std::mem::take(&mut pending);
                                    let pieces = claude_gemini::translate_response_stream(
                                        &t, &model, &orig_req, &tail, &mut params,
                                    ).unwrap_or_default();
                                    let mut bytes_buf: Vec<u8> = Vec::with_capacity(
                                        pieces.iter().map(|p| p.len()).sum()
                                    );
                                    for p in pieces {
                                        bytes_buf.extend_from_slice(&p);
                                    }
                                    return Some((Ok(Bytes::from(bytes_buf)), (upstream, params, pending)));
                                }
                                return None;
                            }
                        }
                    }
                }
            },
        );
        let mut builder = Response::builder().status(status);
        builder = builder.header("content-type", "text/event-stream");
        builder
            .body(Body::from_stream(stream))
            .map_err(|e| ForwardError::RequestError(format!("构建翻译后流式响应失败: {e}")))
    } else {
        // 非流式：读完整 body → 翻译 → 重组
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024)
            .await
            .map_err(|e| ForwardError::RequestError(format!("读取上游响应体失败: {e}")))?;
        let translated = claude_gemini::translate_response_non_stream(
            &t, model, orig_req, &bytes, params,
        )
        .map_err(|e| ForwardError::RequestError(format!("翻译响应失败: {e}")))?;
        let mut builder = Response::builder().status(status);
        builder = builder.header("content-type", "application/json");
        builder
            .body(Body::from(translated))
            .map_err(|e| ForwardError::RequestError(format!("构建翻译后响应失败: {e}")))
    }
}

/// 在字节缓冲中找首个 `\n\n` SSE 事件分隔符。返回分隔符的起始位置。
fn find_event_separator(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

fn is_stream(body: &serde_json::Value) -> bool {
    body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Build authentication headers based on target format.
fn build_auth_headers(provider: &RouteProvider, target: Format) -> Vec<(String, String)> {
    let mut headers = Vec::new();

    if provider.api_key.is_empty() {
        return headers;
    }

    match target {
        Format::Anthropic => {
            headers.push(("x-api-key".to_string(), provider.api_key.clone()));
            headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
        }
        Format::Gemini => {
            // Google Generative AI 用 `x-goog-api-key` header
            headers.push(("x-goog-api-key".to_string(), provider.api_key.clone()));
        }
        Format::OpenAiChat | Format::OpenAiResponses => {
            headers.push((
                "authorization".to_string(),
                format!("Bearer {}", provider.api_key),
            ));
        }
        _ => {
            headers.push((
                "authorization".to_string(),
                format!("Bearer {}", provider.api_key),
            ));
        }
    }

    headers
}

/// Build the upstream URL for the given provider, source, and target protocol.
///
/// 路径生成规则：
/// - passthrough（source == target）：
///   - Anthropic → `{base}/v1/messages`
///   - OpenAI Chat → `{base}/chat/completions`
///   - OpenAI Responses → `{base}/responses`
/// - 翻译（source != target）：
///   - target == Gemini → `{base}/models/{model}:generateContent`
///     或 `:streamGenerateContent?alt=sse`（流式）
fn build_upstream_url(
    provider: &RouteProvider,
    source: Format,
    target: Format,
    model: &str,
    is_stream: bool,
) -> String {
    let base = provider.base_url.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);

    if source == target {
        return match target {
            Format::Anthropic => format!("{}/v1/messages", base),
            Format::OpenAiChat => format!("{}/chat/completions", base),
            Format::OpenAiResponses => format!("{}/responses", base),
            _ => format!("{}/", base),
        };
    }

    match target {
        // 翻译场景：构造 Google Gemini URL
        Format::Gemini => {
            if is_stream {
                format!("{}/models/{}:streamGenerateContent?alt=sse", base, model)
            } else {
                format!("{}/models/{}:generateContent", base, model)
            }
        }
        _ => {
            // 其它翻译场景占位（Phase 2+ 接入）
            format!("{}/", base)
        }
    }
}

/// Google OpenAI 兼容端点 passthrough 探测（用户拍板简化决策 #6）。
///
/// 触发条件：
/// - provider 是 `google-generative-ai` 类型
/// - provider.base_url 含 `generativelanguage.googleapis.com`
/// - 客户端是 OpenAI Chat 协议
///
/// 返回 true 表示走 passthrough（请求体原样，URL 改为 `/v1beta/openai/v1/...`）。
/// Codex Responses 不在此处走 passthrough——Google OpenAI 兼容端点不支持
/// Responses API（需要翻译 Responses → Chat Completions 或 Responses → Gemini 原生）。
fn should_passthrough_google_openai_compat(
    provider: &RouteProvider,
    source_format: Format,
) -> bool {
    http::is_google_provider(&provider.provider_type)
        && http::is_google_openai_compat(&provider.base_url, "/")
        && source_format == Format::OpenAiChat
}

/// 构造 Google OpenAI 兼容端点 URL（passthrough 路径）。
fn build_google_openai_compat_url(
    provider: &RouteProvider,
    model: &str,
    is_stream: bool,
) -> String {
    let base = provider.base_url.trim_end_matches('/');
    // Google 端点 URL：{base}/v1beta/openai/v1/chat/completions
    // provider.base_url 通常已经是 .../v1beta，需要剥掉再追加 /v1beta/openai/v1/...
    let base = base.strip_suffix("/v1beta").unwrap_or(base);
    let path = if is_stream {
        // OpenAI 兼容端点也支持 stream（通过 body 的 stream=true 表达，URL 不变）
        "chat/completions"
    } else {
        "chat/completions"
    };
    format!("{}/v1beta/openai/v1/{}?model={}", base, path, model)
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
