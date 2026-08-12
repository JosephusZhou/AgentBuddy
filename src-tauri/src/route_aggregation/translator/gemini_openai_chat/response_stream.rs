//! Translator: Gemini streaming → OpenAI Chat Completions SSE
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/gemini/openai/chat-completions/gemini_openai_response.go`。
//!
//! OpenAI Chat 流式 chunk 序列：
// - 首帧 `delta.role: "assistant"`
// - 文本增量 `delta.content: "..."`
// - 工具调用增量 `delta.tool_calls: [{index, id, function.name, function.arguments}]`
// - finish_reason frame
// - 末尾 `[DONE]`

use serde_json::{Map, Value};

use super::super::params::{ResponseType, StreamParams};

/// 流式翻译：每个 Gemini chunk → 一组 OpenAI Chat SSE 字节片段。
pub fn translate_response_stream(
    model: &str,
    original_request: &Value,
    raw_chunk: &[u8],
    params: &mut StreamParams,
) -> Result<Vec<Vec<u8>>, String> {
    if raw_chunk.is_empty() {
        return Ok(Vec::new());
    }

    let line = match extract_data_line(raw_chunk) {
        Some(l) => l,
        None => return Ok(Vec::new()),
    };

    if line.is_empty() || line == b"[DONE]" {
        return Ok(Vec::new());
    }

    let chunk: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(e) => return Err(format!("解析 Gemini 流式 chunk 失败: {e}")),
    };

    let mut out: Vec<Vec<u8>> = Vec::new();
    let chat_id = format!("chatcmpl-{}", params_chat_id(model, original_request));
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 1. 首帧：role=assistant
    if !params.has_content
        && params.response_index == 0
        && params.response_type == ResponseType::None
    {
        let frame = serde_json::json!({
            "id": chat_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": ""},
                "finish_reason": null
            }]
        });
        out.push(format_sse_chunk(&frame).into_bytes());
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

    let mut saw_tool_call = false;
    for part in parts {
        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            out.push(emit_content_delta(model, &chat_id, created, text)?);
        } else if let Some(thought) = part.get("thought").and_then(|v| v.as_str()) {
            // Gemini thinking → OpenAI reasoning_content（少数 SDK 支持；
            // 不支持时也保留在 reasoning_content 字段，由 client 决定如何用）
            let frame = serde_json::json!({
                "id": chat_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {"reasoning_content": thought},
                    "finish_reason": null
                }]
            });
            out.push(format_sse_chunk(&frame).into_bytes());
        } else if let Some(fc) = part.get("functionCall") {
            saw_tool_call = true;
            out.push(emit_tool_call_delta(model, &chat_id, created, fc, params)?);
        }
    }

    // 3. finishReason 触发 finish_reason frame
    let finish_reason = candidate.get("finishReason").and_then(|v| v.as_str());
    if let Some(fr) = finish_reason {
        let mapped = if saw_tool_call {
            "tool_calls"
        } else {
            match fr {
                "STOP" => "stop",
                "MAX_TOKENS" => "length",
                "TOOL_CALL" => "tool_calls",
                "SAFETY" => "content_filter",
                _ => "stop",
            }
        };
        let frame = serde_json::json!({
            "id": chat_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": mapped
            }]
        });
        out.push(format_sse_chunk(&frame).into_bytes());

        // usage chunk（如果上游有 usageMetadata）
        if let Some(usage) = chunk.get("usageMetadata") {
            let prompt = usage.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
            let completion = usage.get("candidatesTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
            let frame = serde_json::json!({
                "id": chat_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [],
                "usage": {
                    "prompt_tokens": prompt,
                    "completion_tokens": completion,
                    "total_tokens": prompt + completion
                }
            });
            out.push(format_sse_chunk(&frame).into_bytes());
        }

        // 末尾 [DONE]
        out.push(b"data: [DONE]\n\n".to_vec());

        // 标记已停
        params.response_index = u32::MAX;
    }

    Ok(out)
}

fn emit_content_delta(
    model: &str,
    chat_id: &str,
    created: i64,
    text: &str,
) -> Result<Vec<u8>, String> {
    let frame = serde_json::json!({
        "id": chat_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"content": text},
            "finish_reason": null
        }]
    });
    Ok(format_sse_chunk(&frame).into_bytes())
}

fn emit_tool_call_delta(
    model: &str,
    chat_id: &str,
    created: i64,
    fc: &Value,
    params: &mut StreamParams,
) -> Result<Vec<u8>, String> {
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
    let arguments_str = serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".into());

    // Gemini 不分片 tool call arguments（一次性发完），所以一个 delta 即可
    let frame = serde_json::json!({
        "id": chat_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": original,
                        "arguments": arguments_str
                    }
                }]
            },
            "finish_reason": null
        }]
    });
    Ok(format_sse_chunk(&frame).into_bytes())
}

fn params_chat_id(model: &str, orig: &Value) -> String {
    use xxhash_rust::xxh64::xxh64;
    let seed = orig.to_string();
    format!("{:x}", xxh64(format!("{model}:{seed}").as_bytes(), 0))
}

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

fn format_sse_chunk(data: &Value) -> String {
    format!(
        "data: {}\n\n",
        serde_json::to_string(data).unwrap_or_else(|_| "{}".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> StreamParams {
        StreamParams::default()
    }

    #[test]
    fn first_chunk_emits_role_and_content() {
        let mut p = fresh();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hi"}]},"finishReason":null}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        assert_eq!(out.len(), 2);
        let s = String::from_utf8_lossy(&out[0]);
        assert!(s.contains("\"role\":\"assistant\""));
        assert!(s.contains("chat.completion.chunk"));
        let s = String::from_utf8_lossy(&out[1]);
        assert!(s.contains("\"content\":\"Hi\""));
    }

    #[test]
    fn function_call_emits_tool_calls_delta() {
        let mut p = fresh();
        p.sanitized_name_map.insert("search_web".into(), "search.web".into());
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"search_web","args":{"q":"rust"}}}]},"finishReason":"STOP"}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        // 含 tool_calls
        assert!(s.contains("\"tool_calls\""));
        assert!(s.contains("search.web"));
        // finish_reason 为 tool_calls
        assert!(s.contains("\"finish_reason\":\"tool_calls\""));
        // 末尾 DONE
        assert!(s.contains("[DONE]"));
    }

    #[test]
    fn thinking_emits_reasoning_content() {
        let mut p = fresh();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"thought":"reasoning..."}]},"finishReason":"STOP"}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(s.contains("reasoning_content"));
    }

    #[test]
    fn max_tokens_finish_emits_length() {
        let mut p = fresh();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"OK"}]},"finishReason":"MAX_TOKENS"}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(s.contains("\"finish_reason\":\"length\""));
    }

    #[test]
    fn finish_with_usage_emits_usage_chunk() {
        let mut p = fresh();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"OK"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3}}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(s.contains("\"prompt_tokens\":5"));
        assert!(s.contains("\"completion_tokens\":3"));
        assert!(s.contains("\"total_tokens\":8"));
    }
}