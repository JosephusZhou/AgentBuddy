//! Object schema normalize —— OpenAI / Gemini 等下游不接受 `type: "object"` 但缺
//! `properties` 字段的 schema；自动补一个空 `properties: {}`。
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12

use serde_json::{Map, Value};

/// 递归遍历 `value`，对所有 `{"type": "object"}` schema 补 `properties: {}`。
///
/// 处理 `anyOf` / `oneOf` / `allOf` 嵌套。原地修改。
pub fn normalize_object_properties(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let is_object = matches!(map.get("type"), Some(Value::String(s)) if s == "object");
            if is_object && !map.contains_key("properties") {
                map.insert("properties".to_string(), Value::Object(Map::new()));
            }
            // 递归处理嵌套 schema
            for (_k, v) in map.iter_mut() {
                normalize_object_properties(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_object_properties(v);
            }
        }
        _ => {}
    }
}

/// 给一组 tool function 声明（OpenAI tools[] / Anthropic tools[] / Gemini
/// functionDeclarations[]）做整体 normalize。
///
/// 输入按调用方约定：
/// - Anthropic / OpenAI: `[{type: "function", function: {parameters: {...}}}]`
/// - Gemini: `[{parametersJsonSchema: {...}}]` 或 `[{parameters: {...}}]`
///
/// 这里只处理根级 `parameters` / `parametersJsonSchema` 字段下的 object schema。
pub fn normalize_tool_schemas(tools: &mut Value) {
    let arr = match tools {
        Value::Array(a) => a,
        _ => return,
    };
    for tool in arr.iter_mut() {
        let Some(obj) = tool.as_object_mut() else {
            continue;
        };
        // 兼容三种字段名
        for key in ["parameters", "parametersJsonSchema", "input_schema"] {
            if let Some(v) = obj.get_mut(key) {
                normalize_object_properties(v);
            }
        }
        // OpenAI 风格: tool.function.parameters
        if let Some(Value::Object(func)) = obj.get_mut("function") {
            if let Some(v) = func.get_mut("parameters") {
                normalize_object_properties(v);
            }
        }
    }
}

/// 兼容 CLIProxyAPI 测试的便捷函数：取 &mut Map 走 normalize。
pub fn normalize_root(value: &mut Map<String, Value>) {
    let mut v = Value::Object(std::mem::take(value));
    normalize_object_properties(&mut v);
    if let Value::Object(m) = v {
        *value = m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_schema_gets_properties() {
        let mut v = serde_json::json!({"type": "object"});
        normalize_object_properties(&mut v);
        assert_eq!(v, serde_json::json!({"type": "object", "properties": {}}));
    }

    #[test]
    fn object_schema_with_existing_properties_unchanged() {
        let mut v = serde_json::json!({"type": "object", "properties": {"a": {"type": "string"}}});
        normalize_object_properties(&mut v);
        assert_eq!(v, serde_json::json!({"type": "object", "properties": {"a": {"type": "string"}}}));
    }

    #[test]
    fn non_object_schema_unchanged() {
        let mut v = serde_json::json!({"type": "string"});
        normalize_object_properties(&mut v);
        assert_eq!(v, serde_json::json!({"type": "string"}));
    }

    #[test]
    fn nested_object_schemas_all_normalized() {
        let mut v = serde_json::json!({
            "type": "object",
            "properties": {
                "nested": {"type": "object"},
                "list": {"type": "array", "items": {"type": "object"}}
            }
        });
        normalize_object_properties(&mut v);
        assert_eq!(
            v,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "nested": {"type": "object", "properties": {}},
                    "list": {"type": "array", "items": {"type": "object", "properties": {}}}
                }
            })
        );
    }

    #[test]
    fn anyof_oneof_allof_normalized() {
        let mut v = serde_json::json!({
            "anyOf": [
                {"type": "object"},
                {"type": "string"}
            ]
        });
        normalize_object_properties(&mut v);
        assert_eq!(
            v,
            serde_json::json!({
                "anyOf": [
                    {"type": "object", "properties": {}},
                    {"type": "string"}
                ]
            })
        );
    }

    #[test]
    fn normalize_tool_schemas_handles_openai_format() {
        let mut tools = serde_json::json!([
            {"type": "function", "function": {"parameters": {"type": "object"}}}
        ]);
        normalize_tool_schemas(&mut tools);
        assert_eq!(
            tools,
            serde_json::json!([
                {"type": "function", "function": {"parameters": {"type": "object", "properties": {}}}}
            ])
        );
    }

    #[test]
    fn normalize_tool_schemas_handles_gemini_format() {
        let mut tools = serde_json::json!([
            {"parametersJsonSchema": {"type": "object"}}
        ]);
        normalize_tool_schemas(&mut tools);
        assert_eq!(
            tools,
            serde_json::json!([
                {"parametersJsonSchema": {"type": "object", "properties": {}}}
            ])
        );
    }

    #[test]
    fn normalize_tool_schemas_handles_anthropic_input_schema() {
        let mut tools = serde_json::json!([
            {"input_schema": {"type": "object"}}
        ]);
        normalize_tool_schemas(&mut tools);
        assert_eq!(
            tools,
            serde_json::json!([
                {"input_schema": {"type": "object", "properties": {}}}
            ])
        );
    }

    #[test]
    fn normalize_root_keeps_object_type() {
        let mut m: Map<String, Value> = serde_json::from_value(serde_json::json!({"type": "object"}))
            .unwrap();
        normalize_root(&mut m);
        assert!(m.contains_key("properties"));
    }
}