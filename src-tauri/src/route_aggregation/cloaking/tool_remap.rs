//! OAuth tool name remapping.
//! Reference: CLIProxyAPI claude_executor_request.go remapOAuthToolNames /
//! restoreClaudeOAuthToolNamesFromResponse（请求级别名表语义）。
//!
//! Remaps third-party tool names to Claude Code official names to prevent
//! detection via tool naming patterns.
//!
//! 与上游一致，映射表按请求构建：remap 时记录本请求实际产生的
//! `目标名 → 原始名` 表，响应恢复时只按该表精确反解。避免全局反向查找把
//! 恰好等于 Claude 官方名（如 "Bash"）的客户端自定义工具误改回小写。

use std::collections::HashMap;

/// Map of lowercase tool name → Claude Code official name.
pub static OAUTH_TOOL_RENAME_MAP: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("read", "Read"),
    ("write", "Write"),
    ("edit", "Edit"),
    ("glob", "Glob"),
    ("grep", "Grep"),
    ("task", "Task"),
    ("webfetch", "WebFetch"),
    ("websearch", "WebSearch"),
    ("todowrite", "TodoWrite"),
    ("bashinput", "BashInput"),
    ("notebookedit", "NotebookEdit"),
    ("multiedit", "MultiEdit"),
    ("ls", "LS"),
    ("view", "View"),
];

/// 把名字 remap 为 Claude Code 官方名；实际发生改名时记录 `改名后 → 改名前的
/// 原始名`（first-seen 语义：同一目标名来自多处时保留首次出现的原始名）。
fn remap_name(name: &mut serde_json::Value, map: &mut HashMap<String, String>) {
    let Some(value) = name.as_str() else { return };
    let lower = value.to_ascii_lowercase();
    if let Some((_, target)) = OAUTH_TOOL_RENAME_MAP
        .iter()
        .find(|(source, _)| *source == lower)
    {
        let original = value.to_string();
        let renamed = (*target).to_string();
        if original != renamed {
            *name = serde_json::Value::String(renamed.clone());
            map.entry(renamed).or_insert(original);
        }
    }
}

/// 按本次请求的映射表精确反解：只有本请求 remap 过的目标名才恢复。
fn reverse_name(name: &mut serde_json::Value, map: &HashMap<String, String>) {
    let Some(value) = name.as_str() else { return };
    if let Some(original) = map.get(value) {
        *name = serde_json::Value::String(original.clone());
    }
}

fn remap_content_blocks(value: &mut serde_json::Value, map: &mut HashMap<String, String>) -> bool {
    match value {
        serde_json::Value::Array(values) => values
            .iter_mut()
            .any(|value| remap_content_blocks(value, map)),
        serde_json::Value::Object(map_value) => {
            let mut changed = false;
            let is_tool_use =
                map_value.get("type").and_then(serde_json::Value::as_str) == Some("tool_use");
            if is_tool_use {
                if let Some(name) = map_value.get_mut("name") {
                    let before = name.clone();
                    remap_name(name, map);
                    changed |= *name != before;
                }
            }
            if let Some(content) = map_value.get_mut("content") {
                changed |= remap_content_blocks(content, map);
            }
            if let Some(input) = map_value.get_mut("input") {
                changed |= remap_content_blocks(input, map);
            }
            if let Some(content_block) = map_value.get_mut("content_block") {
                changed |= remap_content_blocks(content_block, map);
            }
            changed
        }
        _ => false,
    }
}

fn reverse_content_blocks(value: &mut serde_json::Value, map: &HashMap<String, String>) -> bool {
    match value {
        serde_json::Value::Array(values) => values
            .iter_mut()
            .any(|value| reverse_content_blocks(value, map)),
        serde_json::Value::Object(map_value) => {
            let mut changed = false;
            let is_tool_use =
                map_value.get("type").and_then(serde_json::Value::as_str) == Some("tool_use");
            if is_tool_use {
                if let Some(name) = map_value.get_mut("name") {
                    let before = name.clone();
                    reverse_name(name, map);
                    changed |= *name != before;
                }
            }
            if let Some(content) = map_value.get_mut("content") {
                changed |= reverse_content_blocks(content, map);
            }
            if let Some(input) = map_value.get_mut("input") {
                changed |= reverse_content_blocks(input, map);
            }
            if let Some(content_block) = map_value.get_mut("content_block") {
                changed |= reverse_content_blocks(content_block, map);
            }
            changed
        }
        _ => false,
    }
}

