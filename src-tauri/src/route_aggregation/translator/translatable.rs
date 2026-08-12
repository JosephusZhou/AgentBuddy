//! Translatable trait — 翻译器接口。
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/interfaces` 包：
//!   - `TranslateRequestFunc`：请求方向（上游协议 → 下游协议）
//!   - `TranslateResponse`：响应方向（带流式 / 非流式两个方法）
//!
//! AgentBuddy 把这两个 trait 合并成 `Translatable`，每个 pair 实现一次。
//! 一个 `Translatable` 实例注册到 `(source_format, target_format)` 注册表 key 上，
//! 由 `ProviderRouter` 在转发时按需查询。

use std::fmt;

use super::params::StreamParams;

/// 翻译错误。所有翻译器返回的统一错误类型（不引入 thiserror / anyhow 新依赖）。
#[derive(Debug, Clone)]
pub enum TranslateError {
    /// JSON parse / serialize 错误。
    Json(String),
    /// 不支持的字段或场景（用于"请求格式不支持"软失败而非 500）。
    Unsupported(String),
    /// 输入不合法（如缺字段、字段类型错）。
    Invalid(String),
    /// 上下游 IO 错误（极少见——翻译器通常不直接 IO；保留扩展位）。
    Io(String),
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslateError::Json(msg) => write!(f, "translate json: {msg}"),
            TranslateError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            TranslateError::Invalid(msg) => write!(f, "invalid input: {msg}"),
            TranslateError::Io(msg) => write!(f, "io: {msg}"),
        }
    }
}

impl std::error::Error for TranslateError {}

impl From<serde_json::Error> for TranslateError {
    fn from(e: serde_json::Error) -> Self {
        TranslateError::Json(e.to_string())
    }
}

/// 翻译器接口。
///
/// # 方法语义
/// - `translate_request`：把上游请求 JSON 翻译成下游请求 JSON。`stream` 标志
///   决定是否需要保留/添加 `stream` 字段及对应 options。`params` 用于
///   跨方向共享状态（tool_name_map / sanitized_name_map / thinking_signature 等），
///   请求方向写入（tool_use / tools 字段），响应方向读出。
/// - `translate_response_stream`：每个上游 SSE chunk 调用一次；返回零或多个
///   下游 SSE 字节片段。状态通过 `params` 跨 chunk 累积。
/// - `translate_response_non_stream`：完整上游响应调用一次，返回完整下游响应。
///
/// 所有方法 `Send + Sync`，注册表可安全跨任务使用。
pub trait Translatable: Send + Sync {
    /// 请求方向：上游协议 → 下游协议。
    fn translate_request(
        &self,
        model: &str,
        raw: &serde_json::Value,
        stream: bool,
        params: &mut StreamParams,
    ) -> Result<serde_json::Value, TranslateError>;

    /// 响应方向（流式）：上游 SSE chunk → 下游 SSE 字节片段列表。
    ///
    /// 返回的 `Vec<Vec<u8>>` 通常每个元素是一行完整 SSE（如 `data: {...}\n\n`），
    /// 调用方直接 flush 给客户端。空 Vec 表示"该 chunk 翻译不出有效内容"。
    fn translate_response_stream(
        &self,
        model: &str,
        original_request: &serde_json::Value,
        translated_request: &serde_json::Value,
        raw_chunk: &[u8],
        params: &mut StreamParams,
    ) -> Result<Vec<Vec<u8>>, TranslateError>;

    /// 响应方向（非流式）：上游响应 JSON → 下游响应 JSON 的字节表示。
    fn translate_response_non_stream(
        &self,
        model: &str,
        original_request: &serde_json::Value,
        translated_request: &serde_json::Value,
        raw: &[u8],
        params: &mut StreamParams,
    ) -> Result<Vec<u8>, TranslateError>;

    /// 翻译器标识（用于日志），如 `claude→gemini`。
    fn name(&self) -> &'static str {
        "anonymous"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_error_display_includes_context() {
        let err = TranslateError::Json("missing field".into());
        assert_eq!(err.to_string(), "translate json: missing field");

        let err = TranslateError::Unsupported("gpt-image".into());
        assert_eq!(err.to_string(), "unsupported: gpt-image");

        let err = TranslateError::Invalid("bad role".into());
        assert_eq!(err.to_string(), "invalid input: bad role");
    }

    #[test]
    fn serde_json_error_converts() {
        let bad = serde_json::from_str::<serde_json::Value>("not json");
        let err = TranslateError::from(bad.unwrap_err());
        assert!(matches!(err, TranslateError::Json(_)));
    }

    /// 占位实现用于 trait object 注册。
    struct DummyTrans;
    impl Translatable for DummyTrans {
        fn translate_request(
            &self,
            _model: &str,
            _raw: &serde_json::Value,
            _stream: bool,
            _params: &mut StreamParams,
        ) -> Result<serde_json::Value, TranslateError> {
            Ok(serde_json::json!({}))
        }
        fn translate_response_stream(
            &self,
            _model: &str,
            _or: &serde_json::Value,
            _tr: &serde_json::Value,
            _c: &[u8],
            _p: &mut StreamParams,
        ) -> Result<Vec<Vec<u8>>, TranslateError> {
            Ok(vec![])
        }
        fn translate_response_non_stream(
            &self,
            _model: &str,
            _or: &serde_json::Value,
            _tr: &serde_json::Value,
            _r: &[u8],
            _p: &mut StreamParams,
        ) -> Result<Vec<u8>, TranslateError> {
            Ok(vec![])
        }
        fn name(&self) -> &'static str {
            "dummy"
        }
    }

    #[test]
    fn translatable_trait_object_usable() {
        let t: Box<dyn Translatable> = Box::new(DummyTrans);
        assert_eq!(t.name(), "dummy");
        let mut p = StreamParams::default();
        let v = t.translate_request("m", &serde_json::json!({}), false, &mut p).unwrap();
        assert_eq!(v, serde_json::json!({}));
    }
}