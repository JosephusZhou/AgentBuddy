//! Structured output 字段映射 → Gemini `responseMimeType` / `responseJsonSchema`。
//!
//! CLIProxyAPI aligned:
//! - 20f83ca - feat(translator): map OpenAI `response_format` to Gemini
//!             structured output settings
//! - 2e91e99 - fix(responses): translate `text.format` into `response_format`
//!             in request conversion
//! Sources: https://github.com/router-for-me/CLIProxyAPI/commit/20f83cae910b98bc39c45193f3e05072afea81f8
//!          https://github.com/router-for-me/CLIProxyAPI/commit/2e91e99e0339f1f2592cb4fa36b1f71ca89bf8dd
//! Last verified: 2026-08-12
//!
//! 兼容两种上游字段：
//! - OpenAI Chat `response_format = {type: "json_object"|"json_schema", json_schema: {...}}`
//! - OpenAI Responses `text.format = {type: "json_schema", name, description, strict, schema}`
//!
//! 两者都映射到 Gemini `generationConfig.responseMimeType = "application/json"` +
//! （如有 schema）`responseJsonSchema`。Gemini 端的 `responseSchema` 字段被删除以
//! 避免与 `responseJsonSchema` 冲突（旧字段 Claude 风格的 schema）。

use serde_json::{Map, Value};

/// 把 OpenAI 风格 structured output 写入 `generationConfig` map（in-place）。
///
/// 返回是否写入了任何字段（false = 没有有效的 structured output 设置）。
pub fn apply_structured_output_to_gemini(
    gen_config: &mut Map<String, Value>,
    response_format: Option<&Value>,
) -> bool {
    let Some(value) = response_format else {
        return false;
    };
    let Some(obj) = value.as_object() else {
        return false;
    };
    let format_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match format_type.as_str() {
        "json_object" => {
            gen_config.insert(
                "responseMimeType".into(),
                Value::String("application/json".into()),
            );
            true
        }
        "json_schema" => {
            gen_config.insert(
                "responseMimeType".into(),
                Value::String("application/json".into()),
            );
            // 旧的 responseSchema 字段（如果之前已设置）被删除以避免与 responseJsonSchema 冲突。
            gen_config.remove("responseSchema");

            // 检查 schema 字段（OpenAI Chat 风格 = `json_schema.schema`）
            if let Some(schema) = obj.get("json_schema").and_then(|v| v.get("schema")) {
                if !schema.is_null() {
                    gen_config.insert("responseJsonSchema".into(), schema.clone());
                    return true;
                }
            }
            // OpenAI Responses 风格 = `text.format.schema` + name/description/strict
            if let Some(schema) = obj.get("schema") {
                if !schema.is_null() {
                    gen_config.insert("responseJsonSchema".into(), schema.clone());
                    return true;
                }
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_config() -> Map<String, Value> {
        Map::new()
    }

    #[test]
    fn no_response_format_is_no_op() {
        let mut gc = empty_config();
        let changed = apply_structured_output_to_gemini(&mut gc, None);
        assert!(!changed);
        assert!(gc.is_empty());
    }

    #[test]
    fn json_object_only_sets_mime_type() {
        let mut gc = empty_config();
        let changed = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({"type": "json_object"})),
        );
        assert!(changed);
        assert_eq!(gc.get("responseMimeType"), Some(&json!("application/json")));
        assert!(gc.get("responseJsonSchema").is_none());
    }

    #[test]
    fn json_schema_with_nested_schema_field() {
        let mut gc = empty_config();
        let schema = json!({
            "type": "object",
            "properties": {"ok": {"type": "boolean"}},
            "required": ["ok"],
            "additionalProperties": false
        });
        let changed = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "response",
                    "description": "structured response",
                    "strict": true,
                    "schema": schema.clone()
                }
            })),
        );
        assert!(changed);
        assert_eq!(gc.get("responseMimeType"), Some(&json!("application/json")));
        assert_eq!(gc.get("responseJsonSchema"), Some(&schema));
    }

    #[test]
    fn json_schema_with_top_level_schema_field() {
        let mut gc = empty_config();
        let schema = json!({"type": "object", "properties": {"x": {"type": "string"}}});
        let changed = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({
                "type": "json_schema",
                "name": "response",
                "schema": schema.clone()
            })),
        );
        assert!(changed);
        assert_eq!(gc.get("responseJsonSchema"), Some(&schema));
    }

    #[test]
    fn json_schema_without_schema_field_only_sets_mime_type() {
        let mut gc = empty_config();
        let changed = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({"type": "json_schema", "json_schema": {"name": "response"}})),
        );
        assert!(changed);
        assert_eq!(gc.get("responseMimeType"), Some(&json!("application/json")));
        assert!(gc.get("responseJsonSchema").is_none());
    }

    #[test]
    fn clears_existing_response_schema_field() {
        // CLIProxyAPI 20f83ca: 只有 `json_schema` 分支会清掉旧的 `responseSchema`
        // 字段（避免与 `responseJsonSchema` 冲突）。`json_object` 不清。
        let mut gc = Map::from_iter([(
            "responseSchema".into(),
            json!({"type": "string"}),
        )]);
        let _ = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({
                "type": "json_schema",
                "json_schema": {"schema": {"type": "object"}}
            })),
        );
        assert!(gc.get("responseSchema").is_none());
        assert_eq!(gc.get("responseJsonSchema"), Some(&json!({"type": "object"})));
    }

    #[test]
    fn json_object_keeps_existing_response_schema_field() {
        // 对称：json_object 不清 responseSchema（与 json_schema 路径不同）。
        let mut gc = Map::from_iter([(
            "responseSchema".into(),
            json!({"type": "string"}),
        )]);
        let _ = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({"type": "json_object"})),
        );
        assert_eq!(gc.get("responseSchema"), Some(&json!({"type": "string"})));
    }

    #[test]
    fn unknown_type_is_no_op() {
        let mut gc = empty_config();
        let changed = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({"type": "text"})),
        );
        assert!(!changed);
        assert!(gc.is_empty());
    }

    #[test]
    fn case_insensitive_type() {
        let mut gc = empty_config();
        let changed = apply_structured_output_to_gemini(
            &mut gc,
            Some(&json!({"type": "JSON_OBJECT"})),
        );
        assert!(changed);
        assert_eq!(gc.get("responseMimeType"), Some(&json!("application/json")));
    }
}
