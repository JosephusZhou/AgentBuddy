//! Claude 多 turn 累积器：合并相邻同 role turn、保持 assistant 内 tool_use 末尾。
//!
//! CLIProxyAPI aligned: 71e8711 - fix(claude): accumulate consecutive role turns
//!                         during request conversion
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/71e87111e9d8e6c0ce3c2d0a419b136fee2e10b0
//! Last verified: 2026-08-12
//!
//! ## 设计要点
//!
//! - 跨请求方向共用：当前 route_aggregation 在两类请求方向需要这个语义：
//!   - `claude_gemini/request.rs`（Anthropic Messages → Gemini）
//!   - 未来 `openai_gemini/request.rs` 可选（OpenAI Chat → Gemini，需不需要取决于客户端）
//! - 输入/输出都是 Gemini 格式 content node（`{"role": "user"|"model", "parts": [...]}`），
//!   由此保持 helper 通用，不耦合 source protocol。
//! - 合并规则：
//!   - **同 role 合并**（`user` ↔ `user`、`model` ↔ `model`）
//!   - **跨 role 不合并**（Flush 上一 turn 后再开启新 turn）
//!   - **assistant (`model`) 内 tool_use 移到末尾**（thinking → text → tool_use）
//!   - **user 内保留原序**（tool_result 与 text 混合）
//! - 跳过空 parts / null / 非 user/model role message，但不破坏当前 turn。
//! - `flush()` 手动控制边界（例如 system instruction 之后的普通 user 应分开）。

use serde_json::{Map, Value};

/// 累积当前 turn 的 Gemini 格式 parts 并最终输出合并后的 contents。
///
/// 状态机：
/// - `current_role: None` → 初始 / 上一 turn 已 flush
/// - `current_role: Some(role)` → 当前 turn 正在累积
#[derive(Debug, Default, Clone)]
pub struct ClaudeMessageAccumulator {
    current_role: Option<String>,
    content_parts: Vec<Value>,
    tool_use_parts: Vec<Value>,
    messages: Vec<Value>,
}

