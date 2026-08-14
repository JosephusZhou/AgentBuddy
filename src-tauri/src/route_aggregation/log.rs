//! In-memory request log for the route aggregation proxy.
//!
//! Logs every inbound request and the corresponding upstream response so the
//! frontend can show a list of recent traffic for debugging (e.g. diagnosing
//! 404s caused by missing route handlers). Entries are stored in a bounded
//! ring buffer (default capacity 2000) and discarded FIFO when full — the goal
//! is operational visibility, not audit, so we keep it in memory only and
//! don't persist across restarts.

use std::collections::VecDeque;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;

use super::logfile::LogFile;
use super::types::RouteGroup;

/// Default ring buffer capacity. Overridable via [`LogStore::with_capacity`].
pub const DEFAULT_LOG_CAP: usize = 2000;

/// Maximum body bytes captured per request/response side. Large enough to see
/// headers + the first message of a typical multi-turn OpenAI Responses request
/// (system prompts can stack past 8 KB). 64 KB still keeps each entry well
/// under a megabyte even with 2000 entries in the ring buffer.
pub const BODY_PREVIEW_MAX_BYTES: usize = 64 * 1024;

/// Inbound protocol of a logged request. Phase 5+：路由聚合只保留两种入站
/// 协议（Claude Messages + Codex Responses），与 RouteGroup 一一对应。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InboundProtocol {
    ClaudeMessages,
    CodexResponses,
    OpenAiModelsList,
}

impl InboundProtocol {
    #[allow(dead_code)]
    pub fn from_group(group: RouteGroup) -> Self {
        match group {
            RouteGroup::ClaudeCode => InboundProtocol::ClaudeMessages,
            RouteGroup::Codex => InboundProtocol::CodexResponses,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            InboundProtocol::ClaudeMessages => "Claude Messages",
            InboundProtocol::CodexResponses => "Codex Responses",
            InboundProtocol::OpenAiModelsList => "Models",
        }
    }
}

/// A single request log entry. Serialized to camelCase for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Monotonic ID assigned at insert time. Higher = newer.
    pub id: u64,
    /// Unix millis when the inbound request entered the handler.
    pub timestamp_ms: i64,
    pub protocol: InboundProtocol,
    pub inbound_method: String,
    pub inbound_path: String,
    /// Sanitized headers (Authorization, Cookie, x-api-key, x-goog-api-key
    /// values are masked to "***"). May be empty if the inbound request had
    /// no relevant fields.
    pub inbound_headers: Vec<[String; 2]>,
    /// Inbound request body, parsed as JSON. Large bodies are truncated to
    /// [`BODY_PREVIEW_MAX_BYTES`] and `inbound_body_truncated` is set.
    pub inbound_body: Option<serde_json::Value>,
    pub inbound_body_truncated: bool,
    /// Original `model` field extracted from the inbound body, if any.
    pub inbound_model: Option<String>,

    /// Provider that handled the request (after failover / circuit-breaker
    /// selection). None if forwarding failed before picking one.
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    /// Final upstream URL after path construction.
    pub upstream_url: Option<String>,
    pub upstream_status: Option<u16>,
    pub upstream_headers: Vec<[String; 2]>,
    pub upstream_body: Option<String>,
    pub upstream_body_truncated: bool,
    /// True when `stream=true` in the request. We don't read streaming bodies
    /// for logging (would break backpressure); `upstream_body` will be None
    /// and only headers / status are captured.
    pub stream: bool,

    /// Total wall time from handler entry to response ready.
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

/// Thread-safe bounded log store. Cloning the Arc gives a new handle to the
/// same underlying buffer.
#[derive(Clone)]
pub struct LogStore {
    inner: Arc<Inner>,
}

struct Inner {
    capacity: usize,
    entries: RwLock<VecDeque<LogEntry>>,
    next_id: RwLock<u64>,
    /// Serializes ID assignment, ring insertion, and file mirroring so a
    /// concurrent request cannot duplicate or reorder a persisted entry.
    append_lock: AsyncMutex<()>,
    /// Optional on-disk sink. When set, every entry appended to the
    /// in-memory ring is also serialized as a single JSON line and flushed
    /// to disk. Uses a synchronous `std::sync::Mutex` because the file write
    /// itself is synchronous inside `LogFile` and we don't want to lock
    /// across an `.await` for the hot path.
    file: std::sync::Mutex<Option<LogFile>>,
}

