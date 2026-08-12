//! Translator pair: Claude Messages → Gemini generateContent (请求 + 响应双向)
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 子模块：
//! - `request.rs`            — Claude Messages 请求 → Gemini generateContent body
//! - `response_stream.rs`    — Gemini 流式 chunk → Anthropic SSE 字节流
//! - `response_non_stream.rs`— Gemini JSON → Anthropic Messages JSON
//!
//! 设计：translation 逻辑放在自由函数中（`translate_response_stream` /
//! `translate_response_non_stream`），`ClaudeToGeminiTranslator` 仅作命名空间 +
//! 注册身份。`Translatable` trait 默认实现返回空，由 forwarder 直接调自由函数。
//!
//! ## 用法
//! ```ignore
//! let translator = ClaudeToGeminiTranslator;
//! let translated = translator.translate_request("gemini-2.5-pro", &body, true)?;
//! // 发送 translated 到 {base}/models/gemini-2.5-pro:streamGenerateContent?alt=sse
//! // 接收响应字节 → 调用 translate_response_stream(...) 得到 Anthropic SSE
//! ```

pub mod request;
pub mod response_non_stream;
pub mod response_stream;

use serde_json::Value;

use super::params::StreamParams;
use super::translatable::{TranslateError, Translatable};

pub use response_non_stream::translate_response_non_stream;
pub use response_stream::translate_response_stream;

/// Unit struct translator — 由 forwarder 通过 `Translatable` 注册表查 (Anthropic, Gemini) 拿到。
///
/// 具体流式 / 非流式响应翻译由本文件 re-export 的 `translate_response_stream` /
/// `translate_response_non_stream` 自由函数完成（不走 trait 方法），允许调用方持有
/// `&mut StreamParams` 而不与 trait 默认实现的 `&mut StreamParams` 签名冲突。
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeToGeminiTranslator;

impl Translatable for ClaudeToGeminiTranslator {
    fn translate_request(
        &self,
        model: &str,
        raw: &Value,
        stream: bool,
        params: &mut StreamParams,
    ) -> Result<Value, TranslateError> {
        request::build_request(model, raw, stream, params)
    }

    fn translate_response_stream(
        &self,
        _model: &str,
        _original_request: &Value,
        _translated_request: &Value,
        _raw_chunk: &[u8],
        _params: &mut StreamParams,
    ) -> Result<Vec<Vec<u8>>, TranslateError> {
        // 调用方应直接调 `translate_response_stream` 自由函数，避免 trait 默认空实现误导。
        Err(TranslateError::Unsupported(
            "use free function translate_response_stream directly".into(),
        ))
    }

    fn translate_response_non_stream(
        &self,
        _model: &str,
        _original_request: &Value,
        _translated_request: &Value,
        _raw: &[u8],
        _params: &mut StreamParams,
    ) -> Result<Vec<u8>, TranslateError> {
        Err(TranslateError::Unsupported(
            "use free function translate_response_non_stream directly".into(),
        ))
    }

    fn name(&self) -> &'static str {
        "claude→gemini"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translator_name() {
        let t = ClaudeToGeminiTranslator;
        assert_eq!(t.name(), "claude→gemini");
    }

    #[test]
    fn request_routes_through_translator() {
        let body = serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let mut params = StreamParams::default();
        let out = ClaudeToGeminiTranslator
            .translate_request("m", &body, false, &mut params)
            .unwrap();
        assert_eq!(
            out,
            serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
            })
        );
    }
}