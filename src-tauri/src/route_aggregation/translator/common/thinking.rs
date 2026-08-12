//! Thinking 模式双向映射 —— Anthropic `thinking.type=enabled/adaptive` ↔ Gemini
//! `generationConfig.thinkingConfig.{thinkingBudget|thinkingLevel}`。
//!
//! CLIProxyAPI aligned: ac8fb97 - feat(thinking): remove thinkingConfig for ModeNone
//!                        with zero budget and no level
//! Source: https://github.com/router-for-me/CLIProxyAPI/commit/ac8fb9706fb84bedfbd1f813738680fdc6767115
//! Last verified: 2026-08-12
//!
//! **Phase 0 占位**：仅定义 `ThinkingMode` 枚举 + ModeNone 清除逻辑的最小实现。
//! 完整 `thinkingBudget` / `thinkingLevel` 映射在 Phase 5 接入 Anthropic / Gemini 翻译器。

use serde_json::{Map, Value};

/// Thinking 模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThinkingMode {
    /// 客户端要求关闭 thinking（Anthropic `type: "disabled"` 或未声明）。
    #[default]
    None,
    /// 客户端要求开启思考并给 token budget（Anthropic `type: "enabled" + budget_tokens`）。
    Enabled,
    /// 客户端要求由模型自适应 thinking（Anthropic `type: "adaptive"`）。
    Adaptive,
}

/// 解析 Anthropic request body 中的 `thinking` 字段。
///
/// Anthropic schema：
/// ```json
/// {"thinking": {"type": "enabled", "budget_tokens": 1024}}
/// {"thinking": {"type": "adaptive"}}
/// {"thinking": {"type": "disabled"}}
/// ```
/// 字段缺失时返回 `ThinkingMode::None`。
pub fn parse_anthropic_thinking(body: &Map<String, Value>) -> ThinkingMode {
    let Some(thinking) = body.get("thinking").and_then(|v| v.as_object()) else {
        return ThinkingMode::None;
    };
    let Some(type_str) = thinking.get("type").and_then(|v| v.as_str()) else {
        return ThinkingMode::None;
    };
    match type_str {
        "enabled" => {
            // budget_tokens 必须 > 0；否则视作 None
            let budget = thinking
                .get("budget_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if budget > 0 {
                ThinkingMode::Enabled
            } else {
                ThinkingMode::None
            }
        }
        "adaptive" => ThinkingMode::Adaptive,
        _ => ThinkingMode::None,
    }
}

/// 从 Anthropic `thinking` 字段抽出 budget_tokens（仅 Enabled 模式有意义）。
pub fn extract_anthropic_budget(body: &Map<String, Value>) -> Option<i64> {
    body.get("thinking")
        .and_then(|v| v.as_object())
        .and_then(|t| t.get("budget_tokens"))
        .and_then(|v| v.as_i64())
}

