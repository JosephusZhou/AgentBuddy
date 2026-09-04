//! App configuration under `~/.agentbuddy/config.json`.
//! Ensures the config directory and file exist on startup, and manages theme + secretsKey.

use crate::crypto;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// 后端兜底默认主题。精确的“是否属于主题注册表”判断由前端 `lib/theme.ts`
/// 唯一维护（启动时检测遗留/非法值并回写还原）；后端此常量仅需是一个合法注册 id，
/// 用于配置文件损坏、theme 字段缺失/形状非法时的安全兜底。
const DEFAULT_THEME: &str = "qoder-light";

/// 串行化进程内对 config.json 的所有读改写操作。
///
/// Models.dev 启动刷新在后台任务中运行，界面同时可能保存主题、代理、备份或路由设置。
/// 如果没有统一锁，两个写入者可能各自读取旧快照，后写入的一方会静默覆盖前一方的字段。
static CONFIG_FILE_LOCK: Mutex<()> = Mutex::new(());

fn lock_config_file() -> Result<MutexGuard<'static, ()>, String> {
    CONFIG_FILE_LOCK
        .lock()
        .map_err(|_| "config.json 锁已损坏".to_string())
}

/// Public config returned to the frontend — never includes secretsKey.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub theme: String,
    #[serde(default)]
    pub backup: BackupSettings,
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]
    pub route_aggregation: crate::route_aggregation::RouteAggregationConfig,
    /// Models.dev 目录最近一次成功缓存的 Unix 时间戳。
    #[serde(default)]
    pub models_dev_cached_at: Option<u64>,
}

fn default_auto_mode() -> String {
    "interval".to_string()
}

fn default_auto_interval_hours() -> u32 {
    24
}

/// 自动备份（定时触发）配置与运行状态，存 config.json `backup.auto`。
/// `passphrase_*` 是经 secretsKey 加密的备份口令三段密文（与 WebDAV 密码同构），
/// 任何 command 都不得回传明文或密文；前端只读取 `hasPassphrase` 存在性标志。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoBackupSettings {
    #[serde(default)]
    pub enabled: bool,
    /// interval | daily
    #[serde(default = "default_auto_mode")]
    pub mode: String,
    /// interval 模式的触发间隔（小时），合法值 6/12/24/72/168。
    #[serde(default = "default_auto_interval_hours")]
    pub interval_hours: u32,
    /// daily 模式的本地时间 "HH:MM"。
    #[serde(default)]
    pub daily_time: String,
    /// 固定备份单元 id（BackupUnitNode 树的叶子 id）。
    #[serde(default)]
    pub unit_ids: Vec<String>,
    /// WebDAV 连接 id 列表。
    #[serde(default)]
    pub webdav_connection_ids: Vec<String>,
    #[serde(default)]
    pub passphrase_salt: String,
    #[serde(default)]
    pub passphrase_nonce: String,
    #[serde(default)]
    pub passphrase_cipher: String,
    /// 最近一次启用的时间锚点（Unix 秒）：interval 模式在首跑前用它推算下次触发，
    /// 避免每次 tick 以 now 为基线导致调度点无限顺延。
    #[serde(default)]
    pub enabled_at: Option<u64>,
    /// 最近一次自动备份尝试时间（Unix 秒）。
    #[serde(default)]
    pub last_run_at: Option<u64>,
    #[serde(default)]
    pub last_ok: Option<bool>,
    /// 最近一次结果文案（不含敏感信息）。
    #[serde(default)]
    pub last_message: Option<String>,
}

impl Default for AutoBackupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_auto_mode(),
            interval_hours: default_auto_interval_hours(),
            daily_time: String::new(),
            unit_ids: Vec::new(),
            webdav_connection_ids: Vec::new(),
            passphrase_salt: String::new(),
            passphrase_nonce: String::new(),
            passphrase_cipher: String::new(),
            enabled_at: None,
            last_run_at: None,
            last_ok: None,
            last_message: None,
        }
    }
}

