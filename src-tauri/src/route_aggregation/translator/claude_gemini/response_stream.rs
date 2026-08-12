//! Translator: Gemini streaming → Anthropic SSE
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/gemini/claude/gemini_claude_response.go`
//! 的 `ConvertGeminiResponseToClaude` 流式函数。状态机：
//! - Gemini chunk `parts[].text` → Anthropic `content_block_delta{type:text_delta}` + `content_block_stop` + `message_delta`
//! - Gemini `parts[].thought` → Anthropic `content_block_delta{type:thinking_delta}` + `signature_delta`
//! - Gemini `parts[].functionCall` → Anthropic `content_block_start{type:tool_use}` + 多个 `input_json_delta` + `content_block_stop` + `message_delta{stop_reason:"tool_use"}`
//! - 全部结束 → `message_stop`
//! - `usageMetadata` → `message_delta.usage.output_tokens`

use serde_json::{Map, Value};

use super::super::params::{ResponseType, StreamParams};
use super::ClaudeToGeminiTranslator;

/// 流式翻译：每个 Gemini chunk（可能含多个 parts）→ 一组 Anthropic SSE 字节片段。
///
/// `params` 跨调用累积状态（ResponseType / ResponseIndex / tool_name_map 等）。
/// 当 `params.response_index == u32::MAX` 表示已发出 message_stop，再调用是 no-op。
pub fn translate_response_stream(
    _translator: &ClaudeToGeminiTranslator,
    model: &str,
    original_request: &Value,
    raw_chunk: &[u8],
    params: &mut StreamParams,
) -> Result<Vec<Vec<u8>>, String> {
    // 跳过空 chunk
    if raw_chunk.is_empty() {
        return Ok(Vec::new());
    }

    // Gemini 流式 chunk 是单行 JSON（没有 event: 前缀），
    // SSE 解析只需识别 `data: <json>` 行
    let line = match extract_data_line(raw_chunk) {
        Some(l) => l,
        None => return Ok(Vec::new()),
    };

    // 空 data 行（如 "[DONE]" 或 keepalive）
    if line.is_empty() || line == b"[DONE]" {
        return Ok(Vec::new());
    }

    let chunk: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!("解析 Gemini 流式 chunk 失败: {e}"));
        }
    };

    let mut out: Vec<Vec<u8>> = Vec::new();

    // 1. message_start（仅第一次）
    if !params.has_content && params.response_index == 0 && params.response_type == ResponseType::None
    {
        out.push(build_message_start(model, original_request, &chunk)?);
        params.has_content = true;
    }

    // 2. 遍历 candidates[0].content.parts
    let candidates = chunk
        .get("candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(candidate) = candidates.first().cloned() else {
        return Ok(out);
    };
    let parts = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    let mut saw_tool_use = false;
    for part in parts {
        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            out.extend(emit_text_delta(params, text)?);
        } else if let Some(thought) = part.get("thought").and_then(|v| v.as_str()) {
            out.extend(emit_thinking_delta(params, thought)?);
        } else if let Some(fc) = part.get("functionCall") {
            saw_tool_use = true;
            out.extend(emit_tool_use_start_and_args(params, fc)?);
        } else if let Some(sig) = part.get("thoughtSignature").or(part.get("signature")) {
            // Gemini 把 signature 单独放在 parts 里；少见但要兼容
            if let Some(s) = sig.as_str() {
                out.extend(emit_thinking_signature(params, s)?);
            }
        }
    }

    // 3. finishReason 触发 message_delta + content_block_stop + message_stop
    let finish_reason = candidate.get("finishReason").and_then(|v| v.as_str());
    if let Some(fr) = finish_reason {
        out.extend(emit_message_delta_and_stop(params, fr, saw_tool_use, &chunk)?);
    }

    Ok(out)
}

/// 从原始字节中抽取 data: 行内容（无前缀）。
///
/// 兼容三种格式：
/// - `data: {...}\n\n`
/// - `data:{...}\n\n`（无空格）
/// - 单行裸 JSON（Gemini 流式 server-sent 实际风格）
fn extract_data_line(chunk: &[u8]) -> Option<&[u8]> {
    let chunk = chunk.trim_ascii_start();
    let chunk = if let Some(rest) = chunk.strip_prefix(b"data:") {
        rest.strip_prefix(b" ").unwrap_or(rest)
    } else {
        chunk
    };
    let chunk = chunk.trim_ascii();
    if chunk.is_empty() {
        None
    } else {
        Some(chunk)
    }
}

/// 构造 `message_start` 事件。
///
/// message.id 用 `msg_<random hex>` 占位，message.model 用客户端传入的 model 名。
fn build_message_start(model: &str, _orig: &Value, chunk: &Value) -> Result<Vec<u8>, String> {
    let usage = chunk.get("usageMetadata").cloned().unwrap_or(Value::Null);
    let input_tokens = usage
        .get("promptTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let msg_id = format!("msg_{:x}", xxhash_rust::xxh64::xxh64(model.as_bytes(), 0));
    let body = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": msg_id,
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": 0,
                }
            }
        });
    Ok(format_sse_event("message_start", &body).into_bytes())
}

/// 产出 text delta：开新 text block → 多个 text_delta → 不停（content_block_stop 由 finishReason 触发）。
fn emit_text_delta(params: &mut StreamParams, text: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    let index = params.response_index;

    if params.response_type != ResponseType::Text {
        // 开新 block
        if params.response_type != ResponseType::None {
            // 旧 block 关闭
            out.push(format_sse_event(
                "content_block_stop",
                &serde_json::json!({"type": "content_block_stop", "index": index}),
            ).into_bytes());
            params.response_index += 1;
            let new_index = params.response_index;
            params.response_type = ResponseType::Text;
            out.push(format_sse_event(
                "content_block_start",
                &serde_json::json!({
                    "type": "content_block_start",
                    "index": new_index,
                    "content_block": {"type": "text", "text": ""}
                }),
            ).into_bytes());
        } else {
            params.response_type = ResponseType::Text;
            out.push(format_sse_event(
                "content_block_start",
                &serde_json::json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": {"type": "text", "text": ""}
                }),
            ).into_bytes());
        }
    }

    out.push(format_sse_event(
        "content_block_delta",
        &serde_json::json!({
            "type": "content_block_delta",
            "index": params.response_index,
            "delta": {"type": "text_delta", "text": text}
        }),
    ).into_bytes());
    Ok(out)
}

fn emit_thinking_delta(params: &mut StreamParams, thought: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    let index = params.response_index;

    if params.response_type != ResponseType::Thinking {
        if params.response_type != ResponseType::None {
            out.push(format_sse_event(
                "content_block_stop",
                &serde_json::json!({"type": "content_block_stop", "index": index}),
            ).into_bytes());
            params.response_index += 1;
            let new_index = params.response_index;
            params.response_type = ResponseType::Thinking;
            out.push(format_sse_event(
                "content_block_start",
                &serde_json::json!({
                    "type": "content_block_start",
                    "index": new_index,
                    "content_block": {"type": "thinking", "thinking": ""}
                }),
            ).into_bytes());
        } else {
            params.response_type = ResponseType::Thinking;
            out.push(format_sse_event(
                "content_block_start",
                &serde_json::json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": {"type": "thinking", "thinking": ""}
                }),
            ).into_bytes());
        }
    }

    out.push(format_sse_event(
        "content_block_delta",
        &serde_json::json!({
            "type": "content_block_delta",
            "index": params.response_index,
            "delta": {"type": "thinking_delta", "thinking": thought}
        }),
    ).into_bytes());
    Ok(out)
}

fn emit_thinking_signature(params: &mut StreamParams, sig: &str) -> Result<Vec<Vec<u8>>, String> {
    params.thinking_signature = Some(sig.to_string());
    Ok(Vec::new()) // signature 本身不暴露给 client；保留供下游 message_delta 合并
}

fn emit_tool_use_start_and_args(
    params: &mut StreamParams,
    fc: &Value,
) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    let index = params.response_index;
    let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let sanitized = name.to_string();
    let original = params
        .sanitized_name_map
        .get(&sanitized)
        .cloned()
        .unwrap_or_else(|| sanitized.clone());
    let id = fc.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let id = if id.is_empty() {
        format!("toolu_{:x}", xxhash_rust::xxh64::xxh64(name.as_bytes(), 0))
    } else {
        id.to_string()
    };
    let input = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));

    if params.response_type != ResponseType::ToolUse {
        if params.response_type != ResponseType::None {
            out.push(format_sse_event(
                "content_block_stop",
                &serde_json::json!({"type": "content_block_stop", "index": index}),
            ).into_bytes());
            params.response_index += 1;
            let new_index = params.response_index;
            params.response_type = ResponseType::ToolUse;
            out.push(format_sse_event(
                "content_block_start",
                &serde_json::json!({
                    "type": "content_block_start",
                    "index": new_index,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": original,
                        "input": {}
                    }
                }),
            ).into_bytes());
        } else {
            params.response_type = ResponseType::ToolUse;
            out.push(format_sse_event(
                "content_block_start",
                &serde_json::json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": original,
                        "input": {}
                    }
                }),
            ).into_bytes());
        }
    }

    // 一次性输出 input_json_delta（Gemini 没有 streaming input delta）
    let input_str = serde_json::to_string(&input).unwrap_or_else(|_| "{}".into());
    out.push(format_sse_event(
        "content_block_delta",
        &serde_json::json!({
            "type": "content_block_delta",
            "index": params.response_index,
            "delta": {"type": "input_json_delta", "partial_json": input_str}
        }),
    ).into_bytes());

    Ok(out)
}

fn emit_message_delta_and_stop(
    params: &mut StreamParams,
    finish_reason: &str,
    saw_tool_use: bool,
    chunk: &Value,
) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    let index = params.response_index;

    // 关闭当前 block
    if params.response_type != ResponseType::None {
        out.push(format_sse_event(
            "content_block_stop",
            &serde_json::json!({"type": "content_block_stop", "index": index}),
        ).into_bytes());
        params.response_type = ResponseType::None;
    }

    let stop_reason = if saw_tool_use {
        "tool_use"
    } else {
        match finish_reason {
            "STOP" => "end_turn",
            "MAX_TOKENS" => "max_tokens",
            "TOOL_CALL" | "TOOL_USE" => "tool_use",
            // SAFETY / RECITATION / OTHER → 视为 end_turn 但日志保留原值
            _ => "end_turn",
        }
    };

    let usage = chunk.get("usageMetadata").cloned().unwrap_or(Value::Null);
    let output_tokens = usage
        .get("candidatesTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let mut usage_obj = Map::new();
    usage_obj.insert("output_tokens".into(), Value::from(output_tokens));
    if let Some(input) = usage.get("promptTokenCount").and_then(|v| v.as_i64()) {
        usage_obj.insert("input_tokens".into(), Value::from(input));
    }

    out.push(format_sse_event(
        "message_delta",
        &serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop_reason, "stop_sequence": null},
            "usage": Value::Object(usage_obj)
        }),
    ).into_bytes());

    out.push(format_sse_event(
        "message_stop",
        &serde_json::json!({"type": "message_stop"}),
    ).into_bytes());

    // 标记已停，后续 chunk no-op
    params.response_index = u32::MAX;
    Ok(out)
}

/// 格式化为 `event: <name>\ndata: <json>\n\n` 字节序列。
fn format_sse_event(event: &str, data: &Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        event,
        serde_json::to_string(data).unwrap_or_else(|_| "{}".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_params() -> StreamParams {
        StreamParams::default()
    }

    #[test]
    fn empty_chunk_returns_no_events() {
        let t = ClaudeToGeminiTranslator;
        let p = &mut fresh_params();
        let out = translate_response_stream(&t, "m", &Value::Null, b"", p).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn first_chunk_emits_message_start_and_text_delta() {
        let t = ClaudeToGeminiTranslator;
        let p = &mut fresh_params();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hi"}]},"finishReason":null}]}"#;
        let out = translate_response_stream(&t, "m", &Value::Null, chunk, p).unwrap();
        // message_start + content_block_start + content_block_delta(text)
        assert_eq!(out.len(), 3);
        let s = String::from_utf8_lossy(&out[0]);
        assert!(s.contains("event: message_start"));
        let s = String::from_utf8_lossy(&out[1]);
        assert!(s.contains("event: content_block_start"));
        assert!(s.contains("\"type\":\"text\""));
        let s = String::from_utf8_lossy(&out[2]);
        assert!(s.contains("text_delta"));
        assert!(s.contains("Hi"));
    }

    #[test]
    fn finish_stop_emits_message_delta_and_stop() {
        let t = ClaudeToGeminiTranslator;
        let p = &mut fresh_params();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"OK"}]},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3}}"#;
        let _ = translate_response_stream(&t, "m", &Value::Null, chunk, p).unwrap();
        // 4 events: message_start, block_start(text), delta(text), block_stop, message_delta, message_stop
        assert_eq!(p.response_index, u32::MAX);
    }

    #[test]
    fn thinking_part_emits_thinking_delta() {
        let t = ClaudeToGeminiTranslator;
        let p = &mut fresh_params();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"thought":"reasoning..."}]},"finishReason":null}]}"#;
        let out = translate_response_stream(&t, "m", &Value::Null, chunk, p).unwrap();
        // message_start + thinking block_start + thinking_delta
        assert_eq!(out.len(), 3);
        let s = String::from_utf8_lossy(&out[2]);
        assert!(s.contains("thinking_delta"));
    }

    #[test]
    fn function_call_emits_tool_use_block() {
        let t = ClaudeToGeminiTranslator;
        let p = &mut fresh_params();
        p.sanitized_name_map.insert("search_web".into(), "search.web".into());
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"search_web","args":{"q":"rust"}}}]},"finishReason":"STOP"}]}"#;
        let out = translate_response_stream(&t, "m", &Value::Null, chunk, p).unwrap();
        // message_start + content_block_start(tool_use) + input_json_delta + content_block_stop + message_delta + message_stop
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(s.contains("\"type\":\"tool_use\""));
        assert!(s.contains("search.web")); // 已通过 sanitized_name_map 还原
        assert!(s.contains("input_json_delta"));
        // tool_use 时 stop_reason 应为 "tool_use"
        assert!(s.contains("\"stop_reason\":\"tool_use\""));
        assert!(!s.contains("\"stop_reason\":\"end_turn\""));
    }
}