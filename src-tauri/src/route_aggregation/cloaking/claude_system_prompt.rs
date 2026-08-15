//! Claude Code system prompt placement and request-shape normalization.
//!
//! Reference: CLIProxyAPI `claude_executor_cloaking.go`.

use serde_json::{json, Map, Value};

pub const CLAUDE_CODE_AGENT_IDENTIFIER: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

const CURRENT_DATE_PREFIX: &str =
    "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# currentDate\nToday's date is ";

const LEGACY_MODELS: &[&str] = &[
    "claude-3-5-haiku-20241022",
    "claude-3-5-haiku-latest",
    "claude-3-7-sonnet-20250219",
    "claude-3-7-sonnet-latest",
    "claude-haiku-4-5",
    "claude-haiku-4-5-20251001",
    "claude-opus-4",
    "claude-opus-4-20250514",
    "claude-opus-4-1",
    "claude-opus-4-1-20250805",
    "claude-opus-4-5",
    "claude-opus-4-5-20251101",
    "claude-opus-4-6",
    "claude-opus-4-7",
    "claude-sonnet-4",
    "claude-sonnet-4-20250514",
    "claude-sonnet-4-5",
    "claude-sonnet-4-5-20250929",
    "claude-sonnet-4-6",
];

fn text_block(text: impl Into<String>) -> Value {
    json!({"type": "text", "text": text.into()})
}

fn cached_text_block(text: impl Into<String>) -> Value {
    json!({"type": "text", "text": text.into(), "cache_control": {"type": "ephemeral"}})
}

fn model_name(body: &Value) -> String {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    model
        .rsplit_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or(model)
}

pub fn uses_legacy_system_reminder_model(body: &Value) -> bool {
    LEGACY_MODELS.contains(&model_name(body).as_str())
}

fn collect_system_texts(system: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(system) = system else {
        return Ok(Vec::new());
    };
    let mut texts = Vec::new();
    let mut append = |text: &str| {
        if !text.trim().is_empty() && text != CLAUDE_CODE_AGENT_IDENTIFIER {
            texts.push(text.to_string());
        }
    };
    match system {
        Value::String(text) => append(text),
        Value::Array(blocks) => {
            for (index, block) in blocks.iter().enumerate() {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if block_type != "text" {
                    return Err(format!(
                        "invalid_request_error: system.{index}.type: Input should be 'text'."
                    ));
                }
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    append(text);
                }
            }
        }
        _ => return Err("invalid_request_error: system must be a string or text array".into()),
    }
    Ok(texts)
}

fn first_user_index(messages: &[Value]) -> Option<usize> {
    messages
        .iter()
        .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
}

fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn reminder(text: &str) -> String {
    let suffix = if text.ends_with('\n') { "" } else { "\n" };
    format!("<system-reminder>\n{text}{suffix}</system-reminder>")
}

fn prepend_legacy_reminders(messages: &mut [Value], texts: &[String]) {
    let Some(index) = first_user_index(messages) else {
        return;
    };
    let Some(content) = messages[index].get_mut("content") else {
        return;
    };
    let reminders: Vec<Value> = texts
        .iter()
        .map(|text| text_block(reminder(text)))
        .collect();
    match content {
        Value::String(existing) => {
            let mut blocks = reminders;
            blocks.push(text_block(existing.clone()));
            *content = Value::Array(blocks);
        }
        Value::Array(blocks) => {
            let mut insert_at = 0;
            while insert_at < blocks.len()
                && blocks[insert_at].get("type").and_then(Value::as_str) == Some("tool_result")
            {
                insert_at += 1;
            }
            let existing: Vec<String> = blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect();
            let new_blocks: Vec<Value> = reminders
                .into_iter()
                .filter(|block| {
                    !existing.iter().any(|text| {
                        text == block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    })
                })
                .collect();
            blocks.splice(insert_at..insert_at, new_blocks);
        }
        _ => {}
    }
}

fn insert_modern_system_turns(messages: &mut Vec<Value>, texts: &[String]) {
    let Some(first_user) = first_user_index(messages) else {
        return;
    };
    let mut insert_at = first_user + 1;
    while insert_at < messages.len()
        && messages[insert_at].get("role").and_then(Value::as_str) == Some("user")
    {
        insert_at += 1;
    }
    let already_present = messages[insert_at..]
        .iter()
        .take(texts.len())
        .zip(texts)
        .all(|(message, text)| {
            message.get("role").and_then(Value::as_str) == Some("system")
                && message_text(message.get("content").unwrap_or(&Value::Null)) == *text
        });
    if already_present && texts.len() <= messages.len().saturating_sub(insert_at) {
        return;
    }
    let system_messages: Vec<Value> = texts
        .iter()
        .map(|text| json!({"role": "system", "content": [cached_text_block(text)]}))
        .collect();
    messages.splice(insert_at..insert_at, system_messages);
}