impl AutoBackupSettings {
    pub fn has_passphrase(&self) -> bool {
        !self.passphrase_cipher.trim().is_empty()
    }
}

/// Backup-related preferences stored in config.json (no secrets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    /// 定时自动备份配置；旧 config.json 缺失该字段时取默认值（关闭）。
    #[serde(default)]
    pub auto: AutoBackupSettings,
    /// Override path to CLIProxyAPI conf; empty = auto-detect.
    #[serde(default)]
    pub cliproxyapi_conf_path: String,
    /// Override sub2api install/root dir; empty = auto-detect.
    #[serde(default)]
    pub sub2api_root_path: String,
    /// Remote WebDAV subdir prefix (no leading slash).
    #[serde(default = "default_remote_dir")]
    pub default_remote_dir: String,
    /// Deprecated: local copies are never kept after upload. Ignored on read/write for compatibility.
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    pub keep_local_copy: bool,
}

fn default_remote_dir() -> String {
    "AgentBuddy".to_string()
}

/// 跨模块共享的测试锁：凡交换 HOME 环境变量、或依赖 HOME 派生路径
///（含 `~/.agentbuddy` 应用数据目录与各 agent 配置）的测试都须持锁，
/// 避免并行测试相互踩踏（dirs::home_dir 受 HOME 环境变量影响）。
#[cfg(test)]
pub static TEST_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 获取 HOME 测试锁（供各模块单测使用）。
#[cfg(test)]
pub fn lock_home_for_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            auto: AutoBackupSettings::default(),
            cliproxyapi_conf_path: String::new(),
            sub2api_root_path: String::new(),
            default_remote_dir: default_remote_dir(),
            keep_local_copy: false,
        }
    }
}

/// Network / outbound HTTP proxy preferences (config.json `network`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    #[serde(default)]
    pub proxy: ProxySettings,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            proxy: ProxySettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    None,
    System,
    Custom,
}

impl Default for ProxyMode {
    fn default() -> Self {
        ProxyMode::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Http,
    Socks5,
}

impl Default for ProxyProtocol {
    fn default() -> Self {
        ProxyProtocol::Http
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    #[serde(default)]
    pub mode: ProxyMode,
    /// Used only when mode == Custom.
    #[serde(default)]
    pub protocol: ProxyProtocol,
    #[serde(default)]
    pub host: String,
    /// 1..=65535 when custom; 0 means unset.
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    /// Stored plaintext in config.json (local-only app preference).
    /// Never logged; UI may leave empty on edit to keep existing.
    #[serde(default)]
    pub password: String,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            mode: ProxyMode::None,
            protocol: ProxyProtocol::Http,
            host: String::new(),
            port: 0,
            username: String::new(),
            password: String::new(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
            backup: BackupSettings::default(),
            network: NetworkSettings::default(),
            route_aggregation: crate::route_aggregation::RouteAggregationConfig::default(),
            models_dev_cached_at: None,
        }
    }
}

pub fn app_dir() -> Result<PathBuf, String> {
    crate::platform::app_data_dir()
}

fn config_path() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("config.json"))
}

fn normalize_theme(value: &str) -> String {
    if is_valid_theme_slug(value) {
        value.to_string()
    } else {
        DEFAULT_THEME.to_string()
    }
}

/// 主题 id 由前端 THEMES 注册表统一维护；后端不复制该列表（避免双源漂移，
/// 否则新增主题时忘同步后端会导致合法主题被误判非法而重置）。
/// 后端仅校验其形状为安全 slug：非空、仅小写字母/数字/连字符、长度受限——
/// 挡住会破坏 `data-theme` 的注入值。“是否属于注册表”的精确判定与遗留值还原在前端完成。
fn is_valid_theme_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_valid_secrets_key(value: &str) -> bool {
    crypto::decode_master_key(value).is_ok()
}

