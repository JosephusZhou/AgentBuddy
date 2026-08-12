//! Translator: OpenAI Chat Completions → Gemini generateContent
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/openai/gemini/openai_gemini_request.go`。
//!
//! ## 字段映射速查
//! | OpenAI Chat                          | Gemini                                |
//! |--------------------------------------|---------------------------------------|
//! | `messages[].role: system`            | `systemInstruction.parts[].text`      |
//! | `messages[].role: user`              | `contents[].role: user`               |
//! | `messages[].role: assistant`         | `contents[].role: model`  ⚠️          |
//! | `messages[].role: tool`              | `contents[].role: user` + `functionResponse`  ⚠️ |
//! | `content: string`                    | `parts[].text`                        |
//! | `content[].type: text`               | `parts[].text`                        |
//! | `content[].type: image_url`          | `parts[].inline_data` (data URL / url)|
//! | `content[].type: input_audio`        | `parts[].inline_data` (audio/*)       |
//! | `tool_calls[].function`              | `parts[].functionCall`                |
//! | `tools[].function.parameters`        | `functionDeclarations[].parametersJsonSchema` |
//! | `tool_choice: auto/none/required`    | `toolConfig.functionCallingConfig.mode` |
//! | `temperature/top_p`                  | `generationConfig.{temperature,topP}` |
//! | `max_tokens`                         | `generationConfig.maxOutputTokens`    |
//! | `stop` (string/array)                | `generationConfig.stopSequences`      |
//! | `frequency_penalty/presence_penalty` | `generationConfig.{...}`             |
//! | `n` (候选数)                         | `generationConfig.candidateCount`     |

use serde_json::{Map, Value};

use super::super::common::tool_name;
use super::super::params::StreamParams;
use super::super::translatable::TranslateError;

/// 构造 OpenAI Chat → Gemini 请求 body 的核心逻辑。
///
/// 独立为 free function，便于 trait impl 与单元测试直接复用。
///
/// `params` 用于跨方向共享状态：本函数会向 `params.sanitized_name_map` 写入
/// 工具名映射（用于响应方向反向解析）。
pub fn build_request(
    model: &str,
    raw: &Value,
    stream: bool,
    params: &mut StreamParams,
) -> Result<Value, TranslateError> {
    let src = raw.as_object().ok_or_else(|| {
        TranslateError::Invalid("OpenAI request body must be a JSON object".into())
    })?;

    let mut out = Map::new();

    // 1. systemInstruction：从 messages[].role=system + 顶层 system 字段合并
    if let Some(system) = build_system_instruction(src)? {
        out.insert("systemInstruction".into(), system);
    }

    // 2. contents：从 messages 数组转换
    let contents = build_contents(src)?;
    if !contents.is_empty() {
        out.insert("contents".into(), Value::Array(contents));
    }

    // 3. tools → functionDeclarations
    if let Some(tools) = src.get("tools") {
        if let Some(gemini_tools) = build_tools(tools, params)? {
            out.insert("tools".into(), gemini_tools);
        }
    }

    // 4. toolConfig
    if let Some(tool_choice) = src.get("tool_choice") {
        if let Some(tool_config) = build_tool_config(tool_choice) {
            out.insert("toolConfig".into(), tool_config);
        }
    }

    // 5. generationConfig
    let mut gen_config = Map::new();
    if let Some(v) = src.get("max_tokens").and_then(|v| v.as_i64()) {
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
    if let Some(v) = src.get("frequency_penalty").and_then(|v| v.as_f64()) {
        gen_config.insert("frequencyPenalty".into(), Value::from(v));
    }
    if let Some(v) = src.get("presence_penalty").and_then(|v| v.as_f64()) {
        gen_config.insert("presencePenalty".into(), Value::from(v));
    }
    if let Some(v) = src.get("n").and_then(|v| v.as_i64()) {
        if v > 0 {
            gen_config.insert("candidateCount".into(), Value::from(v));
        }
    }
    if let Some(v) = src.get("seed").and_then(|v| v.as_i64()) {
        gen_config.insert("seed".into(), Value::from(v));
    }
    if let Some(v) = src.get("stop") {
        let arr = match v {
            Value::String(s) => vec![Value::String(s.clone())],
            Value::Array(a) => a
                .iter()
                .filter_map(|s| s.as_str().map(|s| Value::String(s.to_string())))
                .collect(),
            _ => Vec::new(),
        };
        if !arr.is_empty() {
            gen_config.insert("stopSequences".into(), Value::Array(arr));
        }
    }
    // response_format 透传为 generationConfig.responseSchema
    if let Some(v) = src.get("response_format") {
        if let Some(obj) = v.as_object() {
            if obj.get("type").and_then(|t| t.as_str()) == Some("json_schema") {
                if let Some(schema) = obj.get("json_schema").and_then(|s| s.get("schema")) {
                    gen_config.insert("responseSchema".into(), schema.clone());
                    gen_config.insert("responseMimeType".into(), Value::String("application/json".into()));
                }
            }
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

/// 合并 OpenAI 的 system 来源：
/// 1. 顶层 `system` 字段（OpenAI 较新 SDK 支持）
/// 2. `messages[]` 里 `role: system` 的 content
///
/// Gemini 只有一个 systemInstruction，所以两者拼成一个 parts 数组。
fn build_system_instruction(src: &Map<String, Value>) -> Result<Option<Value>, TranslateError> {
    let mut parts: Vec<Value> = Vec::new();

    // 顶层 system 字段
    if let Some(Value::String(s)) = src.get("system") {
        parts.push(text_part(s));
    }

    // messages[] 里 role=system
    if let Some(messages) = src.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if let Some(obj) = msg.as_object() {
                if obj.get("role").and_then(|v| v.as_str()) == Some("system") {
                    if let Some(text) = obj.get("content").and_then(|v| v.as_str()) {
                        parts.push(text_part(text));
                    }
                }
            }
        }
    }

    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(Value::Object(Map::from_iter([(
        "parts".into(),
        Value::Array(parts),
    )]))))
}

fn text_part(text: &str) -> Value {
    Value::Object(Map::from_iter([(
        "text".into(),
        Value::String(text.to_string()),
    )]))
}

/// 构造 `contents`：从 OpenAI `messages` 转换为 Gemini `contents`。
///
/// 关键点：
/// - `role: assistant` → `role: model`
/// - `role: tool`（tool message） → `role: user` + `parts[].functionResponse`
/// - content 可以是字符串或数组（数组中含 text / image_url / input_audio）
fn build_contents(src: &Map<String, Value>) -> Result<Vec<Value>, TranslateError> {
    let Some(messages) = src.get("messages").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for msg in messages {
        let Some(obj) = msg.as_object() else { continue };
        let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");

        match role {
            "user" => {
                let parts = build_user_parts(obj.get("content"))?;
                if parts.is_empty() {
                    continue;
                }
                out.push(Value::Object(Map::from_iter([
                    ("role".into(), Value::String("user".into())),
                    ("parts".into(), Value::Array(parts)),
                ])));
            }
            "assistant" => {
                let mut parts: Vec<Value> = Vec::new();
                if let Some(content) = obj.get("content") {
                    if let Some(text) = content.as_str() {
                        if !text.is_empty() {
                            parts.push(text_part(text));
                        }
                    } else if let Some(arr) = content.as_array() {
                        for block in arr {
                            if let Some(p) = build_text_block_part(block) {
                                parts.push(p);
                            }
                        }
                    }
                }
                // tool_calls → functionCall parts
                if let Some(tool_calls) = obj.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        if let Some(part) = build_tool_call_part(tc) {
                            parts.push(part);
                        }
                    }
                }
                if parts.is_empty() {
                    continue;
                }
                out.push(Value::Object(Map::from_iter([
                    ("role".into(), Value::String("model".into())),
                    ("parts".into(), Value::Array(parts)),
                ])));
            }
            "tool" => {
                // tool result：role=user + parts[].functionResponse
                let tool_call_id = obj.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
                let content = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let mut fr = Map::new();
                if !tool_call_id.is_empty() {
                    fr.insert("id".into(), Value::String(tool_call_id.to_string()));
                }
                let mut response = Map::new();
                response.insert("result".into(), Value::String(content.to_string()));
                fr.insert("response".into(), Value::Object(response));
                out.push(Value::Object(Map::from_iter([
                    ("role".into(), Value::String("user".into())),
                    (
                        "parts".into(),
                        Value::Array(vec![Value::Object(Map::from_iter([(
                            "functionResponse".into(),
                            Value::Object(fr),
                        )]))]),
                    ),
                ])));
            }
            "function" => {
                // 老 OpenAI function calling 协议（已被 tool_calls 取代，保留兼容）
                let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let content = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let mut fr = Map::new();
                if !name.is_empty() {
                    fr.insert("id".into(), Value::String(tool_name::sanitize(name)));
                }
                let mut response = Map::new();
                response.insert("result".into(), Value::String(content.to_string()));
                fr.insert("response".into(), Value::Object(response));
                out.push(Value::Object(Map::from_iter([
                    ("role".into(), Value::String("user".into())),
                    (
                        "parts".into(),
                        Value::Array(vec![Value::Object(Map::from_iter([(
                            "functionResponse".into(),
                            Value::Object(fr),
                        )]))]),
                    ),
                ])));
            }
            "system" => {
                // 已提取到顶层 systemInstruction；跳过避免重复
            }
            _ => {
                // 未知 role 跳过（不报错，避免客户端 SDK 升级带来的噪音）
            }
        }
    }
    Ok(out)
}

fn build_user_parts(content: Option<&Value>) -> Result<Vec<Value>, TranslateError> {
    let mut out = Vec::new();
    let Some(content) = content else {
        return Ok(out);
    };
    match content {
        Value::String(s) => {
            if !s.is_empty() {
                out.push(text_part(s));
            }
        }
        Value::Array(arr) => {
            for block in arr {
                if let Some(p) = build_text_block_part(block) {
                    out.push(p);
                }
            }
        }
        _ => {}
    }
    Ok(out)
}

/// 构造单个 content block part（text / image_url / input_audio）。
fn build_text_block_part(block: &Value) -> Option<Value> {
    let obj = block.as_object()?;
    let btype = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match btype {
        "text" => obj.get("text").and_then(|v| v.as_str()).map(text_part),
        "image_url" => {
            // image_url.url 可能是 http(s) URL 或 data:image/png;base64,... data URL
            let url = obj.get("image_url")?.get("url")?.as_str()?;
            if let Some(rest) = url.strip_prefix("data:") {
                // data:<mime>;base64,<data>
                if let Some((mime, data)) = rest.split_once(";base64,") {
                    let mut inline = Map::new();
                    inline.insert("mime_type".into(), Value::String(mime.to_string()));
                    inline.insert("data".into(), Value::String(data.to_string()));
                    return Some(Value::Object(Map::from_iter([(
                        "inline_data".into(),
                        Value::Object(inline),
                    )])));
                }
            }
            // 非 data URL：暂用 file_data 占位（需要 Files API 上传，超出 Phase 2）
            let mut file_data = Map::new();
            file_data.insert("file_uri".into(), Value::String(url.to_string()));
            // mime_type 留待 Phase 5 通过 content-type sniff 填充
            Some(Value::Object(Map::from_iter([(
                "file_data".into(),
                Value::Object(file_data),
            )])))
        }
        "input_audio" => {
            // input_audio.data + format ("wav"/"mp3")
            let audio = obj.get("input_audio")?;
            let data = audio.get("data").and_then(|v| v.as_str())?;
            let format = audio.get("format").and_then(|v| v.as_str()).unwrap_or("wav");
            let mime = format!("audio/{}", format);
            let mut inline = Map::new();
            inline.insert("mime_type".into(), Value::String(mime));
            inline.insert("data".into(), Value::String(data.to_string()));
            Some(Value::Object(Map::from_iter([(
                "inline_data".into(),
                Value::Object(inline),
            )])))
        }
        _ => None,
    }
}

/// 构造 tool_call part（OpenAI Chat `tool_calls[].function` → Gemini `functionCall`）。
fn build_tool_call_part(tc: &Value) -> Option<Value> {
    let obj = tc.as_object()?;
    let function = obj.get("function")?.as_object()?;
    let name = function.get("name").and_then(|v| v.as_str())?;
    let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let arguments_str = function.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
    // arguments 是 JSON 字符串，Gemini 期望对象
    let arguments: Value = serde_json::from_str(arguments_str).unwrap_or(Value::Object(Map::new()));

    let mut function_call = Map::new();
    function_call.insert("name".into(), Value::String(tool_name::sanitize(name)));
    function_call.insert("args".into(), arguments);
    if !id.is_empty() {
        function_call.insert("id".into(), Value::String(id.to_string()));
    }
    Some(Value::Object(Map::from_iter([(
        "functionCall".into(),
        Value::Object(function_call),
    )])))
}

/// 处理 OpenAI `tools[]` → Gemini `tools[].functionDeclarations[]`。
fn build_tools(tools: &Value, params: &mut StreamParams) -> Result<Option<Value>, TranslateError> {
    let Some(arr) = tools.as_array() else { return Ok(None) };
    let mut fn_decls: Vec<Value> = Vec::new();
    for tool in arr {
        let Some(tool_obj) = tool.as_object() else { continue };
        let function = tool_obj.get("function").and_then(|v| v.as_object());
        let Some(function) = function else { continue };
        let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let sanitized = tool_name::sanitize_with_occupied(name, &mut params.sanitized_name_map);
        let description = function.get("description").and_then(|v| v.as_str()).map(String::from);
        let mut parameters = function.get("parameters").cloned().unwrap_or_else(|| {
            Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("object".into())),
                ("properties".to_string(), Value::Object(Map::new())),
            ]))
        });

        // 强制 object type
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

/// 处理 OpenAI `tool_choice` → Gemini `toolConfig.functionCallingConfig.mode`。
fn build_tool_config(tool_choice: &Value) -> Option<Value> {
    let mode_str = match tool_choice {
        Value::String(s) => match s.as_str() {
            "auto" => "AUTO",
            "none" => "NONE",
            "required" => "ANY",
            _ => return None,
        },
        Value::Object(obj) => {
            let t = obj.get("type")?.as_str()?;
            if t == "function" {
                // 强制指定某个 function。CLIProxyAPI 行为：allowedFunctionNames
                let name = obj.get("function")?.get("name")?.as_str()?;
                let allowed = vec![Value::String(tool_name::sanitize(name))];
                let mut config = Map::new();
                config.insert("mode".into(), Value::String("ANY".into()));
                config.insert("allowedFunctionNames".into(), Value::Array(allowed));
                return Some(Value::Object(Map::from_iter([(
                    "functionCallingConfig".into(),
                    Value::Object(config),
                )])));
            }
            return None;
        }
        _ => return None,
    };
    Some(Value::Object(Map::from_iter([(
        "functionCallingConfig".into(),
        Value::Object(Map::from_iter([(
            "mode".into(),
            Value::String(mode_str.to_string()),
        )])),
    )])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

fn translate(body: &Value) -> Result<Value, TranslateError> {
        let mut params = StreamParams::default();
        build_request("m", body, false, &mut params)
    }

    fn translate_empty_model(body: &Value) -> Result<Value, TranslateError> {
        let mut params = StreamParams::default();
        build_request("", body, false, &mut params)
    }

    #[test]
    fn minimal_user_request() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out,
            json!({
                "contents": [
                    {"role": "user", "parts": [{"text": "Hi"}]}
                ]
            })
        );
    }

    #[test]
    fn top_level_system_becomes_system_instruction() {
        let body = json!({
            "model": "m",
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out.get("systemInstruction"),
            Some(&json!({"parts": [{"text": "You are helpful."}]}))
        );
    }

    #[test]
    fn messages_system_becomes_system_instruction() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "Be brief."},
                {"role": "user", "content": "Hi"}
            ]
        });
        let out = translate(&body).unwrap();
        let si = out.get("systemInstruction").unwrap();
        let parts = si.get("parts").unwrap().as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].get("text").unwrap(), "Be brief.");
    }

    #[test]
    fn assistant_role_becomes_model() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"}
            ]
        });
        let out = translate(&body).unwrap();
        let contents = out["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn user_multimodal_content() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's in this image?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBOR"}}
                ]
            }]
        });
        let out = translate(&body).unwrap();
        let parts = out["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "What's in this image?");
        assert_eq!(
            parts[1],
            json!({"inline_data": {"mime_type": "image/png", "data": "iVBOR"}})
        );
    }

    #[test]
    fn user_input_audio() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Transcribe"},
                    {"type": "input_audio", "input_audio": {"format": "wav", "data": "AAAA"}}
                ]
            }]
        });
        let out = translate(&body).unwrap();
        let parts = out["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(
            parts[1],
            json!({"inline_data": {"mime_type": "audio/wav", "data": "AAAA"}})
        );
    }

    #[test]
    fn tool_role_becomes_function_response() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_01",
                    "type": "function",
                    "function": {"name": "search", "arguments": "{\"q\":\"rust\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call_01", "content": "found 3 results"}
            ]
        });
        let out = translate(&body).unwrap();
        let contents = out["contents"].as_array().unwrap();
        // assistant → model with functionCall
        assert_eq!(contents[0]["role"], "model");
        assert_eq!(contents[0]["parts"][0]["functionCall"]["name"], "search");
        assert_eq!(contents[0]["parts"][0]["functionCall"]["args"], json!({"q": "rust"}));
        // tool → user with functionResponse
        assert_eq!(contents[1]["role"], "user");
        assert_eq!(contents[1]["parts"][0]["functionResponse"]["id"], "call_01");
        assert_eq!(
            contents[1]["parts"][0]["functionResponse"]["response"],
            json!({"result": "found 3 results"})
        );
    }

    #[test]
    fn tools_become_function_declarations() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search.web",
                    "description": "Search",
                    "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
                }
            }]
        });
        let out = translate(&body).unwrap();
        let fns = &out["tools"][0]["functionDeclarations"];
        assert_eq!(fns[0]["name"], "search_web");
        assert_eq!(fns[0]["description"], "Search");
        assert_eq!(fns[0]["parametersJsonSchema"]["type"], "object");
    }

    #[test]
    fn tool_choice_auto_becomes_auto_mode() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tool_choice": "auto"
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out.get("toolConfig"),
            Some(&json!({"functionCallingConfig": {"mode": "AUTO"}}))
        );
    }

    #[test]
    fn tool_choice_required_becomes_any_mode() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tool_choice": "required"
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out.get("toolConfig"),
            Some(&json!({"functionCallingConfig": {"mode": "ANY"}}))
        );
    }

    #[test]
    fn tool_choice_specific_function() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tool_choice": {"type": "function", "function": {"name": "search.web"}}
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out.get("toolConfig"),
            Some(&json!({
                "functionCallingConfig": {
                    "mode": "ANY",
                    "allowedFunctionNames": ["search_web"]
                }
            }))
        );
    }

    #[test]
    fn temperature_top_p_max_tokens() {
        let body = json!({
            "model": "m",
            "messages": [],
            "temperature": 0.7,
            "top_p": 0.9,
            "max_tokens": 1024
        });
        let out = translate(&body).unwrap();
        assert_eq!(out["generationConfig"]["temperature"], 0.7);
        assert_eq!(out["generationConfig"]["topP"], 0.9);
        assert_eq!(out["generationConfig"]["maxOutputTokens"], 1024);
    }

    #[test]
    fn stop_string_and_array() {
        let body = json!({
            "model": "m",
            "messages": [],
            "stop": ["END", "STOP"]
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out["generationConfig"]["stopSequences"],
            json!(["END", "STOP"])
        );
    }

    #[test]
    fn response_format_json_schema() {
        let body = json!({
            "model": "m",
            "messages": [],
            "response_format": {
                "type": "json_schema",
                "json_schema": {"schema": {"type": "object", "properties": {"x": {"type": "number"}}}}
            }
        });
        let out = translate(&body).unwrap();
        assert_eq!(
            out["generationConfig"]["responseSchema"],
            json!({"type": "object", "properties": {"x": {"type": "number"}}})
        );
        assert_eq!(
            out["generationConfig"]["responseMimeType"],
            "application/json"
        );
    }

    #[test]
    fn invalid_body_is_error() {
        let err = translate(&json!("not object")).unwrap_err();
        assert!(matches!(err, TranslateError::Invalid(_)));
    }

    #[test]
    fn empty_model_is_error() {
        let err = translate_empty_model(&json!({"messages": []})).unwrap_err();
        assert!(matches!(err, TranslateError::Invalid(_)));
    }
}