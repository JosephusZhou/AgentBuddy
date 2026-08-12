//! 输入 ID sanitize 冲突检测 —— Codex Responses 输入项 ID 需要 sanitize，
//! 多个 ID sanitize 后可能撞名；用 deterministic hash-suffix 解决。
//!
//! CLIProxyAPI aligned: db143ae - fix(codex): make input ID sanitization
//!                        collision-resistant and deterministic
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/db143aebac93f9be136ba3d18bd75381d61a2750
//! Last verified: 2026-08-12
//!
//! **Phase 0 占位**：骨架 + 单元测试就绪；具体 Codex Responses 集成在 Phase 4。

use std::collections::HashMap;

/// 当前已占用 ID → 原始 ID 的映射。
#[derive(Debug, Default)]
pub struct IdOccupancy {
    /// 已分配的 sanitized ID → 原始 ID（防止同一原始 ID 被映射两次）。
    occupied: HashMap<String, String>,
}

impl IdOccupancy {
    pub fn new() -> Self {
        Self::default()
    }

    /// 标记一个原始 ID 已存在（不需要 sanitize 时）。
    pub fn mark_preserved(&mut self, original: &str) {
        self.occupied.insert(original.to_string(), original.to_string());
    }

    /// 查询一个 sanitized ID 是否已被占用。
    pub fn is_occupied(&self, sanitized: &str) -> bool {
        self.occupied.contains_key(sanitized)
    }

    /// 查询原始 ID 对应的已分配 sanitized ID。
    pub fn get(&self, sanitized: &str) -> Option<&str> {
        self.occupied.get(sanitized).map(String::as_str)
    }

    /// 已占用 ID 数量。
    pub fn len(&self) -> usize {
        self.occupied.len()
    }

    pub fn is_empty(&self) -> bool {
        self.occupied.is_empty()
    }
}

/// 对一个 ID 做基本 sanitize：保留 `[A-Za-z0-9_\-]` 字符，其它替换为 `_`，最长 64。
/// Phase 4 时具体 Responses 输入 ID sanitizer 在此基础上扩展。
pub fn sanitize_basic_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if out.len() > 64 {
        out.truncate(64);
    }
    out
}

/// 冲突检测版 ID sanitize：基础 sanitize + hash-suffix 兜底。
pub fn sanitize_id_with_occupancy(id: &str, occ: &mut IdOccupancy) -> String {
    let base = sanitize_basic_id(id);
    if !occ.is_occupied(&base) {
        occ.occupied.insert(base.clone(), id.to_string());
        return base;
    }
    // 已占用 — 用 hash + 序号追加
    use xxhash_rust::xxh64::xxh64;
    let hash = format!("{:016x}", xxh64(id.as_bytes(), 0));
    for attempt in 0..64u32 {
        let candidate = if attempt == 0 {
            format!("{}_{}", &base[..base.len().min(64 - 9)], &hash[..8])
        } else {
            format!(
                "{}_{}_{}",
                &base[..base.len().min(64 - 14)],
                &hash[..8],
                attempt
            )
        };
        let candidate = if candidate.len() > 64 {
            candidate[..64].to_string()
        } else {
            candidate
        };
        if !occ.is_occupied(&candidate) {
            occ.occupied.insert(candidate.clone(), id.to_string());
            return candidate;
        }
    }
    let fallback = format!("{}_{}", &base[..base.len().min(8)], hash);
    occ.occupied.insert(fallback.clone(), id.to_string());
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_id_sanitize_keeps_safe_chars() {
        assert_eq!(sanitize_basic_id("abc"), "abc");
        assert_eq!(sanitize_basic_id("abc-123"), "abc-123");
        assert_eq!(sanitize_basic_id("abc.def"), "abc_def");
        assert_eq!(sanitize_basic_id(""), "_");
    }

    #[test]
    fn basic_id_sanitize_truncates() {
        let s = "x".repeat(100);
        assert_eq!(sanitize_basic_id(&s).len(), 64);
    }

    #[test]
    fn occupancy_starts_empty() {
        let occ = IdOccupancy::new();
        assert!(occ.is_empty());
        assert!(!occ.is_occupied("foo"));
    }

    #[test]
    fn mark_preserved_occupies_id() {
        let mut occ = IdOccupancy::new();
        occ.mark_preserved("foo");
        assert!(occ.is_occupied("foo"));
        assert_eq!(occ.get("foo"), Some("foo"));
        assert_eq!(occ.len(), 1);
    }

    #[test]
    fn sanitize_with_occupancy_handles_no_collision() {
        let mut occ = IdOccupancy::new();
        let s = sanitize_id_with_occupancy("a.b", &mut occ);
        assert_eq!(s, "a_b");
        assert!(occ.is_occupied("a_b"));
    }

    #[test]
    fn sanitize_with_occupancy_detects_collision() {
        let mut occ = IdOccupancy::new();
        let s1 = sanitize_id_with_occupancy("a.b", &mut occ);
        let s2 = sanitize_id_with_occupancy("a-b", &mut occ);
        assert_eq!(s1, "a_b");
        assert_ne!(s1, s2);
        assert!(occ.is_occupied(&s1));
        assert!(occ.is_occupied(&s2));
    }
}