/// Ensure `~/.agentbuddy` and `config.json` exist.
/// - Missing directory → create
/// - Missing `skills/` under app dir → create
/// - Missing config file → create with theme + secretsKey
/// - Missing / invalid theme → write default theme
/// - Missing / invalid secretsKey → generate a fresh 32-byte key
pub fn ensure_app_config() -> Result<AppConfig, String> {
    let _config_guard = lock_config_file()?;
    ensure_app_config_locked()
}

fn ensure_app_config_locked() -> Result<AppConfig, String> {
    let dir = app_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create .agentbuddy directory: {}", e))?;
    }

    // Skills library root used by future skills-manage features.
    let skills_dir = dir.join("skills");
    if !skills_dir.exists() {
        fs::create_dir_all(&skills_dir)
            .map_err(|e| format!("Failed to create .agentbuddy/skills directory: {}", e))?;
    }

    let path = config_path()?;
    if !path.exists() {
        let theme = DEFAULT_THEME.to_string();
        let secrets_key = crypto::generate_secrets_key();
        write_full_config(&path, &theme, &secrets_key)?;
        return Ok(AppConfig {
            theme,
            backup: BackupSettings::default(),
            network: NetworkSettings::default(),
            route_aggregation: crate::route_aggregation::RouteAggregationConfig::default(),
            models_dev_cached_at: None,
        });
    }

    let raw =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read config.json: {}", e))?;

    let mut root: Value = if raw.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    };

    let obj = match root.as_object_mut() {
        Some(map) => map,
        None => {
            let theme = DEFAULT_THEME.to_string();
            let secrets_key = crypto::generate_secrets_key();
            write_full_config(&path, &theme, &secrets_key)?;
            return Ok(AppConfig {
                theme,
                backup: BackupSettings::default(),
                network: NetworkSettings::default(),
                route_aggregation: crate::route_aggregation::RouteAggregationConfig::default(),
                models_dev_cached_at: None,
            });
        }
    };

    let mut needs_write = false;

    match obj.get("theme").and_then(|v| v.as_str()) {
        Some(theme) if is_valid_theme_slug(theme) => {}
        Some(_) | None => {
            obj.insert(
                "theme".to_string(),
                Value::String(DEFAULT_THEME.to_string()),
            );
            needs_write = true;
        }
    }

    match obj.get("secretsKey").and_then(|v| v.as_str()) {
        Some(key) if is_valid_secrets_key(key) => {}
        Some(_) | None => {
            obj.insert(
                "secretsKey".to_string(),
                Value::String(crypto::generate_secrets_key()),
            );
            needs_write = true;
        }
    }

    if needs_write {
        write_raw(&path, &Value::Object(obj.clone()))?;
    }

    Ok(AppConfig {
        theme: normalize_theme(
            obj.get("theme")
                .and_then(|v| v.as_str())
                .unwrap_or(DEFAULT_THEME),
        ),
        backup: parse_backup_settings(obj.get("backup")),
        network: parse_network_settings(obj.get("network")),
        route_aggregation: parse_route_aggregation(obj.get("routeAggregation")),
        models_dev_cached_at: parse_models_dev_cached_at(obj.get("modelsDevCachedAt")),
    })
}

fn parse_route_aggregation(
    value: Option<&Value>,
) -> crate::route_aggregation::RouteAggregationConfig {
    let Some(v) = value else {
        return crate::route_aggregation::RouteAggregationConfig::default();
    };
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn parse_models_dev_cached_at(value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| v.as_u64())
}