fn current_date_reminder() -> String {
    let date = chrono::Local::now().format("%Y-%m-%d");
    format!("{CURRENT_DATE_PREFIX}{date}.\n\n      IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>\n")
}

fn inject_current_date(messages: &mut [Value]) {
    let Some(index) = first_user_index(messages) else {
        return;
    };
    let Some(content) = messages[index].get_mut("content") else {
        return;
    };
    let date_text = current_date_reminder();
    match content {
        Value::String(existing) => {
            *content = Value::Array(vec![
                text_block(date_text),
                cached_text_block(existing.clone()),
            ])
        }
        Value::Array(blocks) => {
            blocks.retain(|block| {
                !block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| text.starts_with(CURRENT_DATE_PREFIX))
                    .unwrap_or(false)
            });
            if let Some(block) = blocks.iter_mut().find(|block| {
                block.get("type").and_then(Value::as_str) == Some("text")
                    && !block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| text.starts_with("<system-reminder>"))
                        .unwrap_or(false)
            }) {
                let mut cache = Map::new();
                cache.insert("type".into(), Value::String("ephemeral".into()));
                block["cache_control"] = Value::Object(cache);
            }
            blocks.insert(0, text_block(date_text));
        }
        _ => {}
    }
}

/// Apply Claude Code's top-level system shape and caller-system placement.
pub fn apply_system_policy(
    body: &mut Value,
    strict_mode: bool,
    billing_header: &str,
) -> Result<(), String> {
    let forwarded = collect_system_texts(body.get("system"))?;
    let mut messages = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "invalid_request_error: messages must be an array".to_string())?;
    body["system"] = json!([
        text_block(billing_header),
        cached_text_block(CLAUDE_CODE_AGENT_IDENTIFIER)
    ]);
    if !strict_mode && !forwarded.is_empty() {
        if uses_legacy_system_reminder_model(body) {
            prepend_legacy_reminders(&mut messages, &forwarded);
        } else {
            insert_modern_system_turns(&mut messages, &forwarded);
        }
    }
    inject_current_date(&mut messages);
    body["messages"] = Value::Array(messages);
    Ok(())
}

/// `count_tokens` uses the measured minimal shape and must not receive a new top-level system array.
pub fn relocate_for_count_tokens(body: &mut Value, strict_mode: bool) -> Result<(), String> {
    let forwarded = collect_system_texts(body.get("system"))?;
    body.as_object_mut()
        .ok_or_else(|| "invalid_request_error: request body must be an object".to_string())?
        .remove("system");
    if strict_mode || forwarded.is_empty() {
        return Ok(());
    }
    let legacy = uses_legacy_system_reminder_model(body);
    let messages = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "invalid_request_error: messages must be an array".to_string())?;
    if legacy {
        prepend_legacy_reminders(messages, &forwarded);
    } else {
        insert_modern_system_turns(messages, &forwarded);
    }
    Ok(())
}

#[allow(dead_code)]
pub fn build_system_array(_original_system: Option<&str>) -> Vec<Value> {
    vec![
        text_block(""),
        cached_text_block(CLAUDE_CODE_AGENT_IDENTIFIER),
    ]
}

#[cfg(test)]
mod tests {
    use super::{apply_system_policy, relocate_for_count_tokens};
    use serde_json::json;

    #[test]
    fn places_modern_system_turn_after_user_messages() {
        let mut body = json!({"model": "claude-opus-5", "system": "Follow the project rules.", "messages": [{"role": "user", "content": "Hello"}]});
        apply_system_policy(
            &mut body,
            false,
            "x-anthropic-billing-header: cc_entrypoint=cli;",
        )
        .unwrap();
        assert_eq!(body["system"][0]["type"], "text");
        assert_eq!(body["messages"][1]["role"], "system");
        assert_eq!(
            body["messages"][1]["content"][0]["text"],
            "Follow the project rules."
        );
    }

    #[test]
    fn legacy_system_prompt_becomes_reminder() {
        let mut body = json!({"model": "claude-3-7-sonnet-latest", "system": [{"type": "text", "text": "Be concise."}], "messages": [{"role": "user", "content": "Hello"}]});
        apply_system_policy(&mut body, false, "billing").unwrap();
        assert!(body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("<system-reminder>"));
    }

    #[test]
    fn count_tokens_removes_top_level_system() {
        let mut body = json!({"model": "claude-opus-5", "system": "Count this.", "messages": [{"role": "user", "content": "Hello"}]});
        relocate_for_count_tokens(&mut body, false).unwrap();
        assert!(body.get("system").is_none());
        assert_eq!(body["messages"][1]["role"], "system");
    }

    #[test]
    fn rejects_non_text_system_blocks() {
        let mut body = json!({"messages": [{"role": "user", "content": "Hello"}], "system": [{"type": "image", "source": {}}]});
        let error = apply_system_policy(&mut body, false, "billing").unwrap_err();
        assert!(error.contains("system.0.type"));
    }
}
