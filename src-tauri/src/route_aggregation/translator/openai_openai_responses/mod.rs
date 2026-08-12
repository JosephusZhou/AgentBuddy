//! Translator pair: OpenAI Responses API → Gemini generateContent (请求方向)
//!
//! CLIProxyAPI aligned: 934da237 - fix(openai): preserve structured and stringified
//!                        custom tool outputs during Responses conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//! Last verified: 2026-08-12
//!
//! 反向（响应）见 `translator/gemini_openai_responses/`。

pub mod request;


#[derive(Debug, Default, Clone, Copy)]
pub struct OpenaiResponsesToGeminiTranslator;

impl super::Translatable for OpenaiResponsesToGeminiTranslator {
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
            "use gemini_openai_responses::translate_response_stream".into(),
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
            "use gemini_openai_responses::translate_response_non_stream".into(),
        ))
    }

    fn name(&self) -> &'static str {
        "openai_responses→gemini (request)"
    }
}