fn parse_backup_settings(value: Option<&Value>) -> BackupSettings {
    let Some(v) = value else {
        return BackupSettings::default();
    };
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn parse_network_settings(value: Option<&Value>) -> NetworkSettings {
    let Some(v) = value else {
        return NetworkSettings::default();
    };
    serde_json::from_value(v.clone()).unwrap_or_default()
}

pub fn load_app_config() -> Result<AppConfig, String> {
    ensure_app_config()
}

pub fn load_backup_settings() -> Result<BackupSettings, String> {
    Ok(load_app_config()?.backup)
}

pub fn load_network_settings() -> Result<NetworkSettings, String> {
    Ok(load_app_config()?.network)
}

pub fn load_models_dev_cached_at() -> Result<Option<u64>, String> {
    Ok(load_app_config()?.models_dev_cached_at)
}

/// 记录 Models.dev 最近一次成功写入本地缓存的时间，不改动其它配置项。
pub fn save_models_dev_cached_at(timestamp: u64) -> Result<(), String> {
    let _config_guard = lock_config_file()?;
    let path = config_path()?;
    let _ = ensure_app_config_locked()?;
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "config.json 格式无效".to_string())?;
    obj.insert("modelsDevCachedAt".to_string(), Value::from(timestamp));
    write_raw(&path, &root)
}

/// Normalize + validate proxy settings before write.
pub fn normalize_network_settings(
    mut settings: NetworkSettings,
) -> Result<NetworkSettings, String> {
    settings.proxy.host = settings.proxy.host.trim().to_string();
    settings.proxy.username = settings.proxy.username.trim().to_string();
    // Keep password as-is except trim ends only if entirely whitespace → empty is fine.
    // Do not trim middle of password.

    match settings.proxy.mode {
        ProxyMode::None | ProxyMode::System => {
            // Keep custom fields for when user switches back, but ensure mode is valid.
        }
        ProxyMode::Custom => {
            if settings.proxy.host.is_empty() {
                return Err("自定义代理需要填写主机地址".to_string());
            }
            if settings.proxy.host.contains("://")
                || settings.proxy.host.contains('/')
                || settings.proxy.host.contains('@')
                || settings.proxy.host.contains(' ')
            {
                return Err("主机地址只需填写域名或 IP，不要包含协议、路径或空格".to_string());
            }
            if settings.proxy.port == 0 {
                return Err("请填写有效的代理端口（1–65535）".to_string());
            }
        }
    }

    Ok(settings)
}

pub fn save_network_settings(settings: NetworkSettings) -> Result<NetworkSettings, String> {
    let settings = normalize_network_settings(settings)?;

    let _config_guard = lock_config_file()?;
    let path = config_path()?;
    let _ = ensure_app_config_locked()?;
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "config.json 格式无效".to_string())?;
    let network_val =
        serde_json::to_value(&settings).map_err(|e| format!("序列化网络设置失败: {}", e))?;
    obj.insert("network".to_string(), network_val);
    write_raw(&path, &root)?;
    Ok(settings)
}

/// Save route aggregation config to config.json under the `routeAggregation` key.
pub fn save_route_aggregation_config(
    config: &crate::route_aggregation::RouteAggregationConfig,
) -> Result<(), String> {
    let _config_guard = lock_config_file()?;
    let path = config_path()?;
    let _ = ensure_app_config_locked()?;
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "config.json 格式无效".to_string())?;
    let val = serde_json::to_value(config).map_err(|e| format!("序列化路由聚合配置失败: {}", e))?;
    obj.insert("routeAggregation".to_string(), val);
    write_raw(&path, &root)?;
    Ok(())
}

/// 归一化手动路径字段（trim 与默认目录兜底），backup 段各保存路径共用。
fn normalize_backup_settings(mut settings: BackupSettings) -> BackupSettings {
    settings.cliproxyapi_conf_path = settings.cliproxyapi_conf_path.trim().to_string();
    settings.sub2api_root_path = settings.sub2api_root_path.trim().to_string();
    let remote = settings
        .default_remote_dir
        .trim()
        .trim_matches('/')
        .to_string();
    settings.default_remote_dir = if remote.is_empty() {
        default_remote_dir()
    } else {
        remote
    };
    settings
}

