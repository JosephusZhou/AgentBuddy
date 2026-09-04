//! 自动备份调度：应用运行期间按 `backup.auto` 配置周期触发备份。
//!
//! 设计要点：
//! - 每 30s tick 一次，每轮重新读取设置实现热更新，无需通知机制；
//! - `next_run` 由 last_run_at（首跑前用启用锚点 enabled_at）现算，不持久化；
//! - false→true 重新启用视为新的调度周期，重置锚点与运行状态；
//! - 与手动备份共用 `backup::try_acquire_run_lock` 互斥，占用中跳过本轮；
//! - 无人值守强制口令加密，上传目录固定为 `{remote_dir}/auto` 与手动备份隔离。

use crate::backup;
use crate::config::{self, AutoBackupSettings, BackupSettings};
use crate::crypto;
use chrono::{DateTime, Days, Duration as ChronoDuration, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::AppHandle;

/// 调度 tick 间隔：睡眠唤醒后最多延迟该时长补跑。
const TICK: Duration = Duration::from_secs(30);

/// interval 模式允许的触发间隔（小时）。
pub(crate) const INTERVAL_CHOICES: [u32; 5] = [6, 12, 24, 72, 168];

/// 自动备份是否正在执行（防止执行期间的后继 tick 重复触发/记录跳过）。
static AUTO_RUNNING: AtomicBool = AtomicBool::new(false);

// ===== DTOs =====

/// 自动备份设置 DTO：不含口令任何形态，只含存在性标志。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoBackupSettingsDto {
    pub enabled: bool,
    pub mode: String,
    pub interval_hours: u32,
    pub daily_time: String,
    pub unit_ids: Vec<String>,
    pub webdav_connection_ids: Vec<String>,
    pub has_passphrase: bool,
}

/// 自动备份设置更新载荷。`passphrase`：None = 不变；Some("") = 清除；其余 = 设置。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoBackupSettingsUpdate {
    pub enabled: bool,
    pub mode: String,
    pub interval_hours: u32,
    pub daily_time: String,
    #[serde(default)]
    pub unit_ids: Vec<String>,
    #[serde(default)]
    pub webdav_connection_ids: Vec<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

/// 自动备份运行状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoBackupStatus {
    pub running: bool,
    pub enabled: bool,
    /// 下次触发时间（Unix 秒）；未启用时为 None。
    pub next_run_at: Option<u64>,
    pub last_run_at: Option<u64>,
    pub last_ok: Option<bool>,
    pub last_message: Option<String>,
}

// ===== Settings commands =====

pub fn get_auto_backup_settings() -> Result<AutoBackupSettingsDto, String> {
    Ok(auto_dto(&config::load_backup_settings()?.auto))
}

