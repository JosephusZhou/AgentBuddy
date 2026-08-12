//! Translator pair: Gemini → OpenAI Chat Completions (响应方向)
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! ## 子模块
//! - `response_stream.rs` — Gemini SSE → OpenAI Chat Completions SSE
//! - `response_non_stream.rs` — Gemini JSON → OpenAI Chat Completions JSON
//!
//! 反向（OpenAI Chat → Gemini 请求）见 `translator/openai_gemini/`。

pub mod response_non_stream;
pub mod response_stream;


/// Unit struct — forwarder 通过 `TranslatorRegistry` 拿到本 translator 实例。
///
/// 与 `claude_gemini` 同样设计：实际翻译逻辑放在 re-export 的自由函数，
/// `Translatable` trait 默认 stream/non_stream 返回 `Unsupported` 错误，
/// 由 forwarder 直接调自由函数。
#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiToOpenaiChatTranslator;

impl super::Translatable for GeminiToOpenaiChatTranslator {
    fn translate_request(
        &self,
        _model: &str,
        _raw: &serde_json::Value,
        _stream: bool,
        _params: &mut super::StreamParams,
    ) -> Result<serde_json::Value, super::TranslateError> {
        // 请求方向由 `openai_gemini::OpenaiToGeminiTranslator` 处理；
        // Gemini → OpenAI Chat 客户端场景不需要请求翻译。
        Err(super::TranslateError::Unsupported(
            "GeminiToOpenaiChatTranslator is response-only; use openai_gemini for request".into(),
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
        // 标注：这是响应方向翻译器
        "gemini→openai_chat (response)"
    }
}