impl ClaudeMessageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个 Gemini 格式 content node（`{"role": "user"|"model", "parts": [...]}`）。
    ///
    /// 行为：
    /// - 仅接受 `user` / `model` role；其它 role 跳过。
    /// - 空 `parts` 数组跳过（不破坏当前 turn）。
    /// - 当 `current_role` 与新 node 的 role 不一致时，自动 flush 上一 turn。
    /// - 对 `model` role：含 `functionCall` 的 part 临时收集到 `tool_use_parts`，
    ///   其它 part 进入 `content_parts`，flush 时 `tool_use_parts` 拼到末尾。
    /// - 对 `user` role：保留原 part 顺序。
    pub fn append(&mut self, node: &Value) {
        let Some(obj) = node.as_object() else {
            return;
        };
        let role = match obj.get("role").and_then(|v| v.as_str()) {
            Some(r) if r == "user" || r == "model" => r.to_string(),
            _ => return,
        };
        let parts = match obj.get("parts").and_then(|v| v.as_array()) {
            Some(p) if !p.is_empty() => p,
            _ => return,
        };

        if let Some(curr) = &self.current_role {
            if curr != &role {
                self.flush();
            }
        }
        self.current_role = Some(role.clone());

        for part in parts {
            if role == "model" && part.get("functionCall").is_some() {
                self.tool_use_parts.push(part.clone());
            } else {
                self.content_parts.push(part.clone());
            }
        }
    }

    /// 强制 flush 当前 turn（写入并 reset 内部状态）。
    ///
    /// 当前没有 pending turn 时，no-op。
    /// 允许调用方在任意位置插入 turn 边界（例如 system instruction 后）。
    pub fn flush(&mut self) {
        let Some(role) = self.current_role.take() else {
            return;
        };
        if self.content_parts.is_empty() && self.tool_use_parts.is_empty() {
            return;
        }
        let mut parts = std::mem::take(&mut self.content_parts);
        parts.extend(std::mem::take(&mut self.tool_use_parts));

        let msg = Value::Object(Map::from_iter([
            ("role".into(), Value::String(role)),
            ("parts".into(), Value::Array(parts)),
        ]));
        self.messages.push(msg);
    }

    /// Flush 并返回累积的全部 messages。
    pub fn into_messages(mut self) -> Vec<Value> {
        self.flush();
        self.messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn part_text(text: &str) -> Value {
        json!({"text": text})
    }

    fn part_function_call(id: &str, name: &str) -> Value {
        json!({"functionCall": {"id": id, "name": name, "args": {}}})
    }

    fn part_function_response(id: &str, result: &str) -> Value {
        json!({"functionResponse": {"id": id, "response": {"result": result}}})
    }

    fn node(role: &str, parts: Vec<Value>) -> Value {
        json!({"role": role, "parts": parts})
    }

    /// 对齐 CLIProxyAPI `TestClaudeMessageAccumulatorGroupsAndOrdersAssistantParts`。
    /// assistant turn 内 tool_use 移到 content 末尾，跨多个 message 合并。
    #[test]
    fn groups_and_orders_assistant_parts() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&node(
            "model",
            vec![part_function_call("call_1", "first")],
        ));
        acc.append(&node(
            "model",
            vec![
                json!({"thinking": "reason"}),
                part_text("answer"),
            ],
        ));
        acc.append(&node(
            "model",
            vec![part_function_call("call_2", "second")],
        ));

        let messages = acc.into_messages();
        assert_eq!(messages.len(), 1);
        let parts = messages[0].get("parts").unwrap().as_array().unwrap();
        assert_eq!(parts.len(), 4);
        // 顺序：thinking → text → tool_use → tool_use
        assert_eq!(parts[0].get("thinking").unwrap(), "reason");
        assert_eq!(parts[1].get("text").unwrap(), "answer");
        assert_eq!(parts[2]["functionCall"]["id"], "call_1");
        assert_eq!(parts[3]["functionCall"]["id"], "call_2");
    }

    /// 对齐 CLIProxyAPI `TestClaudeMessageAccumulatorPreservesUserOrderAndRoleBoundaries`。
    /// user turn 保留原序（tool_result + text）。
    #[test]
    fn preserves_user_order_and_role_boundaries() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&node(
            "user",
            vec![part_function_response("call_1", "ok")],
        ));
        acc.append(&node(
            "user",
            vec![part_text("continue")],
        ));
        acc.append(&node(
            "model",
            vec![part_text("done")],
        ));

        let messages = acc.into_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].get("role").unwrap(), "user");
        let user_parts = messages[0].get("parts").unwrap().as_array().unwrap();
        assert_eq!(user_parts.len(), 2);
        assert!(user_parts[0].get("functionResponse").is_some());
        assert_eq!(user_parts[1].get("text").unwrap(), "continue");
        assert_eq!(messages[1].get("role").unwrap(), "model");
    }

    /// 对齐 CLIProxyAPI `TestClaudeMessageAccumulatorSkipsEmptyMessagesWithoutBreakingTurn`。
    /// 空 / null / 非 user/model role 的 message 跳过但不破坏当前 turn。
    #[test]
    fn skips_empty_messages_without_breaking_turn() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&node(
            "model",
            vec![part_text("first")],
        ));
        // 后续各种垃圾输入
        acc.append(&json!({"role": "user"}));
        acc.append(&json!({"role": "user", "content": null}));
        acc.append(&json!({"role": "user", "content": ""}));
        acc.append(&json!({"role": "user", "parts": []}));
        acc.append(&json!({"role": "developer", "parts": [part_text("ignored")]}));
        acc.append(&node(
            "model",
            vec![part_text("second")],
        ));

        let messages = acc.into_messages();
        assert_eq!(messages.len(), 1);
        let parts = messages[0].get("parts").unwrap().as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].get("text").unwrap(), "first");
        assert_eq!(parts[1].get("text").unwrap(), "second");
    }

    /// 对齐 CLIProxyAPI `TestClaudeMessageAccumulatorFlushPreservesExplicitBoundary`。
    /// 手动 flush 插入 turn 边界（例如 system instruction 之后）。
    #[test]
    fn flush_preserves_explicit_boundary() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&node("user", vec![part_text("system reminder")]));
        acc.flush(); // 显式边界
        acc.append(&node("user", vec![part_text("question")]));

        let messages = acc.into_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["parts"][0]["text"], "system reminder");
        assert_eq!(messages[1]["parts"][0]["text"], "question");
    }

    /// 对齐 CLIProxyAPI `TestClaudeMessageAccumulatorPreservesBlockCacheControl`。
    /// cache_control 等额外字段（不在标准 part 字段里）原样保留。
    #[test]
    fn preserves_block_cache_control() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&node(
            "user",
            vec![json!({"text": "cached", "cache_control": {"type": "ephemeral"}})],
        ));
        acc.append(&node("user", vec![part_text("fresh")]));

        let messages = acc.into_messages();
        let first = &messages[0]["parts"][0];
        assert_eq!(
            first.get("cache_control").and_then(|v| v.get("type")).unwrap(),
            "ephemeral"
        );
        assert!(messages[0]["parts"][1].get("cache_control").is_none());
    }

    /// 边界：空 accumulator 直接返回空数组。
    #[test]
    fn empty_accumulator_returns_empty() {
        let acc = ClaudeMessageAccumulator::new();
        let messages = acc.into_messages();
        assert!(messages.is_empty());
    }

    /// 边界：非 object 输入（string / null）静默跳过。
    #[test]
    fn non_object_input_is_silently_skipped() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&json!("not an object"));
        acc.append(&json!(null));
        acc.append(&json!(42));
        let messages = acc.into_messages();
        assert!(messages.is_empty());
    }

    /// 边界：单 user message 单 turn，无 flush 行为干扰。
    #[test]
    fn single_user_message_round_trip() {
        let mut acc = ClaudeMessageAccumulator::new();
        acc.append(&node("user", vec![part_text("hi")]));
        let messages = acc.into_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["parts"][0]["text"], "hi");
    }
}
