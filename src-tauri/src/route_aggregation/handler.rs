//! Request handlers — entry points for Axum routes.
//!
//! The aggregated endpoint always serves both API formats (Claude messages
//! and Codex responses) while the server is running. Each handler
//! authenticates the request against the configured API key list, then
//! delegates to the forwarder and returns the upstream response
//! (with SSE passthrough for streaming).
//!
//! Every handler also writes a `LogEntry` to the shared `LogStore` so the
//! UI's "进出日志" list can show recent traffic.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::time::Instant;

use super::forwarder;
use super::log::{
    body_preview_json, redact_json_for_log, redact_json_text_for_log, sanitize_header_value,
    sanitize_url_for_log, truncate_for_preview, InboundProtocol, LogEntry, BODY_PREVIEW_MAX_BYTES,
};
use super::router::AppState;
use super::RouteGroup;

/// Extract the Bearer token from the Authorization header.
/// Returns None if the header is missing or not a Bearer token.
fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Authenticate a request against the configured API key list.
/// Returns Ok(()) if the token matches any key, or if no key is configured
/// (no-auth mode). Returns Err(Response) with 401 otherwise.
fn authenticate(headers: &HeaderMap, api_keys: &[String]) -> Result<(), Response> {
    if api_keys.is_empty() {
        // No API key configured — open access
        return Ok(());
    }
    match extract_bearer_token(headers) {
        Some(token) if api_keys.iter().any(|k| *k == token) => Ok(()),
        _ => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "无效或缺失的 API Key",
        )),
    }
}

/// Handler for Claude Code messages endpoint: POST /v1/messages
pub async fn handle_claude_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    handle_with_log(
        state,
        RouteGroup::ClaudeCode,
        InboundProtocol::ClaudeMessages,
        headers,
        body,
        false,
    )
    .await
}

/// Handler for Claude Code token counting: POST /v1/messages/count_tokens.
pub async fn handle_claude_count_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    handle_with_log(
        state,
        RouteGroup::ClaudeCode,
        InboundProtocol::ClaudeMessages,
        headers,
        body,
        true,
    )
    .await
}

/// Handler for Codex Responses API: POST /v1/responses
/// Codex CLI uses the Responses API format, not Chat Completions.
pub async fn handle_codex_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    handle_with_log(
        state,
        RouteGroup::Codex,
        InboundProtocol::CodexResponses,
        headers,
        body,
        false,
    )
    .await
}

