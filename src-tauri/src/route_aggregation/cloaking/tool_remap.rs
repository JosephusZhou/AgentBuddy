//! OAuth tool name remapping.
//! Reference: CLIProxyAPI oauthToolRenameMap.
//!
//! Remaps third-party tool names to Claude Code official names to prevent
//! detection via tool naming patterns.

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

fn remap_name(name: &mut serde_json::Value) {
    let Some(value) = name.as_str() else { return };
    let lower = value.to_ascii_lowercase();
    if let Some((_, target)) = OAUTH_TOOL_RENAME_MAP
        .iter()
        .find(|(source, _)| *source == lower)
    {
        *name = serde_json::Value::String((*target).to_string());
    }
}

fn reverse_name(name: &mut serde_json::Value) {
    let Some(value) = name.as_str() else { return };
    if let Some((source, _)) = OAUTH_TOOL_RENAME_MAP
        .iter()
        .find(|(_, target)| *target == value)
    {
        *name = serde_json::Value::String((*source).to_string());
    }
}

fn remap_content_blocks(value: &mut serde_json::Value, reverse: bool) -> bool {
    match value {
        serde_json::Value::Array(values) => values
            .iter_mut()
            .any(|value| remap_content_blocks(value, reverse)),
        serde_json::Value::Object(map) => {
            let mut changed = false;
            let is_tool_use =
                map.get("type").and_then(serde_json::Value::as_str) == Some("tool_use");
            if is_tool_use {
                if let Some(name) = map.get_mut("name") {
                    let before = name.clone();
                    if reverse {
                        reverse_name(name);
                    } else {
                        remap_name(name);
                    }
                    changed |= *name != before;
                }
            }
            if let Some(content) = map.get_mut("content") {
                changed |= remap_content_blocks(content, reverse);
            }
            if let Some(input) = map.get_mut("input") {
                changed |= remap_content_blocks(input, reverse);
            }
            if let Some(content_block) = map.get_mut("content_block") {
                changed |= remap_content_blocks(content_block, reverse);
            }
            changed
        }
        _ => false,
    }
}

/// Remap tool names in the request body's `tools` array to Claude Code official names.
pub fn remap_tool_names_in_request(body: &mut serde_json::Value) {
    if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools.iter_mut() {
            if let Some(name) = tool.get_mut("name") {
                remap_name(name);
            }
        }
    }

    // Also remap tool calls in messages
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for message in messages.iter_mut() {
            if let Some(content) = message.get_mut("content") {
                remap_content_blocks(content, false);
            }
        }
    }
}

/// Reverse-remap tool names in the response body back to original names.
#[allow(dead_code)]
pub fn reverse_remap_tool_names_in_response(body: &mut serde_json::Value) -> bool {
    if let Some(content) = body.get_mut("content") {
        remap_content_blocks(content, true)
    } else {
        remap_content_blocks(body, true)
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
        remap_tool_names_in_request(&mut request);
        assert_eq!(request["tools"][0]["name"], "Bash");
        assert_eq!(request["messages"][0]["content"][0]["name"], "WebFetch");

        let mut response = serde_json::json!({"content": [{"type": "tool_use", "name": "Bash"}]});
        assert!(reverse_remap_tool_names_in_response(&mut response));
        assert_eq!(response["content"][0]["name"], "bash");
    }

    #[test]
    fn does_not_remap_arbitrary_input_name_fields() {
        let mut response = serde_json::json!({
            "content": [{
                "type": "tool_use",
                "name": "Bash",
                "input": {"name": "Bash"}
            }]
        });
        assert!(reverse_remap_tool_names_in_response(&mut response));
        assert_eq!(response["content"][0]["name"], "bash");
        assert_eq!(response["content"][0]["input"]["name"], "Bash");
    }
}
