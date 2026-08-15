//! Claude context-management request policy.

use serde_json::Value;

const CLEAR_THINKING: &str = "clear_thinking_20251015";

fn accepts_clear_thinking(body: &Value) -> bool {
    matches!(
        body.get("thinking")
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str),
        Some("enabled" | "adaptive")
    )
}

pub fn ensure_context_management(body: &mut Value) -> bool {
    if body.get("context_management").is_some() || !accepts_clear_thinking(body) {
        return false;
    }
    body["context_management"] =
        serde_json::json!({"edits": [{"type": CLEAR_THINKING, "keep": "all"}]});
    true
}

pub fn remove_auto_context_management(body: &mut Value, injected_by_us: bool) {
    if injected_by_us
        && !accepts_clear_thinking(body)
        && body
            .get("context_management")
            .and_then(|context| context.get("edits"))
            .and_then(Value::as_array)
            .and_then(|edits| edits.first())
            .and_then(|edit| edit.get("type"))
            .and_then(Value::as_str)
            == Some(CLEAR_THINKING)
    {
        if let Some(object) = body.as_object_mut() {
            object.remove("context_management");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_context_management, remove_auto_context_management};

    #[test]
    fn injects_only_for_enabled_or_adaptive_thinking() {
        let mut enabled = serde_json::json!({"thinking": {"type": "adaptive"}});
        assert!(ensure_context_management(&mut enabled));
        assert_eq!(enabled["context_management"]["edits"][0]["keep"], "all");
        let mut disabled = serde_json::json!({"thinking": {"type": "disabled"}});
        assert!(!ensure_context_management(&mut disabled));
    }

    #[test]
    fn removes_automatic_context_when_thinking_becomes_ineligible() {
        let mut body = serde_json::json!({"thinking": {"type": "adaptive"}});
        ensure_context_management(&mut body);
        body["thinking"]["type"] = "disabled".into();
        remove_auto_context_management(&mut body, true);
        assert!(body.get("context_management").is_none());
    }

    #[test]
    fn preserves_callers_context_management() {
        let mut body = serde_json::json!({
            "thinking": {"type": "disabled"},
            "context_management": {"edits": [{"type": "clear_thinking_20251015"}]}
        });
        remove_auto_context_management(&mut body, false);
        assert!(body.get("context_management").is_some());
    }
}