/// Common handler body shared by all three inbound endpoints.
///
/// Responsibilities:
/// 1. Authenticate against the configured API key list.
/// 2. Run the forwarder (which handles cloaking, failover, and passthrough).
/// 3. Write a LogEntry to the shared LogStore regardless of success/failure
///    so the UI can see the full inbound → outbound lifecycle.
async fn handle_with_log(
    state: AppState,
    group: RouteGroup,
    protocol: InboundProtocol,
    headers: HeaderMap,
    body: serde_json::Value,
    count_tokens: bool,
) -> Response {
    let started = Instant::now();
    let config = state.config.read().await.clone();

    // Re-derive the inbound path from headers (axum's matched_path is in the
    // router; we keep a stable string here instead of plumbing it through).
    let inbound_path = match protocol {
        InboundProtocol::ClaudeMessages if count_tokens => "/v1/messages/count_tokens",
        InboundProtocol::ClaudeMessages => "/v1/messages",
        InboundProtocol::CodexResponses => "/v1/responses",
        InboundProtocol::OpenAiModelsList => "/v1/models",
    }
    .to_string();

    // Capture inbound headers (sanitized).
    let inbound_headers: Vec<[String; 2]> = headers
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str().to_string();
            let value = v.to_str().ok()?.to_string();
            Some([name, sanitize_header_value(k.as_str(), &value)])
        })
        .collect();

    // Inbound body preview (capped).
    let raw_body_str = serde_json::to_string(&redact_json_for_log(&body)).unwrap_or_default();
    let (inbound_body, inbound_body_truncated) =
        body_preview_json(raw_body_str.as_bytes(), BODY_PREVIEW_MAX_BYTES);
    let inbound_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if let Err(resp) = authenticate(&headers, &config.api_keys) {
        // Log auth failures too — they're useful for diagnosing "client can't
        // even reach the proxy" issues.
        let entry = LogEntry {
            id: 0,
            timestamp_ms: chrono_millis(),
            protocol,
            inbound_method: "POST".to_string(),
            inbound_path: inbound_path.clone(),
            inbound_headers: inbound_headers.clone(),
            inbound_body: inbound_body.clone(),
            inbound_body_truncated,
            inbound_model: inbound_model.clone(),
            provider_id: None,
            provider_name: None,
            upstream_url: None,
            upstream_status: Some(resp.status().as_u16()),
            upstream_headers: Vec::new(),
            upstream_body: None,
            upstream_body_truncated: false,
            stream: is_stream,
            duration_ms: started.elapsed().as_millis() as u64,
            success: false,
            error: Some("未通过 API Key 鉴权".to_string()),
        };
        state.log_store.push(entry).await;
        return resp;
    }

    let forward_result = forwarder::forward(
        group,
        body,
        &headers,
        &config,
        &state.provider_router,
        count_tokens,
    )
    .await;

    match forward_result {
        Ok(result) => {
            let upstream_status = Some(result.response.status().as_u16());
            let upstream_headers = collect_response_headers(&result.response);
            let upstream_body_str = if is_stream {
                None
            } else if result.body_preview.is_empty() {
                Some(String::new())
            } else {
                Some(redact_json_text_for_log(&String::from_utf8_lossy(
                    &result.body_preview,
                )))
            };
            let entry = LogEntry {
                id: 0,
                timestamp_ms: chrono_millis(),
                protocol,
                inbound_method: "POST".to_string(),
                inbound_path,
                inbound_headers,
                inbound_body,
                inbound_body_truncated,
                inbound_model,
                provider_id: Some(result.provider_id),
                provider_name: Some(result.provider_name),
                upstream_url: Some(sanitize_url_for_log(&result.upstream_url)),
                upstream_status,
                upstream_headers,
                upstream_body: upstream_body_str,
                upstream_body_truncated: result.body_truncated,
                stream: is_stream,
                duration_ms: started.elapsed().as_millis() as u64,
                success: result.response.status().is_success(),
                error: None,
            };
            state.log_store.push(entry).await;
            result.response
        }
        Err(err) => {
            let entry = LogEntry {
                id: 0,
                timestamp_ms: chrono_millis(),
                protocol,
                inbound_method: "POST".to_string(),
                inbound_path,
                inbound_headers,
                inbound_body,
                inbound_body_truncated,
                inbound_model,
                provider_id: None,
                provider_name: None,
                upstream_url: None,
                upstream_status: None,
                upstream_headers: Vec::new(),
                upstream_body: None,
                upstream_body_truncated: false,
                stream: is_stream,
                duration_ms: started.elapsed().as_millis() as u64,
                success: false,
                error: Some(error_label(&err)),
            };
            state.log_store.push(entry).await;
            forward_error_to_response(err)
        }
    }
}

fn error_label(err: &forwarder::ForwardError) -> String {
    match err {
        forwarder::ForwardError::NoAvailableProvider => "没有可用的供应商".to_string(),
        forwarder::ForwardError::AllProvidersFailed => "所有供应商均请求失败".to_string(),
        forwarder::ForwardError::CloakingError(msg) => format!("伪装处理失败: {msg}"),
        forwarder::ForwardError::RequestError(msg) => msg.clone(),
    }
}

