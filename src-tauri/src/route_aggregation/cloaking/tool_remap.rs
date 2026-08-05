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

/// Remap tool names in the request body's `tools` array to Claude Code official names.
pub fn remap_tool_names_in_request(body: &mut serde_json::Value) {
    if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools.iter_mut() {
            if let Some(name) = tool.get("name").and_then(|n| n.as_str()) {
                let lower = name.to_lowercase();
                for (from, to) in OAUTH_TOOL_RENAME_MAP {
                    if lower == *from {
                        if let Some(name_val) = tool.get_mut("name") {
                            *name_val = serde_json::Value::String(to.to_string());
                        }
                        break;
                    }
                }
            }
        }
    }

    // Also remap tool calls in messages
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for message in messages.iter_mut() {
            if let Some(content) = message.get_mut("content").and_then(|c| c.as_array_mut()) {
                for block in content.iter_mut() {
                    if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                        let lower = name.to_lowercase();
                        for (from, to) in OAUTH_TOOL_RENAME_MAP {
                            if lower == *from {
                                if let Some(name_val) = block.get_mut("name") {
                                    *name_val = serde_json::Value::String(to.to_string());
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Reverse-remap tool names in the response body back to original names.
#[allow(dead_code)]
pub fn reverse_remap_tool_names_in_response(body: &mut serde_json::Value) {
    // Build reverse map
    let reverse: std::collections::HashMap<&str, &str> = OAUTH_TOOL_RENAME_MAP
        .iter()
        .map(|(from, to)| (*to, *from))
        .collect();

    // Remap in any tool_use blocks within response content
    if let Some(content) = body.get_mut("content").and_then(|c| c.as_array_mut()) {
        for block in content.iter_mut() {
            if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                if let Some(&original) = reverse.get(name) {
                    if let Some(name_val) = block.get_mut("name") {
                        *name_val = serde_json::Value::String(original.to_string());
                    }
                }
            }
        }
    }
}
