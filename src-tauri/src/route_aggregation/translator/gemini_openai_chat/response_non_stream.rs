//! Translator: Gemini non-stream → OpenAI Chat Completions JSON
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! Gemini JSON → OpenAI Chat JSON 一次性翻译。

use serde_json::{Map, Value};

use super::super::params::StreamParams;

/// 非流式翻译：完整 Gemini JSON → 完整 OpenAI Chat JSON 字节。
pub fn translate_response_non_stream(
    model: &str,
    raw: &[u8],
    params: &mut StreamParams,
) -> Result<Vec<u8>, String> {
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

    // 1. 构造 message.content + tool_calls
    let mut content_text = String::new();
    let mut reasoning_content: Option<String> = None;
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut saw_tool_call = false;

    for part in parts {
        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            if !content_text.is_empty() {
                content_text.push('\n');
            }
            content_text.push_str(text);
        } else if let Some(thought) = part.get("thought").and_then(|v| v.as_str()) {
            reasoning_content = Some(thought.to_string());
        } else if let Some(fc) = part.get("functionCall") {
            saw_tool_call = true;
            let sanitized = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let original = params
                .sanitized_name_map
                .get(sanitized)
                .cloned()
                .unwrap_or_else(|| sanitized.to_string());
            let id = fc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let id = if id.is_empty() {
                format!("call_{:x}", xxhash_rust::xxh64::xxh64(sanitized.as_bytes(), 0))
            } else {
                id.to_string()
            };
            let arguments = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
            let arguments_str =
                serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".into());
            tool_calls.push(Value::Object(Map::from_iter([
                ("id".to_string(), Value::String(id)),
                ("type".to_string(), Value::String("function".into())),
                (
                    "function".to_string(),
                    Value::Object(Map::from_iter([
                        ("name".to_string(), Value::String(original)),
                        ("arguments".to_string(), Value::String(arguments_str)),
                    ])),
                ),
            ])));
        }
    }

    // 2. message
    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".into()));
    if content_text.is_empty() && !tool_calls.is_empty() {
        message.insert("content".to_string(), Value::Null);
    } else {
        message.insert("content".to_string(), Value::String(content_text));
    }
    if let Some(rc) = reasoning_content {
        message.insert("reasoning_content".to_string(), Value::String(rc));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    // 3. finish_reason
    let finish_reason = if saw_tool_call {
        "tool_calls"
    } else {
        match finish_reason_raw {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            "TOOL_CALL" => "tool_calls",
            "SAFETY" => "content_filter",
            _ => "stop",
        }
    };

    // 4. usage
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
        ("prompt_tokens".to_string(), Value::from(prompt_tokens)),
        ("completion_tokens".to_string(), Value::from(completion_tokens)),
        ("total_tokens".to_string(), Value::from(prompt_tokens + completion_tokens)),
    ]));

    // 5. id + created
    let chat_id = format!(
        "chatcmpl-{:x}",
        xxhash_rust::xxh64::xxh64(model.as_bytes(), 0)
    );
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let body = Value::Object(Map::from_iter([
        ("id".to_string(), Value::String(chat_id)),
        ("object".to_string(), Value::String("chat.completion".into())),
        ("created".to_string(), Value::from(created)),
        ("model".to_string(), Value::String(model.to_string())),
        (
            "choices".to_string(),
            Value::Array(vec![Value::Object(Map::from_iter([
                ("index".to_string(), Value::from(0)),
                ("message".to_string(), Value::Object(message)),
                ("finish_reason".to_string(), Value::String(finish_reason.to_string())),
            ]))]),
        ),
        ("usage".to_string(), usage_obj),
    ]));

    serde_json::to_vec(&body).map_err(|e| format!("序列化 OpenAI 响应失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_response() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hello, world!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 3}
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["role"], "assistant");
        assert_eq!(v["choices"][0]["message"]["content"], "Hello, world!");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["prompt_tokens"], 5);
        assert_eq!(v["usage"]["completion_tokens"], 3);
        assert_eq!(v["usage"]["total_tokens"], 8);
    }

    #[test]
    fn tool_call_response() {
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
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
        let tc = &v["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "search.web");
        assert_eq!(tc["function"]["arguments"], "{\"q\":\"rust\"}");
    }

    #[test]
    fn max_tokens_finish() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "truncated"}]},
                "finishReason": "MAX_TOKENS"
            }]
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "length");
    }

    #[test]
    fn safety_finish() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": []},
                "finishReason": "SAFETY"
            }]
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "content_filter");
    }

    #[test]
    fn thinking_part_becomes_reasoning_content() {
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"thought": "reasoning..."},
                    {"text": "answer"}
                ]},
                "finishReason": "STOP"
            }]
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream("m", raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["choices"][0]["message"]["reasoning_content"], "reasoning...");
        assert_eq!(v["choices"][0]["message"]["content"], "answer");
    }
}