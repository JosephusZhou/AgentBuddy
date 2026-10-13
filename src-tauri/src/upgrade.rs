//! 启动时的配置升级：按版本顺序执行各版本的适配清理工作。
//!
//! `config::run_startup_config_upgrade` 持有 config.json 锁并驱动本模块：
//! 读取存量配置版本，与当前运行版本比较后，**按升序**执行所有高于存量版本、
//! 且不高于运行版本的适配步骤（见 `upgrade_steps` 注册表）；全部步骤成功后才
//! 把运行版本号写回 `config.json`。
//!
//! 基石版本为 `BASELINE_VERSION`（0.2.1）：该版本本身没有此适配机制，适配步骤
//! 从它之后的版本开始登记。**当前登记的适配版本为 0.3.0**（首个携带适配工作的
//! 发布版本）。用户跨多个版本升级时，所有介于存量版本与运行版本之间的登记
//! 步骤会按升序补齐执行（例如存量 0.2.1 → 运行 0.3.0，执行 0.3.0 的适配工作；
//! 存量 0.2.1 → 运行 0.4.0 时若无 0.3.x~0.4.x 登记版本，则无需执行任何适配）。

use serde_json::Value;
use std::fs;

/// 引入版本升级适配机制的基石版本：该版本本身没有此功能，
/// 登记在它之后的适配步骤才会上场。
pub const BASELINE_VERSION: &str = "0.2.1";

/// 单个版本的适配升级步骤。
pub struct UpgradeStep {
    /// 引入本步骤适配工作的 app 版本号（点分语义版本）。
    pub version: &'static str,
    /// 本版本的适配清理逻辑。可同时改写 config.json 根对象与执行外部清理
    /// （数据库、遗留文件）。返回 Err 时整轮升级中止、版本号不推进，下次启动
    /// 从存量版本重新按序重试（各适配逻辑均为幂等操作，重复执行无害）。
    pub apply: fn(&mut Value) -> Result<(), String>,
}

/// 已从注册表移除、需要在适配升级时从配置与数据库中清掉的 agent 标识。
/// 覆盖：国际版 codebuddy、CodeBuddy CN（codebuddy-cn）、Oh-My-Pi（oh-my-pi/omp）。
const REMOVED_AGENT_NAMES: &[&str] = &["codebuddy", "codebuddy-cn", "oh-my-pi", "omp"];

/// 判断配置键名是否涉及已下线 agent（大小写不敏感；兼容 camelCase / kebab / snake 写法）。
/// - `codebuddy`：任意位置子串（含 codebuddy-cn / CodeBuddyCN 等变体）
/// - `ohmy`：任意位置子串（含 oh-my-pi / ohMyPi 等变体）
/// - `omp`：仅匹配独立键名或以 `omp` 前缀的键（如 omp / ompConfig / omp-cli），
///   避免误删 prompt / component / company 等普通英文键
fn is_legacy_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let compact: String = lower.chars().filter(|c| *c != '-' && *c != '_').collect();
    compact.contains("codebuddy") || compact.contains("ohmy") || compact.starts_with("omp")
}

/// 递归剔除 JSON 对象中键名涉及已下线 agent 的字段，返回移除数量。
/// 只处理对象键名，不动字符串值（避免误伤代理口令等合法内容）。
pub fn scrub_legacy_agent_keys(root: &mut Value) -> usize {
    fn walk(v: &mut Value, removed: &mut usize) {
        let Value::Object(map) = v else {
            return;
        };
        let drop: Vec<String> = map.keys().filter(|k| is_legacy_key(k)).cloned().collect();
        for key in drop {
            if map.remove(&key).is_some() {
                *removed += 1;
            }
        }
        for child in map.values_mut() {
            walk(child, removed);
        }
    }
    let mut removed = 0usize;
    walk(root, &mut removed);
    removed
}

/// 清理数据库中已下线 agent 的残留：agents 缓存行、MCP 关联、供应商绑定。
pub fn clean_legacy_agent_data() -> Result<(), String> {
    let purged = crate::db::purge_removed_agents(REMOVED_AGENT_NAMES)?;
    let scrubbed = crate::db::scrub_mcp_applied_agents(REMOVED_AGENT_NAMES)?;
    let removed_bindings =
        crate::db::delete_agent_provider_bindings_by_agent_type(REMOVED_AGENT_NAMES)?;
    let total = purged + scrubbed + removed_bindings;
    if total > 0 {
        eprintln!(
            "[agent-buddy] 配置升级：清理 {} 条 agent 缓存、{} 条 MCP 关联、{} 条供应商绑定",
            purged, scrubbed, removed_bindings
        );
    }
    Ok(())
}

/// 最佳努力清理遗留 `skills-meta.json`（已迁移进 SQLite）中的相关键。
pub fn scrub_legacy_skills_meta() -> Result<(), String> {
    let path = crate::config::app_dir()?.join("skills-meta.json");
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取 skills-meta.json 失败: {e}"))?;
    let mut root: Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析 skills-meta.json 失败: {e}"))?;
    let removed = scrub_legacy_agent_keys(&mut root);
    if removed == 0 {
        return Ok(());
    }
    let text = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("序列化 skills-meta.json 失败: {e}"))?;
    fs::write(&path, format!("{text}\n"))
        .map_err(|e| format!("写入 skills-meta.json 失败: {e}"))?;
    eprintln!("[agent-buddy] 配置升级：清理 skills-meta.json 中 {removed} 个相关键");
    Ok(())
}

/* ===== 各版本适配步骤 ===== */

