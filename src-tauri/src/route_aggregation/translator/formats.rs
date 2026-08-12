//! API format registry — used as the registry key for translators.
//!
//! CLIProxyAPI aligned: c13dbcc2 - feat(translator): add test and logic to ensure
//!                        object schemas include properties field
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/c13dbcc24e1373e353338d90bdb38b8e4722e22b
//! Last verified: 2026-08-12
//!
//! 对应 CLIProxyAPI 的 `sdktranslator.FromString(...)` 注册键。AgentBuddy 这里把
//! 所有支持的协议平铺成 7 种枚举，与 `provider_router::RouteGroup`（仅 Claude/Codex
//! 两组）是不同维度——Format 描述"这条请求是哪种协议"，RouteGroup 描述"客户端入口"。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Format {
    /// Anthropic Messages API（`POST /v1/messages`，content block 多模态）
    Anthropic,
    /// OpenAI Chat Completions（`POST /v1/chat/completions`，tool/function 多模态）
    OpenAiChat,
    /// OpenAI Responses API（`POST /v1/responses`，item 数组，Codex CLI 主路径）
    OpenAiResponses,
    /// Google Gemini generateContent（`POST /v1beta/models/{m}:generateContent`）
    Gemini,
    /// Codex CLI 原生 wire format（items 数组 + tool calls，自有 schema）
    CodexNative,
    /// OpenAI Interactions API（CLIProxyAPI 私有扩展，预留）
    Interactions,
    /// Antigravity CLI（Google 内部协议，预留）
    Antigravity,
}

impl Format {
    /// 人类可读标签，仅用于日志/调试。
    pub const fn as_str(self) -> &'static str {
        match self {
            Format::Anthropic => "anthropic",
            Format::OpenAiChat => "openai_chat",
            Format::OpenAiResponses => "openai_responses",
            Format::Gemini => "gemini",
            Format::CodexNative => "codex_native",
            Format::Interactions => "interactions",
            Format::Antigravity => "antigravity",
        }
    }

    /// 路由聚合入口的协议（与 `RouteGroup` 对应）。一个 Format 可以映射到 0 或 1 个入口。
    pub const fn entry_route_group(self) -> Option<crate::route_aggregation::RouteGroup> {
        match self {
            Format::Anthropic => Some(crate::route_aggregation::RouteGroup::ClaudeCode),
            Format::CodexNative => Some(crate::route_aggregation::RouteGroup::Codex),
            Format::OpenAiChat => Some(crate::route_aggregation::RouteGroup::Codex),
            Format::OpenAiResponses => Some(crate::route_aggregation::RouteGroup::Codex),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_as_str_is_stable() {
        assert_eq!(Format::Anthropic.as_str(), "anthropic");
        assert_eq!(Format::OpenAiChat.as_str(), "openai_chat");
        assert_eq!(Format::OpenAiResponses.as_str(), "openai_responses");
        assert_eq!(Format::Gemini.as_str(), "gemini");
        assert_eq!(Format::CodexNative.as_str(), "codex_native");
    }

    #[test]
    fn entry_route_group_maps_clients() {
        use crate::route_aggregation::RouteGroup;
        assert_eq!(Format::Anthropic.entry_route_group(), Some(RouteGroup::ClaudeCode));
        assert_eq!(Format::CodexNative.entry_route_group(), Some(RouteGroup::Codex));
        assert_eq!(Format::OpenAiChat.entry_route_group(), Some(RouteGroup::Codex));
        assert_eq!(Format::OpenAiResponses.entry_route_group(), Some(RouteGroup::Codex));
        assert_eq!(Format::Gemini.entry_route_group(), None);
    }
}