/// 在已持有 CONFIG_FILE_LOCK 的前提下把 backup 段整体写回 config.json。
fn write_backup_settings_locked(path: &Path, settings: &BackupSettings) -> Result<(), String> {
    let raw = fs::read_to_string(path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "config.json 格式无效".to_string())?;
    let backup_val =
        serde_json::to_value(settings).map_err(|e| format!("序列化备份设置失败: {}", e))?;
    obj.insert("backup".to_string(), backup_val);
    write_raw(path, &root)?;
    Ok(())
}

/// backup 段的原子读-改-写：全程持有 CONFIG_FILE_LOCK，读出磁盘现状交给闭包
/// 修改后整体写回。手动备份设置、自动备份设置与调度线程的运行状态回写都必须
/// 经由本函数修改 backup 段，闭合「读-改」与「写」之间被并发写入覆盖的窗口。
///
/// 读取失败或 config.json 无法解析时直接报错放弃写入：修改型操作在无法确认
/// 现状时宁可失败，也不把默认值静默落盘覆盖已有配置（如自动备份的口令密文）。
/// 闭包返回与现状完全相同的内容时不产生写盘。
///
/// 注意：闭包在 CONFIG_FILE_LOCK 内执行，不得再调用任何拿这把锁的函数
///（load_secrets_key / load_backup_settings / save_backup_settings 等），
/// 需要锁的数据应在进入本函数前取好。
pub fn update_backup_settings_atomic<F>(f: F) -> Result<BackupSettings, String>
where
    F: FnOnce(BackupSettings) -> Result<BackupSettings, String>,
{
    let _config_guard = lock_config_file()?;
    let path = config_path()?;
    let _ = ensure_app_config_locked()?;

    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("读取 config.json 失败，已取消保存: {}", e))?;
    let root: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("config.json 无法解析，已取消保存: {}", e))?;
    let current = parse_backup_settings(root.get("backup"));

    // 闭包按值接管现状（所有权移交后还需与结果比较判断是否跳写），先克隆一份。
    let settings = normalize_backup_settings(f(current.clone())?);
    if settings == current {
        return Ok(settings);
    }
    write_backup_settings_locked(&path, &settings)?;
    Ok(settings)
}

/// Load the master secrets key for encryption. Never expose this via Tauri commands.
pub fn load_secrets_key() -> Result<[u8; 32], String> {
    let _config_guard = lock_config_file()?;
    ensure_app_config_locked()?;
    let path = config_path()?;
    let raw =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read config.json: {}", e))?;
    let root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let encoded = root
        .get("secretsKey")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "secretsKey missing from config.json".to_string())?;
    crypto::decode_master_key(encoded)
}

pub fn set_theme(theme: String) -> Result<AppConfig, String> {
    let theme = normalize_theme(&theme);
    let _config_guard = lock_config_file()?;
    let path = config_path()?;

    // Make sure dir + file baseline (incl. secretsKey) exist first.
    let _ = ensure_app_config_locked()?;

    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));

    if let Some(obj) = root.as_object_mut() {
        obj.insert("theme".to_string(), Value::String(theme.clone()));
    } else {
        root = json!({
            "theme": theme.clone(),
            "secretsKey": crypto::generate_secrets_key(),
        });
    }

    write_raw(&path, &root)?;

    Ok(AppConfig {
        theme,
        backup: parse_backup_settings(root.get("backup")),
        network: parse_network_settings(root.get("network")),
        route_aggregation: parse_route_aggregation(root.get("routeAggregation")),
        models_dev_cached_at: parse_models_dev_cached_at(root.get("modelsDevCachedAt")),
    })
}

fn write_full_config(path: &PathBuf, theme: &str, secrets_key: &str) -> Result<(), String> {
    let mut map = Map::new();
    map.insert("theme".to_string(), Value::String(theme.to_string()));
    map.insert(
        "secretsKey".to_string(),
        Value::String(secrets_key.to_string()),
    );
    write_raw(path, &Value::Object(map))
}