/// 0.3.0：清理已下线 agent（codebuddy 系列 / oh-my-pi 系列）在配置文件与数据库中的残留。
fn migrate_v_0_3_0(root: &mut Value) -> Result<(), String> {
    clean_legacy_agent_data()?;
    scrub_legacy_skills_meta()?;
    let removed = scrub_legacy_agent_keys(root);
    if removed > 0 {
        eprintln!("[agent-buddy] 配置升级：清理 config.json 中 {removed} 个相关键");
    }
    Ok(())
}

/// 适配步骤注册表：**必须按版本升序排列**。
/// 当前登记的适配版本为 0.3.0；新增适配版本时，在末尾追加 `UpgradeStep`
/// 并实现对应的 `migrate_v_*` 函数（版本号必须是真实发布的版本）。
pub fn upgrade_steps() -> &'static [UpgradeStep] {
    let steps: &'static [UpgradeStep] = &[UpgradeStep {
        version: "0.3.0",
        apply: migrate_v_0_3_0,
    }];
    // 适配步骤必须登记在基石版本（BASELINE_VERSION）之后；
    // 严格升序由 `steps_start_after_baseline_and_are_ascending` 测试保证。
    debug_assert!(
        crate::config::compare_versions(steps[0].version, BASELINE_VERSION)
            == std::cmp::Ordering::Greater,
        "upgrade steps must be registered after the baseline version"
    );
    steps
}

/// 给定存量配置版本与当前运行版本，返回**需要按升序执行**的适配步骤：
/// 步骤版本严格高于存量版本（存量缺失视为 `0.0.0`），且不高于运行版本。
pub fn pending_steps<'a>(
    stored: Option<&str>,
    running: &str,
    steps: &'a [UpgradeStep],
) -> Vec<&'a UpgradeStep> {
    use std::cmp::Ordering;
    let base = stored.unwrap_or("0.0.0");
    steps
        .iter()
        .filter(|s| {
            crate::config::compare_versions(s.version, base) == Ordering::Greater
                && crate::config::compare_versions(s.version, running) != Ordering::Greater
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scrub_keys_recursively() {
        let mut root = json!({
            "theme": "qoder-light",
            "backup": { "enabled": true },
            "codebuddy": { "anything": 1 },
            "omp": "legacy",
            "nested": {
                "ohMyPi": { "providerId": "x" },
                "keep": { "prompt": "ok" }
            }
        });
        let removed = scrub_legacy_agent_keys(&mut root);
        assert_eq!(removed, 3);
        assert!(root.get("codebuddy").is_none());
        assert!(root.get("omp").is_none());
        assert!(root.get("nested").unwrap().get("ohMyPi").is_none());
        assert_eq!(root["nested"]["keep"]["prompt"], "ok");
        assert_eq!(root["backup"]["enabled"], true);
    }

    #[test]
    fn scrub_ignores_clean_keys() {
        let mut root = json!({
            "theme": "dark",
            "backup": { "auto": { "mode": "interval" } },
            "routeAggregation": { "apiKeys": [] }
        });
        let removed = scrub_legacy_agent_keys(&mut root);
        assert_eq!(removed, 0);
        assert!(root.get("theme").is_some());
    }

    #[test]
    fn version_cmp_detects_older_config() {
        // compare_versions 在 config 模块中定义；此处验证语义一致的等价实现。
        assert!(crate::config::compare_versions("0.2.1", "0.2.1").is_eq());
        assert!(crate::config::compare_versions("0.1.9", "0.2.1").is_lt());
        assert!(crate::config::compare_versions("1.0.0", "0.9.9").is_gt());
        assert!(crate::config::compare_versions("0.2.1", "0.10.0").is_lt());
    }

    #[test]
    fn steps_start_after_baseline_and_are_ascending() {
        use std::cmp::Ordering;
        let steps = upgrade_steps();
        // 当前仅登记 0.3.0 一个真实发布的适配版本
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].version, "0.3.0");
        // 首个步骤版本必须高于基石版本（0.2.1 之后才引入适配机制）
        assert_eq!(
            crate::config::compare_versions(steps[0].version, BASELINE_VERSION),
            Ordering::Greater
        );
        // 注册表必须严格升序，保证升级按序执行
        for w in steps.windows(2) {
            assert_eq!(
                crate::config::compare_versions(w[0].version, w[1].version),
                Ordering::Less
            );
        }
    }

    #[test]
    fn pending_steps_picks_versions_between_stored_and_running_in_order() {
        let steps = upgrade_steps();
        let versions =
            |p: Vec<&UpgradeStep>| -> Vec<&'static str> { p.iter().map(|s| s.version).collect() };

        // 存量 0.2.1 → 运行 0.3.0：执行 0.3.0 的适配工作
        assert_eq!(
            versions(pending_steps(Some("0.2.1"), "0.3.0", steps)),
            vec!["0.3.0"]
        );
        // 存量缺失（老版本无 version 字段）：同样执行
        assert_eq!(versions(pending_steps(None, "0.3.0", steps)), vec!["0.3.0"]);
        // 存量低于基石版本（如 0.1.x 时代）：同样执行
        assert_eq!(
            versions(pending_steps(Some("0.1.0"), "0.3.0", steps)),
            vec!["0.3.0"]
        );
        // 存量等于运行版本（已是最新）：无待执行步骤
        assert!(pending_steps(Some("0.3.0"), "0.3.0", steps).is_empty());
        // 存量高于运行版本（异常降级）：不执行任何步骤
        assert!(pending_steps(Some("0.4.0"), "0.3.0", steps).is_empty());
        // 运行版本低于登记的适配版本（如当前开发版本 0.2.1 本体）：不执行
        assert!(pending_steps(Some("0.2.1"), "0.2.1", steps).is_empty());
        // 存量已越过登记版本（0.3.0 → 0.4.0）：无中间登记版本，不执行
        assert!(pending_steps(Some("0.3.0"), "0.4.0", steps).is_empty());
    }
}
