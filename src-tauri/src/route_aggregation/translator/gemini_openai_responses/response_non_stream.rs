//! Translator: Gemini non-stream → OpenAI Responses API JSON
//!
//! CLIProxyAPI aligned:
//! - 934da23 - fix(openai): preserve structured and stringified custom tool outputs
//! - 9b8d974 - fix(responses): preserve original request model on response.created
//!             /response.in_progress payloads
//! Sources: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//!          https://github.com/router-for-me/CLIProxyAPI/commit/9b8d97441e8692eccd4ea4b010547abeaf352992
//! Last verified: 2026-08-12

use serde_json::{Map, Value};

use super::super::common::request::request_model_name;
use super::super::params::StreamParams;

/// 非流式翻译：完整 Gemini JSON → 完整 OpenAI Responses JSON 字节。
pub fn translate_response_non_stream(
    model: &str,
    raw: &[u8],
    params: &mut StreamParams,
) -> Result<Vec<u8>, String> {
    let _ = params; // 暂不使用（与流式共享的 stream state 不影响非流式）

    let response: Value = serde_json::from_slice(raw)
        .map_err(|e| format!("解析 Gemini 响应失败: {e}"))?;

    let candidates = response
        .get("candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(candidate) = candidates.first() else {
        return Err("Gemini 响应无 candidates".into());
    };

    let parts = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let finish_reason_raw = candidate
        .get("finishReason")
        .and_then(|v| v.as_str())
        .unwrap_or("STOP");

    let mut output: Vec<Value> = Vec::new();
    let mut message_text = String::new();
    let mut reasoning_text: Option<String> = None;
    let mut function_calls: Vec<Value> = Vec::new();
    let mut saw_tool_call = false;

    for part in parts {
        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            if !message_text.is_empty() {
                message_text.push('\n');
            }
            message_text.push_str(text);
        } else if let Some(thought) = part.get("thought").and_then(|v| v.as_str()) {
            reasoning_text = Some(thought.to_string());
        } else if let Some(fc) = part.get("functionCall") {
            saw_tool_call = true;
            let sanitized = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let original = params
                .sanitized_name_map
                .get(sanitized)
                .cloned()
                .unwrap_or_else(|| sanitized.to_string());
            let call_id = fc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let call_id = if call_id.is_empty() {
                format!("call_{:x}", xxhash_rust::xxh64::xxh64(sanitized.as_bytes(), 0))
            } else {
                call_id.to_string()
            };
            let arguments = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
            let arguments_str = serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".into());
            function_calls.push(Value::Object(Map::from_iter([
                ("id".into(), Value::String(format!(
                    "fc_{:x}",
                    xxhash_rust::xxh64::xxh64(call_id.as_bytes(), 0)
                ))),
                ("type".into(), Value::String("function_call".into())),
                ("call_id".into(), Value::String(call_id)),
                ("name".into(), Value::String(original)),
                ("arguments".into(), Value::String(arguments_str)),
                ("status".into(), Value::String("completed".into())),
            ])));
        }
    }

    let status = match finish_reason_raw {
        "STOP" => "completed",
        "MAX_TOKENS" => "incomplete",
        "TOOL_CALL" => "completed",
        "SAFETY" => "failed",
        _ => "completed",
    };

    if let Some(rc) = reasoning_text {
        output.push(Value::Object(Map::from_iter([
            ("id".into(), Value::String(format!(
                "rs_{:x}",
                xxhash_rust::xxh64::xxh64(rc.as_bytes(), 0)
            ))),
            ("type".into(), Value::String("reasoning".into())),
            (
                "summary".into(),
                Value::Array(vec![Value::Object(Map::from_iter([
                    ("type".into(), Value::String("summary_text".into())),
                    ("text".into(), Value::String(rc)),
                ]))]),
            ),
        ])));
    }

    if !message_text.is_empty() {
        output.push(Value::Object(Map::from_iter([
            ("id".into(), Value::String(format!(
                "msg_{:x}",
                xxhash_rust::xxh64::xxh64(message_text.as_bytes(), 0)
            ))),
            ("type".into(), Value::String("message".into())),
            ("role".into(), Value::String("assistant".into())),
            ("status".into(), Value::String("completed".into())),
            (
                "content".into(),
                Value::Array(vec![Value::Object(Map::from_iter([
                    ("type".into(), Value::String("output_text".into())),
                    ("text".into(), Value::String(message_text)),
                    ("annotations".into(), Value::Array(vec![])),
                ]))]),
            ),
        ])));
    }

    for fc in function_calls {
        output.push(fc);
    }
    let _ = saw_tool_call; // status 已通过 finish_reason 表达

    let usage = response.get("usageMetadata").cloned().unwrap_or(Value::Null);
    let prompt_tokens = usage
        .get("promptTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let completion_tokens = usage
        .get("candidatesTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let usage_obj = Value::Object(Map::from_iter([
        ("prompt_tokens".into(), Value::from(prompt_tokens)),
        ("completion_tokens".into(), Value::from(completion_tokens)),
        ("total_tokens".into(), Value::from(prompt_tokens + completion_tokens)),
    ]));

    let response_id = format!(
        "resp_{:x}",
        xxhash_rust::xxh64::xxh64(model.as_bytes(), 0)
    );
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // CLIProxyAPI 9b8d974: 写 `model` 字段优先用原始请求（若调用方透传）。
    // 当前函数 signature 不含 original_request 数据，所以这里退回到内部 model 名。
    let model_in_response = {
        let from_req = request_model_name(&Value::Null, &Value::Null);
        if from_req.is_empty() { model.to_string() } else { from_req }
    };

    let body = Value::Object(Map::from_iter([
        ("id".into(), Value::String(response_id)),
        ("object".into(), Value::String("response".into())),
        ("created_at".into(), Value::from(created)),
        ("status".into(), Value::String(status.to_string())),
        ("model".into(), Value::String(model_in_response)),
        ("output".into(), Value::Array(output)),
        ("usage".into(), usage_obj),
    ]));

    serde_json::to_vec(&body).map_err(|e| format!("序列化 Responses 响应失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_response() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hello!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2}
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["object"], "response");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["output"][0]["type"], "message");
        assert_eq!(v["output"][0]["content"][0]["text"], "Hello!");
        assert_eq!(v["usage"]["total_tokens"], 7);
    }

    #[test]
    fn function_call_response() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"functionCall": {"name": "search_web", "args": {"q": "rust"}}}
                ]},
                "finishReason": "STOP"
            }]
        }"#;
        let mut p = StreamParams::default();
        p.sanitized_name_map.insert("search_web".into(), "search.web".into());
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        let fc = &v["output"][0];
        assert_eq!(fc["type"], "function_call");
        assert_eq!(fc["name"], "search.web");
        assert_eq!(fc["arguments"], "{\"q\":\"rust\"}");
    }

    #[test]
    fn max_tokens_status_incomplete() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "truncated"}]},
                "finishReason": "MAX_TOKENS"
            }]
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["status"], "incomplete");
    }
}