impl LogStore {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_LOG_CAP)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                capacity: capacity.max(1),
                entries: RwLock::new(VecDeque::with_capacity(capacity)),
                next_id: RwLock::new(1),
                append_lock: AsyncMutex::new(()),
                file: std::sync::Mutex::new(None),
            }),
        }
    }

    /// Attach an on-disk sink. Existing entries are not replayed; only new
    /// entries written after this call will be persisted. Best-effort: if
    /// the cell is already populated (e.g. setup runs twice), the second
    /// call is ignored.
    pub fn attach_file(&self, file: LogFile) {
        if let Ok(mut slot) = self.inner.file.lock() {
            if slot.is_none() {
                *slot = Some(file);
            }
        }
    }

    /// Path of the on-disk log file, if attached.
    pub fn file_path(&self) -> Option<std::path::PathBuf> {
        self.inner
            .file
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|f| f.path()))
    }

    /// Append a new entry, assigning its ID. FIFO-evicts when full. Also
    /// mirrors the entry to the on-disk file if one is attached.
    pub async fn push(&self, mut entry: LogEntry) {
        let _append_guard = self.inner.append_lock.lock().await;
        let mut id_guard = self.inner.next_id.write().await;
        entry.id = *id_guard;
        *id_guard = id_guard.wrapping_add(1);
        drop(id_guard);

        // Keep the exact entry assigned above. Reading the ring's back after
        // releasing its lock is racy: another request may already have made
        // a newer entry the back item.
        let disk_entry = entry.clone();

        let mut entries = self.inner.entries.write().await;
        while entries.len() >= self.inner.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
        drop(entries);

        // Move the synchronous file operation off the async runtime. The
        // append guard remains held so persisted lines retain ID order.
        let file = self.inner.file.lock().ok().and_then(|guard| guard.clone());
        if let Some(file) = file {
            let _ = tokio::task::spawn_blocking(move || file.write_entry(&disk_entry)).await;
        }
    }

    /// Snapshot all entries (newest last). Cloned for the frontend consumer.
    pub async fn snapshot(&self) -> Vec<LogEntry> {
        self.inner.entries.read().await.iter().cloned().collect()
    }

    pub async fn clear(&self) {
        let _append_guard = self.inner.append_lock.lock().await;
        self.inner.entries.write().await.clear();
        let file = self.inner.file.lock().ok().and_then(|guard| guard.clone());
        if let Some(file) = file {
            let _ = tokio::task::spawn_blocking(move || file.clear()).await;
        }
    }

    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        self.inner.entries.read().await.len()
    }

    #[allow(dead_code)]
    pub async fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Sanitize a header value for logging: mask Authorization / Cookie /
/// token-bearing headers. Everything else is returned unchanged.
pub fn sanitize_header_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization"
            | "cookie"
            | "x-api-key"
            | "x-goog-api-key"
            | "api-key"
            | "proxy-authorization"
    ) {
        return "***".to_string();
    }
    value.to_string()
}

/// Mask credential-like query parameters before an upstream URL is shown in
/// diagnostics. API keys normally travel in headers, but custom gateways may
/// still encode them in the URL.
pub fn sanitize_url_for_log(url: &str) -> String {
    let Some((base, query_and_fragment)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = query_and_fragment
        .split_once('#')
        .map_or((query_and_fragment, None), |(query, fragment)| {
            (query, Some(fragment))
        });
    let query = query
        .split('&')
        .map(|part| {
            let Some((key, value)) = part.split_once('=') else {
                return part.to_string();
            };
            let compact = key.to_ascii_lowercase().replace(['-', '_'], "");
            if compact == "key"
                || compact == "apikey"
                || compact == "token"
                || compact.ends_with("token")
                || compact.contains("password")
                || compact.contains("secret")
                || compact.contains("authorization")
                || compact.contains("credential")
            {
                format!("{key}=***")
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    let fragment = fragment.map_or(String::new(), |value| format!("#{value}"));
    format!("{base}?{query}{fragment}")
}

/// Redact common credential fields before a JSON body is shown in diagnostics.
/// Prompts and tool arguments remain visible, but values under credential-like
/// keys never enter the in-memory or on-disk log.
pub fn redact_json_for_log(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let compact = lower.replace(['-', '_'], "");
                    let redacted = compact == "cookie"
                        || compact == "authorization"
                        || compact == "proxyauthorization"
                        || compact == "apikey"
                        || compact == "xapikey"
                        || compact == "xgoogapikey"
                        || lower == "token"
                        || lower.ends_with("_token")
                        || lower.ends_with("-token")
                        || lower.contains("api_key")
                        || lower.contains("apikey")
                        || lower.contains("auth_token")
                        || lower.contains("access_token")
                        || lower.contains("refresh_token")
                        || lower.contains("password")
                        || lower.contains("secret")
                        || lower.contains("authorization")
                        || lower.contains("credential");
                    (
                        key.clone(),
                        if redacted {
                            serde_json::Value::String("***".to_string())
                        } else {
                            redact_json_for_log(value)
                        },
                    )
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact_json_for_log).collect())
        }
        _ => value.clone(),
    }
}

