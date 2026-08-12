//! Translator pair: OpenAI Chat Completions → Gemini generateContent (请求方向)
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! ## 子模块
//! - `request.rs` — OpenAI Chat 请求 → Gemini generateContent body
//!
//! 响应方向由 `gemini_openai_chat::GeminiToOpenaiChatTranslator` 处理。
//!
//! ## 用法
//! forwarder 通过 `(Format::OpenAiChat, Format::Gemini)` 注册表查到本 translator 实例，
//! 调 `translate_request(...)` 拿到 Gemini body，发送给 Google provider。

pub mod request;


/// Unit struct translator — OpenAI Chat → Gemini 请求翻译。
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenaiToGeminiTranslator;

impl super::Translatable for OpenaiToGeminiTranslator {
    fn translate_request(
        &self,
        model: &str,
        raw: &serde_json::Value,
        stream: bool,
        params: &mut super::StreamParams,
    ) -> Result<serde_json::Value, super::TranslateError> {
        request::build_request(model, raw, stream, params)
    }

    fn translate_response_stream(
        &self,
        _model: &str,
        _original_request: &serde_json::Value,
        _translated_request: &serde_json::Value,
        _raw_chunk: &[u8],
        _params: &mut super::StreamParams,
    ) -> Result<Vec<Vec<u8>>, super::TranslateError> {
        Err(super::TranslateError::Unsupported(
            "use gemini_openai_chat::translate_response_stream for response".into(),
        ))
    }

    fn translate_response_non_stream(
        &self,
        _model: &str,
        _original_request: &serde_json::Value,
        _translated_request: &serde_json::Value,
        _raw: &[u8],
        _params: &mut super::StreamParams,
    ) -> Result<Vec<u8>, super::TranslateError> {
        Err(super::TranslateError::Unsupported(
            "use gemini_openai_chat::translate_response_non_stream for response".into(),
        ))
    }

    fn name(&self) -> &'static str {
        "openai_chat→gemini (request)"
    }
}