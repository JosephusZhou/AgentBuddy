//! 请求模型名解析工具。
//!
//! CLIProxyAPI aligned: 9b8d974 - fix(responses): preserve original request model
//!                        on response.created/response.in_progress payloads
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/9b8d97441e8692eccd4ea4b010547abeaf352992
//! Last verified: 2026-08-12
//!
//! 用于响应方向写 `response.created` / `response.in_progress` 事件的 `response.model`
//! 字段：客户端期望看到原始请求的模型名，而不是被翻译后 backend 用的内部模型名。

use serde_json::Value;

/// 从客户原始请求 JSON 提取 `model` 字段作为响应事件中的 `response.model`。
///
/// 优先用 `original_request`（客户端实际发出的请求），fallback 到 `request`（被翻译后
/// 给 backend 用的请求）。两者都没有时返回空字符串，调用方应进一步 fallback 到
/// `{model}` 参数（forge 给出的内部模型名）。
///
/// 支持两种嵌套形式：
/// - 顶层 `model` 字段（标准 OpenAI / Anthropic / Responses）
/// - 嵌套 `request.model`（CLIProxyAPI 某些 executor 包装后的形状）
pub fn request_model_name(original_request: &Value, request: &Value) -> String {
    for raw in [original_request, request] {
        if let Some(name) = extract_model_name(raw) {
            return name;
        }
    }
    String::new()
}

fn extract_model_name(raw: &Value) -> Option<String> {
    let Some(root) = raw.as_object() else {
        return None;
    };
    for path in ["model", "request.model"] {
        let v = match path {
            "model" => root.get("model"),
            "request.model" => root.get("request").and_then(|r| r.get("model")),
            _ => unreachable!(),
        };
        if let Some(s) = v.and_then(|x| x.as_str()) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prefers_original_request_model() {
        let original = json!({"model": "gpt-5"});
        let request = json!({"model": "translated-model"});
        assert_eq!(request_model_name(&original, &request), "gpt-5");
    }

    #[test]
    fn falls_back_to_request_model() {
        let original = json!({});
        let request = json!({"model": "translated-model"});
        assert_eq!(request_model_name(&original, &request), "translated-model");
    }

    #[test]
    fn empty_when_no_model_anywhere() {
        let original = json!({});
        let request = json!({});
        assert!(request_model_name(&original, &request).is_empty());
    }

    #[test]
    fn supports_wrapped_request_model_path() {
        let original = json!({});
        let request = json!({"request": {"model": "wrapped-model"}});
        assert_eq!(request_model_name(&original, &request), "wrapped-model");
    }

    #[test]
    fn trims_whitespace() {
        let original = json!({"model": "  gpt-5  "});
        let request = json!({});
        assert_eq!(request_model_name(&original, &request), "gpt-5");
    }

    #[test]
    fn ignores_empty_model_string() {
        let original = json!({"model": ""});
        let request = json!({"model": "real-model"});
        assert_eq!(request_model_name(&original, &request), "real-model");
    }

    #[test]
    fn non_object_inputs_return_empty() {
        let original = json!("string");
        let request = json!(null);
        assert!(request_model_name(&original, &request).is_empty());
    }
}