pub fn update_auto_backup_settings(
    update: AutoBackupSettingsUpdate,
) -> Result<AutoBackupSettingsDto, String> {
    let mode = update.mode.trim().to_string();
    let daily_time = update.daily_time.trim().to_string();
    let unit_ids = normalize_id_list(&update.unit_ids);
    let dav_ids = normalize_id_list(&update.webdav_connection_ids);
    validate_auto_config(
        &mode,
        update.interval_hours,
        &daily_time,
        &unit_ids,
        &dav_ids,
    )?;

    // 口令加密放在配置锁之外：load_secrets_key 与原子读改写共用一把锁，
    // 闭包内再取会死锁。三段密文经 secretsKey 加密，明文只存在于入参中。
    // None = 不变；Clear = 清除；Set = 设置新口令。
    enum PassphraseChange {
        Clear,
        Set(crypto::EncryptedSecret),
    }
    let passphrase_change = match update.passphrase.as_deref() {
        None => None,
        Some("") => Some(PassphraseChange::Clear),
        Some(plain) => {
            let master = config::load_secrets_key()?;
            Some(PassphraseChange::Set(crypto::encrypt_secret(
                &master, plain,
            )?))
        }
    };

    config::update_backup_settings_atomic(move |mut settings| {
        let mut has_pass = settings.auto.has_passphrase();
        match passphrase_change {
            None => {}
            Some(PassphraseChange::Clear) => {
                settings.auto.passphrase_salt.clear();
                settings.auto.passphrase_nonce.clear();
                settings.auto.passphrase_cipher.clear();
                has_pass = false;
            }
            Some(PassphraseChange::Set(enc)) => {
                settings.auto.passphrase_salt = enc.salt;
                settings.auto.passphrase_nonce = enc.nonce;
                settings.auto.passphrase_cipher = enc.cipher;
                has_pass = true;
            }
        }

        // 无人值守安全默认：含密内容必须口令加密，没有「我已知晓风险」旁路。
        if update.enabled && !has_pass && unit_ids_contain_secrets(&unit_ids) {
            return Err("自动备份所选内容可能包含密钥，必须先设置备份口令".to_string());
        }

        // 启用锚点：false→true 视为新的调度周期，重置锚点与上次运行状态，
        // 避免过期的 last_run_at 让 interval 模式在重新启用后立即补跑旧计划。
        if update.enabled {
            let reenabled = !settings.auto.enabled;
            if reenabled || settings.auto.enabled_at.is_none() {
                settings.auto.enabled_at = Some(now_secs());
            }
            if reenabled {
                settings.auto.last_run_at = None;
                settings.auto.last_ok = None;
                settings.auto.last_message = None;
            }
        } else {
            settings.auto.enabled_at = None;
        }

        settings.auto.enabled = update.enabled;
        settings.auto.mode = mode;
        settings.auto.interval_hours = update.interval_hours;
        settings.auto.daily_time = daily_time;
        settings.auto.unit_ids = unit_ids;
        settings.auto.webdav_connection_ids = dav_ids;
        Ok(settings)
    })?;

    get_auto_backup_settings()
}

pub fn get_auto_backup_status() -> Result<AutoBackupStatus, String> {
    let settings = config::load_backup_settings()?;
    let auto = &settings.auto;
    let now = Local::now();
    let next_run_at = if auto.enabled {
        next_run_after(
            &auto.mode,
            auto.interval_hours,
            &auto.daily_time,
            now,
            auto.last_run_at.map(unix_to_local),
            auto.enabled_at.map(unix_to_local),
        )
        .map(|dt| dt.timestamp().max(0) as u64)
    } else {
        None
    };
    Ok(AutoBackupStatus {
        running: crate::backup::try_acquire_run_lock().is_none(),
        enabled: auto.enabled,
        next_run_at,
        last_run_at: auto.last_run_at,
        last_ok: auto.last_ok,
        last_message: auto.last_message.clone(),
    })
}

/// 按自动备份配置立即执行一轮（复用互斥锁与 auto 远端子目录）。
pub fn run_auto_backup_now(app: AppHandle) -> Result<backup::BackupRunResult, String> {
    let _guard = crate::backup::try_acquire_run_lock()
        .ok_or_else(|| "已有备份正在进行，请稍后再试".to_string())?;
    execute_auto(&app)
}

// ===== Scheduler =====

/// 在 Tauri setup 中启动常驻调度线程；进程退出时随线程结束，无需清理。
pub fn spawn_auto_backup_scheduler(app: AppHandle) {
    if let Err(e) = std::thread::Builder::new()
        .name("auto-backup".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            if let Err(e) = scheduler_tick(&app) {
                eprintln!("[auto-backup] tick error: {}", e);
            }
        })
    {
        eprintln!("[auto-backup] failed to spawn scheduler thread: {}", e);
    }
}

fn scheduler_tick(app: &AppHandle) -> Result<(), String> {
    let settings = config::load_backup_settings()?;
    let auto = &settings.auto;
    if !auto.enabled || AUTO_RUNNING.load(Ordering::Acquire) {
        return Ok(());
    }

    let now = Local::now();
    let next = next_run_after(
        &auto.mode,
        auto.interval_hours,
        &auto.daily_time,
        now,
        auto.last_run_at.map(unix_to_local),
        auto.enabled_at.map(unix_to_local),
    );
    if !is_due(now, next) {
        return Ok(());
    }

    // 到期：手动备份进行中则跳过本轮（顺延由 last_run 未更新自然实现）。
    let Some(_run_guard) = crate::backup::try_acquire_run_lock() else {
        return record_skip();
    };
    AUTO_RUNNING.store(true, Ordering::Release);
    let _running_guard = AutoRunningGuard;
    execute_auto(app).map(|_| ())
}

