//! Configurable sensitive-word obfuscation using zero-width spaces.
//!
//! Reference: CLIProxyAPI `helps/cloak_obfuscate.go`. Unlike the upstream
//! generic payload helper, the Claude path deliberately limits mutation to
//! system text and message content so tool schemas and metadata remain intact.

const DEFAULT_WORDS: &[&str] = &[
    "proxy",
    "relay",
    "forward",
    "upstream",
    "agentbuddy",
    "route",
    "aggregat",
];

#[derive(Debug, Clone)]
pub struct SensitiveWordMatcher {
    words: Vec<String>,
}

impl SensitiveWordMatcher {
    pub fn new(words: &[String]) -> Self {
        let mut normalized: Vec<String> = words
            .iter()
            .map(|word| word.trim().to_ascii_lowercase())
            .filter(|word| word.chars().count() >= 2 && !word.contains('\u{200b}'))
            .collect();
        normalized.sort_by_key(|word| std::cmp::Reverse(word.len()));
        normalized.dedup();
        Self { words: normalized }
    }

    pub fn from_defaults() -> Self {
        Self::new(
            &DEFAULT_WORDS
                .iter()
                .map(|word| (*word).to_string())
                .collect::<Vec<_>>(),
        )
    }

    pub fn obfuscate_text(&self, text: &str) -> String {
        self.words.iter().fold(text.to_string(), |value, word| {
            replace_sensitive(&value, word)
        })
    }
}

/// Insert a zero-width space after the first character of each configured word.
#[allow(dead_code)]
pub fn obfuscate_sensitive_words(text: &str) -> String {
    SensitiveWordMatcher::from_defaults().obfuscate_text(text)
}

fn replace_sensitive(text: &str, word: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let positions: Vec<usize> = lower.match_indices(word).map(|(index, _)| index).collect();
    if positions.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len() + positions.len() * 3);
    let bytes = text.as_bytes();
    let mut last_end = 0;
    for position in positions {
        if position < last_end {
            continue;
        }
        let word_end = position + word.len();
        if word_end > bytes.len()
            || !text.is_char_boundary(position)
            || !text.is_char_boundary(word_end)
        {
            continue;
        }
        let first_end = text[position..]
            .chars()
            .next()
            .map(|character| position + character.len_utf8())
            .unwrap_or(position);
        result.push_str(&text[last_end..position]);
        result.push_str(&text[position..first_end]);
        result.push('\u{200b}');
        result.push_str(&text[first_end..word_end]);
        last_end = word_end;
    }
    result.push_str(&text[last_end..]);
    result
}

fn obfuscate_text_value(value: &mut serde_json::Value, matcher: &SensitiveWordMatcher) {
    if let Some(text) = value.as_str() {
        *value = serde_json::Value::String(matcher.obfuscate_text(text));
    }
}

fn obfuscate_system(body: &mut serde_json::Value, matcher: &SensitiveWordMatcher) {
    match body.get_mut("system") {
        Some(serde_json::Value::String(text)) => {
            *text = matcher.obfuscate_text(text);
        }
        Some(serde_json::Value::Array(blocks)) => {
            for block in blocks {
                if block.get("type").and_then(|value| value.as_str()) == Some("text") {
                    if let Some(text) = block.get_mut("text") {
                        obfuscate_text_value(text, matcher);
                    }
                }
            }
        }
        _ => {}
    }
}

fn obfuscate_messages(body: &mut serde_json::Value, matcher: &SensitiveWordMatcher) {
    let Some(messages) = body
        .get_mut("messages")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    for message in messages {
        match message.get_mut("content") {
            Some(serde_json::Value::String(text)) => *text = matcher.obfuscate_text(text),
            Some(serde_json::Value::Array(blocks)) => {
                for block in blocks {
                    if block.get("type").and_then(|value| value.as_str()) == Some("text") {
                        if let Some(text) = block.get_mut("text") {
                            obfuscate_text_value(text, matcher);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn obfuscate_claude_body(body: &mut serde_json::Value, words: &[String]) {
    let matcher = if words.is_empty() {
        SensitiveWordMatcher::from_defaults()
    } else {
        SensitiveWordMatcher::new(words)
    };
    obfuscate_system(body, &matcher);
    obfuscate_messages(body, &matcher);
}

/// Legacy helper retained for callers that intentionally need recursive strings.
#[allow(dead_code)]
pub fn obfuscate_body_strings(body: &mut serde_json::Value) {
    let matcher = SensitiveWordMatcher::from_defaults();
    match body {
        serde_json::Value::String(text) => *text = matcher.obfuscate_text(text),
        serde_json::Value::Array(values) => values.iter_mut().for_each(obfuscate_body_strings),
        serde_json::Value::Object(values) => values.values_mut().for_each(obfuscate_body_strings),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        obfuscate_body_strings, obfuscate_claude_body, obfuscate_sensitive_words,
        SensitiveWordMatcher,
    };

    #[test]
    fn matcher_prefers_longest_words_and_deduplicates() {
        let matcher = SensitiveWordMatcher::new(&["route".into(), "router".into(), "route".into()]);
        assert_eq!(
            matcher.obfuscate_text("router route"),
            "r\u{200b}outer r\u{200b}oute"
        );
    }

    #[test]
    fn claude_scope_does_not_mutate_tool_schema_or_metadata() {
        let mut body = serde_json::json!({
            "system": [{"type": "text", "text": "Use the proxy."}],
            "metadata": {"note": "proxy"},
            "tools": [{"name": "proxy_tool", "description": "proxy"}],
            "messages": [{"role": "user", "content": "relay"}]
        });
        obfuscate_claude_body(&mut body, &[]);
        assert_eq!(body["system"][0]["text"], "Use the p\u{200b}roxy.");
        assert_eq!(body["messages"][0]["content"], "r\u{200b}elay");
        assert_eq!(body["metadata"]["note"], "proxy");
        assert_eq!(body["tools"][0]["name"], "proxy_tool");
    }

    #[test]
    fn recursive_compatibility_helper_keeps_utf8_boundaries() {
        assert_eq!(obfuscate_sensitive_words("İProxy"), "İP\u{200b}roxy");
        let mut value = serde_json::json!({"nested": ["proxy"]});
        obfuscate_body_strings(&mut value);
        assert_eq!(value["nested"][0], "p\u{200b}roxy");
    }
}
