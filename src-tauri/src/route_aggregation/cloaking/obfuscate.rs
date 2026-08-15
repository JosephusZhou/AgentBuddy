//! Sensitive word obfuscation using zero-width spaces.
//! Reference: CLIProxyAPI helps/cloak_obfuscate.go

/// Insert a zero-width space (U+200B) after the first character of sensitive words
/// to bypass text-based detection without changing visible output.
pub fn obfuscate_sensitive_words(text: &str) -> String {
    let sensitive_words = [
        "proxy",
        "relay",
        "forward",
        "upstream",
        "agentbuddy",
        "route",
        "aggregat",
    ];

    let mut result = text.to_string();
    for word in &sensitive_words {
        result = replace_sensitive(&result, word);
    }
    result
}

fn replace_sensitive(text: &str, word: &str) -> String {
    // The configured words are ASCII. ASCII-only folding preserves byte
    // offsets, unlike Unicode lowercasing which may expand a preceding rune.
    let lower = text.to_ascii_lowercase();
    let positions: Vec<usize> = lower.match_indices(word).map(|(i, _)| i).collect();

    if positions.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len() + positions.len() * 3);
    let bytes = text.as_bytes();
    let mut last_end = 0;

    for pos in positions {
        result.push_str(&text[last_end..pos]);
        // Push the first character of the word
        let word_end = pos + word.len();
        if word_end <= bytes.len() {
            // Find the char boundary
            let first_char_end = next_char_boundary(bytes, pos);
            result.push_str(&text[pos..first_char_end]);
            // Insert zero-width space
            result.push('\u{200B}');
            // Push the rest of the word
            result.push_str(&text[first_char_end..word_end]);
            last_end = word_end;
        } else {
            result.push_str(&text[pos..]);
            last_end = text.len();
        }
    }
    result.push_str(&text[last_end..]);
    result
}

fn next_char_boundary(bytes: &[u8], pos: usize) -> usize {
    if pos >= bytes.len() {
        return pos;
    }
    // Find the end of the UTF-8 character starting at pos
    let first_byte = bytes[pos];
    let char_len = if first_byte < 0x80 {
        1
    } else if first_byte < 0xC0 {
        1 // continuation byte, shouldn't start here
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    };
    (pos + char_len).min(bytes.len())
}

/// Obfuscate text in JSON string values within a body.
pub fn obfuscate_body_strings(body: &mut serde_json::Value) {
    match body {
        serde_json::Value::String(s) => {
            *s = obfuscate_sensitive_words(s);
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                obfuscate_body_strings(v);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                obfuscate_body_strings(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{obfuscate_body_strings, obfuscate_sensitive_words};

    #[test]
    fn obfuscates_case_insensitively_without_corrupting_utf8_offsets() {
        assert_eq!(
            obfuscate_sensitive_words("İProxy"),
            "İP\u{200B}roxy"
        );
    }

    #[test]
    fn obfuscates_nested_json_string_values() {
        let mut body = serde_json::json!({
            "system": [{"type": "text", "text": "Use the proxy."}],
            "messages": [{"content": "relay"}]
        });

        obfuscate_body_strings(&mut body);

        assert_eq!(body["system"][0]["text"], "Use the p\u{200B}roxy.");
        assert_eq!(body["messages"][0]["content"], "r\u{200B}elay");
    }
}
