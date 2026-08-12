//! Tool name sanitize —— Gemini 函数名限制 `[a-zA-Z][a-zA-Z0-9_]*`，
//! 客户端传过来的名字（如 `search.web`、`foo:bar`）需要替换为合法名。
//!
//! CLIProxyAPI aligned: db143ae - fix(codex): make input ID sanitization
//!                        collision-resistant and deterministic
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/db143aebac93f9be136ba3d18bd75381d61a2750
//! Last verified: 2026-08-12

use std::collections::HashMap;

/// Gemini 合法函数名字符集：`[a-zA-Z][a-zA-Z0-9_]*`，最长 64 字符（工程经验值）。
const VALID_FIRST: std::ops::RangeInclusive<char> = 'A'..='Z';
const VALID_FIRST_LOWER: std::ops::RangeInclusive<char> = 'a'..='z';
const VALID_REST: std::ops::RangeInclusive<char> = 'A'..='Z';
const VALID_REST_LOWER: std::ops::RangeInclusive<char> = 'a'..='z';
const VALID_REST_DIGIT: std::ops::RangeInclusive<char> = '0'..='9';

const MAX_LEN: usize = 64;

fn is_valid_first(c: char) -> bool {
    VALID_FIRST.contains(&c) || VALID_FIRST_LOWER.contains(&c) || c == '_'
}

fn is_valid_rest(c: char) -> bool {
    is_valid_first(c) || VALID_REST.contains(&c) || VALID_REST_LOWER.contains(&c) || VALID_REST_DIGIT.contains(&c)
}

/// 判断 `name` 是否已经是 Gemini 合法函数名。
pub fn is_valid_gemini_function_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !is_valid_first(first) {
        return false;
    }
    if !chars.all(is_valid_rest) {
        return false;
    }
    name.len() <= MAX_LEN
}

