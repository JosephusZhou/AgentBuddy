//! Translator: Gemini streaming → OpenAI Responses API SSE
//!
//! CLIProxyAPI aligned: 934da237 - fix(openai): preserve structured and stringified
//!                        custom tool outputs during Responses conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/gemini/openai/responses/gemini_openai_responses_response.go`。
//!
//! OpenAI Responses 流式 event types（Codex CLI 关心）：
//! - `response.created` — 响应创建
//! - `response.output_item.added` — 新 output item (message / function_call)
//! - `response.content_part.added` — message 内的 content part (output_text)
//! - `response.output_text.delta` — 文本增量
//! - `response.function_call_arguments.delta` — 工具参数增量
//! - `response.output_item.done` — item 完成
//! - `response.completed` — 响应完成（含 usage）
//! - `response.error` — 错误
//!
//! Gemini 流式 chunk → Responses 事件序列：
//! - 第一帧：`response.created`
//! - text part → `response.output_item.added` (message) + `response.content_part.added` (output_text) + 多个 `response.output_text.delta`
//! - functionCall part → `response.output_item.added` (function_call) + `response.function_call_arguments.delta` (一次性) + `response.output_item.done`
//! - finishReason → `response.completed`（含 usage）
//! - 末尾：`[DONE]`

use serde_json::{Map, Value};

use super::super::params::{ResponseType, StreamParams};

/// 流式翻译：每个 Gemini chunk → 一组 OpenAI Responses SSE 字节片段。
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
    let response_id = format!(
        "resp_{:x}",
        xxhash_rust::xxh64::xxh64(format!("{}:{}", model, original_request.to_string()).as_bytes(), 0)
    );
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 1. 首帧：response.created
    if !params.has_content
        && params.response_index == 0
        && params.response_type == ResponseType::None
    {
        let frame = serde_json::json!({
            "type": "response.created",
            "response": {
                "id": response_id,
                "object": "response",
                "created_at": created,
                "status": "in_progress",
                "model": model
            }
        });
        out.push(format_sse_event("response.created", &frame).into_bytes());
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
            out.extend(emit_text_deltas(&response_id, model, params, text)?);
        } else if let Some(thought) = part.get("thought").and_then(|v| v.as_str()) {
            // Gemini thinking → Responses reasoning item
            out.extend(emit_reasoning_summary(&response_id, model, params, thought)?);
        } else if let Some(fc) = part.get("functionCall") {
            saw_tool_call = true;
            out.push(emit_function_call_item(&response_id, model, params, fc)?);
        }
    }

    // 3. finishReason → response.completed
    let finish_reason = candidate.get("finishReason").and_then(|v| v.as_str());
    if let Some(fr) = finish_reason {
        let status = match fr {
            "STOP" => "completed",
            "MAX_TOKENS" => "incomplete",
            "TOOL_CALL" => "completed",
            "SAFETY" => "failed",
            _ => "completed",
        };
        let output = build_output(params, saw_tool_call);
        let usage = build_usage(&chunk);
        let mut response_obj = Map::new();
        response_obj.insert("id".into(), Value::String(response_id.clone()));
        response_obj.insert("object".into(), Value::String("response".into()));
        response_obj.insert("created_at".into(), Value::from(created));
        response_obj.insert("status".into(), Value::String(status.to_string()));
        response_obj.insert("model".into(), Value::String(model.to_string()));
        response_obj.insert("output".into(), output);
        if let Some(u) = usage {
            response_obj.insert("usage".into(), u);
        }
        let frame = serde_json::json!({
            "type": "response.completed",
            "response": Value::Object(response_obj)
        });
        out.push(format_sse_event("response.completed", &frame).into_bytes());
        out.push(b"data: [DONE]\n\n".to_vec());
        params.response_index = u32::MAX;
    }

    Ok(out)
}

