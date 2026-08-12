//! 流式响应状态机（StreamParams）——跨 SSE chunk 累积翻译所需的中间状态。
//!
//! CLIProxyAPI aligned: 934da237 - fix(openai): preserve structured and stringified
//!                        custom tool outputs during Responses conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/934da2379d6272a704953a02322b666b2a2efa3e
//! Last verified: 2026-08-12
//!
//! 设计参考 CLIProxyAPI `internal/translator/gemini/claude/gemini_claude_response.go`
//! 的 `Params` struct。每个 SSE chunk 由翻译器消费，过程中更新本结构；调用方在
//! "一次响应"开始时 `Default::default()` 一个新实例，结束后丢弃。

use std::collections::HashMap;

/// 当前 content_block 的语义类型。`None` 表示尚未产出任何 block。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponseType {
    #[default]
    None,
    /// 文本 block（Anthropic `text` / OpenAI `content` / Gemini `text`）
    Text,
    /// 思考 block（Anthropic `thinking` / Gemini `thought` / OpenAI `reasoning`）
    Thinking,
    /// 工具调用 block（Anthropic `tool_use` / OpenAI `tool_calls` / Gemini `functionCall`）
    ToolUse,
}

impl ResponseType {
    pub const fn as_str(self) -> &'static str {
        match self {
            ResponseType::None => "none",
            ResponseType::Text => "text",
            ResponseType::Thinking => "thinking",
            ResponseType::ToolUse => "tool_use",
        }
    }
}

/// 流式状态机的累积状态。每个请求响应一组，独立于其它请求。
#[derive(Debug, Default)]
pub struct StreamParams {
    /// 当前正在产出的 block 语义类型。
    pub response_type: ResponseType,
    /// 当前正在产出的 block 索引（Anthropic SSE 需要，OpenAI 不用）。
    pub response_index: u32,
    /// 是否已经产出过至少一个 block（用于决定 message_delta stop_reason 归并策略）。
    pub has_content: bool,
    /// 客户端原名 → sanitize 后的名。翻译工具调用时由 `common::tool_name` 写入。
    pub tool_name_map: HashMap<String, String>,
    /// sanitize 后的名 → 客户端原名。响应方向反向映射时使用。
    pub sanitized_name_map: HashMap<String, String>,
    /// 当前 chunk 对应的 tool call id（仅在 `response_type == ToolUse` 时有效）。
    pub current_tool_call_id: Option<String>,
    /// 当前 chunk 对应的 tool call name（仅在 `response_type == ToolUse` 且首 chunk 时有效）。
    pub current_tool_call_name: Option<String>,
    /// 已累积的 thinking signature。Anthropic 多轮回放需要此值。
    pub thinking_signature: Option<String>,
    /// 响应是否已经发出 `response.completed` 终止事件。
    ///
    /// 对齐 CLIProxyAPI `4d9bf91` 修复：完成事件只发一次。`[DONE]` 与
    /// `finishReason` 同时到达时（如上游 buggy OpenAI 兼容 server）也要幂等。
    pub completed: bool,
}

impl StreamParams {
    pub fn new() -> Self {
        Self::default()
    }

    /// 重置到初始状态（用于非流式响应也复用一个实例）。
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_none() {
        let p = StreamParams::default();
        assert_eq!(p.response_type, ResponseType::None);
        assert_eq!(p.response_index, 0);
        assert!(!p.has_content);
        assert!(p.tool_name_map.is_empty());
        assert!(p.current_tool_call_id.is_none());
        assert!(p.thinking_signature.is_none());
        assert!(!p.completed);
    }

    #[test]
    fn reset_clears_state() {
        let mut p = StreamParams::default();
        p.response_type = ResponseType::ToolUse;
        p.response_index = 3;
        p.has_content = true;
        p.tool_name_map.insert("foo".into(), "foo_a".into());
        p.thinking_signature = Some("sig".into());

        p.reset();
        assert_eq!(p.response_type, ResponseType::None);
        assert!(p.tool_name_map.is_empty());
        assert!(p.thinking_signature.is_none());
    }

    #[test]
    fn response_type_label() {
        assert_eq!(ResponseType::None.as_str(), "none");
        assert_eq!(ResponseType::Text.as_str(), "text");
        assert_eq!(ResponseType::Thinking.as_str(), "thinking");
        assert_eq!(ResponseType::ToolUse.as_str(), "tool_use");
    }

    /// 模拟跨 chunk 状态机：第一个 chunk 见到 tool name 后，
    /// 后续 chunk 通过 tool_name_map 仍然能查到原名。
    #[test]
    fn tool_name_map_roundtrip_across_chunks() {
        let mut p = StreamParams::default();
        let original = "search__web";
        let sanitized = "search__web_a1b2";
        p.tool_name_map.insert(original.into(), sanitized.into());
        p.sanitized_name_map.insert(sanitized.into(), original.into());

        // 第二 chunk 拿 sanitize 名反查
        let recovered = p.sanitized_name_map.get(sanitized).cloned().unwrap_or_default();
        assert_eq!(recovered, original);
    }
}