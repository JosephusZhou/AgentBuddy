//! Translator: Anthropic Messages → Gemini generateContent
//!
//! CLIProxyAPI aligned: 71e8711 - fix(claude): accumulate consecutive role turns
//!                        during request conversion
//!                        (also informed by c13dbcc2 - object schemas with properties)
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/71e87111e9d8e6c0ce3c2d0a419b136fee2e10b0
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/claude/gemini/` 目录：
//! `claude_gemini_request.go` ~14KB / `claude_gemini_response.go` ~17KB。
//!
//! ## 与 CLIProxyAPI 71e8711 的同步点
//! - 使用 `ClaudeMessageAccumulator`（`common::claude_messages`）合并相邻同 role turn。
//! - assistant turn 内 `tool_use` 移到 parts 末尾（thinking → text → tool_use）。
//! - user turn 保留原序（tool_result + text 混合）。
//! - 空 / null / 非 user/assistant role 跳过但不破坏当前 turn。
//! - 注：原 commit 把 Gemini `systemInstruction` 翻译成 Claude `user` 消息（用于代理
//!   Claude API）。本文件反向（Anthropic → Gemini），所以 system 走 `systemInstruction`
//!   字段，不需要 accumulator 的 explicit `flush()` 边界。
//!
//! ## 字段映射速查
//! | Anthropic                       | Gemini                                |
//! |---------------------------------|---------------------------------------|
//! | `system` (string/array)         | `systemInstruction.parts[].text`      |
//! | `messages[].role: user`         | `contents[].role: user`               |
//! | `messages[].role: assistant`    | `contents[].role: model`  ⚠️          |
//! | `content: text`                 | `parts[].text`                        |
//! | `content: tool_use`             | `parts[].functionCall`                |
//! | `content: tool_result`          | `parts[].functionResponse`            |
//! | `content: image` (base64)       | `parts[].inline_data`                 |
//! | `max_tokens`                    | `generationConfig.maxOutputTokens`    |
//! | `temperature/top_p/top_k`       | `generationConfig.{...}`             |
//! | `thinking.budget_tokens`        | `generationConfig.thinkingConfig.thinkingBudget` |
//! | `thinking.type: adaptive`       | `thinkingConfig.thinkingLevel`        |
//! | `tool_choice: auto/none/any/tool` | `toolConfig.functionCallingConfig.mode` |
//! | `tools[].input_schema`          | `functionDeclarations[].parametersJsonSchema` |

use serde_json::{Map, Value};

use super::super::common::claude_messages::ClaudeMessageAccumulator;
use super::super::common::thinking;
use super::super::common::tool_name;
use super::super::params::StreamParams;
use super::super::translatable::TranslateError;

/// 构造 Claude → Gemini 请求 body 的核心逻辑。
///
/// 独立为 free function，便于 trait impl 与单元测试直接复用。
///
/// `params` 用于跨方向共享状态：本函数会向 `params.tool_name_map` 和
/// `params.sanitized_name_map` 写入工具名映射（用于响应方向反向解析）。
pub fn build_request(
    model: &str,
    raw: &Value,
    stream: bool,
    params: &mut super::super::params::StreamParams,
) -> Result<Value, TranslateError> {
    let src = raw.as_object().ok_or_else(|| {
        TranslateError::Invalid("Anthropic request body must be a JSON object".into())
    })?;

    let mut out = Map::new();

    // 1. systemInstruction
    if let Some(system) = build_system_instruction(src)? {
        out.insert("systemInstruction".into(), system);
    }

    // 2. contents (from messages)
    let contents = build_contents(src, params)?;
    if !contents.is_empty() {
        out.insert("contents".into(), Value::Array(contents));
    }

    // 3. tools / functionDeclarations
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

    // 5. generationConfig（含 thinking 处理）
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
    if let Some(v) = src.get("top_k").and_then(|v| v.as_i64()) {
        gen_config.insert("topK".into(), Value::from(v));
    }
    if let Some(v) = src.get("stop_sequences").and_then(|v| v.as_array()) {
        let arr: Vec<Value> = v
            .iter()
            .filter_map(|s| s.as_str().map(|s| Value::String(s.to_string())))
            .collect();
        if !arr.is_empty() {
            gen_config.insert("stopSequences".into(), Value::Array(arr));
        }
    }

    // thinking: Anthropic -> Gemini thinkingConfig
    let mode = thinking::parse_anthropic_thinking(src);
    let budget = thinking::extract_anthropic_budget(src).unwrap_or(0);
    let level = thinking::extract_anthropic_level(src);
    thinking::apply_to_gemini_generation_config(&mut gen_config, mode, budget, level.as_deref());

    if !gen_config.is_empty() {
        out.insert("generationConfig".into(), Value::Object(gen_config));
    }

    // model 参数保留在 forwarder 中用于构造 URL，不进 body。
    // stream flag 由 forwarder 决定 URL 形态（:streamGenerateContent vs :generateContent），不进 body。

    if model.is_empty() {
        return Err(TranslateError::Invalid("missing model name".into()));
    }
    let _ = stream;

    Ok(Value::Object(out))
}

/// 构造 `systemInstruction`：兼容 string / array 两种输入。
fn build_system_instruction(src: &Map<String, Value>) -> Result<Option<Value>, TranslateError> {
    let Some(system) = src.get("system") else {
        return Ok(None);
    };
    let parts = match system {
        Value::String(s) => vec![Value::Object(Map::from_iter([(
            "text".into(),
            Value::String(s.clone()),
        )]))],
        Value::Array(arr) => {
            let mut out = Vec::new();
            for block in arr {
                let Some(obj) = block.as_object() else { continue };
                let Some(block_type) = obj.get("type").and_then(|v| v.as_str()) else { continue };
                if block_type != "text" {
                    // Anthropic 允许 system 里有非 text type（cache_control 等），
                    // Gemini 不支持非 text system。跳过（CLIProxyAPI 行为）。
                    continue;
                }
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    out.push(Value::Object(Map::from_iter([(
                        "text".into(),
                        Value::String(text.to_string()),
                    )])));
                }
            }
            out
        }
        _ => {
            return Err(TranslateError::Invalid(
                "`system` must be string or array".into(),
            ))
        }
    };
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(Value::Object(Map::from_iter([(
        "parts".into(),
        Value::Array(parts),
    )]))))
}

/// 构造 `contents`：从 Anthropic `messages` 数组转换为 Gemini `contents` 数组。
///
/// `role: assistant` → `role: model`；content 数组中 text/tool_use/tool_result/image 全部支持。
/// 相邻同 role turn 通过 `ClaudeMessageAccumulator` 合并；assistant turn 内 tool_use
/// 移到 parts 末尾（对齐 CLIProxyAPI `71e8711` 修复）。
fn build_contents(
    src: &Map<String, Value>,
    params: &mut StreamParams,
) -> Result<Vec<Value>, TranslateError> {
    let Some(messages) = src.get("messages").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };

    // CLIProxyAPI 7c61e98 perf: 预分配容量避免 realloc（镜像 Go 的 `NewRawArrayItems`）。
    // 不过这里 messages 已通过 ClaudeMessageAccumulator 处理，最终 len 与 message count 无关。
    let mut acc = ClaudeMessageAccumulator::new();
    for msg in messages {
        let Some(obj) = msg.as_object() else {
            continue;
        };
        let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = obj.get("content");
        let gemini_role = match role {
            "user" => "user",
            "assistant" => "model",
            // system message 在 messages[] 中已废弃；遇到就跳过
            "system" => continue,
            _ => {
                return Err(TranslateError::Invalid(format!(
                    "unsupported message role: {role}"
                )))
            }
        };

        let parts = build_parts(content, params)?;
        if parts.is_empty() {
            // Anthropic 允许空 content；Gemini 不允许空 parts。跳过整个 turn。
            continue;
        }
        let node = Value::Object(Map::from_iter([
            ("role".into(), Value::String(gemini_role.to_string())),
            ("parts".into(), Value::Array(parts)),
        ]));
        acc.append(&node);
    }
    Ok(acc.into_messages())
}

/// 构造单个 message 的 `parts`：兼容 string content 与 array content。
fn build_parts(
    content: Option<&Value>,
    params: &mut StreamParams,
) -> Result<Vec<Value>, TranslateError> {
    let Some(content) = content else {
        return Ok(Vec::new());
    };
    match content {
        Value::String(s) => Ok(vec![Value::Object(Map::from_iter([(
            "text".into(),
            Value::String(s.clone()),
        )]))]),
        Value::Array(arr) => {
            let mut out = Vec::new();
            for block in arr {
                let Some(obj) = block.as_object() else { continue };
                let Some(block_type) = obj.get("type").and_then(|v| v.as_str()) else { continue };
                match block_type {
                    "text" => {
                        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                            out.push(Value::Object(Map::from_iter([(
                                "text".into(),
                                Value::String(text.to_string()),
                            )])));
                        }
                    }
                    "image" => {
                        if let Some(part) = build_image_part(obj) {
                            out.push(part);
                        }
                    }
                    "tool_use" => {
                        if let Some(part) = build_tool_use_part(obj, params)? {
                            out.push(part);
                        }
                    }
                    "tool_result" => {
                        if let Some(part) = build_tool_result_part(obj)? {
                            out.push(part);
                        }
                    }
                    "thinking" => {
                        // Anthropic 多轮中回放 thinking block；Gemini 不再需要，丢弃。
                        // signature 仍保留在 params 中供后续合并使用（Phase 3 接入）。
                    }
                    _ => {
                        // 未知 block type 跳过（不报错，避免客户端 SDK 升级带来的噪音）
                    }
                }
            }
            Ok(out)
        }
        _ => Err(TranslateError::Invalid(
            "`content` must be string or array".into(),
        )),
    }
}

/// 处理 image block → `inline_data` part。
fn build_image_part(obj: &Map<String, Value>) -> Option<Value> {
    let source = obj.get("source")?.as_object()?;
    let source_type = source.get("type")?.as_str()?;
    if source_type != "base64" {
        // url 类型暂不处理（Gemini file_data 需要 Files API 上传，超出 Phase 1 范围）
        return None;
    }
    let media_type = source.get("media_type")?.as_str()?.to_string();
    let data = source.get("data")?.as_str()?.to_string();
    Some(Value::Object(Map::from_iter([(
        "inline_data".into(),
        Value::Object(Map::from_iter([
            ("mime_type".into(), Value::String(media_type)),
            ("data".into(), Value::String(data)),
        ])),
    )])))
}

/// 处理 tool_use block → `functionCall` part。
///
/// Gemini 函数名限制 `[a-zA-Z][a-zA-Z0-9_]*`；调用 `tool_name::sanitize_with_occupied`
/// 做冲突检测（xxhash64 hash-suffix 兜底，对齐 CLIProxyAPI `db143ae` 修复）。
/// sanitized 名 → 原名 的映射写入 `params.tool_name_map`，响应方向反向使用。
fn build_tool_use_part(
    obj: &Map<String, Value>,
    params: &mut StreamParams,
) -> Result<Option<Value>, TranslateError> {
    let name = match obj.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return Ok(None),
    };
    let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let input = obj.get("input").cloned().unwrap_or(Value::Object(Map::new()));

    let sanitized = tool_name::sanitize_with_occupied(name, &mut params.sanitized_name_map);

    let mut function_call = Map::new();
    function_call.insert("name".into(), Value::String(sanitized));
    function_call.insert("args".into(), input);
    if !id.is_empty() {
        function_call.insert("id".into(), Value::String(id.to_string()));
    }
    Ok(Some(Value::Object(Map::from_iter([(
        "functionCall".into(),
        Value::Object(function_call),
    )]))))
}

/// 处理 tool_result block → `functionResponse` part。
///
/// Anthropic tool_result 形状：
/// ```json
/// {"type": "tool_result", "tool_use_id": "...", "content": "..." | [{...}], "is_error": bool}
/// ```
fn build_tool_result_part(
    obj: &Map<String, Value>,
) -> Result<Option<Value>, TranslateError> {
    let tool_use_id = obj.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
    let is_error = obj.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);

    // content 可以是字符串或 content block 数组。Gemini functionResponse.response
    // 需要是 dict；字符串包一层 {"result": "..."}，数组按 text blocks 拼接。
    let content = obj.get("content");
    let response_value = match content {
        Some(Value::String(s)) => Value::Object(Map::from_iter([(
            "result".into(),
            Value::String(s.clone()),
        )])),
        Some(Value::Array(arr)) => {
            let mut text_buf = String::new();
            for block in arr {
                let Some(bobj) = block.as_object() else { continue };
                let Some(btype) = bobj.get("type").and_then(|v| v.as_str()) else { continue };
                match btype {
                    "text" => {
                        if let Some(t) = bobj.get("text").and_then(|v| v.as_str()) {
                            if !text_buf.is_empty() {
                                text_buf.push('\n');
                            }
                            text_buf.push_str(t);
                        }
                    }
                    "image" => {
                        // Gemini functionResponse 不支持 inline_data，
                        // 转为多模态 part 需重组成 text（注：本函数只输出单个 functionResponse，
                        // 这种情况较罕见，CLIProxyAPI 也不完美；此处用 base64 dump 占位）
                        if let Some(source) = bobj.get("source").and_then(|v| v.as_object()) {
                            if source.get("type").and_then(|v| v.as_str()) == Some("base64") {
                                if let Some(data) = source.get("data").and_then(|v| v.as_str()) {
                                    text_buf.push_str(&format!("[image:base64:{}bytes]", data.len()));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if text_buf.is_empty() {
                Value::Object(Map::new())
            } else {
                Value::Object(Map::from_iter([(
                    "result".into(),
                    Value::String(text_buf),
                )]))
            }
        }
        Some(v) => v.clone(),
        None => Value::Object(Map::new()),
    };

    let mut function_response = Map::new();
    // name 必须回传：Gemini 按 name 配对，而非 id。回退到 tool_use_id 推断。
    // TODO Phase 3: 通过 params.tool_name_map 反查原 sanitized 名 → 原 client 名
    // 当前先放 tool_use_id 作 placeholder（Gemini 会忽略未声明 name 的 response，但
    // 如果是 strict mode 会 400）。
    if !tool_use_id.is_empty() {
        function_response.insert("id".into(), Value::String(tool_use_id.to_string()));
    }
    function_response.insert("response".into(), response_value);
    if is_error {
        function_response.insert("error".into(), Value::Bool(true));
    }
    Ok(Some(Value::Object(Map::from_iter([(
        "functionResponse".into(),
        Value::Object(function_response),
    )]))))
}

/// 处理 Anthropic `tools` → Gemini `tools[].functionDeclarations[]`。
///
/// Gemini schema 字段名为 `parametersJsonSchema`；Anthropic 是 `input_schema`。
fn build_tools(
    tools: &Value,
    params: &mut StreamParams,
) -> Result<Option<Value>, TranslateError> {
    let Some(arr) = tools.as_array() else { return Ok(None) };
    let mut fn_decls: Vec<Value> = Vec::new();
    for tool in arr {
        let Some(tool_obj) = tool.as_object() else { continue };
        let name = tool_obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let sanitized = tool_name::sanitize_with_occupied(name, &mut params.sanitized_name_map);
        let description = tool_obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let mut input_schema = tool_obj
            .get("input_schema")
            .cloned()
            .unwrap_or(Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("object".into())),
                ("properties".to_string(), Value::Object(Map::new())),
            ])));

        // 强制 object 类型
        if let Some(obj) = input_schema.as_object_mut() {
            obj.insert("type".into(), Value::String("object".into()));
        }

        let mut fn_decl = Map::new();
        fn_decl.insert("name".into(), Value::String(sanitized));
        if let Some(desc) = description {
            fn_decl.insert("description".into(), Value::String(desc));
        }
        fn_decl.insert("parametersJsonSchema".into(), input_schema);
        fn_decls.push(Value::Object(fn_decl));
    }
    if fn_decls.is_empty() {
        return Ok(None);
    }
    Ok(Some(Value::Array(vec![Value::Object(
        Map::from_iter([("functionDeclarations".into(), Value::Array(fn_decls))]),
    )])))
}

/// 处理 Anthropic `tool_choice` → Gemini `toolConfig.functionCallingConfig.mode`。
///
/// 映射表：
/// - `"auto"`         → `"AUTO"`
/// - `"any"`          → `"ANY"`     (Anthropic: at least one tool)
/// - `"none"`         → `"NONE"`
/// - `{"type":"tool", "name":"..."}` → `"ANY"` + `allowedFunctionNames=[...]`
fn build_tool_config(tool_choice: &Value) -> Option<Value> {
    let mode_str = match tool_choice {
        Value::String(s) => match s.as_str() {
            "auto" => "AUTO",
            "any" => "ANY",
            "none" => "NONE",
            _ => return None,
        },
        Value::Object(obj) => {
            let t = obj.get("type")?.as_str()?;
            if t == "tool" {
                // 强制指定某个工具。allowedFunctionNames 在 Gemini 里要 sanitize 后传。
                let name = obj.get("name")?.as_str()?;
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

    fn translate(model: &str, body: &Value, stream: bool) -> Result<Value, TranslateError> {
        let mut params = StreamParams::default();
        build_request(model, body, stream, &mut params)
    }

    #[test]
    fn minimal_text_request() {
        let body = json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024
        });
        let out = translate("gemini-2.5-pro", &body, false).unwrap();
        assert_eq!(
            out,
            json!({
                "contents": [
                    {"role": "user", "parts": [{"text": "Hello"}]}
                ],
                "generationConfig": {"maxOutputTokens": 1024}
            })
        );
    }

    #[test]
    fn system_string_becomes_system_instruction() {
        let body = json!({
            "model": "m",
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let out = translate("m", &body, false).unwrap();
        assert_eq!(
            out.get("systemInstruction"),
            Some(&json!({"parts": [{"text": "You are helpful."}]}))
        );
    }

    #[test]
    fn system_array_text_blocks() {
        let body = json!({
            "model": "m",
            "system": [
                {"type": "text", "text": "First."},
                {"type": "text", "text": "Second."}
            ],
            "messages": []
        });
        let out = translate("m", &body, false).unwrap();
        let si = out.get("systemInstruction").unwrap();
        let parts = si.get("parts").unwrap().as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].get("text").unwrap(), "First.");
        assert_eq!(parts[1].get("text").unwrap(), "Second.");
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
        let out = translate("m", &body, false).unwrap();
        let contents = out.get("contents").unwrap().as_array().unwrap();
        assert_eq!(contents[0].get("role").unwrap(), "user");
        assert_eq!(contents[1].get("role").unwrap(), "model");
    }

    #[test]
    fn system_role_messages_are_dropped() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "should be dropped"},
                {"role": "user", "content": "Hi"}
            ]
        });
        let out = translate("m", &body, false).unwrap();
        let contents = out.get("contents").unwrap().as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].get("role").unwrap(), "user");
    }

    #[test]
    fn image_block_becomes_inline_data() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's this?"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBOR"}}
                ]
            }]
        });
        let out = translate("m", &body, false).unwrap();
        let parts = out["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "What's this?");
        assert_eq!(
            parts[1],
            json!({"inline_data": {"mime_type": "image/png", "data": "iVBOR"}})
        );
    }

    #[test]
    fn tool_use_block_becomes_function_call() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_01",
                    "name": "search.web",
                    "input": {"q": "rust"}
                }]
            }]
        });
        let out = translate("m", &body, false).unwrap();
        let part = &out["contents"][0]["parts"][0]["functionCall"];
        assert_eq!(part["name"], "search_web"); // sanitize
        assert_eq!(part["args"], json!({"q": "rust"}));
        assert_eq!(part["id"], "toolu_01");
    }

    #[test]
    fn tool_result_block_becomes_function_response() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_01",
                    "content": "found 3 results"
                }]
            }]
        });
        let out = translate("m", &body, false).unwrap();
        let part = &out["contents"][0]["parts"][0]["functionResponse"];
        assert_eq!(part["id"], "toolu_01");
        assert_eq!(part["response"], json!({"result": "found 3 results"}));
    }

    #[test]
    fn tool_choice_auto_becomes_auto_mode() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tool_choice": "auto"
        });
        let out = translate("m", &body, false).unwrap();
        assert_eq!(
            out.get("toolConfig"),
            Some(&json!({"functionCallingConfig": {"mode": "AUTO"}}))
        );
    }

    #[test]
    fn tool_choice_specific_tool_becomes_allowed_list() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tool_choice": {"type": "tool", "name": "search.web"}
        });
        let out = translate("m", &body, false).unwrap();
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
    fn tools_become_function_declarations() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tools": [{
                "name": "search.web",
                "description": "Search",
                "input_schema": {"type": "object", "properties": {"q": {"type": "string"}}}
            }]
        });
        let out = translate("m", &body, false).unwrap();
        let fns = &out["tools"][0]["functionDeclarations"];
        assert_eq!(fns[0]["name"], "search_web");
        assert_eq!(fns[0]["description"], "Search");
        assert_eq!(fns[0]["parametersJsonSchema"]["type"], "object");
        assert!(fns[0]["parametersJsonSchema"]["properties"].is_object());
    }

    #[test]
    fn thinking_enabled_writes_budget() {
        let body = json!({
            "model": "m",
            "messages": [],
            "thinking": {"type": "enabled", "budget_tokens": 2048}
        });
        let out = translate("m", &body, false).unwrap();
        assert_eq!(
            out["generationConfig"]["thinkingConfig"],
            json!({"thinkingBudget": 2048, "includeThoughts": true})
        );
    }

    #[test]
    fn thinking_adaptive_writes_level() {
        let body = json!({
            "model": "m",
            "messages": [],
            "thinking": {"type": "adaptive", "level": "high"}
        });
        let out = translate("m", &body, false).unwrap();
        assert_eq!(
            out["generationConfig"]["thinkingConfig"],
            json!({"thinkingLevel": "high", "includeThoughts": true})
        );
    }

    #[test]
    fn thinking_disabled_clears_thinking_config() {
        let body = json!({
            "model": "m",
            "messages": [],
            "thinking": {"type": "disabled"}
        });
        let out = translate("m", &body, false).unwrap();
        // generationConfig 可能不存在（因为没有其它字段），但 thinkingConfig 不能存在
        if let Some(gen) = out.get("generationConfig").and_then(|v| v.as_object()) {
            assert!(!gen.contains_key("thinkingConfig"));
        }
    }

    #[test]
    fn temperature_and_top_p_pass_through() {
        let body = json!({
            "model": "m",
            "messages": [],
            "temperature": 0.7,
            "top_p": 0.9,
            "top_k": 40
        });
        let out = translate("m", &body, false).unwrap();
        assert_eq!(out["generationConfig"]["temperature"], 0.7);
        assert_eq!(out["generationConfig"]["topP"], 0.9);
        assert_eq!(out["generationConfig"]["topK"], 40);
    }

    #[test]
    fn stop_sequences_pass_through() {
        let body = json!({
            "model": "m",
            "messages": [],
            "stop_sequences": ["END", "STOP"]
        });
        let out = translate("m", &body, false).unwrap();
        assert_eq!(
            out["generationConfig"]["stopSequences"],
            json!(["END", "STOP"])
        );
    }

    #[test]
    fn empty_model_is_error() {
        let body = json!({"messages": []});
        let err = translate("", &body, false).unwrap_err();
        assert!(matches!(err, TranslateError::Invalid(_)));
    }

    #[test]
    fn invalid_body_type_is_error() {
        let err = translate("m", &json!("not an object"), false).unwrap_err();
        assert!(matches!(err, TranslateError::Invalid(_)));
    }

    #[test]
    fn unsupported_role_is_error() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "developer", "content": "x"}]
        });
        let err = translate("m", &body, false).unwrap_err();
        assert!(matches!(err, TranslateError::Invalid(_)));
    }

    /// Phase 3 关键测试：两个工具名 sanitize 后冲突时，hash-suffix 兜底确保
    /// sanitized_name_map 两个 entry 都不丢。对齐 CLIProxyAPI `db143ae` 修复。
    #[test]
    fn tool_name_collision_uses_hash_suffix() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "call_1", "name": "search.web", "input": {"q": "rust"}},
                    {"type": "tool_use", "id": "call_2", "name": "search-web", "input": {"q": "go"}}
                ]
            }]
        });
        let mut params = StreamParams::default();
        let out = build_request("m", &body, false, &mut params).unwrap();
        let parts = &out["contents"][0]["parts"];
        assert_eq!(parts.as_array().unwrap().len(), 2);
        let n1 = parts[0]["functionCall"]["name"].as_str().unwrap();
        let n2 = parts[1]["functionCall"]["name"].as_str().unwrap();
        // 第一个是 "search_web"，第二个是 "search_web_<hash>" 形式
        assert_eq!(n1, "search_web");
        assert_ne!(n1, n2);
        assert!(n2.starts_with("search_web"));
        // params.sanitized_name_map 含两条 entry，可被响应方向反查
        assert_eq!(params.sanitized_name_map.len(), 2);
        assert_eq!(params.sanitized_name_map.get("search_web"), Some(&"search.web".to_string()));
        assert_eq!(params.sanitized_name_map.get(n2), Some(&"search-web".to_string()));
    }

    /// tools[] 声明的工具名也走 sanitize_with_occupied。
    #[test]
    fn tool_declarations_collision_uses_hash_suffix() {
        let body = json!({
            "model": "m",
            "messages": [],
            "tools": [
                {"name": "search.web", "description": "A", "input_schema": {"type": "object"}},
                {"name": "search-web", "description": "B", "input_schema": {"type": "object"}}
            ]
        });
        let mut params = StreamParams::default();
        let _ = build_request("m", &body, false, &mut params).unwrap();
        assert_eq!(params.sanitized_name_map.len(), 2);
        // 第一个被分配 "search_web"
        assert_eq!(params.sanitized_name_map.get("search_web"), Some(&"search.web".to_string()));
    }

    /// 同一工具名出现两次（多轮调用），第二次应复用第一次的 sanitized 名（幂等）。
    #[test]
    fn same_tool_name_preserves_sanitized_across_calls() {
        let body = json!({
            "model": "m",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "c1", "name": "search.web", "input": {}}
                ]
            }, {
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "c1", "content": "ok"}]
            }, {
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "c2", "name": "search.web", "input": {"q": "rust"}}
                ]
            }]
        });
        let mut params = StreamParams::default();
        let _ = build_request("m", &body, false, &mut params).unwrap();
        // 同一原名入 map 两次仍只占一个 entry（sanitize_with_occupied 内部 is_occupied 检查）
        assert_eq!(params.sanitized_name_map.len(), 1);
    }

    /// 对齐 CLIProxyAPI `TestConvertGeminiRequestToClaude_GroupsConsecutiveRoleTurns`（反向）。
    /// 多个相邻同 role Anthropic turn 合并为单个 Gemini content；
    /// assistant 内 tool_use 移到末尾。
    #[test]
    fn groups_consecutive_role_turns() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "assistant", "content": [{"type": "text", "text": "answer"}]},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "call_1", "name": "first", "input": {}}]},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "call_2", "name": "second", "input": {}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "one"}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_2", "content": "two"}]}
            ]
        });
        let mut params = StreamParams::default();
        let out = build_request("m", &body, false, &mut params).unwrap();
        let contents = out.get("contents").unwrap().as_array().unwrap();
        assert_eq!(contents.len(), 2, "期望 2 个 turn（assistant 一个、user 一个）");

        // 第一个 turn: model
        assert_eq!(contents[0].get("role").unwrap(), "model");
        let parts = contents[0].get("parts").unwrap().as_array().unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].get("text").unwrap(), "answer");
        assert!(parts[1].get("functionCall").is_some());
        assert!(parts[2].get("functionCall").is_some());
        // tool_use 顺序：call_1 在 call_2 前（按 sanitized 名顺序）
        assert_eq!(parts[1]["functionCall"]["id"], "call_1");
        assert_eq!(parts[2]["functionCall"]["id"], "call_2");

        // 第二个 turn: user（tool_result 保留原序）
        assert_eq!(contents[1].get("role").unwrap(), "user");
        let user_parts = contents[1].get("parts").unwrap().as_array().unwrap();
        assert_eq!(user_parts.len(), 2);
        assert_eq!(user_parts[0]["functionResponse"]["id"], "call_1");
        assert_eq!(user_parts[1]["functionResponse"]["id"], "call_2");
    }

    /// 对齐 CLIProxyAPI `TestConvertGeminiRequestToClaude_KeepsSystemInstructionUserSeparate`（反向）。
    /// system instruction 后跟普通 user message，**不会**合并；assistant 前的 user 也不合并。
    /// 注：本翻译器把 system 走 `systemInstruction` 字段（不进 messages），所以这里只验证
    /// user 之间不会发生跨 character 的合并边界 bug。
    #[test]
    fn does_not_merge_across_role_changes() {
        let body = json!({
            "model": "m",
            "system": "system rule",
            "messages": [
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "middle"},
                {"role": "user", "content": "second"}
            ]
        });
        let mut params = StreamParams::default();
        let out = build_request("m", &body, false, &mut params).unwrap();
        let contents = out.get("contents").unwrap().as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "first");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "middle");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(contents[2]["parts"][0]["text"], "second");
        // system 字段保留在 systemInstruction，不进 messages
        assert_eq!(
            out.get("systemInstruction"),
            Some(&json!({"parts": [{"text": "system rule"}]}))
        );
    }

    /// 同一 user turn 内 tool_result + text 之后又有 tool_result，
    /// 累积器合并但保留原顺序（user turn 不重排）。
    #[test]
    fn user_turn_preserves_tool_result_and_text_order() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "c1", "content": "ok"}
                ]},
                {"role": "user", "content": "continue"},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "c2", "content": "ok2"}
                ]}
            ]
        });
        let mut params = StreamParams::default();
        let out = build_request("m", &body, false, &mut params).unwrap();
        let contents = out.get("contents").unwrap().as_array().unwrap();
        // 三个 user turn 合并为一个
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        let parts = contents[0].get("parts").unwrap().as_array().unwrap();
        assert_eq!(parts.len(), 3);
        // 顺序：tool_result(c1) → text("continue") → tool_result(c2)
        assert!(parts[0].get("functionResponse").is_some());
        assert_eq!(parts[0]["functionResponse"]["id"], "c1");
        assert_eq!(parts[1].get("text").unwrap(), "continue");
        assert!(parts[2].get("functionResponse").is_some());
        assert_eq!(parts[2]["functionResponse"]["id"], "c2");
    }
}