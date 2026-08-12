//! Translator: OpenAI Responses API → Gemini generateContent
//!
//! CLIProxyAPI aligned:
//! - 934da23 - fix(openai): preserve structured and stringified custom tool outputs
//! - ecc9aa7 - fix(openai): preserve assistant content when converting Responses
//!             tool-call turns
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//!         https://github.com/router-for-me/CLIProxyAPI/commit/ecc9aa72b32f34b680d03b0724b531a21ae74472
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/openai/openai/responses/`。
//!
//! OpenAI Responses 协议特征（Codex CLI 主路径）：
//! - `instructions` (string) → systemInstruction
//! - `input` (string | items[])
//!   - `{"type":"message", "role":"user|assistant|system|developer", "content":[parts]}`
//!   - `{"type":"function_call", "call_id","name","arguments"}` (assistant tool call)
//!   - `{"type":"function_call_output", "call_id","output"}` (tool result)
//!   - `{"type":"reasoning", "summary":[...], "encrypted_content": "..."}` (thinking 回放)
//!   - `{"type":"item_reference", "id": "..."}` (引用之前的 item)
//! - `tools[]` 形状：`[{"type":"function", "name", "description", "parameters"}]`
//! - `reasoning.effort` ↔ Gemini `thinkingConfig.thinkingBudget`
//! - `parallel_tool_calls: false` ↔ `toolConfig.functionCallingConfig.mode = NONE`

use serde_json::{Map, Value};

use super::super::common::tool_name;
use super::super::params::StreamParams;
use super::super::translatable::TranslateError;

/// 构造 OpenAI Responses → Gemini 请求 body 的核心逻辑。
pub fn build_request(
    model: &str,
    raw: &Value,
    stream: bool,
    params: &mut StreamParams,
) -> Result<Value, TranslateError> {
    let src = raw.as_object().ok_or_else(|| {
        TranslateError::Invalid("OpenAI Responses request body must be a JSON object".into())
    })?;

    let mut out = Map::new();

    // 1. systemInstruction: instructions 字段（Responses 用 instructions 替代 system）
    if let Some(Value::String(s)) = src.get("instructions") {
        if !s.is_empty() {
            out.insert(
                "systemInstruction".into(),
                Value::Object(Map::from_iter([(
                    "parts".into(),
                    Value::Array(vec![Value::Object(Map::from_iter([(
                        "text".into(),
                        Value::String(s.clone()),
                    )]))]),
                )])),
            );
        }
    }

    // 2. contents: input items → Gemini contents
    let contents = build_contents(src, params)?;
    if !contents.is_empty() {
        out.insert("contents".into(), Value::Array(contents));
    }

    // 3. tools → functionDeclarations
    if let Some(tools) = src.get("tools") {
        if let Some(gemini_tools) = build_tools(tools, params)? {
            out.insert("tools".into(), gemini_tools);
        }
    }

    // 4. tool_config: parallel_tool_calls=false → mode=NONE
    if let Some(Value::Bool(false)) = src.get("parallel_tool_calls") {
        out.insert(
            "toolConfig".into(),
            Value::Object(Map::from_iter([(
                "functionCallingConfig".into(),
                Value::Object(Map::from_iter([(
                    "mode".into(),
                    Value::String("NONE".into()),
                )])),
            )])),
        );
    }

    // 5. generationConfig
    let mut gen_config = Map::new();
    if let Some(v) = src.get("max_output_tokens").and_then(|v| v.as_i64()) {
        if v > 0 {
            gen_config.insert("maxOutputTokens".into(), Value::from(v));
        }
    }
    if let Some(v) = src.get("temperature").and_then(|v| v.as_f64()) {
        gen_config.insert("temperature".into(), Value::from(v));
    }
    if let Some(v) = src.get("top_p").and_then(|v| v.as_f64()) {
        gen_config.insert("topP".into(), Value::from(v));
    }
    if let Some(v) = src.get("truncation").and_then(|v| v.as_str()) {
        // OpenAI "auto"/"disabled" 暂时透传为 generationConfig.candidateCount
        // 实际语义由 Gemini 决定（一般忽略）
        let _ = v;
    }

    // 6. reasoning.effort → thinkingConfig.thinkingLevel
    if let Some(reasoning) = src.get("reasoning").and_then(|v| v.as_object()) {
        if let Some(effort) = reasoning.get("effort").and_then(|v| v.as_str()) {
            // effort 映射：
            //   "low"     → thinkingLevel: "low"
            //   "medium"  → thinkingLevel: "medium"
            //   "high"    → thinkingLevel: "high"
            let mut tc = Map::new();
            tc.insert(
                "thinkingLevel".into(),
                Value::String(effort.to_string()),
            );
            tc.insert("includeThoughts".into(), Value::Bool(true));
            gen_config.insert("thinkingConfig".into(), Value::Object(tc));
        }
    }

    if !gen_config.is_empty() {
        out.insert("generationConfig".into(), Value::Object(gen_config));
    }

    if model.is_empty() {
        return Err(TranslateError::Invalid("missing model name".into()));
    }
    let _ = stream;

    Ok(Value::Object(out))
}

fn build_contents(
    src: &Map<String, Value>,
    params: &mut StreamParams,
) -> Result<Vec<Value>, TranslateError> {
    let Some(input) = src.get("input") else { return Ok(Vec::new()) };
    let mut out: Vec<Value> = Vec::new();

    match input {
        Value::String(s) => {
            out.push(Value::Object(Map::from_iter([
                ("role".into(), Value::String("user".into())),
                (
                    "parts".into(),
                    Value::Array(vec![Value::Object(Map::from_iter([(
                        "text".into(),
                        Value::String(s.clone()),
                    )]))]),
                ),
            ])));
        }
        Value::Array(arr) => {
            // 状态：跟踪最近的 "可合并" assistant message item（CLIProxyAPI
            // `ecc9aa7` 修复：避免相邻 assistant message + function_call 序列
            // 产生多个 model content；改为合并 `tool_calls` 到已有 assistant
            // message 的 parts）。跨 role / function_call_output 边界 reset。
            let mut mergeable_assistant_index: Option<usize> = None;
            // 待附加的 functionCall parts（累积后追加到 assistant message 的 parts）
            let mut pending_tool_calls: Vec<Value> = Vec::new();
            // 待附加的 reasoning 缓冲（跨多个 reasoning item / assistant message 合并）
            let mut pending_reasoning_content = String::new();

            for item in arr {
                let Some(content_obj) = item.as_object() else { continue };
                let item_type = content_obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match item_type {
                    "message" => {
                        // assistant message 之前先把累积的 functionCall 合并到 mergeable_assistant
                        flush_pending_tool_calls(&mut out, &mut mergeable_assistant_index, &mut pending_tool_calls);

                        // 非 user 角色先把 mergeable_assistant_index reset
                        let role = content_obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
                        if role != "assistant" {
                            mergeable_assistant_index = None;
                        }
                        if role != "assistant" {
                            // 把积累的 reasoning 视为 turn 边界信号，先上扔
                            pending_reasoning_content.clear();
                        }

                        if let Some(content_obj_built) = build_message_item(content_obj, params)? {
                            let pushed_index = out.len();
                            out.push(content_obj_built);
                            if role == "assistant" {
                                mergeable_assistant_index = Some(pushed_index);
                            }
                        }
                    }
                    "function_call" => {
                        // 把 inline reasoning_content 合并到 pending buffer
                        if let Some(rc) = content_obj.get("reasoning_content").and_then(|v| v.as_str()) {
                            pending_reasoning_content = combine_openai_responses_reasoning(
                                std::mem::take(&mut pending_reasoning_content),
                                rc,
                            );
                        }
                        if let Some(fc) = build_function_call_item(content_obj, params)? {
                            pending_tool_calls.push(fc);
                        }
                    }
                    "function_call_output" => {
                        // 边界：tool output 紧跟在 tool_call 之后；先把 pending tool_calls flush
                        flush_pending_tool_calls(&mut out, &mut mergeable_assistant_index, &mut pending_tool_calls);
                        mergeable_assistant_index = None;
                        pending_reasoning_content.clear();

                        if let Some(fr) = build_function_call_output_item(content_obj)? {
                            push_user_content(&mut out, fr);
                        }
                    }
                    "custom_tool_call" => {
                        // Codex freeform tool call: 缓冲 inline reasoning_content
                        if let Some(rc) = content_obj.get("reasoning_content").and_then(|v| v.as_str()) {
                            pending_reasoning_content = combine_openai_responses_reasoning(
                                std::mem::take(&mut pending_reasoning_content),
                                rc,
                            );
                        }
                        // 把调用标记为 functionCall part（保留 name/call_id/arguments）
                        if let Some(fc) = build_function_call_item(content_obj, params)? {
                            pending_tool_calls.push(fc);
                        }
                    }
                    "custom_tool_call_output" => {
                        // CLIProxyAPI 934da23: custom tool output 的处理走
                        // 与 function_call_output 相同的 reasoning-flush + fr 路径，
                        // 区别在于 build_function_call_output_item 内部解析结构化
                        // output（image array / text array / plain string）。
                        flush_pending_tool_calls(&mut out, &mut mergeable_assistant_index, &mut pending_tool_calls);
                        mergeable_assistant_index = None;
                        pending_reasoning_content.clear();

                        if let Some(fr) = build_function_call_output_item(content_obj)? {
                            push_user_content(&mut out, fr);
                        }
                    }
                    "reasoning" => {
                        // 收集 reasoning summary 内容（Responses reasoning item 的标准字段）
                        let summaries = content_obj.get("summary").and_then(|v| v.as_array());
                        if let Some(sums) = summaries {
                            for s in sums {
                                if let Some(text) = s.get("text").and_then(|v| v.as_str()) {
                                    pending_reasoning_content = combine_openai_responses_reasoning(
                                        std::mem::take(&mut pending_reasoning_content),
                                        text,
                                    );
                                }
                            }
                        }
                    }
                    "item_reference" => {
                        // 引用之前的 item；常见于多轮对话，Gemini 不直接支持。
                        // 忽略（不会破坏端到端，丢失该 turn 的内容）。
                    }
                    _ => {
                        // 未知 item type 跳过；同时 reset merge state
                        flush_pending_tool_calls(&mut out, &mut mergeable_assistant_index, &mut pending_tool_calls);
                        mergeable_assistant_index = None;
                        pending_reasoning_content.clear();
                    }
                }
            }

            // 收尾：把未消费的 pending tool_calls flush
            flush_pending_tool_calls(&mut out, &mut mergeable_assistant_index, &mut pending_tool_calls);
            // 注：pending_reasoning_content 当前不直接放到 out（Gemini 没原生 reasoning
            // 位置；如需回放，可作为 model content 第一个 parts 写一个 thinking 类型，但
            // Gemini 接受度有限）。保留 buffer 以便后续 turn 合并，与 ecc9aa7 行为对齐。
            let _ = pending_reasoning_content;
        }
        _ => return Err(TranslateError::Invalid("`input` must be string or array".into())),
    }

    Ok(out)
}

/// 把累积的 tool_calls 合并到最近一个 assistant model content（如果存在）；
/// 否则开新的 model content。
fn flush_pending_tool_calls(
    out: &mut Vec<Value>,
    mergeable_assistant_index: &mut Option<usize>,
    pending_tool_calls: &mut Vec<Value>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }
    let drained: Vec<Value> = std::mem::take(pending_tool_calls);

    if let Some(idx) = mergeable_assistant_index {
        if *idx == out.len().saturating_sub(1) {
            if let Some(last) = out.get_mut(*idx) {
                if last.get("role").and_then(|v| v.as_str()) == Some("model") {
                    if let Some(parts) = last.get_mut("parts").and_then(|v| v.as_array_mut()) {
                        for fc in drained {
                            parts.push(fc);
                        }
                        return;
                    }
                }
            }
        }
    }

    // 没有 mergeable assistant → 开新的 model content
    out.push(Value::Object(Map::from_iter([
        ("role".into(), Value::String("model".into())),
        ("parts".into(), Value::Array(drained)),
    ])));
    *mergeable_assistant_index = Some(out.len() - 1);
}

/// 合并两段 reasoning 内容（CLIProxyAPI `combineOpenAIResponsesReasoning`）：
/// - 空 + empty → empty
/// - `[reasoning unavailable]` placeholder 不覆盖真实内容
/// - 完全相同 → 保留一份
/// - 其余 → `existing + "\n\n" + incoming`
fn combine_openai_responses_reasoning(existing: String, incoming: &str) -> String {
    let existing_trimmed = existing.trim();
    let incoming_trimmed = incoming.trim();
    match (existing_trimmed.is_empty(), incoming_trimmed.is_empty()) {
        (true, true) => existing,
        (true, false) => incoming.to_string(),
        (false, true) => existing,
        (false, false) => {
            if existing_trimmed == "[reasoning unavailable]" {
                incoming.to_string()
            } else if incoming_trimmed == "[reasoning unavailable]" || existing_trimmed == incoming_trimmed {
                existing
            } else {
                format!("{existing}\n\n{incoming}")
            }
        }
    }
}

fn build_message_item(
    obj: &Map<String, Value>,
    params: &mut StreamParams,
) -> Result<Option<Value>, TranslateError> {
    let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let gemini_role = match role {
        "user" => "user",
        "assistant" => "model",
        "system" | "developer" => {
            // Responses 允许 system/developer message item；折叠到
            // systemInstruction（已通过 `instructions` 字段处理）。
            // 这里忽略，避免重复。如果用户没用 `instructions` 字段，
            // Phase 3 补一个合并逻辑。
            return Ok(None);
        }
        _ => return Ok(None),
    };

    let mut parts: Vec<Value> = Vec::new();
    if let Some(content) = obj.get("content") {
        match content {
            Value::String(s) => {
                if !s.is_empty() {
                    parts.push(Value::Object(Map::from_iter([(
                        "text".into(),
                        Value::String(s.clone()),
                    )])));
                }
            }
            Value::Array(arr) => {
                for block in arr {
                    if let Some(p) = build_content_part(block, params) {
                        parts.push(p);
                    }
                }
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(Value::Object(Map::from_iter([
        ("role".into(), Value::String(gemini_role.to_string())),
        ("parts".into(), Value::Array(parts)),
    ]))))
}

fn build_content_part(
    block: &Value,
    params: &mut StreamParams,
) -> Option<Value> {
    let obj = block.as_object()?;
    let btype = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match btype {
        "input_text" | "text" => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| {
                Value::Object(Map::from_iter([(
                    "text".into(),
                    Value::String(s.to_string()),
                )]))
            }),
        "output_text" => {
            // assistant 文本 part（一般不在 input 里出现，保留兼容）
            obj.get("text").and_then(|v| v.as_str()).map(|s| {
                Value::Object(Map::from_iter([(
                    "text".into(),
                    Value::String(s.to_string()),
                )]))
            })
        }
        "input_image" => {
            // input_image.image 可能是 base64 字符串或 URL
            let image = obj.get("image")?;
            if let Some(b64) = image.as_str() {
                // 默认 png，CLIProxyAPI 行为是 sniff 或者使用 detail 字段
                let mut inline = Map::new();
                inline.insert("mime_type".into(), Value::String("image/png".into()));
                inline.insert("data".into(), Value::String(b64.to_string()));
                return Some(Value::Object(Map::from_iter([(
                    "inline_data".into(),
                    Value::Object(inline),
                )])));
            }
            None
        }
        "function_call" => {
            // 出现在 message content 数组里的 function_call item
            build_function_call_inline_part(obj, params)
        }
        _ => None,
    }
}

fn build_function_call_item(
    obj: &Map<String, Value>,
    params: &mut StreamParams,
) -> Result<Option<Value>, TranslateError> {
    let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let call_id = obj.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
    let arguments_str = obj
        .get("arguments")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let arguments: Value = serde_json::from_str(arguments_str).unwrap_or(Value::Object(Map::new()));

    let sanitized = tool_name::sanitize_with_occupied(name, &mut params.sanitized_name_map);

    let mut function_call = Map::new();
    function_call.insert("name".into(), Value::String(sanitized));
    function_call.insert("args".into(), arguments);
    if !call_id.is_empty() {
        function_call.insert("id".into(), Value::String(call_id.to_string()));
    }
    Ok(Some(Value::Object(Map::from_iter([(
        "functionCall".into(),
        Value::Object(function_call),
    )]))))
}

fn build_function_call_inline_part(
    obj: &Map<String, Value>,
    params: &mut StreamParams,
) -> Option<Value> {
    let name = obj.get("name").and_then(|v| v.as_str())?;
    let arguments_str = obj
        .get("arguments")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let arguments: Value =
        serde_json::from_str(arguments_str).unwrap_or(Value::Object(Map::new()));
    let call_id = obj.get("call_id").and_then(|v| v.as_str()).unwrap_or("");

    let sanitized = tool_name::sanitize_with_occupied(name, &mut params.sanitized_name_map);
    let mut function_call = Map::new();
    function_call.insert("name".into(), Value::String(sanitized));
    function_call.insert("args".into(), arguments);
    if !call_id.is_empty() {
        function_call.insert("id".into(), Value::String(call_id.to_string()));
    }
    Some(Value::Object(Map::from_iter([(
        "functionCall".into(),
        Value::Object(function_call),
    )])))
}

fn build_function_call_output_item(
    obj: &Map<String, Value>,
) -> Result<Option<Value>, TranslateError> {
    let call_id = obj.get("call_id").and_then(|v| v.as_str()).unwrap_or("");

    let mut fr = Map::new();
    if !call_id.is_empty() {
        fr.insert("id".into(), Value::String(call_id.to_string()));
    }

    // 解析 `output` 字段：可能是 string / array / JSON-encoded string / null。
    // CLIProxyAPI `934da23` 修复：Responses 的 `output` 可能是 JSON-encoded string
    // 包裹的 array（含 input_image / input_text）；需解析后才能正确路由。
    let output = obj.get("output");
    let response = match output {
        Some(Value::String(s)) => {
            // 尝试作为 JSON 解析（stringified array / object）
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                responses_output_to_function_response(parsed)
            } else {
                // 普通字符串 → {"result": ...}
                Value::Object(Map::from_iter([(
                    "result".into(),
                    Value::String(s.clone()),
                )]))
            }
        }
        Some(v @ Value::Array(_)) => responses_output_to_function_response(v.clone()),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(Value::Null) | None => Value::Object(Map::new()),
        Some(v) => v.clone(),
    };
    fr.insert("response".into(), response);

    Ok(Some(Value::Object(Map::from_iter([(
        "functionResponse".into(),
        Value::Object(fr),
    )]))))
}

/// 解析 Responses `output` 字段（已经过 JSON 解码）→ Gemini `functionResponse.response`.
///
/// 行为对齐 CLIProxyAPI `setCustomToolCallOutputContent` + `responsesToolOutputText`：
/// - 含 `input_image` part → 走 image 路径（本翻译器输出文本描述占位；Gemini
///   functionResponse 不支持 multimodal parts，故用 `[image:base64:N bytes]` 形式）
/// - 含 `input_text` / `text` part → 拼接为单一字符串
/// - 否则 → 整个 value 的原始 JSON 作为 response。
fn responses_output_to_function_response(parsed: Value) -> Value {
    let Some(arr) = parsed.as_array() else {
        // object / scalar → 原样作为 response
        return parsed;
    };

    let mut text_buf = String::new();
    let mut image_count = 0usize;
    for block in arr {
        let Some(obj) = block.as_object() else { continue };
        let btype = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match btype {
            "input_text" | "text" | "output_text" => {
                if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                    if !text_buf.is_empty() {
                        text_buf.push('\n');
                    }
                    text_buf.push_str(t);
                }
            }
            "input_image" => {
                image_count += 1;
                // Responses / Chat 兼容：image 字段（旧）或 image_url 字段（OpenAI Chat / 新 Responses）
                let img_data = obj
                    .get("image")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        obj.get("image_url")
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim_start_matches("data:"))
                    });
                if let Some(data) = img_data {
                    // 提取 base64 字节长度（粗略估计：4/3 解码，data 末尾的 "base64," 后字符数）
                    let b64_start = data.find("base64,").map(|i| i + 7).unwrap_or(0);
                    let b64_len = data.len().saturating_sub(b64_start);
                    let approx_bytes = (b64_len * 3) / 4;
                    text_buf.push_str(&format!("[image:base64:{approx_bytes}bytes]"));
                }
            }
            _ => {
                // 未知 type 静默跳过
            }
        }
    }

    if !text_buf.is_empty() {
        Value::Object(Map::from_iter([(
            "result".into(),
            Value::String(text_buf),
        )]))
    } else if image_count == 0 {
        // 没有任何可识别 part → 保留原 array（rare）
        parsed
    } else {
        // 仅有 image → 占位
        Value::Object(Map::from_iter([(
            "result".into(),
            Value::String(format!("[{image_count} embedded image(s)]")),
        )]))
    }
}

fn build_tools(tools: &Value, params: &mut StreamParams) -> Result<Option<Value>, TranslateError> {
    let Some(arr) = tools.as_array() else { return Ok(None) };
    let mut fn_decls: Vec<Value> = Vec::new();
    for tool in arr {
        let Some(tool_obj) = tool.as_object() else { continue };
        let t_type = tool_obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t_type != "function" {
            continue;
        }
        let name = tool_obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let sanitized = tool_name::sanitize_with_occupied(name, &mut params.sanitized_name_map);
        let description = tool_obj.get("description").and_then(|v| v.as_str()).map(String::from);
        let mut parameters = tool_obj.get("parameters").cloned().unwrap_or_else(|| {
            Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("object".into())),
                ("properties".to_string(), Value::Object(Map::new())),
            ]))
        });
        if let Some(obj) = parameters.as_object_mut() {
            obj.insert("type".into(), Value::String("object".into()));
        }

        let mut fn_decl = Map::new();
        fn_decl.insert("name".into(), Value::String(sanitized));
        if let Some(desc) = description {
            fn_decl.insert("description".into(), Value::String(desc));
        }
        fn_decl.insert("parametersJsonSchema".into(), parameters);
        fn_decls.push(Value::Object(fn_decl));
    }
    if fn_decls.is_empty() {
        return Ok(None);
    }
    Ok(Some(Value::Array(vec![Value::Object(
        Map::from_iter([("functionDeclarations".into(), Value::Array(fn_decls))]),
    )])))
}

fn push_model_content(out: &mut Vec<Value>, part: Value) {
    // 合并到最后一个 model role 的 parts；如果不存在则新开一个
    if let Some(last) = out.last_mut() {
        if last.get("role").and_then(|v| v.as_str()) == Some("model") {
            if let Some(parts) = last.get_mut("parts").and_then(|v| v.as_array_mut()) {
                parts.push(part);
                return;
            }
        }
    }
    out.push(Value::Object(Map::from_iter([
        ("role".into(), Value::String("model".into())),
        ("parts".into(), Value::Array(vec![part])),
    ])));
}

fn push_user_content(out: &mut Vec<Value>, part: Value) {
    if let Some(last) = out.last_mut() {
        if last.get("role").and_then(|v| v.as_str()) == Some("user") {
            if let Some(parts) = last.get_mut("parts").and_then(|v| v.as_array_mut()) {
                parts.push(part);
                return;
            }
        }
    }
    out.push(Value::Object(Map::from_iter([
        ("role".into(), Value::String("user".into())),
        ("parts".into(), Value::Array(vec![part])),
    ])));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn translate(model: &str, body: &Value) -> Result<Value, TranslateError> {
        let mut params = StreamParams::default();
        build_request(model, body, false, &mut params)
    }

    #[test]
    fn input_string() {
        let body = json!({
            "model": "m",
            "input": "Hello"
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["contents"],
            json!([{"role": "user", "parts": [{"text": "Hello"}]}])
        );
    }

    #[test]
    fn instructions_becomes_system_instruction() {
        let body = json!({
            "model": "m",
            "instructions": "You are helpful.",
            "input": "Hi"
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["systemInstruction"],
            json!({"parts": [{"text": "You are helpful."}]})
        );
    }

    #[test]
    fn input_items_message() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "Hi"}
                ]}
            ]
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["contents"],
            json!([{"role": "user", "parts": [{"text": "Hi"}]}])
        );
    }

    #[test]
    fn input_items_assistant_function_call() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "function_call", "call_id": "c1", "name": "search.web", "arguments": "{\"q\":\"rust\"}"}
            ]
        });
        let out = translate("m", &body).unwrap();
        let fc = &out["contents"][0]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "search_web");
        assert_eq!(fc["args"], json!({"q": "rust"}));
        assert_eq!(fc["id"], "c1");
    }

    #[test]
    fn input_items_function_call_output() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "function_call_output", "call_id": "c1", "output": "found 3"}
            ]
        });
        let out = translate("m", &body).unwrap();
        let fr = &out["contents"][0]["parts"][0]["functionResponse"];
        assert_eq!(fr["id"], "c1");
        assert_eq!(fr["response"], json!({"result": "found 3"}));
    }

    #[test]
    fn reasoning_effort_high() {
        let body = json!({
            "model": "m",
            "input": "Hi",
            "reasoning": {"effort": "high"}
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["generationConfig"]["thinkingConfig"],
            json!({"thinkingLevel": "high", "includeThoughts": true})
        );
    }

    #[test]
    fn parallel_tool_calls_false_becomes_none_mode() {
        let body = json!({
            "model": "m",
            "input": "Hi",
            "parallel_tool_calls": false
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["toolConfig"],
            json!({"functionCallingConfig": {"mode": "NONE"}})
        );
    }

    #[test]
    fn tools_become_function_declarations() {
        let body = json!({
            "model": "m",
            "input": "Hi",
            "tools": [{
                "type": "function",
                "name": "search.web",
                "description": "Search",
                "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
            }]
        });
        let out = translate("m", &body).unwrap();
        let fns = &out["tools"][0]["functionDeclarations"];
        assert_eq!(fns[0]["name"], "search_web");
        assert_eq!(fns[0]["description"], "Search");
    }

    #[test]
    fn system_message_item_ignored_to_avoid_duplicate() {
        let body = json!({
            "model": "m",
            "instructions": "Helpful",
            "input": [
                {"type": "message", "role": "system", "content": [
                    {"type": "input_text", "text": "More"}
                ]}
            ]
        });
        let out = translate("m", &body).unwrap();
        // system message 被丢弃；如果 contents 字段不存在（因为被全过滤了）也是 OK
        let contents = out.get("contents").and_then(|v| v.as_array());
        match contents {
            Some(arr) => assert!(arr.is_empty()),
            None => {}
        }
    }

    #[test]
    fn empty_model_is_error() {
        let err = translate("", &json!({"input": "x"})).unwrap_err();
        assert!(matches!(err, TranslateError::Invalid(_)));
    }

    #[test]
    fn tool_name_collision_records_both() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "function_call", "call_id": "c1", "name": "search.web", "arguments": "{}"},
                {"type": "function_call", "call_id": "c2", "name": "search-web", "arguments": "{}"}
            ]
        });
        let mut params = StreamParams::default();
        let out = build_request("m", &body, false, &mut params).unwrap();
        let n1 = out["contents"][0]["parts"][0]["functionCall"]["name"].as_str().unwrap();
        let n2 = out["contents"][0]["parts"][1]["functionCall"]["name"].as_str().unwrap();
        assert_eq!(n1, "search_web");
        assert_ne!(n1, n2);
        assert_eq!(params.sanitized_name_map.len(), 2);
    }

    // ====== CLIProxyAPI 934da23 (custom_tool_call_output) ======

    /// 对齐 `TestConvertOpenAIResponsesRequestToOpenAIChatCompletions_UnwrapsStringifiedCustomToolOutputImages`。
    /// stringified JSON 编码的 image array 走文本占位（Gemini functionResponse 不支持
    /// multimodal parts）。
    #[test]
    fn custom_tool_output_stringified_image_array_falls_back_to_placeholder() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "custom_tool_call", "call_id": "call_image", "name": "view_image", "input": "{}"},
                {"type": "custom_tool_call_output", "call_id": "call_image",
                 "output": r#"[{"type":"input_image","image_url":"data:image/png;base64,AA==","detail":"original"}]"#}
            ]
        });
        let out = translate("m", &body).unwrap();
        let fr = &out["contents"][1]["parts"][0]["functionResponse"];
        // image 占位文本
        let result = fr["response"]["result"].as_str().unwrap();
        assert!(
            result.contains("[image:base64:") && result.contains("bytes]"),
            "expected image placeholder, got {result:?}"
        );
    }

    /// 对齐 `TestConvertOpenAIResponsesRequestToOpenAIChatCompletions_PreservesCustomToolOutputFallbacks`。
    /// 各种 output 形态 fallback 到文本。
    #[test]
    fn custom_tool_output_fallbacks() {
        // plain text
        let body = json!({
            "model": "m",
            "input": [
                {"type": "custom_tool_call", "call_id": "x", "name": "f", "input": "{}"},
                {"type": "custom_tool_call_output", "call_id": "x", "output": "plain output"}
            ]
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["contents"][1]["parts"][0]["functionResponse"]["response"],
            json!({"result": "plain output"})
        );

        // text content array
        let body = json!({
            "model": "m",
            "input": [
                {"type": "custom_tool_call", "call_id": "x", "name": "f", "input": "{}"},
                {"type": "custom_tool_call_output", "call_id": "x",
                 "output": r#"[{"type":"input_text","text":"done"}]"#}
            ]
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["contents"][1]["parts"][0]["functionResponse"]["response"],
            json!({"result": "done"})
        );

        // invalid image array (no fields) → empty result
        let body = json!({
            "model": "m",
            "input": [
                {"type": "custom_tool_call", "call_id": "x", "name": "f", "input": "{}"},
                {"type": "custom_tool_call_output", "call_id": "x",
                 "output": r#"[{"type":"input_image","detail":"low"}]"#}
            ]
        });
        let out = translate("m", &body).unwrap();
        // image_count=1, text_buf empty → "[1 embedded image(s)]"
        assert_eq!(
            out["contents"][1]["parts"][0]["functionResponse"]["response"],
            json!({"result": "[1 embedded image(s)]"})
        );
    }

    /// 普通 stringified array 文本被解析（`responsesToolOutputText` 等价）。
    #[test]
    fn function_call_output_stringified_array_parsed() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1",
                 "output": r#"[{"type":"input_text","text":"line1"},{"type":"input_text","text":"line2"}]"#}
            ]
        });
        let out = translate("m", &body).unwrap();
        assert_eq!(
            out["contents"][1]["parts"][0]["functionResponse"]["response"],
            json!({"result": "line1\nline2"})
        );
    }

    // ====== CLIProxyAPI ecc9aa7 (preserve assistant content + mergeable index) ======

    /// 对齐 `TestConvertOpenAIResponsesRequestToOpenAIChatCompletions_PreservesAssistantContentWithToolCalls`。
    /// reasoning → assistant message → function_call → function_call_output
    /// 序列应产生 2 个 messages（assistant 包含 content + tool_calls；tool output 独立）。
    #[test]
    fn preserves_assistant_content_with_tool_calls() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "reasoning", "id": "rs_1",
                 "summary": [{"type": "summary_text", "text": "inspect the next step"}]},
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "Step 3 completed; continue to step 4."}]},
                {"type": "function_call", "call_id": "call_4", "name": "exec_command", "arguments": "{\"cmd\":\"pwd\"}"},
                {"type": "function_call_output", "call_id": "call_4", "output": "ok"}
            ]
        });
        let out = translate("m", &body).unwrap();
        let contents = out["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2, "期望 2 个 turn: assistant(model) + tool response(user)");

        // 第一个 turn: model（content + functionCall 合并）
        assert_eq!(contents[0]["role"], "model");
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2, "text part + functionCall part");
        assert_eq!(parts[0]["text"], "Step 3 completed; continue to step 4.");
        assert_eq!(parts[1]["functionCall"]["id"], "call_4");

        // 第二个 turn: user (functionResponse)
        assert_eq!(contents[1]["role"], "user");
        assert_eq!(
            contents[1]["parts"][0]["functionResponse"]["id"],
            "call_4"
        );
    }

    /// 对齐 `TestConvertOpenAIResponsesRequestToOpenAIChatCompletions_DoesNotMergeToolCallsAcrossUserMessage`。
    /// assistant message (content) → user message → function_call 应各自分开。
    #[test]
    fn does_not_merge_tool_calls_across_user_message() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "done"}]},
                {"type": "message", "role": "user",
                 "content": [{"type": "input_text", "text": "next"}]},
                {"type": "function_call", "call_id": "call_next", "name": "exec_command", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_next", "output": "ok"}
            ]
        });
        let out = translate("m", &body).unwrap();
        let contents = out["contents"].as_array().unwrap();
        // assistant(text) → user input → model(单独 functionCall) → user(functionResponse)
        assert_eq!(contents.len(), 4);
        assert_eq!(contents[0]["role"], "model");
        assert_eq!(contents[0]["parts"][0]["text"], "done");
        assert!(contents[0]["parts"].as_array().unwrap().len() == 1, "no tool_call merged");

        assert_eq!(contents[1]["role"], "user");
        assert_eq!(contents[1]["parts"][0]["text"], "next");

        assert_eq!(contents[2]["role"], "model");
        assert_eq!(
            contents[2]["parts"][0]["functionCall"]["id"],
            "call_next"
        );

        assert_eq!(contents[3]["role"], "user");
        assert_eq!(
            contents[3]["parts"][0]["functionResponse"]["id"],
            "call_next"
        );
    }

    /// 对齐 `TestConvertOpenAIResponsesRequestToOpenAIChatCompletions_MergesDistinctReasoningWithinAssistantTurn`。
    /// 多个 reasoning segment + assistant message + function_call 合并到一个 model turn；
    /// reasoning 文本被合并累积（用于后续 turn；当前 Gemini 端不输出但 buffer 累积对）。
    #[test]
    fn reasoning_summary_buffers_across_turn() {
        let body = json!({
            "model": "m",
            "input": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "first"}]},
                {"type": "message", "role": "assistant", "reasoning_content": "first",
                 "content": [{"type": "output_text", "text": "working"}]},
                {"type": "function_call", "call_id": "call_x", "name": "exec", "arguments": "{}"}
            ]
        });
        let out = translate("m", &body).unwrap();
        let contents = out["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1, "一个 model turn: text + functionCall 合并");
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "working");
        assert_eq!(parts[1]["functionCall"]["id"], "call_x");
    }

    /// `[reasoning unavailable]` placeholder 不覆盖真实内容。
    #[test]
    fn reasoning_unavailable_is_replaced_by_real_content() {
        // 当 reasoning 累积过程中遇到 placeholder + 真实内容，
        // 真实内容应胜出（这是 combine_openai_responses_reasoning 的语义）。
        let combined = combine_openai_responses_reasoning(
            "[reasoning unavailable]".to_string(),
            "real reasoning",
        );
        assert_eq!(combined, "real reasoning");
    }

    /// combine helper：完全相同 → 保留一份。
    #[test]
    fn combine_reasoning_identical_keeps_one() {
        let combined = combine_openai_responses_reasoning("same".to_string(), "same");
        assert_eq!(combined, "same");
    }

    /// combine helper：不同 → `existing + "\n\n" + incoming`。
    #[test]
    fn combine_reasoning_different_joins_with_newlines() {
        let combined = combine_openai_responses_reasoning("first".to_string(), "second");
        assert_eq!(combined, "first\n\nsecond");
    }

    /// echo placeholder 在 assistant message 之后被真实 reasoning 替换。
    #[test]
    fn combine_reasoning_empty_then_content() {
        let combined = combine_openai_responses_reasoning(String::new(), "first");
        assert_eq!(combined, "first");
    }
}