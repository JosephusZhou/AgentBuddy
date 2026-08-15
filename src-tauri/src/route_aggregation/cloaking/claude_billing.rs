//! Claude billing header and CCH signing.
//!
//! Reference: CLIProxyAPI `claude_signing.go` and
//! `claude_executor_cloaking.go`. CCH is deliberately calculated over bytes,
//! not a reserialized `serde_json::Value`, because object order and escaping
//! are part of the upstream hash input.

use serde_json::Value;
use sha2::{Digest, Sha256};
use xxhash_rust::xxh64;

const FINGERPRINT_SALT: &str = "59cf53e54c78";
const CCH_SEED: u64 = 0x4D659218E32A3268;

/// Generate the billing value without the outer header name.
pub fn generate_billing_header(version: &str, message_text: &str) -> String {
    let fingerprint = compute_fingerprint(message_text, version);
    format!("cc_version={version}.{fingerprint}; cc_entrypoint=cli; cch=00000;")
}

fn compute_fingerprint(message_text: &str, version: &str) -> String {
    let runes: Vec<char> = message_text.chars().collect();
    let mut selected = String::new();
    for index in [4usize, 7, 20] {
        selected.push(runes.get(index).copied().unwrap_or('0'));
    }
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_SALT.as_bytes());
    hasher.update(selected.as_bytes());
    hasher.update(version.as_bytes());
    hex::encode(&hasher.finalize()[..3])
}