/// Remap tool names in the request body's `tools` array to Claude Code official names.
///
/// 返回本次请求的 `目标名 → 原始名` 映射表，调用方必须在响应侧原样传回
/// `reverse_remap_tool_names_in_response` 做对称恢复。
pub fn remap_tool_names_in_request(body: &mut serde_json::Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools.iter_mut() {
            if let Some(name) = tool.get_mut("name") {
                remap_name(name, &mut map);
            }
        }
    }

    // Also remap tool calls in messages
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for message in messages.iter_mut() {
            if let Some(content) = message.get_mut("content") {
                remap_content_blocks(content, &mut map);
            }
        }
    }
    map
}

/// Reverse-remap tool names in the response body back to original names, using
/// the request-scoped map returned by `remap_tool_names_in_request`.
#[allow(dead_code)]
pub fn reverse_remap_tool_names_in_response(
    body: &mut serde_json::Value,
    map: &HashMap<String, String>,
) -> bool {
    if map.is_empty() {
        return false;
    }
    if let Some(content) = body.get_mut("content") {
        reverse_content_blocks(content, map)
    } else {
        reverse_content_blocks(body, map)
    }
}

#[cfg(test)]
mod tests {
    use super::{remap_tool_names_in_request, reverse_remap_tool_names_in_response};

    #[test]
    fn request_and_response_tool_names_round_trip() {
        let mut request = serde_json::json!({
            "tools": [{"name": "bash"}],
            "messages": [{"content": [{"type": "tool_use", "name": "webfetch", "input": {"command": "ls"}}]}]
        });
        let map = remap_tool_names_in_request(&mut request);
        assert_eq!(request["tools"][0]["name"], "Bash");
        assert_eq!(request["messages"][0]["content"][0]["name"], "WebFetch");

        let mut response = serde_json::json!({"content": [{"type": "tool_use", "name": "Bash"}]});
        assert!(reverse_remap_tool_names_in_response(&mut response, &map));
        assert_eq!(response["content"][0]["name"], "bash");
    }

    #[test]
    fn does_not_remap_arbitrary_input_name_fields() {
        let mut request = serde_json::json!({
            "tools": [{"name": "bash"}]
        });
        let map = remap_tool_names_in_request(&mut request);

        let mut response = serde_json::json!({
            "content": [{
                "type": "tool_use",
                "name": "Bash",
                "input": {"name": "Bash"}
            }]
        });
        assert!(reverse_remap_tool_names_in_response(&mut response, &map));
        assert_eq!(response["content"][0]["name"], "bash");
        assert_eq!(response["content"][0]["input"]["name"], "Bash");
    }

    #[test]
    fn official_case_name_is_not_reversed_without_remap() {
        // 客户端工具名本身就是 Claude 官方名 "Bash"（大写）：remap 等值不记录
        // 映射，响应恢复不得把 "Bash" 误改回 "bash"（旧全局反向查找的碰撞 bug）。
        let mut request = serde_json::json!({
            "tools": [{"name": "Bash"}]
        });
        let map = remap_tool_names_in_request(&mut request);
        assert!(map.is_empty(), "等值 remap 不应记录映射");

        let mut response = serde_json::json!({"content": [{"type": "tool_use", "name": "Bash"}]});
        assert!(!reverse_remap_tool_names_in_response(&mut response, &map));
        assert_eq!(response["content"][0]["name"], "Bash");
    }

    #[test]
    fn empty_map_never_touches_response() {
        let mut response = serde_json::json!({"content": [{"type": "tool_use", "name": "Read"}]});
        let map = std::collections::HashMap::new();
        assert!(!reverse_remap_tool_names_in_response(&mut response, &map));
        assert_eq!(response["content"][0]["name"], "Read");
    }
}
