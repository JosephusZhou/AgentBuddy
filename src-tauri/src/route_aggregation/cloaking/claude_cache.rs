//! Claude prompt-cache breakpoint placement and normalization.

use serde_json::Value;

fn has_cache_control(block: &Value) -> bool {
    block.get("cache_control").is_some()
}
fn set_ephemeral(block: &mut Value) {
    if block.is_object() && !has_cache_control(block) {
        block["cache_control"] = serde_json::json!({"type": "ephemeral"});
    }
}

fn visit_blocks_mut(body: &mut Value, mut visit: impl FnMut(&mut Value)) {
    if let Some(blocks) = body.get_mut("tools").and_then(Value::as_array_mut) {
        for block in blocks {
            visit(block);
        }
    }
    if let Some(blocks) = body.get_mut("system").and_then(Value::as_array_mut) {
        for block in blocks {
            visit(block);
        }
    }
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
                for block in blocks {
                    visit(block);
                }
            }
        }
    }
}

pub fn count_cache_controls(body: &Value) -> usize {
    let mut copy = body.clone();
    let mut count = 0;
    visit_blocks_mut(&mut copy, |block| {
        if has_cache_control(block) {
            count += 1
        }
    });
    count
}

pub fn ensure_cache_control(body: &mut Value) {
    if body
        .get("system")
        .and_then(Value::as_array)
        .map(|blocks| !blocks.is_empty())
        .unwrap_or(false)
    {
        if let Some(last) = body
            .get_mut("system")
            .and_then(Value::as_array_mut)
            .and_then(|blocks| blocks.last_mut())
        {
            set_ephemeral(last);
        }
        return;
    }
    if let Some(last) = body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|blocks| blocks.last_mut())
    {
        set_ephemeral(last);
        return;
    }
    if let Some(message) = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|messages| messages.last_mut())
    {
        match message.get_mut("content") {
            Some(Value::Array(blocks)) => {
                if let Some(block) = blocks
                    .iter_mut()
                    .rev()
                    .find(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                {
                    set_ephemeral(block);
                }
            }
            Some(Value::String(text)) => {
                let original = std::mem::take(text);
                message["content"] = serde_json::json!([{"type": "text", "text": original, "cache_control": {"type": "ephemeral"}}]);
            }
            _ => {}
        }
    }
}

pub fn upgrade_ttl(body: &mut Value, ttl: &str) {
    if ttl.is_empty() {
        return;
    }
    visit_blocks_mut(body, |block| {
        if let Some(cache) = block
            .get_mut("cache_control")
            .filter(|value| value.is_object())
        {
            if cache.get("ttl").is_none() {
                cache["ttl"] = Value::String(ttl.to_string());
            }
        }
    });
}

pub fn normalize_ttl_order(body: &mut Value) {
    let mut seen_short = false;
    visit_blocks_mut(body, |block| {
        let Some(cache) = block
            .get_mut("cache_control")
            .filter(|value| value.is_object())
        else {
            return;
        };
        match cache.get("ttl").and_then(Value::as_str) {
            Some("1h") if seen_short => {
                cache.as_object_mut().unwrap().remove("ttl");
            }
            Some("1h") => {}
            _ => seen_short = true,
        }
    });
}

fn remove_cache_control(block: &mut Value) -> bool {
    block
        .as_object_mut()
        .and_then(|object| object.remove("cache_control"))
        .is_some()
}

pub fn enforce_limit(body: &mut Value, max_blocks: usize) {
    while count_cache_controls(body) > max_blocks {
        let mut removed = false;
        if let Some(blocks) = body.get_mut("system").and_then(Value::as_array_mut) {
            if let Some(last) = blocks.iter().rposition(has_cache_control) {
                for (index, block) in blocks.iter_mut().enumerate() {
                    if index != last && has_cache_control(block) {
                        removed = remove_cache_control(block);
                        if removed {
                            break;
                        }
                    }
                }
            }
        }
        if removed {
            continue;
        }
        if let Some(blocks) = body.get_mut("tools").and_then(Value::as_array_mut) {
            if let Some(last) = blocks.iter().rposition(has_cache_control) {
                for (index, block) in blocks.iter_mut().enumerate() {
                    if index != last && has_cache_control(block) {
                        removed = remove_cache_control(block);
                        if removed {
                            break;
                        }
                    }
                }
            }
        }
        if removed {
            continue;
        }
        if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
            for message in messages {
                if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
                    if let Some(block) = blocks.iter_mut().find(|block| has_cache_control(block)) {
                        removed = remove_cache_control(block);
                    }
                }
                if removed {
                    break;
                }
            }
        }
        if !removed {
            break;
        }
    }
}

pub fn normalize(body: &mut Value, max_blocks: usize, ttl: Option<&str>) {
    ensure_cache_control(body);
    if let Some(ttl) = ttl {
        upgrade_ttl(body, ttl);
    }
    normalize_ttl_order(body);
    enforce_limit(body, max_blocks);
}

#[cfg(test)]
mod tests {
    use super::{count_cache_controls, normalize};

    #[test]
    fn places_and_limits_cache_breakpoints() {
        let mut body = serde_json::json!({"system": [{"type": "text", "text": "one", "cache_control": {"type": "ephemeral"}}, {"type": "text", "text": "two"}], "tools": [{"name": "a", "cache_control": {"type": "ephemeral"}}], "messages": [{"content": [{"type": "text", "text": "hello", "cache_control": {"type": "ephemeral"}}]}]});
        normalize(&mut body, 2, None);
        assert_eq!(count_cache_controls(&body), 2);
        assert!(body["system"][1].get("cache_control").is_some());
    }

    #[test]
    fn downgrades_invalid_ttl_order() {
        let mut body = serde_json::json!({"system": [{"type": "text", "cache_control": {"type": "ephemeral", "ttl": "5m"}}, {"type": "text", "cache_control": {"type": "ephemeral", "ttl": "1h"}}]});
        normalize(&mut body, 4, None);
        assert!(body["system"][1]["cache_control"].get("ttl").is_none());
    }
}