/// DROP 时复位 `AUTO_RUNNING`，执行轮 panic 也不会卡死调度。
struct AutoRunningGuard;
impl Drop for AutoRunningGuard {
    fn drop(&mut self) {
        AUTO_RUNNING.store(false, Ordering::Release);
    }
}

/// 执行一轮自动备份并写回运行状态。调用方需已持有备份运行锁。
fn execute_auto(app: &AppHandle) -> Result<backup::BackupRunResult, String> {
    let settings = config::load_backup_settings()?;
    let payload = match build_auto_payload(&settings) {
        Ok(p) => p,
        Err(e) => {
            let _ = record_run(false, &e);
            return Err(e);
        }
    };
    match backup::execute_backup(app.clone(), payload, Some(backup::TRIGGER_AUTO.to_string())) {
        Ok(result) => {
            let _ = record_run(result.ok, &result.message);
            Ok(result)
        }
        Err(e) => {
            let _ = record_run(false, &e);
            Err(e)
        }
    }
}

// ===== Pure helpers =====

/// 计算自动备份的下一次触发时间（不持久化，每轮现算）。
///
/// - `daily`：今天/明天本地 `HH:MM`（按墙钟调度，与 last_run 无关）；
/// - `interval`：`last_run + N 小时`，无 last_run 时用启用锚点 `enabled_at`；
///   两者都没有时返回 None（永不触发，等待首次启用写入锚点）。
///
/// 泛型 TimeZone 便于单测注入固定时区，不依赖 `TZ` 环境变量。
pub(crate) fn next_run_after<Tz: TimeZone>(
    mode: &str,
    interval_hours: u32,
    daily_time: &str,
    now: DateTime<Tz>,
    last_run_at: Option<DateTime<Tz>>,
    enabled_at: Option<DateTime<Tz>>,
) -> Option<DateTime<Tz>> {
    match mode.trim() {
        "daily" => {
            let (h, m) = parse_daily_time(daily_time)?;
            let today = now
                .timezone()
                .from_local_datetime(&now.date_naive().and_hms_opt(h, m, 0)?)
                .earliest()?;
            if today > now {
                Some(today)
            } else {
                let tomorrow_naive = (now.date_naive() + Days::new(1)).and_hms_opt(h, m, 0)?;
                now.timezone()
                    .from_local_datetime(&tomorrow_naive)
                    .earliest()
            }
        }
        "interval" => {
            // 0 小时会导致每轮 tick 立即到期，视为配置非法。
            if interval_hours == 0 {
                return None;
            }
            let base = last_run_at.or(enabled_at)?;
            base.checked_add_signed(ChronoDuration::hours(i64::from(interval_hours)))
        }
        _ => None,
    }
}

/// 到期判断：`next <= now` 即到期（含错过补跑场景）。
pub(crate) fn is_due<Tz: TimeZone>(now: DateTime<Tz>, next_run_at: Option<DateTime<Tz>>) -> bool {
    next_run_at.is_some_and(|next| now >= next)
}