pub fn serialize_body_without_html_escaping(value: &Value) -> Result<Vec<u8>, String> {
    let mut output =
        serde_json::to_vec(value).map_err(|error| format!("serialize CCH body: {error}"))?;
    // This serde_json release does not expose Serializer::escape_html. These
    // escapes are only emitted inside JSON strings, so replacing them keeps
    // valid JSON while matching the upstream encoder's raw HTML characters.
    for (escaped, raw) in [
        (br#"\\u003c"#, b"<".as_slice()),
        (br#"\\u003e"#, b">".as_slice()),
        (br#"\\u0026"#, b"&".as_slice()),
    ] {
        let mut replaced = Vec::with_capacity(output.len());
        let mut position = 0;
        while position < output.len() {
            if output[position..].starts_with(escaped) {
                replaced.extend_from_slice(raw);
                position += escaped.len();
            } else {
                replaced.push(output[position]);
                position += 1;
            }
        }
        output = replaced;
    }
    Ok(output)
}

pub fn billing_message_text(body: &Value) -> String {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .filter_map(|message| message.get("content"))
        .filter_map(|content| match content {
            Value::String(text) => Some(text.clone()),
            Value::Array(blocks) => Some(
                blocks
                    .iter()
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .next_back()
                    .unwrap_or_default()
                    .to_string(),
            ),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .next_back()
        .unwrap_or_default()
}

/// Insert the billing placeholder and sign the final body in one operation.
/// Returns the billing value used in the body and the signed body bytes.
pub fn finalize_body_with_cch(
    body: &mut Value,
    version: &str,
) -> Result<(String, Vec<u8>), String> {
    let fallback_billing = format!(
        "x-anthropic-billing-header: {}",
        generate_billing_header(version, &billing_message_text(body))
    );
    let system = body
        .get_mut("system")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "CCH requires a system text array".to_string())?;
    let first = system
        .first_mut()
        .ok_or_else(|| "CCH requires a billing system block".to_string())?;
    if first.get("type").and_then(Value::as_str) != Some("text") {
        return Err("CCH billing block must be a text block".into());
    }
    let billing_block = first
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| text.starts_with("x-anthropic-billing-header:"))
        .map(ToString::to_string)
        .unwrap_or(fallback_billing);
    let billing_block = if billing_block.contains("cch=") {
        let start = billing_block.find("cch=").unwrap() + 4;
        let end = billing_block[start..]
            .find(';')
            .map(|offset| start + offset)
            .unwrap_or(billing_block.len());
        format!("{}00000{}", &billing_block[..start], &billing_block[end..])
    } else {
        format!("{billing_block} cch=00000;")
    };
    first["text"] = Value::String(billing_block.clone());

    let unsigned = serialize_body_without_html_escaping(body)?;
    let signed = sign_serialized_body(&unsigned)?;
    let signed_body: Value = serde_json::from_slice(&signed)
        .map_err(|error| format!("decode signed CCH body: {error}"))?;
    *body = signed_body;
    let signed_billing = body["system"][0]["text"]
        .as_str()
        .unwrap_or(&billing_block)
        .to_string();
    Ok((signed_billing, signed))
}

fn sign_serialized_body(body: &[u8]) -> Result<Vec<u8>, String> {
    let (offset, _) =
        billing_cch_offset(body).ok_or_else(|| "CCH placeholder not found".to_string())?;
    let mut unsigned = body.to_vec();
    unsigned[offset..offset + 5].copy_from_slice(b"00000");
    let normalized = normalize_cch_input(&unsigned)?;
    let digest = xxh64::xxh64(&normalized, CCH_SEED) & 0xFFFFF;
    let value = format!("{digest:05x}");
    unsigned[offset..offset + 5].copy_from_slice(value.as_bytes());
    Ok(unsigned)
}

fn billing_cch_offset(body: &[u8]) -> Option<(usize, usize)> {
    let marker = b"x-anthropic-billing-header:";
    let start = body
        .windows(marker.len())
        .position(|window| window == marker)?;
    let search_start = start + marker.len();
    let cch_rel = body[search_start..]
        .windows(4)
        .position(|window| window == b"cch=")?;
    let offset = search_start + cch_rel + 4;
    if body
        .get(offset..offset + 5)?
        .iter()
        .all(|byte| byte.is_ascii_hexdigit())
        && body.get(offset + 5) == Some(&b';')
    {
        Some((offset, 5))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct Edit {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy)]
struct Member {
    start: usize,
    end: usize,
    comma_before: Option<usize>,
    comma_after: Option<usize>,
    excluded: bool,
}

struct Scanner<'a> {
    body: &'a [u8],
    pos: usize,
    edits: Vec<Edit>,
}

fn normalize_cch_input(body: &[u8]) -> Result<Vec<u8>, String> {
    if serde_json::from_slice::<Value>(body).is_err() {
        return Err("invalid JSON body".into());
    }
    let mut scanner = Scanner {
        body,
        pos: 0,
        edits: Vec::new(),
    };
    scanner.parse_value(true)?;
    scanner.skip_whitespace();
    if scanner.pos != body.len() {
        return Err(format!("unexpected JSON data at byte {}", scanner.pos));
    }
    scanner.edits.sort_by_key(|edit| edit.start);
    let mut normalized = Vec::with_capacity(body.len());
    let mut last = 0;
    for edit in scanner.edits {
        if edit.start < last || edit.end > body.len() {
            return Err(format!("overlapping CCH edit at byte {}", edit.start));
        }
        normalized.extend_from_slice(&body[last..edit.start]);
        last = edit.end;
    }
    normalized.extend_from_slice(&body[last..]);
    Ok(normalized)
}

impl<'a> Scanner<'a> {
    fn parse_value(&mut self, collect: bool) -> Result<(), String> {
        self.skip_whitespace();
        let byte = *self.body.get(self.pos).ok_or("missing JSON value")?;
        match byte {
            b'{' => self.parse_object(collect),
            b'[' => self.parse_array(collect),
            b'"' => self.parse_string().map(|_| ()),
            _ => {
                let start = self.pos;
                while let Some(byte) = self.body.get(self.pos) {
                    if matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                        break;
                    }
                    self.pos += 1;
                }
                if self.pos == start {
                    Err(format!("missing JSON value at byte {start}"))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn parse_object(&mut self, collect: bool) -> Result<(), String> {
        self.pos += 1;
        self.skip_whitespace();
        if self.consume(b'}') {
            return Ok(());
        }
        let mut members = Vec::new();
        let mut comma_before = None;
        loop {
            self.skip_whitespace();
            let member_start = self.pos;
            let (key_start, key_end) = self.parse_string()?;
            self.skip_whitespace();
            if !self.consume(b':') {
                return Err(format!("missing object colon at byte {}", self.pos));
            }
            self.skip_whitespace();
            let key = &self.body[key_start..key_end];
            let excluded = collect && is_excluded_key(key);
            if collect && key == br#""model""# && self.body.get(self.pos) == Some(&b'"') {
                let (start, end) = self.parse_string()?;
                self.add_edit(start + 1, end - 1);
            } else {
                self.parse_value(collect && !excluded)?;
            }
            let member_end = self.pos;
            self.skip_whitespace();
            let comma_after = if self.consume(b',') {
                Some(self.pos - 1)
            } else {
                None
            };
            members.push(Member {
                start: member_start,
                end: member_end,
                comma_before,
                comma_after,
                excluded,
            });
            if comma_after.is_some() {
                comma_before = comma_after;
                continue;
            }
            if !self.consume(b'}') {
                return Err(format!("missing object end at byte {}", self.pos));
            }
            break;
        }
        if collect {
            self.remove_excluded_members(&members);
        }
        Ok(())
    }

    fn parse_array(&mut self, collect: bool) -> Result<(), String> {
        self.pos += 1;
        self.skip_whitespace();
        if self.consume(b']') {
            return Ok(());
        }
        loop {
            self.parse_value(collect)?;
            self.skip_whitespace();
            if self.consume(b',') {
                continue;
            }
            if !self.consume(b']') {
                return Err(format!("missing array end at byte {}", self.pos));
            }
            return Ok(());
        }
    }

    fn parse_string(&mut self) -> Result<(usize, usize), String> {
        if !self.consume(b'"') {
            return Err(format!("missing JSON string at byte {}", self.pos));
        }
        let start = self.pos - 1;
        while self.pos < self.body.len() {
            match self.body[self.pos] {
                b'\\' => self.pos = self.pos.saturating_add(2),
                b'"' => {
                    self.pos += 1;
                    return Ok((start, self.pos));
                }
                _ => self.pos += 1,
            }
        }
        Err(format!("unterminated JSON string at byte {start}"))
    }

    fn remove_excluded_members(&mut self, members: &[Member]) {
        let mut start = 0;
        while start < members.len() {
            if !members[start].excluded {
                start += 1;
                continue;
            }
            let mut end = start;
            while end + 1 < members.len() && members[end + 1].excluded {
                end += 1;
            }
            match (end + 1 < members.len(), start > 0 && end > start, start > 0) {
                (true, _, _) => {
                    self.add_edit(members[start].start, members[end].comma_after.unwrap() + 1)
                }
                (false, true, _) => self.add_edit(members[start].start, members[end].end),
                (false, false, true) => {
                    self.add_edit(members[start].comma_before.unwrap(), members[end].end)
                }
                (false, false, false) => self.add_edit(members[start].start, members[end].end),
            }
            start = end + 1;
        }
    }

    fn add_edit(&mut self, start: usize, end: usize) {
        if start < end {
            self.edits.push(Edit { start, end });
        }
    }
    fn skip_whitespace(&mut self) {
        while matches!(self.body.get(self.pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }
    fn consume(&mut self, byte: u8) -> bool {
        if self.body.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

fn is_excluded_key(key: &[u8]) -> bool {
    matches!(
        key,
        br#""max_tokens""# | br#""fallbacks""# | br#""fallback_credit_token""#
    )
}

#[cfg(test)]
mod tests {
    use super::{compute_fingerprint, finalize_body_with_cch, normalize_cch_input};
    use serde_json::json;

    #[test]
    fn fingerprint_uses_zero_fallbacks_for_short_messages() {
        assert_eq!(compute_fingerprint("abc", "2.1.220").len(), 6);
    }

    #[test]
    fn excluded_dispatch_fields_do_not_change_normalized_hash_input() {
        let first = br#"{"model":"claude-opus-5","max_tokens":10,"messages":[]}"#;
        let second = br#"{"model":"claude-opus-5","max_tokens":999,"messages":[]}"#;
        assert_eq!(
            normalize_cch_input(first).unwrap(),
            normalize_cch_input(second).unwrap()
        );
    }

    #[test]
    fn finalizer_replaces_placeholder_without_losing_json_shape() {
        let mut body = json!({"model":"claude-opus-5","messages":[{"role":"user","content":"Hello"}],"system":[{"type":"text","text":"placeholder"}]});
        let (billing, signed) = finalize_body_with_cch(&mut body, "2.1.220").unwrap();
        assert!(billing.contains("cch="));
        assert!(String::from_utf8(signed).unwrap().contains("cch="));
        assert!(body["system"][0]["text"].as_str().unwrap().contains("cch="));
    }
}