fn write_raw(path: &Path, value: &Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    // 临时文件 + rename 原子替换：进程中途崩溃不会留下半截 config.json。
    // 半截文件会被读取端按损坏处理并重置 secretsKey，导致已有密文全部不可解，
    // 代价远大于多一次 rename。并发写由 CONFIG_FILE_LOCK 串行化，临时名不会冲突。
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, format!("{}\n", pretty))
        .map_err(|e| format!("Failed to write config.json.tmp: {}", e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Failed to replace config.json: {}", e)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_known_and_new_theme_slugs() {
        for id in [
            "qoder-light",
            "qoder-dark",
            "claude",
            "catppuccin-mocha",
            "synthwave-84",
        ] {
            assert!(is_valid_theme_slug(id), "should accept {id}");
            assert_eq!(normalize_theme(id), id);
        }
    }

    #[test]
    fn legacy_ids_pass_shape_check_and_are_restored_by_frontend() {
        // 旧遗留 id（light/dark）形状合法，故后端原样透传、不重置——
        // 精确的“不在注册表→还原成默认”判定由前端 lib/theme.ts 负责。
        for legacy in ["light", "dark"] {
            assert!(is_valid_theme_slug(legacy));
            assert_eq!(normalize_theme(legacy), legacy);
        }
    }

    #[test]
    fn rejects_malformed_theme_values() {
        // 空串、大写、下划线、点号、路径分隔符、注入字符、超长值都应被拒并回退默认。
        for bad in [
            "",
            "Dark",
            "one_dark",
            "a.b",
            "../evil",
            "\" onload=x",
            "with space",
            &"x".repeat(65),
        ] {
            assert!(!is_valid_theme_slug(bad), "should reject {bad:?}");
            assert_eq!(normalize_theme(bad), DEFAULT_THEME);
        }
    }

    #[test]
    fn slug_length_boundary() {
        assert!(is_valid_theme_slug(&"a".repeat(64)));
        assert!(!is_valid_theme_slug(&"a".repeat(65)));
    }

    #[test]
    fn auto_backup_settings_default_on_legacy_config() {
        // 旧 config.json 的 backup 段没有 auto 字段：解析应取默认值（关闭），
        // 已有的手动路径字段不受影响。
        let parsed: BackupSettings =
            serde_json::from_value(json!({ "defaultRemoteDir": "Backups" })).unwrap();
        assert_eq!(parsed.default_remote_dir, "Backups");
        assert!(!parsed.auto.enabled);
        assert_eq!(parsed.auto.mode, "interval");
        assert_eq!(parsed.auto.interval_hours, 24);
        assert!(!parsed.auto.has_passphrase());
    }

    #[test]
    fn auto_backup_settings_roundtrip_keeps_passphrase_fields() {
        let auto = AutoBackupSettings {
            enabled: true,
            mode: "daily".into(),
            daily_time: "09:30".into(),
            unit_ids: vec!["app:agentbuddy:db".into()],
            webdav_connection_ids: vec!["dav-1".into()],
            passphrase_salt: "cw==".into(),
            passphrase_nonce: "cw==".into(),
            passphrase_cipher: "cw==".into(),
            enabled_at: Some(1_700_000_000),
            last_run_at: Some(1_700_000_100),
            last_ok: Some(true),
            last_message: Some("ok".into()),
            ..AutoBackupSettings::default()
        };
        let settings = BackupSettings {
            auto: auto.clone(),
            ..BackupSettings::default()
        };
        let val = serde_json::to_value(&settings).unwrap();
        // 密文字段必须落盘（save_backup_settings 依赖完整序列化持久化 auto 段）。
        assert_eq!(val["auto"]["passphraseCipher"], "cw==");
        let back: BackupSettings = serde_json::from_value(val).unwrap();
        assert_eq!(back.auto, auto);
        assert!(back.auto.has_passphrase());
    }
}
