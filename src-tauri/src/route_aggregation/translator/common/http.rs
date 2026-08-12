//! Google OpenAI 兼容端点探测 —— 当 provider 是 Google 直接 key 且目标路径匹配
//! `/v1beta/openai/v1/...` 时，跳过自建翻译直接转发。
//!
//! CLIProxyAPI aligned: 150e7f0 - fix(auth): repair force-mapped Responses SSE
//!                        framing for WS forwarder
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/150e7f0dc50e3d3a0f7c4e552cc402ae105eb2a0
//! Last verified: 2026-08-12
//!
//! **Phase 0 占位**：仅放 URL 匹配常量与 helper。ProviderRouter 集成在 Phase 2。

/// Google Generative AI OpenAI 兼容端点 host。
pub const GOOGLE_OPENAI_COMPAT_HOST: &str = "generativelanguage.googleapis.com";

/// OpenAI 兼容端点路径前缀（含 `/v1beta/openai/v1`，后续跟 chat/completions 等）。
pub const GOOGLE_OPENAI_COMPAT_PATH_PREFIX: &str = "/v1beta/openai/v1/";

/// 判断给定的 base_url + path 是否可以直接 passthrough 到 Google OpenAI 兼容端点。
///
/// 触发条件：
/// - `base_url` 含 `generativelanguage.googleapis.com`
/// - `path` 以 `/v1beta/openai/v1/` 开头
pub fn is_google_openai_compat(base_url: &str, path: &str) -> bool {
    base_url.contains(GOOGLE_OPENAI_COMPAT_HOST) && path.starts_with(GOOGLE_OPENAI_COMPAT_PATH_PREFIX)
}

/// Provider 是否是 Google Generative AI。
pub fn is_google_provider(provider_type: &str) -> bool {
    provider_type == crate::ai_provider::TYPE_GOOGLE_GENERATIVE_AI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_google_openai_compat() {
        assert!(is_google_openai_compat(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/openai/v1/chat/completions"
        ));
        assert!(is_google_openai_compat(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "/v1beta/openai/v1/responses"
        ));
    }

    #[test]
    fn rejects_non_google_hosts() {
        assert!(!is_google_openai_compat(
            "https://api.openai.com/v1",
            "/v1/chat/completions"
        ));
        assert!(!is_google_openai_compat(
            "https://openrouter.ai/api/v1",
            "/chat/completions"
        ));
    }

    #[test]
    fn rejects_google_but_wrong_path() {
        assert!(!is_google_openai_compat(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models"
        ));
        assert!(!is_google_openai_compat(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1/chat/completions"
        ));
    }

    #[test]
    fn google_provider_recognized() {
        use crate::ai_provider::TYPE_GOOGLE_GENERATIVE_AI;
        assert!(is_google_provider(TYPE_GOOGLE_GENERATIVE_AI));
        assert!(!is_google_provider("anthropic"));
        assert!(!is_google_provider("openai"));
        assert!(!is_google_provider("universal"));
    }
}