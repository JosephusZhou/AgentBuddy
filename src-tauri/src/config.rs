//! App configuration under `~/.agentbuddy/config.json`.
//! Ensures the config directory and file exist on startup, and manages theme + secretsKey.

use crate::crypto;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;

/// 后端兜底默认主题。精确的“是否属于主题注册表”判断由前端 `lib/theme.ts`
/// 唯一维护（启动时检测遗留/非法值并回写还原）；后端此常量仅需是一个合法注册 id，
/// 用于配置文件损坏、theme 字段缺失/形状非法时的安全兜底。
const DEFAULT_THEME: &str = "qoder-light";

/// Public config returned to the frontend — never includes secretsKey.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub theme: String,
    #[serde(default)]
    pub backup: BackupSettings,
    #[serde(default)]
    pub network: NetworkSettings,
}

/// Backup-related preferences stored in config.json (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
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

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
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
        });
    }

    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read config.json: {}", e))?;

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
            });
        }
    };

    let mut needs_write = false;

    match obj.get("theme").and_then(|v| v.as_str()) {
        Some(theme) if is_valid_theme_slug(theme) => {}
        Some(_) | None => {
            obj.insert("theme".to_string(), Value::String(DEFAULT_THEME.to_string()));
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
    })
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

/// Normalize + validate proxy settings before write.
pub fn normalize_network_settings(mut settings: NetworkSettings) -> Result<NetworkSettings, String> {
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

    let path = config_path()?;
    let _ = ensure_app_config()?;
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "config.json 格式无效".to_string())?;
    let network_val = serde_json::to_value(&settings)
        .map_err(|e| format!("序列化网络设置失败: {}", e))?;
    obj.insert("network".to_string(), network_val);
    write_raw(&path, &root)?;
    Ok(settings)
}

pub fn save_backup_settings(settings: BackupSettings) -> Result<BackupSettings, String> {
    let mut settings = settings;
    settings.cliproxyapi_conf_path = settings.cliproxyapi_conf_path.trim().to_string();
    settings.sub2api_root_path = settings.sub2api_root_path.trim().to_string();
    let remote = settings.default_remote_dir.trim().trim_matches('/').to_string();
    settings.default_remote_dir = if remote.is_empty() {
        default_remote_dir()
    } else {
        remote
    };

    let path = config_path()?;
    let _ = ensure_app_config()?;
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let obj = root.as_object_mut().ok_or_else(|| "config.json 格式无效".to_string())?;
    let backup_val =
        serde_json::to_value(&settings).map_err(|e| format!("序列化备份设置失败: {}", e))?;
    obj.insert("backup".to_string(), backup_val);
    write_raw(&path, &root)?;
    Ok(settings)
}

/// Load the master secrets key for encryption. Never expose this via Tauri commands.
pub fn load_secrets_key() -> Result<[u8; 32], String> {
    ensure_app_config()?;
    let path = config_path()?;
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read config.json: {}", e))?;
    let root: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let encoded = root
        .get("secretsKey")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "secretsKey missing from config.json".to_string())?;
    crypto::decode_master_key(encoded)
}

pub fn set_theme(theme: String) -> Result<AppConfig, String> {
    let theme = normalize_theme(&theme);
    let path = config_path()?;

    // Make sure dir + file baseline (incl. secretsKey) exist first.
    let _ = ensure_app_config()?;

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

fn write_raw(path: &PathBuf, value: &Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(path, format!("{}\n", pretty))
        .map_err(|e| format!("Failed to write config.json: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_known_and_new_theme_slugs() {
        for id in ["qoder-light", "qoder-dark", "claude", "catppuccin-mocha", "synthwave-84"] {
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
}