fn emit_text_deltas(
    response_id: &str,
    model: &str,
    params: &mut StreamParams,
    text: &str,
) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    if params.response_type != ResponseType::Text {
        // 关闭前一个 block
        if params.response_type == ResponseType::ToolUse {
            // function_call 已 done；无需 content_part.done
        }
        // 新开 message item
        let item_id = format!("msg_{:x}", xxhash_rust::xxh64::xxh64(text.as_bytes(), 0));
        params.current_tool_call_id = Some(item_id.clone());
        let add_frame = serde_json::json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "status": "in_progress",
                "content": []
            }
        });
        out.push(format_sse_event("response.output_item.added", &add_frame).into_bytes());

        // 新开 content part (output_text)
        let part_add = serde_json::json!({
            "type": "response.content_part.added",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "part": {"type": "output_text", "text": "", "annotations": []}
        });
        out.push(format_sse_event("response.content_part.added", &part_add).into_bytes());

        params.response_type = ResponseType::Text;
        params.response_index = 1;
        let _ = model;
        let _ = response_id;
    }

    // 文本 delta
    let delta = serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": params.current_tool_call_id.clone().unwrap_or_default(),
        "output_index": 0,
        "content_index": 0,
        "delta": text
    });
    out.push(format_sse_event("response.output_text.delta", &delta).into_bytes());
    Ok(out)
}

fn emit_reasoning_summary(
    _response_id: &str,
    _model: &str,
    params: &mut StreamParams,
    thought: &str,
) -> Result<Vec<Vec<u8>>, String> {
    // thinking part → reasoning item 一次性 summary
    let mut out = Vec::new();
    if params.response_type == ResponseType::Thinking {
        return Ok(out);
    }
    params.response_type = ResponseType::Thinking;

    let item_id = format!(
        "rs_{:x}",
        xxhash_rust::xxh64::xxh64(thought.as_bytes(), 0)
    );
    let add = serde_json::json!({
        "type": "response.output_item.added",
        "output_index": 0,
        "item": {
            "id": item_id,
            "type": "reasoning",
            "summary": []
        }
    });
    out.push(format_sse_event("response.output_item.added", &add).into_bytes());

    let part_add = serde_json::json!({
        "type": "response.content_part.added",
        "item_id": item_id,
        "output_index": 0,
        "content_index": 0,
        "part": {"type": "summary_text", "text": thought}
    });
    out.push(format_sse_event("response.content_part.added", &part_add).into_bytes());

    let done = serde_json::json!({
        "type": "response.output_item.done",
        "output_index": 0,
        "item": {
            "id": item_id,
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": thought}]
        }
    });
    out.push(format_sse_event("response.output_item.done", &done).into_bytes());
    Ok(out)
}

