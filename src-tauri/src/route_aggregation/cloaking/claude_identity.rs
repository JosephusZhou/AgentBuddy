//! Stable Claude identity generation for non-native callers.
//!
//! The upstream implementation optionally persists this identity in home KV.
//! AgentBuddy uses a scoped in-process cache so retries within one route scope
//! remain correlated without writing credentials or identity data to disk.

use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static USER_IDS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, String>> {
    USER_IDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random_hex(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut value);
    hex::encode(value)
}

fn new_user_id() -> String {
    format!(
        "user_{}_account_{}_session_{}",
        random_hex(32),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4()
    )
}

pub fn stable_user_id(scope: &str) -> String {
    let key = if scope.trim().is_empty() {
        "default"
    } else {
        scope.trim()
    };
    let mut identities = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    identities
        .entry(key.to_string())
        .or_insert_with(new_user_id)
        .clone()
}

pub fn inject_user_id(body: &mut serde_json::Value, scope: &str) {
    if !body.is_object() {
        return;
    }
    if !body
        .get("metadata")
        .map(|value| value.is_object())
        .unwrap_or(false)
    {
        body["metadata"] = serde_json::json!({});
    }
    body["metadata"]["user_id"] = serde_json::Value::String(stable_user_id(scope));
}

#[allow(dead_code)]
pub fn clear_cache() {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

#[cfg(test)]
mod tests {
    use super::{clear_cache, inject_user_id, stable_user_id};

    #[test]
    fn identity_is_stable_per_scope_and_has_native_shape() {
        clear_cache();
        let first = stable_user_id("account-a");
        let second = stable_user_id("account-a");
        let other = stable_user_id("account-b");
        assert_eq!(first, second);
        assert_ne!(first, other);
        assert!(first.starts_with("user_"));
        assert!(first.contains("_account_"));
        assert!(first.contains("_session_"));
    }

    #[test]
    fn injection_creates_metadata_without_overwriting_request_shape() {
        clear_cache();
        let mut body = serde_json::json!({"messages": []});
        inject_user_id(&mut body, "default");
        assert!(body["metadata"]["user_id"]
            .as_str()
            .unwrap()
            .starts_with("user_"));
    }
}
