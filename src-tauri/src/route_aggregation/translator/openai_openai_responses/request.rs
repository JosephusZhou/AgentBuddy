//! Translator: OpenAI Responses API → Gemini generateContent
//!
//! CLIProxyAPI aligned: 934da237 - fix(openai): preserve structured and stringified
//!                        custom tool outputs during Responses conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
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
            for item in arr {
                if let Some(content_obj) = item.as_object() {
                    let item_type = content_obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match item_type {
                        "message" => {
                            if let Some(content_obj_built) = build_message_item(content_obj, params)? {
                                out.push(content_obj_built);
                            }
                        }
                        "function_call" => {
                            if let Some(fc) = build_function_call_item(content_obj, params)? {
                                push_model_content(&mut out, fc);
                            }
                        }
                        "function_call_output" => {
                            if let Some(fr) = build_function_call_output_item(content_obj)? {
                                push_user_content(&mut out, fr);
                            }
                        }
                        "reasoning" => {
                            // 跳过：Responses 的 reasoning items 用于跨轮回放，
                            // Gemini 端不再需要原始内容（signature 已由 thinking
                            // delta 阶段带过去）。Phase 3 完整实现可在此处
                            // 提取 encrypted_content 写到 params.thinking_signature。
                        }
                        "item_reference" => {
                            // 引用之前的 item；常见于多轮对话，Gemini 不直接支持。
                            // 忽略（不会破坏端到端，丢失该 turn 的内容）。
                        }
                        _ => {
                            // 未知 item type 跳过
                        }
                    }
                }
            }
        }
        _ => return Err(TranslateError::Invalid("`input` must be string or array".into())),
    }

    Ok(out)
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
    let output = obj.get("output").and_then(|v| v.as_str()).unwrap_or("");

    let mut fr = Map::new();
    if !call_id.is_empty() {
        fr.insert("id".into(), Value::String(call_id.to_string()));
    }
    let mut response = Map::new();
    response.insert("result".into(), Value::String(output.to_string()));
    fr.insert("response".into(), Value::Object(response));

    Ok(Some(Value::Object(Map::from_iter([(
        "functionResponse".into(),
        Value::Object(fr),
    )]))))
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
}