fn forward_error_to_response(err: forwarder::ForwardError) -> Response {
    match err {
        forwarder::ForwardError::NoAvailableProvider => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, "没有可用的供应商")
        }
        forwarder::ForwardError::AllProvidersFailed => {
            error_response(StatusCode::BAD_GATEWAY, "所有供应商均请求失败")
        }
        forwarder::ForwardError::CloakingError(msg) => {
            error_response(StatusCode::BAD_REQUEST, &msg)
        }
        forwarder::ForwardError::RequestError(msg) => error_response(StatusCode::BAD_GATEWAY, &msg),
    }
}

fn collect_response_headers(resp: &Response) -> Vec<[String; 2]> {
    resp.headers()
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str().to_string();
            let value = v.to_str().ok()?.to_string();
            Some([name.clone(), sanitize_header_value(&name, &value)])
        })
        .collect()
}

fn chrono_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Handler for listing models: GET /v1/models
///
/// Authenticates against the API key list and returns the union of all
/// checked providers' custom model lists (`ai_providers.custom_models_json`)。
/// **不**向任何 provider 的远端 /v1/models 发起请求——配置侧的自定义模型列表
/// 即为对外暴露的全部模型。
///
/// The request is logged through the shared `LogStore` so the UI's
/// 进出日志 list shows model-list calls alongside the POST endpoints —
/// `InboundProtocol::OpenAiModelsList` distinguishes them.
pub async fn handle_list_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let started = Instant::now();
    let config = state.config.read().await.clone();

    // Sanitize inbound headers (mirrors handle_with_log so the UI's
    // debug view shows the same Authorization / x-api-key masking).
    let inbound_headers: Vec<[String; 2]> = headers
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str().to_string();
            let value = v.to_str().ok()?.to_string();
            Some([name, sanitize_header_value(k.as_str(), &value)])
        })
        .collect();
    let inbound_path = "/v1/models".to_string();

    if let Err(resp) = authenticate(&headers, &config.api_keys) {
        let entry = LogEntry {
            id: 0,
            timestamp_ms: chrono_millis(),
            protocol: InboundProtocol::OpenAiModelsList,
            inbound_method: "GET".to_string(),
            inbound_path,
            inbound_headers,
            inbound_body: None,
            inbound_body_truncated: false,
            inbound_model: None,
            provider_id: None,
            provider_name: None,
            upstream_url: None,
            upstream_status: Some(resp.status().as_u16()),
            upstream_headers: Vec::new(),
            upstream_body: None,
            upstream_body_truncated: false,
            stream: false,
            duration_ms: started.elapsed().as_millis() as u64,
            success: false,
            error: Some("未通过 API Key 鉴权".to_string()),
        };
        state.log_store.push(entry).await;
        return resp;
    }

    // 取所有启用 provider 的 customModels 并集：纯内存读取，无网络 I/O。
    let ids = state.provider_router.get_enabled_model_ids().await;

    // Build the response
    let data: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "object": "model",
                "owned_by": "route-aggregation",
            })
        })
        .collect();

    let body = serde_json::json!({
        "object": "list",
        "data": data,
    });
    let body_str = body.to_string();
    let (upstream_body, upstream_body_truncated) =
        truncate_for_preview(body_str.as_bytes(), BODY_PREVIEW_MAX_BYTES);

    let entry = LogEntry {
        id: 0,
        timestamp_ms: chrono_millis(),
        protocol: InboundProtocol::OpenAiModelsList,
        inbound_method: "GET".to_string(),
        inbound_path,
        inbound_headers,
        inbound_body: None,
        inbound_body_truncated: false,
        inbound_model: None,
        provider_id: None,
        provider_name: None,
        upstream_url: None,
        upstream_status: Some(StatusCode::OK.as_u16()),
        upstream_headers: Vec::new(),
        upstream_body: Some(upstream_body),
        upstream_body_truncated,
        stream: false,
        duration_ms: started.elapsed().as_millis() as u64,
        success: true,
        error: None,
    };
    state.log_store.push(entry).await;

    (StatusCode::OK, Json(body)).into_response()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "type": "route_aggregation_error",
            "message": message,
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