/// 简单版 sanitize：把不合法字符替换为 `_`，裁剪到 64 字符。
/// 不处理冲突 —— 仅作为单名 sanitize 使用。冲突检测交给调用方配合 `tool_name_map`。
pub fn sanitize(name: &str) -> String {
    if name.is_empty() {
        return "_".to_string();
    }
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    out.push(if is_valid_first(first) { first } else { '_' });
    for c in chars {
        if is_valid_rest(c) {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.len() > MAX_LEN {
        out.truncate(MAX_LEN);
    }
    out
}

/// 冲突检测版 sanitize：在已经注册过的 map 上追加 hash-suffix 直到不冲突。
///
/// # 算法
/// 1. 先检查 `name` 是否已经在 `occupied` 里登记过（原名查）—— 若是，幂等返回原 sanitized 名。
/// 2. 基础 `sanitize(name)`，若 sanitized 名未冲突，直接返回。
/// 3. 否则追加 `-<xxhash64(name) hex 8 位>` 直到不冲突。
///
/// `occupied` 应与 `params.sanitized_name_map` 同步维护。
pub fn sanitize_with_occupied(name: &str, occupied: &mut HashMap<String, String>) -> String {
    // 1. 幂等检查：同一原名多次 sanitize 应返回同一 sanitized 名
    if let Some((existing_sanitized, _)) =
        occupied.iter().find(|(_, v)| v.as_str() == name)
    {
        return existing_sanitized.clone();
    }
    // 2. 基础 sanitize
    let base = sanitize(name);
    if !occupied.contains_key(&base) {
        occupied.insert(base.clone(), name.to_string());
        return base;
    }
    // 3. 冲突：用 xxhash64 + 截断做 deterministic suffix
    let hash = xxhash64_as_hex(name);
    for attempt in 0..64u32 {
        let candidate = if attempt == 0 {
            format!("{}_{}", &base[..base.len().min(MAX_LEN - 9)], &hash[..8])
        } else {
            format!(
                "{}_{}_{}",
                &base[..base.len().min(MAX_LEN - 14)],
                &hash[..8],
                attempt
            )
        };
        let candidate = if candidate.len() > MAX_LEN {
            candidate[..MAX_LEN].to_string()
        } else {
            candidate
        };
        if !occupied.contains_key(&candidate) {
            occupied.insert(candidate.clone(), name.to_string());
            return candidate;
        }
    }
    // 极端情况：64 次都冲突，用 hash 全长兜底
    let fallback = format!("{}_{}", &base[..base.len().min(8)], hash);
    occupied.insert(fallback.clone(), name.to_string());
    fallback
}

/// 还原：根据 sanitize 后的名查 `params.sanitized_name_map` 找到原名。
/// 若未命中返回 sanitize 后的名本身（最坏情况是用户看到 sanitized 名）。
pub fn recover(sanitized: &str, sanitized_name_map: &HashMap<String, String>) -> String {
    sanitized_name_map
        .get(sanitized)
        .cloned()
        .unwrap_or_else(|| sanitized.to_string())
}

fn xxhash64_as_hex(s: &str) -> String {
    use xxhash_rust::xxh64::xxh64;
    format!("{:016x}", xxh64(s.as_bytes(), 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names_pass_through() {
        assert!(is_valid_gemini_function_name("foo"));
        assert!(is_valid_gemini_function_name("foo_bar"));
        assert!(is_valid_gemini_function_name("_underscore_start"));
        assert!(is_valid_gemini_function_name("CamelCase"));
        assert!(is_valid_gemini_function_name("name123"));
    }

    #[test]
    fn invalid_names_are_rejected() {
        assert!(!is_valid_gemini_function_name(""));
        assert!(!is_valid_gemini_function_name("1foo"));
        assert!(!is_valid_gemini_function_name("foo.bar"));
        assert!(!is_valid_gemini_function_name("foo:bar"));
        assert!(!is_valid_gemini_function_name("foo-bar"));
        assert!(!is_valid_gemini_function_name("foo bar"));
    }

    #[test]
    fn sanitize_replaces_invalid_chars() {
        assert_eq!(sanitize("foo"), "foo");
        assert_eq!(sanitize("foo.bar"), "foo_bar");
        assert_eq!(sanitize("foo:bar"), "foo_bar");
        assert_eq!(sanitize("foo-bar"), "foo_bar");
        assert_eq!(sanitize("foo bar"), "foo_bar");
        assert_eq!(sanitize("1foo"), "_foo");
        assert_eq!(sanitize(""), "_");
    }

    #[test]
    fn sanitize_truncates_to_max_len() {
        let long = "a".repeat(100);
        let s = sanitize(&long);
        assert_eq!(s.len(), MAX_LEN);
    }

    #[test]
    fn sanitize_with_occupied_handles_no_collision() {
        let mut occupied = HashMap::new();
        let s = sanitize_with_occupied("foo.bar", &mut occupied);
        assert_eq!(s, "foo_bar");
        assert_eq!(occupied.get("foo_bar"), Some(&"foo.bar".to_string()));
    }

    #[test]
    fn sanitize_with_occupied_detects_collision() {
        let mut occupied = HashMap::new();
        // 两个原名 sanitize 后都是 "foo_bar"
        let s1 = sanitize_with_occupied("foo.bar", &mut occupied);
        let s2 = sanitize_with_occupied("foo-bar", &mut occupied);
        assert_eq!(s1, "foo_bar");
        assert_ne!(s1, s2);
        assert_eq!(occupied.get(&s1), Some(&"foo.bar".to_string()));
        assert_eq!(occupied.get(&s2), Some(&"foo-bar".to_string()));
    }

    #[test]
    fn sanitize_is_deterministic_across_runs() {
        let mut occ1 = HashMap::new();
        let mut occ2 = HashMap::new();
        let s1 = sanitize_with_occupied("foo.bar", &mut occ1);
        let s2 = sanitize_with_occupied("foo.bar", &mut occ2);
        assert_eq!(s1, s2);
    }

    #[test]
    fn recover_returns_original_or_sanitized() {
        let mut map = HashMap::new();
        map.insert("foo_bar".to_string(), "foo.bar".to_string());
        assert_eq!(recover("foo_bar", &map), "foo.bar");
        assert_eq!(recover("not_in_map", &map), "not_in_map");
    }
}