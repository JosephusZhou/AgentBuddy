//! Translator pair: Gemini → OpenAI Responses API (Codex CLI 路径)
//!
//! CLIProxyAPI aligned: 934da237 - fix(openai): preserve structured and stringified
//!                        custom tool outputs during Responses conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//! Last verified: 2026-08-12
//!
//! ## 子模块
//! - `response_stream.rs` — Gemini SSE → OpenAI Responses SSE
//! - `response_non_stream.rs` — Gemini JSON → OpenAI Responses JSON
//!
//! 反向（OpenAI Responses → Gemini 请求）见 `translator/openai_openai_responses/`。

pub mod response_non_stream;
pub mod response_stream;


#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiToOpenaiResponsesTranslator;

impl super::Translatable for GeminiToOpenaiResponsesTranslator {
    fn translate_request(
        &self,
        _model: &str,
        _raw: &serde_json::Value,
        _stream: bool,
        _params: &mut super::StreamParams,
    ) -> Result<serde_json::Value, super::TranslateError> {
        Err(super::TranslateError::Unsupported(
            "response-only translator; use openai_openai_responses for request".into(),
        ))
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
            "use free function translate_response_stream directly".into(),
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
            "use free function translate_response_non_stream directly".into(),
        ))
    }

    fn name(&self) -> &'static str {
        "gemini→openai_responses (response)"
    }
}