/// 解析 "HH:MM"；非法返回 None。
fn parse_daily_time(value: &str) -> Option<(u32, u32)> {
    let (h, m) = value.trim().split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

fn validate_auto_config(
    mode: &str,
    interval_hours: u32,
    daily_time: &str,
    unit_ids: &[String],
    webdav_connection_ids: &[String],
) -> Result<(), String> {
    if unit_ids.is_empty() {
        return Err("请至少选择一个自动备份内容".to_string());
    }
    if webdav_connection_ids.is_empty() {
        return Err("请至少选择一个自动备份 WebDAV 目标".to_string());
    }
    match mode {
        "interval" => {
            if !INTERVAL_CHOICES.contains(&interval_hours) {
                return Err("备份间隔仅支持每 6/12/24/72/168 小时".to_string());
            }
        }
        "daily" => {
            if parse_daily_time(daily_time).is_none() {
                return Err("每天定时时间格式应为 HH:MM（例如 09:30）".to_string());
            }
        }
        other => return Err(format!("未知的自动备份模式: {other}")),
    }
    Ok(())
}

/// 与 `run_backup_upload` 的 contains_secrets 同口径：应用数据 / 工具 / Agent
/// 单元默认视为含密钥（config.json 含 secretsKey，Agent 配置含 auth/token）。
fn unit_ids_contain_secrets(unit_ids: &[String]) -> bool {
    unit_ids.iter().any(|id| {
        let id = id.trim();
        id.starts_with("app:agentbuddy") || id.starts_with("tool:") || id.starts_with("agent:")
    })
}

fn normalize_id_list(ids: &[String]) -> Vec<String> {
    ids.iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn auto_dto(auto: &AutoBackupSettings) -> AutoBackupSettingsDto {
    AutoBackupSettingsDto {
        enabled: auto.enabled,
        mode: auto.mode.clone(),
        interval_hours: auto.interval_hours,
        daily_time: auto.daily_time.clone(),
        unit_ids: auto.unit_ids.clone(),
        webdav_connection_ids: auto.webdav_connection_ids.clone(),
        has_passphrase: auto.has_passphrase(),
    }
}

// ===== Run helpers =====

/// 由设置组装备份载荷：口令解密 + 远端目录固定为 `{remote_dir}/auto`。
fn build_auto_payload(settings: &BackupSettings) -> Result<backup::BackupRunPayload, String> {
    let auto = &settings.auto;
    if auto.unit_ids.is_empty() {
        return Err("自动备份未选择备份内容".to_string());
    }
    if auto.webdav_connection_ids.is_empty() {
        return Err("自动备份未选择 WebDAV 目标".to_string());
    }
    let passphrase = decrypt_auto_passphrase(auto)?;
    let base = settings.default_remote_dir.trim().trim_matches('/');
    let base = if base.is_empty() { "AgentBuddy" } else { base };
    Ok(backup::BackupRunPayload {
        unit_ids: auto.unit_ids.clone(),
        webdav_connection_ids: auto.webdav_connection_ids.clone(),
        // 强制加密：自动备份没有明文上传旁路。
        passphrase: Some(passphrase),
        remote_prefix: Some(format!("{base}/auto")),
        acknowledge_plaintext_secrets: false,
    })
}

fn decrypt_auto_passphrase(auto: &AutoBackupSettings) -> Result<String, String> {
    if !auto.has_passphrase() {
        return Err("自动备份口令未设置".to_string());
    }
    let master = config::load_secrets_key()?;
    crypto::decrypt_secret(
        &master,
        &auto.passphrase_salt,
        &auto.passphrase_nonce,
        &auto.passphrase_cipher,
    )
    .map_err(|e| format!("读取自动备份口令失败: {e}"))
}

/// 记录一轮自动备份的尝试结果（成功失败都算一次尝试，下一周期自然重试）。
fn record_run(ok: bool, message: &str) -> Result<(), String> {
    config::update_backup_settings_atomic(|mut settings| {
        settings.auto.last_run_at = Some(now_secs());
        settings.auto.last_ok = Some(ok);
        settings.auto.last_message = Some(message.chars().take(500).collect());
        Ok(settings)
    })
    .map(|_| ())
}

/// 手动备份占用运行锁时记录一次跳过。清空 last_ok：跳过不是一次运行，
/// 不沿用上次结果的「成功/失败」标记，否则页面会同时显示
/// 「上次运行 · 成功」和「跳过本轮」。内容未变化时不写盘（原子读改写内置），
/// 连续占用期间不会每 30s 重写文件。
fn record_skip() -> Result<(), String> {
    const MSG: &str = "跳过本轮：已有备份正在进行";
    config::update_backup_settings_atomic(|mut settings| {
        if settings.auto.last_message.as_deref() == Some(MSG) && settings.auto.last_ok.is_none() {
            return Ok(settings);
        }
        settings.auto.last_ok = None;
        settings.auto.last_message = Some(MSG.to_string());
        Ok(settings)
    })
    .map(|_| ())
}

fn now_secs() -> u64 {
    Local::now().timestamp().max(0) as u64
}

fn unix_to_local(secs: u64) -> DateTime<Local> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
        .with_timezone(&Local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use std::fs;

    /// 与 opencode_config 同款：临时 HOME + 测试锁，隔离真实 ~/.agentbuddy。
    /// Drop 时恢复原 HOME，避免污染进程内后续测试。
    struct TempHome {
        path: std::path::PathBuf,
        prev_home: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempHome {
        fn new() -> Self {
            let guard = crate::config::lock_home_for_test();
            let path = std::env::temp_dir().join(format!(
                "agentbuddy-sched-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            let prev_home = std::env::var_os("HOME");
            std::env::set_var("HOME", &path);
            Self {
                path,
                prev_home,
                _guard: guard,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match self.prev_home.take() {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn update_payload(enabled: bool) -> AutoBackupSettingsUpdate {
        AutoBackupSettingsUpdate {
            enabled,
            mode: "interval".into(),
            interval_hours: 24,
            daily_time: String::new(),
            unit_ids: vec!["agent:codex".into()],
            webdav_connection_ids: vec!["dav-1".into()],
            passphrase: None,
        }
    }

    #[test]
    fn update_reenable_resets_run_state_and_anchor() {
        let _home = TempHome::new();

        // 预置：已停用，残留上一周期的运行状态；带口令密文以通过含密单元校验。
        let mut seeded = BackupSettings::default();
        seeded.auto.last_run_at = Some(1_000);
        seeded.auto.last_ok = Some(true);
        seeded.auto.last_message = Some("上次成功".into());
        seeded.auto.passphrase_cipher = "stale".into();
        config::update_backup_settings_atomic(|_| Ok(seeded)).unwrap();

        update_auto_backup_settings(update_payload(true)).unwrap();

        let auto = config::load_backup_settings().unwrap().auto;
        assert!(auto.enabled);
        assert!(auto.enabled_at.is_some());
        // 重新启用是新的调度周期：过期的 last_run 不得让首跑立即触发。
        assert_eq!(auto.last_run_at, None);
        assert_eq!(auto.last_ok, None);
        assert_eq!(auto.last_message, None);
    }

    #[test]
    fn update_resave_while_enabled_preserves_run_state() {
        let _home = TempHome::new();

        // 预置口令密文：agent:codex 属含密单元，启用前必须有口令。
        let mut seeded = BackupSettings::default();
        seeded.auto.passphrase_cipher = "stale".into();
        config::update_backup_settings_atomic(|_| Ok(seeded)).unwrap();

        update_auto_backup_settings(update_payload(true)).unwrap();
        let anchor = config::load_backup_settings().unwrap().auto.enabled_at;

        // 模拟一轮运行后，保持启用再次保存：锚点与运行状态都应保留。
        config::update_backup_settings_atomic(|mut settings| {
            settings.auto.last_run_at = Some(2_000);
            settings.auto.last_ok = Some(true);
            settings.auto.last_message = Some("备份完成".into());
            Ok(settings)
        })
        .unwrap();

        update_auto_backup_settings(update_payload(true)).unwrap();

        let auto = config::load_backup_settings().unwrap().auto;
        assert_eq!(auto.enabled_at, anchor);
        assert_eq!(auto.last_run_at, Some(2_000));
        assert_eq!(auto.last_ok, Some(true));
    }

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).expect("fixed offset")
    }

    fn at((y, mo, d, h, mi): (i32, u32, u32, u32, u32)) -> DateTime<FixedOffset> {
        tz().with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid local time")
    }

    #[test]
    fn interval_mode_uses_last_run_anchor() {
        let now = at((2026, 9, 1, 12, 0));
        let last = at((2026, 9, 1, 7, 0));
        let next = next_run_after("interval", 6, "", now, Some(last), None);
        assert_eq!(next, Some(at((2026, 9, 1, 13, 0))));
    }

    #[test]
    fn interval_mode_uses_enabled_anchor_before_first_run() {
        let now = at((2026, 9, 1, 12, 0));
        let enabled = at((2026, 9, 1, 10, 0));
        let next = next_run_after("interval", 6, "", now, None, Some(enabled));
        assert_eq!(next, Some(at((2026, 9, 1, 16, 0))));
    }

    #[test]
    fn interval_mode_without_anchor_is_never_due() {
        let now = at((2026, 9, 1, 12, 0));
        assert_eq!(next_run_after("interval", 6, "", now, None, None), None);
        assert_eq!(next_run_after("interval", 0, "", now, None, None), None);
    }

    #[test]
    fn interval_mode_missed_run_is_due_immediately() {
        // 睡眠/关机跨过调度点：next 已成过去时，唤醒后第一个 tick 到期补跑。
        let now = at((2026, 9, 3, 9, 0));
        let last = at((2026, 9, 1, 12, 0));
        let next = next_run_after("interval", 6, "", now, Some(last), None);
        assert_eq!(next, Some(at((2026, 9, 1, 18, 0))));
        assert!(is_due(now, next));
    }

    #[test]
    fn daily_mode_today_when_time_ahead() {
        let now = at((2026, 9, 1, 8, 0));
        let next = next_run_after("daily", 0, "09:30", now, None, None);
        assert_eq!(next, Some(at((2026, 9, 1, 9, 30))));
    }

    #[test]
    fn daily_mode_tomorrow_when_time_passed() {
        let now = at((2026, 9, 1, 10, 0));
        let last = at((2026, 9, 1, 9, 30));
        let next = next_run_after("daily", 0, "09:30", now, Some(last), None);
        assert_eq!(next, Some(at((2026, 9, 2, 9, 30))));
    }

    #[test]
    fn daily_mode_accepts_non_padded_time() {
        let now = at((2026, 9, 1, 8, 0));
        assert_eq!(
            next_run_after("daily", 0, "9:05", now, None, None),
            Some(at((2026, 9, 1, 9, 5)))
        );
    }

    #[test]
    fn daily_mode_invalid_time_is_none() {
        let now = at((2026, 9, 1, 10, 0));
        for bad in ["25:00", "12:60", "abc", "", "12"] {
            assert_eq!(
                next_run_after("daily", 0, bad, now, None, None),
                None,
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn unknown_mode_is_never_due() {
        let now = at((2026, 9, 1, 10, 0));
        assert_eq!(next_run_after("weekly", 6, "", now, None, None), None);
    }

    #[test]
    fn is_due_semantics() {
        let now = at((2026, 9, 1, 12, 0));
        assert!(!is_due(now, None));
        assert!(is_due(now, Some(at((2026, 9, 1, 12, 0)))));
        assert!(!is_due(now, Some(at((2026, 9, 1, 12, 1)))));
    }

    #[test]
    fn secrets_detection_matches_backup_kernel_rule() {
        assert!(unit_ids_contain_secrets(&["app:agentbuddy:db".into()]));
        assert!(unit_ids_contain_secrets(&["tool:cliproxyapi".into()]));
        assert!(unit_ids_contain_secrets(&["agent:codex".into()]));
        assert!(!unit_ids_contain_secrets(&[]));
    }

    #[test]
    fn validate_auto_config_rejects_bad_shapes() {
        let units = vec!["agent:codex".to_string()];
        let dav = vec!["dav-1".to_string()];
        assert!(validate_auto_config("interval", 6, "", &units, &dav).is_ok());
        assert!(validate_auto_config("interval", 5, "", &units, &dav).is_err());
        assert!(validate_auto_config("daily", 0, "09:30", &units, &dav).is_ok());
        assert!(validate_auto_config("daily", 0, "24:00", &units, &dav).is_err());
        assert!(validate_auto_config("interval", 6, "", &[], &dav).is_err());
        assert!(validate_auto_config("interval", 6, "", &units, &[]).is_err());
        assert!(validate_auto_config("monthly", 6, "", &units, &dav).is_err());
    }

    #[test]
    fn normalize_id_list_trims_and_drops_empty() {
        let out = normalize_id_list(&[" a ".to_string(), String::new(), "b".to_string()]);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }
}
