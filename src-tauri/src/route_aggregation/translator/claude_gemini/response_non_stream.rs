//! Translator: Gemini non-stream → Anthropic Messages JSON
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/gemini/claude/gemini_claude_response.go`
//! 的 `ConvertGeminiResponseToClaudeNonStream`。
//!
//! Gemini 非流式响应：
//! ```json
//! {
//!   "candidates": [{
//!     "content": {"role": "model", "parts": [{"text": "..."}, {"functionCall": {...}}]},
//!     "finishReason": "STOP"
//!   }],
//!   "usageMetadata": {"promptTokenCount": N, "candidatesTokenCount": M}
//! }
//! ```
//!
//! → Anthropic Messages JSON：
//! ```json
//! {
//!   "id": "msg_xxx", "type": "message", "role": "assistant", "model": "...",
//!   "content": [{"type": "text", "text": "..."}, {"type": "tool_use", ...}],
//!   "stop_reason": "end_turn" | "max_tokens" | "tool_use",
//!   "usage": {"input_tokens": N, "output_tokens": M}
//! }
//! ```

use serde_json::{Map, Value};

use super::super::common::multimodal;
use super::super::params::StreamParams;
use super::ClaudeToGeminiTranslator;

/// 非流式翻译：完整 Gemini JSON → 完整 Anthropic Messages JSON 字节。
pub fn translate_response_non_stream(
    _translator: &ClaudeToGeminiTranslator,
    model: &str,
    _original_request: &Value,
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
    let finish_reason = candidate
        .get("finishReason")
        .and_then(|v| v.as_str())
        .unwrap_or("STOP");

    // 1. 构造 content blocks
    let mut content: Vec<Value> = Vec::new();
    let mut saw_tool_use = false;
    for part in &parts {
        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            content.push(Value::Object(Map::from_iter([
                ("type".into(), Value::String("text".into())),
                ("text".into(), Value::String(text.to_string())),
            ])));
        } else if let Some(thought) = part.get("thought").and_then(|v| v.as_str()) {
            content.push(Value::Object(Map::from_iter([
                ("type".into(), Value::String("thinking".into())),
                ("thinking".into(), Value::String(thought.to_string())),
            ])));
        } else if let Some(fc) = part.get("functionCall") {
            saw_tool_use = true;
            let sanitized = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let original = params
                .sanitized_name_map
                .get(sanitized)
                .cloned()
                .unwrap_or_else(|| sanitized.to_string());
            let id = fc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let id = if id.is_empty() {
                format!("toolu_{:x}", xxhash_rust::xxh64::xxh64(sanitized.as_bytes(), 0))
            } else {
                id.to_string()
            };
            let input = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
            content.push(Value::Object(Map::from_iter([
                ("type".into(), Value::String("tool_use".into())),
                ("id".into(), Value::String(id)),
                ("name".into(), Value::String(original)),
                ("input".into(), input),
            ])));
        } else if let Some(inline_data) = part.get("inline_data").and_then(|v| v.as_object()) {
            // Gemini 输出图片 → Claude image block
            let mime = inline_data.get("mime_type").and_then(|v| v.as_str()).unwrap_or("");
            let data = inline_data.get("data").and_then(|v| v.as_str()).unwrap_or("");
            if !mime.is_empty() && !data.is_empty() {
                let d = multimodal::InlineData::from_base64(mime, data);
                content.push(d.to_anthropic_image());
            }
        }
    }

    // 2. stop_reason
    let stop_reason = if saw_tool_use {
        "tool_use"
    } else {
        match finish_reason {
            "STOP" => "end_turn",
            "MAX_TOKENS" => "max_tokens",
            "TOOL_CALL" | "TOOL_USE" => "tool_use",
            _ => "end_turn",
        }
    };

    // 3. usage
    let usage = response.get("usageMetadata").cloned().unwrap_or(Value::Null);
    let input_tokens = usage
        .get("promptTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("candidatesTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    // 4. id
    let msg_id = format!("msg_{:x}", xxhash_rust::xxh64::xxh64(model.as_bytes(), 0));

    let body = Value::Object(Map::from_iter([
        ("id".into(), Value::String(msg_id)),
        ("type".into(), Value::String("message".into())),
        ("role".into(), Value::String("assistant".into())),
        ("model".into(), Value::String(model.to_string())),
        ("content".into(), Value::Array(content)),
        ("stop_reason".into(), Value::String(stop_reason.to_string())),
        (
            "stop_sequence".into(),
            Value::Null,
        ),
        (
            "usage".into(),
            Value::Object(Map::from_iter([
                ("input_tokens".into(), Value::from(input_tokens)),
                ("output_tokens".into(), Value::from(output_tokens)),
            ])),
        ),
    ]));

    serde_json::to_vec(&body).map_err(|e| format!("序列化 Anthropic 响应失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_response() {
        let t = ClaudeToGeminiTranslator;
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hello, world!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 3}
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream(&t, "gemini-2.5-pro", &Value::Null, raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["model"], "gemini-2.5-pro");
        assert_eq!(v["stop_reason"], "end_turn");
        assert_eq!(v["content"][0]["text"], "Hello, world!");
        assert_eq!(v["usage"]["input_tokens"], 5);
        assert_eq!(v["usage"]["output_tokens"], 3);
    }

    #[test]
    fn tool_use_response() {
        let t = ClaudeToGeminiTranslator;
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
        let out = translate_response_non_stream(&t, "m", &Value::Null, raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["stop_reason"], "tool_use");
        assert_eq!(v["content"][0]["type"], "tool_use");
        assert_eq!(v["content"][0]["name"], "search.web");
    }

    #[test]
    fn max_tokens_finish() {
        let t = ClaudeToGeminiTranslator;
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "truncated..."}]},
                "finishReason": "MAX_TOKENS"
            }]
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream(&t, "m", &Value::Null, raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["stop_reason"], "max_tokens");
    }

    #[test]
    fn thinking_part_becomes_thinking_block() {
        let t = ClaudeToGeminiTranslator;
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
        let out = translate_response_non_stream(&t, "m", &Value::Null, raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["content"][0]["type"], "thinking");
        assert_eq!(v["content"][1]["type"], "text");
    }

    #[test]
    fn inline_data_becomes_image_block() {
        let t = ClaudeToGeminiTranslator;
        let raw = br#"{
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "see this image:"},
                    {"inline_data": {"mime_type": "image/png", "data": "iVBOR"}}
                ]},
                "finishReason": "STOP"
            }]
        }"#;
        let mut p = StreamParams::default();
        let out = translate_response_non_stream(&t, "m", &Value::Null, raw, &mut p).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][1]["type"], "image");
        assert_eq!(v["content"][1]["source"]["type"], "base64");
        assert_eq!(v["content"][1]["source"]["media_type"], "image/png");
        assert_eq!(v["content"][1]["source"]["data"], "iVBOR");
    }
}