/// Redact credential fields in a JSON response preview when the preview is a
/// complete JSON value. Non-JSON responses are returned unchanged because they
/// cannot be safely transformed without changing their protocol payload.
pub fn redact_json_text_for_log(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .map(|value| redact_json_for_log(&value).to_string())
        .unwrap_or_else(|_| text.to_string())
}

/// Truncate a byte slice to `max` bytes, returning the prefix as `String`
/// (lossy UTF-8 conversion if truncation lands mid-codepoint) and a flag.
pub fn truncate_for_preview(bytes: &[u8], max: usize) -> (String, bool) {
    if bytes.len() <= max {
        (String::from_utf8_lossy(bytes).into_owned(), false)
    } else {
        (String::from_utf8_lossy(&bytes[..max]).into_owned(), true)
    }
}

/// Convenience: try to parse a body as JSON. If parsing fails (e.g. when the
/// body is truncated mid-object), fall back to storing the raw truncated string
/// as a JSON `Value::String` so the log still has something human-readable.
pub fn body_preview_json(raw: &[u8], max: usize) -> (Option<serde_json::Value>, bool) {
    let (preview, truncated) = truncate_for_preview(raw, max);
    match serde_json::from_str::<serde_json::Value>(&preview) {
        Ok(v) => (Some(v), truncated),
        Err(_) => (Some(serde_json::Value::String(preview)), truncated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_credential_headers() {
        assert_eq!(
            sanitize_header_value("Authorization", "Bearer secret"),
            "***"
        );
        assert_eq!(sanitize_header_value("x-api-key", "secret"), "***");
        assert_eq!(
            sanitize_header_value("content-type", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn masks_credential_query_parameters() {
        assert_eq!(
            sanitize_url_for_log(
                "https://example.test/v1?api_key=secret&model=demo&access-token=secret#frag"
            ),
            "https://example.test/v1?api_key=***&model=demo&access-token=***#frag"
        );
    }

    #[test]
    fn redacts_nested_credential_fields() {
        let value = serde_json::json!({
            "prompt": "keep this",
            "api-key": "secret",
            "headers": {"authorization": "Bearer secret", "cookie": "session"},
            "items": [{"access_token": "secret"}],
        });
        let redacted = redact_json_for_log(&value);

        assert_eq!(redacted["prompt"], "keep this");
        assert_eq!(redacted["api-key"], "***");
        assert_eq!(redacted["headers"]["authorization"], "***");
        assert_eq!(redacted["headers"]["cookie"], "***");
        assert_eq!(redacted["items"][0]["access_token"], "***");
    }

    #[tokio::test]
    async fn concurrent_push_assigns_ordered_ids_and_respects_capacity() {
        let store = LogStore::with_capacity(32);
        let mut tasks = Vec::new();
        for index in 0..64 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .push(LogEntry {
                        id: 0,
                        timestamp_ms: index,
                        protocol: InboundProtocol::ClaudeMessages,
                        inbound_method: "POST".into(),
                        inbound_path: "/v1/messages".into(),
                        inbound_headers: Vec::new(),
                        inbound_body: None,
                        inbound_body_truncated: false,
                        inbound_model: None,
                        provider_id: None,
                        provider_name: None,
                        upstream_url: None,
                        upstream_status: None,
                        upstream_headers: Vec::new(),
                        upstream_body: None,
                        upstream_body_truncated: false,
                        stream: false,
                        duration_ms: 0,
                        success: true,
                        error: None,
                    })
                    .await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let ids: Vec<u64> = store
            .snapshot()
            .await
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids, (33..=64).collect::<Vec<_>>());
    }
}