/// 从 Anthropic `thinking` 字段抽出 level（Adaptive 模式无 budget）。
pub fn extract_anthropic_level(body: &Map<String, Value>) -> Option<String> {
    body.get("thinking")
        .and_then(|v| v.as_object())
        .and_then(|t| t.get("level"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// 把 `ThinkingMode` + budget/level 写入 Gemini `generationConfig.thinkingConfig`。
///
/// ModeNone + Budget==0 + Level==空 → **删除** `thinkingConfig` 字段（防止 Gemini
/// 报"thinking_config.thinking_budget must be > 0"）。
pub fn apply_to_gemini_generation_config(
    gen_config: &mut Map<String, Value>,
    mode: ThinkingMode,
    budget: i64,
    level: Option<&str>,
) {
    match mode {
        ThinkingMode::None => {
            // ModeNone + budget==0 + level 缺失 → 清除 thinkingConfig
            if budget == 0 && level.is_none() {
                gen_config.remove("thinkingConfig");
                return;
            }
            // 其它情况保留（兼容"客户端显式设了 budget 但 type=disabled"）
        }
        ThinkingMode::Enabled => {
            let mut tc = Map::new();
            tc.insert("thinkingBudget".into(), Value::from(budget.max(1)));
            tc.insert("includeThoughts".into(), Value::Bool(true));
            gen_config.insert("thinkingConfig".into(), Value::Object(tc));
            return;
        }
        ThinkingMode::Adaptive => {
            let mut tc = Map::new();
            if let Some(lv) = level {
                tc.insert("thinkingLevel".into(), Value::String(lv.to_string()));
            }
            tc.insert("includeThoughts".into(), Value::Bool(true));
            gen_config.insert("thinkingConfig".into(), Value::Object(tc));
            return;
        }
    }

    // ModeNone 但有 budget 或 level：写入但不带 includeThoughts
    let mut tc = Map::new();
    if budget > 0 {
        tc.insert("thinkingBudget".into(), Value::from(budget));
    }
    if let Some(lv) = level {
        tc.insert("thinkingLevel".into(), Value::String(lv.to_string()));
    }
    if !tc.is_empty() {
        gen_config.insert("thinkingConfig".into(), Value::Object(tc));
    } else {
        gen_config.remove("thinkingConfig");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_missing_thinking_returns_none() {
        let body = json!({});
        let m = parse_anthropic_thinking(body.as_object().unwrap());
        assert_eq!(m, ThinkingMode::None);
    }

    #[test]
    fn parse_disabled_returns_none() {
        let body = json!({"thinking": {"type": "disabled"}});
        assert_eq!(parse_anthropic_thinking(body.as_object().unwrap()), ThinkingMode::None);
    }

    #[test]
    fn parse_enabled_with_positive_budget() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 1024}});
        assert_eq!(parse_anthropic_thinking(body.as_object().unwrap()), ThinkingMode::Enabled);
        assert_eq!(extract_anthropic_budget(body.as_object().unwrap()), Some(1024));
    }

    #[test]
    fn parse_enabled_with_zero_budget_returns_none() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 0}});
        assert_eq!(parse_anthropic_thinking(body.as_object().unwrap()), ThinkingMode::None);
    }

    #[test]
    fn parse_adaptive_returns_adaptive() {
        let body = json!({"thinking": {"type": "adaptive"}});
        assert_eq!(parse_anthropic_thinking(body.as_object().unwrap()), ThinkingMode::Adaptive);
    }

    #[test]
    fn apply_mode_none_zero_clears_thinking_config() {
        let mut g = serde_json::Map::new();
        g.insert("maxOutputTokens".into(), json!(1024));
        g.insert("thinkingConfig".into(), json!({"thinkingBudget": 100})); // pre-existing
        apply_to_gemini_generation_config(&mut g, ThinkingMode::None, 0, None);
        assert!(!g.contains_key("thinkingConfig"));
        // 其它字段保留
        assert_eq!(g.get("maxOutputTokens"), Some(&json!(1024)));
    }

    #[test]
    fn apply_enabled_writes_budget() {
        let mut g = serde_json::Map::new();
        apply_to_gemini_generation_config(&mut g, ThinkingMode::Enabled, 1024, None);
        assert_eq!(
            g.get("thinkingConfig"),
            Some(&json!({"thinkingBudget": 1024, "includeThoughts": true}))
        );
    }

    #[test]
    fn apply_adaptive_writes_level() {
        let mut g = serde_json::Map::new();
        apply_to_gemini_generation_config(&mut g, ThinkingMode::Adaptive, 0, Some("high"));
        assert_eq!(
            g.get("thinkingConfig"),
            Some(&json!({"thinkingLevel": "high", "includeThoughts": true}))
        );
    }

    #[test]
    fn apply_enabled_with_zero_budget_clamps_to_one() {
        let mut g = serde_json::Map::new();
        apply_to_gemini_generation_config(&mut g, ThinkingMode::Enabled, 0, None);
        // budget 至少 1
        let tc = g.get("thinkingConfig").unwrap();
        assert_eq!(tc.get("thinkingBudget"), Some(&json!(1)));
    }
}