fn emit_function_call_item(
    response_id: &str,
    model: &str,
    params: &mut StreamParams,
    fc: &Value,
) -> Result<Vec<u8>, String> {
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
    let item_id = format!("fc_{:x}", xxhash_rust::xxh64::xxh64(call_id.as_bytes(), 0));

    // 关闭前一个 message（如有）
    if params.response_type == ResponseType::Text {
        let done = serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "id": params.current_tool_call_id.clone().unwrap_or_default(),
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": ""}]
            }
        });
        let mut out = vec![format_sse_event("response.output_item.done", &done).into_bytes()];
        params.response_type = ResponseType::ToolUse;
        params.current_tool_call_id = Some(item_id.clone());

        // 新开 function_call item
        let add = serde_json::json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "id": item_id,
                "type": "function_call",
                "call_id": call_id,
                "name": original,
                "arguments": "",
                "status": "in_progress"
            }
        });
        out.push(format_sse_event("response.output_item.added", &add).into_bytes());

        // 一次性 arguments delta
        let delta = serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "item_id": item_id,
            "output_index": 0,
            "delta": arguments_str
        });
        out.push(format_sse_event("response.function_call_arguments.delta", &delta).into_bytes());

        // item done
        let item_done = serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "id": item_id,
                "type": "function_call",
                "call_id": call_id,
                "name": original,
                "arguments": arguments_str,
                "status": "completed"
            }
        });
        out.push(format_sse_event("response.output_item.done", &item_done).into_bytes());
        let _ = response_id;
        let _ = model;
        return Ok(out.concat());
    }
    params.response_type = ResponseType::ToolUse;
    params.current_tool_call_id = Some(item_id.clone());
    let mut out = Vec::new();

    let add = serde_json::json!({
        "type": "response.output_item.added",
        "output_index": 0,
        "item": {
            "id": item_id,
            "type": "function_call",
            "call_id": call_id,
            "name": original,
            "arguments": "",
            "status": "in_progress"
        }
    });
    out.push(format_sse_event("response.output_item.added", &add).into_bytes());

    let delta = serde_json::json!({
        "type": "response.function_call_arguments.delta",
        "item_id": item_id,
        "output_index": 0,
        "delta": arguments_str
    });
    out.push(format_sse_event("response.function_call_arguments.delta", &delta).into_bytes());

    let item_done = serde_json::json!({
        "type": "response.output_item.done",
        "output_index": 0,
        "item": {
            "id": item_id,
            "type": "function_call",
            "call_id": call_id,
            "name": original,
            "arguments": arguments_str,
            "status": "completed"
        }
    });
    out.push(format_sse_event("response.output_item.done", &item_done).into_bytes());
    Ok(out.concat())
}

fn build_output(params: &StreamParams, _saw_tool_call: bool) -> Value {
    let mut output: Vec<Value> = Vec::new();
    if params.response_type == ResponseType::Text {
        // 上一帧可能没发 done，这里补
        if let Some(id) = &params.current_tool_call_id {
            output.push(Value::Object(Map::from_iter([
                ("id".into(), Value::String(id.clone())),
                ("type".into(), Value::String("message".into())),
                ("role".into(), Value::String("assistant".into())),
                ("status".into(), Value::String("completed".into())),
                (
                    "content".into(),
                    Value::Array(vec![Value::Object(Map::from_iter([
                        ("type".into(), Value::String("output_text".into())),
                        ("text".into(), Value::String(String::new())),
                    ]))]),
                ),
            ])));
        }
    }
    Value::Array(output)
}

fn build_usage(chunk: &Value) -> Option<Value> {
    let usage = chunk.get("usageMetadata")?;
    let prompt = usage.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
    let completion = usage.get("candidatesTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
    Some(Value::Object(Map::from_iter([
        ("prompt_tokens".into(), Value::from(prompt)),
        ("completion_tokens".into(), Value::from(completion)),
        ("total_tokens".into(), Value::from(prompt + completion)),
    ])))
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

    fn fresh() -> StreamParams {
        StreamParams::default()
    }

    #[test]
    fn first_chunk_emits_response_created() {
        let mut p = fresh();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Hi"}]},"finishReason":null}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s = String::from_utf8_lossy(&out[0]);
        assert!(s.contains("response.created"));
    }

    #[test]
    fn function_call_emits_full_item_sequence() {
        let mut p = fresh();
        p.sanitized_name_map.insert("search_web".into(), "search.web".into());
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"search_web","args":{"q":"rust"}}}]},"finishReason":"STOP"}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(s.contains("response.output_item.added"));
        assert!(s.contains("function_call"));
        assert!(s.contains("search.web"));
        assert!(s.contains("function_call_arguments.delta"));
        assert!(s.contains("response.output_item.done"));
        assert!(s.contains("response.completed"));
        assert!(s.contains("[DONE]"));
    }

    #[test]
    fn thinking_emits_reasoning_item() {
        let mut p = fresh();
        let chunk = br#"data: {"candidates":[{"content":{"role":"model","parts":[{"thought":"reasoning..."}]},"finishReason":"STOP"}]}"#;
        let out = translate_response_stream("m", &Value::Null, chunk, &mut p).unwrap();
        let s: String = out
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(s.contains("\"type\":\"reasoning\""));
    }
}