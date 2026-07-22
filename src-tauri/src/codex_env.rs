//! Multi-environment manager for Codex CLI via `CODEX_HOME`.
//!
//! Default root stays at `~/.codex`. Extra environments live under
//! `$HOME/.codex-<slug>` (or a user-chosen path still inside `$HOME`).
//! Shell aliases are written into a managed marker block (independent from Claude Env).
//!
//! Token（API Key）写入各环境 `$CODEX_HOME/auth.json`：
//! `{ "OPENAI_API_KEY": "<token>" }`（合并保留其它键；权限 0o600）。

use crate::db;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ENV_ID: &str = "default";
const MARKER_BEGIN: &str = "# >>> AgentBuddy Codex Env (managed) >>>";
const MARKER_END: &str = "# <<< AgentBuddy Codex Env (managed) <<<";

const CORE_FILES: &[&str] = &["config.toml", "AGENTS.md"];
const CORE_DIRS: &[&str] = &["skills"];

/* ===== DTOs ===== */

/// Public list item returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvironment {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub config_dir: String,
    pub alias_name: String,
    pub is_default: bool,
    pub source: String,
    pub notes: String,
    pub alias_installed: bool,
    pub dir_exists: bool,
    pub has_config: bool,
    pub has_skills: bool,
    pub has_auth: bool,
    /// MCP sync status vs default ~/.codex/config.toml [mcp_servers].
    /// default | in_sync | out_of_sync | missing | no_global
    pub mcp_sync_status: String,
    pub mcp_server_count: u32,
    pub global_mcp_server_count: u32,
    /// config.toml → model（实时读取，不入库）
    pub model: String,
    /// config.toml → model_provider
    pub model_provider: String,
    /// resolved base URL（custom provider base_url 或 openai_base_url）
    pub base_url: String,
    /// 列表接口不回传明文 token；编辑时用 `get_codex_env_secret` 按需拉取。
    pub api_key: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Internal DB row (no runtime probes).
#[derive(Debug, Clone)]
pub struct CodexEnvironmentRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub config_dir: String,
    pub alias_name: String,
    pub is_default: bool,
    pub source: String,
    pub notes: String,
    pub alias_installed: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvUpsertPayload {
    pub id: Option<String>,
    pub name: String,
    pub slug: String,
    pub config_dir: String,
    pub alias_name: String,
    pub notes: Option<String>,
    /// None=不改；Some("")=删除；Some(v)=写入
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvClonePayload {
    pub source_id: String,
    pub name: String,
    pub slug: String,
    pub config_dir: String,
    pub alias_name: String,
    pub notes: Option<String>,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    /// When true, copy [mcp_servers] from default ~/.codex/config.toml.
    pub sync_mcp: Option<bool>,
    /// When true, write shell alias after creation. Defaults to false.
    pub install_alias: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvImportPayload {
    pub config_dir: String,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub alias_name: Option<String>,
    pub notes: Option<String>,
    pub install_alias: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvCandidate {
    pub path: String,
    pub suggested_name: String,
    pub suggested_slug: String,
    pub suggested_alias: String,
    pub has_config: bool,
    pub has_skills: bool,
    pub has_auth: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvSniffResult {
    pub candidates: Vec<CodexEnvCandidate>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvShellStatus {
    pub zshrc_path: String,
    pub zshrc_exists: bool,
    pub block_present: bool,
    pub aliases: Vec<String>,
    pub preview: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvActionResult {
    pub ok: bool,
    pub message: String,
    pub environment: Option<CodexEnvironment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvMcpSyncItem {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub status: String,
    pub server_count: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvMcpSyncResult {
    pub ok: bool,
    pub message: String,
    pub global_server_count: u32,
    pub global_server_names: Vec<String>,
    pub results: Vec<CodexEnvMcpSyncItem>,
}

/* ===== helpers ===== */

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())
}

fn expand_path(input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("配置目录不能为空".into());
    }
    let home = home_dir()?;
    if trimmed == "~" {
        return Ok(home);
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return Ok(home.join(rest));
    }
    if trimmed.starts_with('~') {
        return Err("不支持 ~user 形式的路径，请使用 ~/ 或绝对路径".into());
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(home.join(path))
    }
}

fn path_inside_home(path: &Path) -> Result<(), String> {
    let home = home_dir()?;
    let home_canon = home.canonicalize().unwrap_or_else(|_| home.clone());
    if path.exists() {
        let canon = path
            .canonicalize()
            .map_err(|e| format!("无法解析路径 {}: {}", path.display(), e))?;
        if !canon.starts_with(&home_canon) {
            return Err("配置目录必须位于用户主目录内".into());
        }
    } else {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            home.join(path)
        };
        if !abs.starts_with(&home) && !abs.starts_with(&home_canon) {
            return Err("配置目录必须位于用户主目录内".into());
        }
        if abs
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("配置目录路径包含非法的 '..'".into());
        }
        if abs == home || abs == home_canon {
            return Err("配置目录不能是用户主目录本身".into());
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("显示名称不能为空".into());
    }
    if name.chars().count() > 40 {
        return Err("显示名称不能超过 40 个字符".into());
    }
    Ok(name.to_string())
}

fn validate_slug(slug: &str) -> Result<String, String> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty() {
        return Err("slug 不能为空".into());
    }
    if slug.len() > 32 {
        return Err("slug 不能超过 32 个字符".into());
    }
    let re_ok = slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !re_ok || slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return Err("slug 仅允许小写字母、数字与单个连字符，且不能首尾为连字符".into());
    }
    if slug == "default" {
        return Err("slug「default」已保留给默认环境".into());
    }
    Ok(slug)
}

fn validate_alias(alias: &str, allow_codex: bool) -> Result<String, String> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Err("别名不能为空".into());
    }
    if alias.len() > 40 {
        return Err("别名不能超过 40 个字符".into());
    }
    let mut chars = alias.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err("别名必须以字母开头".into());
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("别名仅允许字母、数字、下划线与连字符".into());
    }
    if !allow_codex && alias == "codex" {
        return Err("非默认环境不能使用别名「codex」，以免覆盖原生命令".into());
    }
    Ok(alias.to_string())
}

fn is_dir_empty(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Err(format!("目标路径已存在且不是目录: {}", path.display()));
    }
    let mut entries = fs::read_dir(path)
        .map_err(|e| format!("无法读取目录 {}: {}", path.display(), e))?;
    Ok(entries.next().is_none())
}

fn probe_dir(path: &Path) -> (bool, bool, bool, bool) {
    let exists = path.is_dir();
    if !exists {
        return (false, false, false, false);
    }
    let has_config = path.join("config.toml").is_file();
    let has_skills = path.join("skills").is_dir();
    let has_auth = path.join("auth.json").is_file();
    (true, has_config, has_skills, has_auth)
}

fn default_codex_home() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".codex"))
}

fn shared_config_path() -> Result<PathBuf, String> {
    Ok(default_codex_home()?.join("config.toml"))
}

fn env_config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("config.toml")
}

fn env_auth_path(config_dir: &Path) -> PathBuf {
    config_dir.join("auth.json")
}

/* ===== auth.json (OPENAI_API_KEY) ===== */

/// Read `OPENAI_API_KEY` from `$CODEX_HOME/auth.json`. Empty if missing/unreadable.
fn read_auth_openai_api_key(config_dir: &Path) -> String {
    let path = env_auth_path(config_dir);
    if !path.is_file() {
        return String::new();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return String::new();
    };
    let Ok(val) = serde_json::from_str::<JsonValue>(&raw) else {
        return String::new();
    };
    val.get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn write_auth_json_secret(path: &Path, value: &JsonValue) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| format!("序列化 auth.json 失败: {}", e))?;
    let text = format!("{}\n", text);
    atomic_write(path, &text)?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    Ok(())
}

/// Apply Token edits to `$CODEX_HOME/auth.json`.
///
/// - `None` = leave unchanged
/// - `Some("")` = remove `OPENAI_API_KEY` (delete file if object becomes empty)
/// - `Some(v)` = set/merge `OPENAI_API_KEY` (preserve other keys)
///
/// Returns `true` if the file changed.
fn apply_auth_json_edit(config_dir: &Path, api_key: Option<&str>) -> Result<bool, String> {
    let Some(key) = api_key.map(|s| s.trim().to_string()) else {
        return Ok(false);
    };

    fs::create_dir_all(config_dir)
        .map_err(|e| format!("创建目录 {} 失败: {}", config_dir.display(), e))?;

    let path = env_auth_path(config_dir);

    if key.is_empty() {
        if !path.is_file() {
            return Ok(false);
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("读取 auth.json 失败: {}", e))?;
        let mut val: JsonValue = if raw.trim().is_empty() {
            JsonValue::Object(Map::new())
        } else {
            serde_json::from_str(&raw).map_err(|e| format!("解析 auth.json 失败: {}", e))?
        };
        let obj = match val.as_object_mut() {
            Some(o) => o,
            None => {
                // 非对象：清空整个文件视为清除 token
                fs::remove_file(&path)
                    .map_err(|e| format!("删除 auth.json 失败: {}", e))?;
                return Ok(true);
            }
        };
        if obj.remove("OPENAI_API_KEY").is_none() {
            return Ok(false);
        }
        if obj.is_empty() {
            fs::remove_file(&path)
                .map_err(|e| format!("删除 auth.json 失败: {}", e))?;
        } else {
            write_auth_json_secret(&path, &val)?;
        }
        return Ok(true);
    }

    let prev = read_auth_openai_api_key(config_dir);
    if prev == key {
        return Ok(false);
    }

    let mut val: JsonValue = if path.is_file() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("读取 auth.json 失败: {}", e))?;
        if raw.trim().is_empty() {
            JsonValue::Object(Map::new())
        } else {
            match serde_json::from_str::<JsonValue>(&raw) {
                Ok(v) if v.is_object() => v,
                Ok(_) => JsonValue::Object(Map::new()),
                Err(e) => return Err(format!("解析 auth.json 失败: {}", e)),
            }
        }
    } else {
        JsonValue::Object(Map::new())
    };

    let obj = val
        .as_object_mut()
        .ok_or_else(|| "auth.json 根节点必须是对象".to_string())?;
    obj.insert("OPENAI_API_KEY".into(), JsonValue::String(key));
    write_auth_json_secret(&path, &val)?;
    Ok(true)
}

/* ===== config.toml managed fields ===== */

struct ManagedConfig {
    model: String,
    model_provider: String,
    base_url: String,
    /// Prefer auth.json → OPENAI_API_KEY; fall back to custom provider
    /// experimental_bearer_token for legacy display only.
    bearer_token: String,
    has_auth_file: bool,
}

fn toml_str(item: Option<&toml_edit::Item>) -> String {
    item.and_then(|i| i.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn read_managed_config(config_dir: &Path) -> ManagedConfig {
    let has_auth_file = env_auth_path(config_dir).is_file();
    // Token 优先读 auth.json → OPENAI_API_KEY；兼容旧版 config.toml experimental_bearer_token。
    let auth_key = read_auth_openai_api_key(config_dir);
    let path = env_config_path(config_dir);
    if !path.is_file() {
        return ManagedConfig {
            model: String::new(),
            model_provider: String::new(),
            base_url: String::new(),
            bearer_token: auth_key,
            has_auth_file,
        };
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return ManagedConfig {
            model: String::new(),
            model_provider: String::new(),
            base_url: String::new(),
            bearer_token: auth_key,
            has_auth_file,
        };
    };
    let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() else {
        return ManagedConfig {
            model: String::new(),
            model_provider: String::new(),
            base_url: String::new(),
            bearer_token: auth_key,
            has_auth_file,
        };
    };

    let model = toml_str(doc.get("model"));
    let model_provider = toml_str(doc.get("model_provider"));
    let openai_base = toml_str(doc.get("openai_base_url"));

    let mut base_url = String::new();
    let mut legacy_bearer = String::new();
    if !model_provider.is_empty() {
        if let Some(providers) = doc.get("model_providers").and_then(|i| i.as_table()) {
            if let Some(prov) = providers.get(model_provider.as_str()).and_then(|i| i.as_table()) {
                base_url = toml_str(prov.get("base_url"));
                legacy_bearer = toml_str(prov.get("experimental_bearer_token"));
            }
        }
    }
    if base_url.is_empty() {
        base_url = openai_base;
    }

    let bearer_token = if !auth_key.is_empty() {
        auth_key
    } else {
        legacy_bearer
    };

    ManagedConfig {
        model,
        model_provider,
        base_url,
        bearer_token,
        has_auth_file,
    }
}

/// Apply managed edits to config.toml and/or auth.json.
///
/// Per-field: None = leave; Some("") = clear; Some(v) = set.
/// base_url targets custom provider table when model_provider is a custom id with a table,
/// otherwise writes top-level openai_base_url.
/// api_key writes `$CODEX_HOME/auth.json` → `OPENAI_API_KEY` (not config.toml).
fn apply_managed_config_edit(
    config_dir: &Path,
    model: Option<&str>,
    model_provider: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<Vec<String>, String> {
    let model = model.map(|s| s.trim().to_string());
    let provider = model_provider.map(|s| s.trim().to_string());
    let base = base_url.map(|s| s.trim().to_string());
    let key = api_key.map(|s| s.trim().to_string());
    if model.is_none() && provider.is_none() && base.is_none() && key.is_none() {
        return Ok(Vec::new());
    }

    fs::create_dir_all(config_dir)
        .map_err(|e| format!("创建目录 {} 失败: {}", config_dir.display(), e))?;

    let mut changed = Vec::new();

    // Token → auth.json（与 config.toml 解耦，任意 provider 均可写入）
    if apply_auth_json_edit(config_dir, key.as_deref())? {
        changed.push("Token".into());
    }

    // 仅 model / provider / base_url 需要改 config.toml
    if model.is_none() && provider.is_none() && base.is_none() {
        return Ok(changed);
    }

    let path = env_config_path(config_dir);
    let mut doc = if path.is_file() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("读取 config.toml 失败: {}", e))?;
        if raw.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            raw.parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析 config.toml 失败: {}", e))?
        }
    } else {
        toml_edit::DocumentMut::new()
    };

    let mut toml_changed = false;

    if let Some(val) = model {
        let prev = toml_str(doc.get("model"));
        if val.is_empty() {
            if doc.remove("model").is_some() {
                changed.push("模型".into());
                toml_changed = true;
            }
        } else if prev != val {
            doc["model"] = toml_edit::value(val);
            changed.push("模型".into());
            toml_changed = true;
        }
    }

    if let Some(val) = provider {
        let prev = toml_str(doc.get("model_provider"));
        if val.is_empty() {
            if doc.remove("model_provider").is_some() {
                changed.push("Provider".into());
                toml_changed = true;
            }
        } else if prev != val {
            doc["model_provider"] = toml_edit::value(val);
            changed.push("Provider".into());
            toml_changed = true;
        }
    }

    // Resolve active provider after potential provider edit.
    let active_provider = toml_str(doc.get("model_provider"));

    if let Some(val) = base {
        let use_custom = !active_provider.is_empty()
            && active_provider != "openai"
            && active_provider != "ollama"
            && active_provider != "lmstudio"
            && active_provider != "amazon-bedrock";

        if use_custom {
            // Ensure model_providers.<id> table exists
            if doc.get("model_providers").is_none() {
                doc["model_providers"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let providers = doc["model_providers"]
                .as_table_mut()
                .ok_or_else(|| "config.toml 的 model_providers 必须是表".to_string())?;
            if providers.get(active_provider.as_str()).is_none() {
                let mut t = toml_edit::Table::new();
                t["name"] = toml_edit::value(active_provider.as_str());
                providers.insert(active_provider.as_str(), toml_edit::Item::Table(t));
            }
            let prov = providers
                .get_mut(active_provider.as_str())
                .and_then(|i| i.as_table_mut())
                .ok_or_else(|| format!("无法写入 model_providers.{}", active_provider))?;
            let prev = toml_str(prov.get("base_url"));
            if val.is_empty() {
                if prov.remove("base_url").is_some() {
                    changed.push("Base URL".into());
                    toml_changed = true;
                }
            } else if prev != val {
                prov["base_url"] = toml_edit::value(val);
                changed.push("Base URL".into());
                toml_changed = true;
            }
        } else {
            let prev = toml_str(doc.get("openai_base_url"));
            if val.is_empty() {
                if doc.remove("openai_base_url").is_some() {
                    changed.push("Base URL".into());
                    toml_changed = true;
                }
            } else if prev != val {
                doc["openai_base_url"] = toml_edit::value(val);
                changed.push("Base URL".into());
                toml_changed = true;
            }
        }
    }

    if !toml_changed {
        return Ok(changed);
    }

    let content = doc.to_string();
    let create_mode = fs::metadata(&path).ok().map(|m| m.permissions().mode());
    atomic_write(&path, &content)?;
    if let Some(mode) = create_mode {
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode));
    } else {
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(changed)
}

fn apply_config_overrides(
    config_dir: &Path,
    model: Option<&str>,
    model_provider: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<Vec<String>, String> {
    fn non_empty(o: Option<&str>) -> Option<&str> {
        o.map(str::trim).filter(|s| !s.is_empty())
    }
    apply_managed_config_edit(
        config_dir,
        non_empty(model),
        non_empty(model_provider),
        non_empty(base_url),
        non_empty(api_key),
    )
}

/* ===== MCP (toml [mcp_servers]) ===== */

fn read_mcp_server_names(path: &Path) -> (BTreeSet<String>, bool) {
    if !path.is_file() {
        return (BTreeSet::new(), false);
    }
    let Ok(raw) = fs::read_to_string(path) else {
        return (BTreeSet::new(), true);
    };
    if raw.trim().is_empty() {
        return (BTreeSet::new(), true);
    }
    let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() else {
        return (BTreeSet::new(), true);
    };
    let mut names = BTreeSet::new();
    if let Some(table) = doc.get("mcp_servers").and_then(|i| i.as_table()) {
        for (k, _) in table.iter() {
            names.insert(k.to_string());
        }
    }
    (names, true)
}

fn read_mcp_servers_item(path: &Path) -> Result<Option<toml_edit::Item>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let doc = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))?;
    Ok(doc.get("mcp_servers").cloned())
}

/// Replace entire [mcp_servers] table in target config.toml, preserving other keys.
fn write_mcp_servers_item(path: &Path, servers: Option<&toml_edit::Item>) -> Result<(), String> {
    let mut doc = if path.is_file() {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
        if raw.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            raw.parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))?
        }
    } else {
        toml_edit::DocumentMut::new()
    };

    match servers {
        Some(item) => {
            doc["mcp_servers"] = item.clone();
        }
        None => {
            let _ = doc.remove("mcp_servers");
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
    }
    atomic_write(path, &doc.to_string())?;
    Ok(())
}

fn sync_mcp_servers_to_dir(config_dir: &Path) -> Result<(u32, Vec<String>), String> {
    if !config_dir.is_dir() {
        return Err(format!("环境目录不存在: {}", config_dir.display()));
    }
    let src = shared_config_path()?;
    let (names_set, _) = read_mcp_server_names(&src);
    let mut names: Vec<String> = names_set.iter().cloned().collect();
    names.sort();
    let count = names.len() as u32;
    let servers = read_mcp_servers_item(&src)?;
    let dst = env_config_path(config_dir);

    if let (Ok(a), Ok(b)) = (src.canonicalize(), dst.canonicalize()) {
        if a == b {
            return Ok((count, names));
        }
    }

    write_mcp_servers_item(&dst, servers.as_ref())?;
    Ok((count, names))
}

fn mcp_status_for_row(
    row: &CodexEnvironmentRow,
    dir_exists: bool,
    global_names: &BTreeSet<String>,
    global_file_exists: bool,
) -> (String, u32) {
    if row.is_default {
        return ("default".into(), global_names.len() as u32);
    }
    if !dir_exists {
        return ("missing".into(), 0);
    }
    let local_path = env_config_path(Path::new(&row.config_dir));
    if !global_file_exists && global_names.is_empty() {
        let (local_names, _) = read_mcp_server_names(&local_path);
        return ("no_global".into(), local_names.len() as u32);
    }
    if !local_path.is_file() {
        return if global_names.is_empty() {
            ("in_sync".into(), 0)
        } else {
            ("missing".into(), 0)
        };
    }
    let (local_names, _) = read_mcp_server_names(&local_path);
    let count = local_names.len() as u32;
    if &local_names == global_names {
        ("in_sync".into(), count)
    } else {
        ("out_of_sync".into(), count)
    }
}

fn row_to_public(row: CodexEnvironmentRow) -> CodexEnvironment {
    let path = PathBuf::from(&row.config_dir);
    let (dir_exists, has_config, has_skills, has_auth_file) = probe_dir(&path);
    let global_path = shared_config_path().unwrap_or_else(|_| PathBuf::from("/dev/null"));
    let (global_names, global_exists) = read_mcp_server_names(&global_path);
    let global_count = global_names.len() as u32;
    let (status, local_count) =
        mcp_status_for_row(&row, dir_exists, &global_names, global_exists);

    let managed = if dir_exists {
        read_managed_config(&path)
    } else {
        ManagedConfig {
            model: String::new(),
            model_provider: String::new(),
            base_url: String::new(),
            bearer_token: String::new(),
            has_auth_file: false,
        }
    };
    let has_auth = has_auth_file || managed.has_auth_file || !managed.bearer_token.is_empty();

    CodexEnvironment {
        id: row.id,
        name: row.name,
        slug: row.slug,
        config_dir: row.config_dir,
        alias_name: row.alias_name,
        is_default: row.is_default,
        source: row.source,
        notes: row.notes,
        alias_installed: row.alias_installed,
        dir_exists,
        has_config,
        has_skills,
        has_auth,
        mcp_sync_status: status,
        mcp_server_count: local_count,
        global_mcp_server_count: global_count,
        model: managed.model,
        model_provider: managed.model_provider,
        base_url: managed.base_url,
        api_key: String::new(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/* ===== copy / move ===== */

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目录 {} 失败: {}", dst.display(), e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("读取目录 {} 失败: {}", src.display(), e))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
        let ty = entry
            .file_type()
            .map_err(|e| format!("读取文件类型失败: {}", e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
            }
            fs::copy(&from, &to).map_err(|e| {
                format!(
                    "复制文件 {} → {} 失败: {}",
                    from.display(),
                    to.display(),
                    e
                )
            })?;
        }
    }
    Ok(())
}

fn copy_core(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源环境目录不存在: {}", src.display()));
    }
    fs::create_dir_all(dst).map_err(|e| format!("创建目标目录 {} 失败: {}", dst.display(), e))?;

    for name in CORE_FILES {
        let from = src.join(name);
        if from.is_file() {
            let to = dst.join(name);
            fs::copy(&from, &to)
                .map_err(|e| format!("复制 {} 失败: {}", name, e))?;
        }
    }
    for name in CORE_DIRS {
        let from = src.join(name);
        if from.is_dir() {
            let to = dst.join(name);
            copy_dir_recursive(&from, &to)?;
        }
    }
    // Official requirement: CODEX_HOME must exist before use.
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvMoveOutcome {
    NoMove,
    Renamed,
    Copied,
}

fn copy_dir_strict(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目录 {} 失败: {}", dst.display(), e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("读取目录 {} 失败: {}", src.display(), e))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
        let ty = entry
            .file_type()
            .map_err(|e| format!("读取文件类型失败: {}", e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_symlink() {
            return Err(format!(
                "目录含符号链接，无法安全迁移：{}。请手工处理后重试。",
                from.display()
            ));
        } else if ty.is_dir() {
            copy_dir_strict(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to).map_err(|e| {
                format!("复制文件 {} → {} 失败: {}", from.display(), to.display(), e)
            })?;
        } else {
            return Err(format!("目录含特殊文件，无法安全迁移：{}", from.display()));
        }
    }
    Ok(())
}

fn move_environment_dir(src: &Path, dst: &Path) -> Result<EnvMoveOutcome, String> {
    if src.exists() && !src.is_dir() {
        return Err(format!("原路径存在但不是目录：{}", src.display()));
    }
    if !src.is_dir() || is_dir_empty(src)? {
        return Ok(EnvMoveOutcome::NoMove);
    }
    if !is_dir_empty(dst)? {
        return Err(format!("新路径已存在且非空：{}", dst.display()));
    }
    if dst.exists() {
        fs::remove_dir(dst)
            .map_err(|e| format!("清理空目标目录 {} 失败: {}", dst.display(), e))?;
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标父目录 {} 失败: {}", parent.display(), e))?;
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(EnvMoveOutcome::Renamed),
        Err(rename_err) => {
            if let Err(copy_err) = copy_dir_strict(src, dst) {
                let _ = fs::remove_dir_all(dst);
                return Err(format!(
                    "迁移目录失败（重命名：{}；复制回退：{}）",
                    rename_err, copy_err
                ));
            }
            if let Err(rm_err) = fs::remove_dir_all(src) {
                let _ = fs::remove_dir_all(dst);
                return Err(format!(
                    "已复制到新路径但删除旧目录 {} 失败: {}；已回滚新副本，请检查旧目录后重试",
                    src.display(),
                    rm_err
                ));
            }
            Ok(EnvMoveOutcome::Copied)
        }
    }
}

fn ensure_unique_fields(
    id: &str,
    slug: &str,
    config_dir: &str,
    alias_name: &str,
) -> Result<(), String> {
    let rows = db::load_codex_environment_rows()?;
    for r in rows {
        if r.id == id {
            continue;
        }
        if r.slug == slug {
            return Err(format!("slug「{}」已被环境「{}」占用", slug, r.name));
        }
        if r.config_dir == config_dir {
            return Err(format!(
                "配置目录「{}」已被环境「{}」占用",
                config_dir, r.name
            ));
        }
        if r.alias_name == alias_name {
            return Err(format!(
                "别名「{}」已被环境「{}」占用",
                alias_name, r.name
            ));
        }
    }
    Ok(())
}

fn ensure_default_environment() -> Result<(), String> {
    let rows = db::load_codex_environment_rows()?;
    if rows.iter().any(|r| r.is_default) {
        return Ok(());
    }
    let config_dir = default_codex_home()?;
    let now = now_secs();
    let row = CodexEnvironmentRow {
        id: DEFAULT_ENV_ID.into(),
        name: "默认环境".into(),
        slug: "default".into(),
        config_dir: config_dir.to_string_lossy().to_string(),
        alias_name: "codex".into(),
        is_default: true,
        source: "default".into(),
        notes: "直接运行 codex 使用此环境（不写入 shell 别名块）".into(),
        alias_installed: false,
        created_at: now,
        updated_at: now,
    };
    db::upsert_codex_environment_row(&row)
}

fn slug_from_dirname(dirname: &str) -> String {
    if dirname == ".codex" {
        return "default".into();
    }
    let rest = dirname.strip_prefix(".codex-").unwrap_or(dirname);
    let cleaned: String = rest
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let collapsed = cleaned
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "env".into()
    } else {
        collapsed.chars().take(32).collect()
    }
}

fn config_dir_for_shell(config_dir: &str) -> Result<String, String> {
    let home = home_dir()?;
    let path = PathBuf::from(config_dir);
    if let Ok(rel) = path.strip_prefix(&home) {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        return Ok(format!("$HOME/{}", rel_str));
    }
    Ok(config_dir.to_string())
}

/* ===== Shell marker block ===== */

#[derive(Clone, Copy, PartialEq)]
enum ShellKind {
    Zsh,
    Bash,
    Fish,
}

fn detect_shell_kind() -> ShellKind {
    let sh = std::env::var("SHELL").unwrap_or_default();
    if sh.ends_with("/bash") || sh == "bash" {
        ShellKind::Bash
    } else if sh.ends_with("/fish") || sh == "fish" {
        ShellKind::Fish
    } else {
        ShellKind::Zsh
    }
}

fn shell_rc() -> Result<(PathBuf, ShellKind), String> {
    let home = home_dir()?;
    let kind = detect_shell_kind();
    let path = match kind {
        ShellKind::Zsh => home.join(".zshrc"),
        ShellKind::Bash => {
            let bp = home.join(".bash_profile");
            let br = home.join(".bashrc");
            if bp.is_file() {
                bp
            } else if br.is_file() {
                br
            } else {
                bp
            }
        }
        ShellKind::Fish => home.join(".config/fish/config.fish"),
    };
    Ok((path, kind))
}

fn rc_hint() -> String {
    match shell_rc() {
        Ok((path, _)) => display_path_for_msg(&path.to_string_lossy()),
        Err(_) => "~/.zshrc".to_string(),
    }
}

fn build_alias_lines(
    rows: &[CodexEnvironmentRow],
    shell: ShellKind,
) -> Result<Vec<(String, String)>, String> {
    let mut items: Vec<(String, String)> = Vec::new();
    for r in rows {
        if r.is_default || !r.alias_installed {
            continue;
        }
        let path = PathBuf::from(&r.config_dir);
        if !path.is_dir() {
            continue;
        }
        let shell_path = config_dir_for_shell(&r.config_dir)?;
        let line = if shell == ShellKind::Fish {
            format!(
                "alias {}=\"env CODEX_HOME={} codex\"",
                r.alias_name, shell_path
            )
        } else {
            format!(
                "alias {}=\"CODEX_HOME={} codex\"",
                r.alias_name, shell_path
            )
        };
        items.push((r.alias_name.clone(), line));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(items)
}

fn render_marker_block(lines: &[String]) -> String {
    let mut out = String::new();
    out.push_str(MARKER_BEGIN);
    out.push('\n');
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(MARKER_END);
    out.push('\n');
    out
}

pub fn apply_marker_block(content: &str, block: &str) -> String {
    if let Some(start) = content.find(MARKER_BEGIN) {
        if let Some(end_rel) = content[start..].find(MARKER_END) {
            let end = start + end_rel + MARKER_END.len();
            let mut end_adj = end;
            if content[end_adj..].starts_with('\n') {
                end_adj += 1;
            }
            let mut new_content = String::with_capacity(content.len() + block.len());
            new_content.push_str(&content[..start]);
            while new_content.ends_with("\n\n\n") {
                new_content.pop();
            }
            new_content.push_str(block);
            new_content.push_str(&content[end_adj..]);
            return new_content;
        }
    }
    let mut new_content = content.to_string();
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    if !new_content.is_empty() && !new_content.ends_with("\n\n") {
        new_content.push('\n');
    }
    new_content.push_str(block);
    new_content
}

pub fn remove_marker_block(content: &str) -> (String, bool) {
    if let Some(start) = content.find(MARKER_BEGIN) {
        if let Some(end_rel) = content[start..].find(MARKER_END) {
            let end = start + end_rel + MARKER_END.len();
            let mut end_adj = end;
            if content[end_adj..].starts_with('\n') {
                end_adj += 1;
            }
            let mut new_content = String::new();
            new_content.push_str(&content[..start]);
            new_content.push_str(&content[end_adj..]);
            while new_content.contains("\n\n\n") {
                new_content = new_content.replace("\n\n\n", "\n\n");
            }
            return (new_content, true);
        }
    }
    (content.to_string(), false)
}

fn parse_aliases_from_block(content: &str) -> Vec<String> {
    let Some(start) = content.find(MARKER_BEGIN) else {
        return Vec::new();
    };
    let Some(end_rel) = content[start..].find(MARKER_END) else {
        return Vec::new();
    };
    let block = &content[start..start + end_rel];
    let mut aliases = Vec::new();
    for line in block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("alias ") {
            if let Some(eq) = rest.find('=') {
                let name = rest[..eq].trim();
                if !name.is_empty() {
                    aliases.push(name.to_string());
                }
            }
        }
    }
    aliases
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无效路径: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("创建目录 {} 失败: {}", parent.display(), e))?;
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = parent.join(format!(
        ".{}.agentbuddy-{}-{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("tmp"),
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut f =
            fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {}", e))?;
        f.write_all(content.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("写入 {} 失败: {}", path.display(), e)
    })?;
    Ok(())
}

fn display_path_for_msg(abs: &str) -> String {
    if let Ok(home) = home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = abs.strip_prefix(home_str.as_ref()) {
            if rest.starts_with('/') {
                return format!("~{}", rest);
            }
        }
    }
    abs.to_string()
}

/* ===== Public API ===== */

pub fn list_environments() -> Result<Vec<CodexEnvironment>, String> {
    // Highest priority: Codex (CLI and/or ChatGPT App) must be installed.
    // Config dir alone (e.g. leftover ~/.codex) is not enough — same rule as agent sniff.
    // When missing, return empty so the UI shows the not-installed empty state
    // instead of a synthetic "default" card.
    if !crate::sniff::is_agent_installed("codex") {
        return Ok(Vec::new());
    }
    ensure_default_environment()?;
    let rows = db::load_codex_environment_rows()?;
    Ok(rows.into_iter().map(row_to_public).collect())
}

pub fn sniff_environments() -> Result<CodexEnvSniffResult, String> {
    if !crate::sniff::is_agent_installed("codex") {
        return Ok(CodexEnvSniffResult {
            candidates: Vec::new(),
            message: "未检测到 Codex CLI，请先安装后再扫描环境".into(),
        });
    }
    ensure_default_environment()?;
    let home = home_dir()?;
    let registered: std::collections::HashSet<String> = db::load_codex_environment_rows()?
        .into_iter()
        .map(|r| r.config_dir)
        .collect();

    let mut candidates = Vec::new();
    let entries = fs::read_dir(&home).map_err(|e| format!("无法读取主目录: {}", e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".codex" {
            continue;
        }
        if !name.starts_with(".codex-") {
            continue;
        }
        let suffix = &name[".codex-".len()..];
        if suffix.is_empty() {
            continue;
        }
        if !suffix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        if registered.contains(&path_str) {
            continue;
        }
        let slug = slug_from_dirname(&name);
        let (dir_exists, has_config, has_skills, has_auth) = probe_dir(&path);
        if !dir_exists {
            continue;
        }
        // Require at least one Codex signal
        if !has_config && !has_skills && !has_auth {
            continue;
        }
        candidates.push(CodexEnvCandidate {
            path: path_str,
            suggested_name: format!("Codex · {}", slug),
            suggested_slug: slug.clone(),
            suggested_alias: format!("codex-{}", slug),
            has_config,
            has_skills,
            has_auth,
        });
    }
    candidates.sort_by(|a, b| a.suggested_slug.cmp(&b.suggested_slug));
    let count = candidates.len();
    Ok(CodexEnvSniffResult {
        candidates,
        message: if count == 0 {
            "未发现未登记的 .codex-* 目录".into()
        } else {
            format!("发现 {} 个可导入目录", count)
        },
    })
}

pub fn import_environment(payload: CodexEnvImportPayload) -> Result<CodexEnvActionResult, String> {
    ensure_default_environment()?;
    let config_dir = expand_path(&payload.config_dir)?;
    path_inside_home(&config_dir)?;
    if !config_dir.is_dir() {
        return Err(format!("目录不存在: {}", config_dir.display()));
    }
    let config_dir_str = config_dir.to_string_lossy().to_string();

    let dirname = config_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("env");
    let slug = if let Some(s) = payload.slug.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        validate_slug(s)?
    } else {
        validate_slug(&slug_from_dirname(dirname))?
    };
    let alias = if let Some(a) = payload
        .alias_name
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        validate_alias(a, false)?
    } else {
        validate_alias(&format!("codex-{}", slug), false)?
    };
    let name = if let Some(n) = payload.name.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        validate_name(n)?
    } else {
        validate_name(&format!("Codex · {}", slug))?
    };
    let notes = payload.notes.unwrap_or_default().trim().to_string();
    let id = format!("env-{}-{}", slug, now_secs());

    ensure_unique_fields(&id, &slug, &config_dir_str, &alias)?;

    let now = now_secs();
    let mut row = CodexEnvironmentRow {
        id: id.clone(),
        name: name.clone(),
        slug,
        config_dir: config_dir_str,
        alias_name: alias,
        is_default: false,
        source: "imported".into(),
        notes,
        alias_installed: false,
        created_at: now,
        updated_at: now,
    };
    db::upsert_codex_environment_row(&row)?;

    let mut message = format!("已导入环境「{}」", name);
    let (alias_installed, alias_msg) =
        install_alias_after_create(&row, payload.install_alias.unwrap_or(false));
    row.alias_installed = alias_installed;
    message.push_str(&alias_msg);

    Ok(CodexEnvActionResult {
        ok: true,
        message,
        environment: Some(row_to_public(row)),
    })
}

pub fn clone_environment(payload: CodexEnvClonePayload) -> Result<CodexEnvActionResult, String> {
    ensure_default_environment()?;
    let source = db::get_codex_environment_row(&payload.source_id)?
        .ok_or_else(|| format!("源环境不存在: {}", payload.source_id))?;
    let src_path = PathBuf::from(&source.config_dir);
    if !src_path.is_dir() {
        return Err(format!(
            "源环境目录不存在: {}。请确认 Codex 已初始化该环境。",
            source.config_dir
        ));
    }

    let name = validate_name(&payload.name)?;
    let slug = validate_slug(&payload.slug)?;
    let alias = validate_alias(&payload.alias_name, false)?;
    let notes = payload.notes.unwrap_or_default().trim().to_string();
    let dst = expand_path(&payload.config_dir)?;
    path_inside_home(&dst)?;
    if dst == src_path {
        return Err("目标目录不能与源目录相同".into());
    }
    if !is_dir_empty(&dst)? {
        return Err(format!(
            "目标目录已存在且非空: {}。请选择空目录或其它路径。",
            dst.display()
        ));
    }
    let dst_str = dst.to_string_lossy().to_string();
    let id = format!("env-{}-{}", slug, now_secs());
    ensure_unique_fields(&id, &slug, &dst_str, &alias)?;

    if let Err(e) = copy_core(&src_path, &dst) {
        let _ = fs::remove_dir_all(&dst);
        return Err(e);
    }

    let override_fields = match apply_config_overrides(
        &dst,
        payload.model.as_deref(),
        payload.model_provider.as_deref(),
        payload.base_url.as_deref(),
        payload.api_key.as_deref(),
    ) {
        Ok(fields) => fields,
        Err(e) => {
            let _ = fs::remove_dir_all(&dst);
            return Err(e);
        }
    };

    let now = now_secs();
    let mut row = CodexEnvironmentRow {
        id: id.clone(),
        name: name.clone(),
        slug,
        config_dir: dst_str,
        alias_name: alias,
        is_default: false,
        source: "managed".into(),
        notes,
        alias_installed: false,
        created_at: now,
        updated_at: now,
    };
    db::upsert_codex_environment_row(&row)?;

    let mut message = format!("已从「{}」复制核心配置到「{}」。", source.name, name);
    if override_fields.is_empty() {
        message.push_str("model / provider / base_url / Token 沿用源环境（Token 不会从源环境复制）。");
    } else {
        message.push_str(&format!(
            "已覆盖 {}。",
            override_fields.join("、")
        ));
    }

    let sync_mcp = payload.sync_mcp.unwrap_or(false);
    if sync_mcp {
        match sync_mcp_servers_to_dir(&dst) {
            Ok((count, _)) => {
                if count == 0 {
                    message.push_str(" 已尝试同步默认环境 MCP（当前无 mcp_servers）。");
                } else {
                    message.push_str(&format!(" 已同步默认环境 MCP（{} 个 server）。", count));
                }
            }
            Err(e) => {
                message.push_str(&format!(" MCP 同步失败：{}。", e));
            }
        }
    }

    let (alias_installed, alias_msg) =
        install_alias_after_create(&row, payload.install_alias.unwrap_or(false));
    row.alias_installed = alias_installed;
    message.push_str(&alias_msg);
    message.push_str("新环境不含 auth / 会话历史，首次启动可能需要重新登录。");

    Ok(CodexEnvActionResult {
        ok: true,
        message,
        environment: Some(row_to_public(row)),
    })
}

pub fn upsert_environment(payload: CodexEnvUpsertPayload) -> Result<CodexEnvActionResult, String> {
    ensure_default_environment()?;
    let name = validate_name(&payload.name)?;
    let notes = payload.notes.unwrap_or_default().trim().to_string();
    let id = payload
        .id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "更新环境需要提供 id；新建请使用「从现有环境复制」".to_string())?;

    let existing = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let previous_config_dir = existing.config_dir.clone();

    let (slug, config_dir, alias_name, is_default, source) = if existing.is_default {
        let _ = validate_alias("codex", true)?;
        (
            existing.slug,
            existing.config_dir,
            existing.alias_name,
            true,
            existing.source,
        )
    } else {
        let slug = validate_slug(&payload.slug)?;
        let alias = validate_alias(&payload.alias_name, false)?;
        let requested = expand_path(&payload.config_dir)?;
        path_inside_home(&requested)?;
        let requested_str = requested.to_string_lossy().to_string();
        if requested_str != previous_config_dir && !is_dir_empty(&requested)? {
            return Err(format!("新路径已存在且非空: {}", requested.display()));
        }
        (slug, requested_str, alias, false, existing.source)
    };

    ensure_unique_fields(&id, &slug, &config_dir, &alias_name)?;

    let mut move_outcome = EnvMoveOutcome::NoMove;
    if !is_default && config_dir != previous_config_dir {
        let src = PathBuf::from(&previous_config_dir);
        let dst = PathBuf::from(&config_dir);
        move_outcome = move_environment_dir(&src, &dst)?;
    }

    let now = now_secs();
    let row = CodexEnvironmentRow {
        id: id.clone(),
        name: name.clone(),
        slug,
        config_dir: config_dir.clone(),
        alias_name,
        is_default,
        source,
        notes,
        alias_installed: existing.alias_installed,
        created_at: existing.created_at,
        updated_at: now,
    };
    db::upsert_codex_environment_row(&row).map_err(|e| {
        if move_outcome != EnvMoveOutcome::NoMove {
            let src = PathBuf::from(&previous_config_dir);
            let dst = PathBuf::from(&config_dir);
            if fs::rename(&dst, &src).is_err() {
                return format!(
                    "保存失败: {}。注意：目录可能已迁移到 {}，但数据库未更新，请手工核对 {} 与 {}",
                    e,
                    dst.display(),
                    src.display(),
                    dst.display()
                );
            }
        }
        format!("保存失败: {}", e)
    })?;

    if !is_default && existing.alias_installed {
        let _ = rewrite_shell_block_from_db();
    }

    let mut message = format!("已更新环境「{}」", name);
    match move_outcome {
        EnvMoveOutcome::Renamed => {
            message.push_str(&format!("；已迁移目录到 {}", config_dir));
        }
        EnvMoveOutcome::Copied => {
            message.push_str(&format!("；已复制迁移目录到 {} 并移除旧目录", config_dir));
        }
        EnvMoveOutcome::NoMove => {
            if !is_default && config_dir != previous_config_dir {
                message.push_str("；原目录为空或不存在，仅更新配置路径");
            }
        }
    }

    if !is_default {
        let dir = PathBuf::from(&row.config_dir);
        if dir.is_dir() {
            match apply_managed_config_edit(
                &dir,
                payload.model.as_deref(),
                payload.model_provider.as_deref(),
                payload.base_url.as_deref(),
                payload.api_key.as_deref(),
            ) {
                Ok(changed) if !changed.is_empty() => {
                    // Token 写 auth.json；其余写 config.toml —— 统一列出字段名即可
                    message.push_str(&format!("；已更新 {}", changed.join("、")));
                }
                Ok(_) => {}
                Err(e) => {
                    message.push_str(&format!("；但配置更新失败：{}", e));
                }
            }
        } else if payload.model.is_some()
            || payload.model_provider.is_some()
            || payload.base_url.is_some()
            || payload.api_key.is_some()
        {
            message.push_str("；环境目录不存在，未写入 model / provider / base_url / Token");
        }
    }

    Ok(CodexEnvActionResult {
        ok: true,
        message,
        environment: Some(row_to_public(row)),
    })
}

pub fn delete_environment(id: String, delete_files: bool) -> Result<CodexEnvActionResult, String> {
    ensure_default_environment()?;
    let existing = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    if existing.is_default {
        return Err("不能删除默认环境".into());
    }

    if delete_files {
        let path = PathBuf::from(&existing.config_dir);
        path_inside_home(&path)?;
        if path.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("删除目录 {} 失败: {}", path.display(), e))?;
        }
    }

    db::delete_codex_environment_row(&id)?;
    let _ = rewrite_shell_block_from_db();

    Ok(CodexEnvActionResult {
        ok: true,
        message: if delete_files {
            format!("已删除环境「{}」及其配置目录", existing.name)
        } else {
            format!(
                "已从列表移除「{}」（磁盘目录保留：{}）",
                existing.name, existing.config_dir
            )
        },
        environment: None,
    })
}

fn rewrite_shell_block_from_db() -> Result<CodexEnvShellStatus, String> {
    ensure_default_environment()?;
    let rows = db::load_codex_environment_rows()?;
    let (zshrc, shell) = shell_rc()?;
    let items = build_alias_lines(&rows, shell)?;
    let lines: Vec<String> = items.iter().map(|(_, l)| l.clone()).collect();
    let aliases: Vec<String> = items.iter().map(|(a, _)| a.clone()).collect();
    let zshrc_exists = zshrc.is_file();
    let current = if zshrc_exists {
        fs::read_to_string(&zshrc).map_err(|e| format!("读取 shell 配置失败: {}", e))?
    } else {
        String::new()
    };

    if lines.is_empty() {
        if current.contains(MARKER_BEGIN) {
            let (next, _) = remove_marker_block(&current);
            atomic_write(&zshrc, &next)?;
            return Ok(CodexEnvShellStatus {
                zshrc_path: zshrc.to_string_lossy().to_string(),
                zshrc_exists: true,
                block_present: false,
                aliases: vec![],
                preview: String::new(),
                message: format!(
                    "已清除 {} 中的 AgentBuddy Codex 标记块（当前没有启用的别名）",
                    display_path_for_msg(&zshrc.to_string_lossy())
                ),
            });
        }
        return Ok(CodexEnvShellStatus {
            zshrc_path: zshrc.to_string_lossy().to_string(),
            zshrc_exists,
            block_present: false,
            aliases: vec![],
            preview: String::new(),
            message: "当前没有启用的 shell 别名".into(),
        });
    }

    let block = render_marker_block(&lines);
    let next = apply_marker_block(&current, &block);
    atomic_write(&zshrc, &next)?;

    Ok(CodexEnvShellStatus {
        zshrc_path: zshrc.to_string_lossy().to_string(),
        zshrc_exists: true,
        block_present: true,
        aliases: aliases.clone(),
        preview: block,
        message: format!(
            "已同步 {} 个 alias 到 {rc}。请执行 source {rc} 或新开终端后生效。",
            aliases.len(),
            rc = display_path_for_msg(&zshrc.to_string_lossy())
        ),
    })
}

fn install_alias_after_create(row: &CodexEnvironmentRow, want: bool) -> (bool, String) {
    if !want || row.is_default {
        return (false, String::new());
    }
    let path = PathBuf::from(&row.config_dir);
    if !path.is_dir() {
        return (false, " 未自动写入别名：环境目录不存在。".into());
    }
    if let Err(e) = db::set_codex_env_alias_installed(&row.id, true) {
        return (false, format!(" 别名写入失败：{}。", e));
    }
    match rewrite_shell_block_from_db() {
        Ok(_) => (
            true,
            format!(
                " 已写入 shell 别名 {}（执行 source {} 或新开终端后生效）。",
                row.alias_name,
                rc_hint()
            ),
        ),
        Err(e) => {
            let _ = db::set_codex_env_alias_installed(&row.id, false);
            (false, format!(" 别名写入失败：{}。", e))
        }
    }
}

pub fn install_env_alias(id: String) -> Result<CodexEnvShellStatus, String> {
    ensure_default_environment()?;
    let existing = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    if existing.is_default {
        return Err("默认环境不支持写入 shell 别名，请直接运行 codex".into());
    }
    let path = PathBuf::from(&existing.config_dir);
    if !path.is_dir() {
        return Err(format!(
            "环境目录不存在，无法写入别名: {}",
            existing.config_dir
        ));
    }
    // Ensure CODEX_HOME exists (official requirement).
    fs::create_dir_all(&path)
        .map_err(|e| format!("创建 CODEX_HOME 失败: {}", e))?;
    db::set_codex_env_alias_installed(&id, true)?;
    let mut status = rewrite_shell_block_from_db()?;
    status.message = format!(
        "已为「{}」写入别名 {}。请执行 source {} 或新开终端后生效。",
        existing.name,
        existing.alias_name,
        rc_hint()
    );
    Ok(status)
}

pub fn remove_env_alias(id: String) -> Result<CodexEnvShellStatus, String> {
    ensure_default_environment()?;
    let existing = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    if existing.is_default {
        return Err("默认环境没有 shell 别名可移除".into());
    }
    db::set_codex_env_alias_installed(&id, false)?;
    let mut status = rewrite_shell_block_from_db()?;
    status.message = format!(
        "已移除「{}」的别名 {}。请执行 source {} 或新开终端后生效。",
        existing.name,
        existing.alias_name,
        rc_hint()
    );
    Ok(status)
}

pub fn remove_all_aliases() -> Result<CodexEnvShellStatus, String> {
    let (zshrc, _) = shell_rc()?;
    let zshrc_exists = zshrc.is_file();
    if !zshrc_exists {
        db::set_codex_env_alias_installed_all(false)?;
        return Ok(CodexEnvShellStatus {
            zshrc_path: zshrc.to_string_lossy().to_string(),
            zshrc_exists: false,
            block_present: false,
            aliases: vec![],
            preview: String::new(),
            message: format!(
                "{} 不存在，无需移除",
                display_path_for_msg(&zshrc.to_string_lossy())
            ),
        });
    }
    let current =
        fs::read_to_string(&zshrc).map_err(|e| format!("读取 shell 配置失败: {}", e))?;
    let (next, removed) = remove_marker_block(&current);
    if removed {
        atomic_write(&zshrc, &next)?;
    }
    db::set_codex_env_alias_installed_all(false)?;
    Ok(CodexEnvShellStatus {
        zshrc_path: zshrc.to_string_lossy().to_string(),
        zshrc_exists: true,
        block_present: false,
        aliases: vec![],
        preview: String::new(),
        message: if removed {
            format!(
                "已从 {} 移除 AgentBuddy Codex 环境标记块",
                display_path_for_msg(&zshrc.to_string_lossy())
            )
        } else {
            format!(
                "{} 中未找到 AgentBuddy Codex 标记块",
                display_path_for_msg(&zshrc.to_string_lossy())
            )
        },
    })
}

pub fn get_shell_status() -> Result<CodexEnvShellStatus, String> {
    ensure_default_environment()?;
    let rows = db::load_codex_environment_rows()?;
    let (zshrc, shell) = shell_rc()?;
    let items = build_alias_lines(&rows, shell)?;
    let lines: Vec<String> = items.iter().map(|(_, l)| l.clone()).collect();
    let preview = if lines.is_empty() {
        String::new()
    } else {
        render_marker_block(&lines)
    };
    let zshrc_exists = zshrc.is_file();
    let (block_present, aliases) = if zshrc_exists {
        let content =
            fs::read_to_string(&zshrc).map_err(|e| format!("读取 shell 配置失败: {}", e))?;
        let present = content.contains(MARKER_BEGIN) && content.contains(MARKER_END);
        let aliases = if present {
            parse_aliases_from_block(&content)
        } else {
            Vec::new()
        };
        (present, aliases)
    } else {
        (false, Vec::new())
    };

    Ok(CodexEnvShellStatus {
        zshrc_path: zshrc.to_string_lossy().to_string(),
        zshrc_exists,
        block_present,
        aliases,
        preview,
        message: if block_present {
            format!(
                "已在 {} 中检测到 AgentBuddy Codex 标记块",
                display_path_for_msg(&zshrc.to_string_lossy())
            )
        } else {
            "尚未写入 shell 别名".to_string()
        },
    })
}

pub fn reveal_dir(id: String) -> Result<CodexEnvActionResult, String> {
    let row = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let path = PathBuf::from(&row.config_dir);
    if !path.exists() {
        return Err(format!("目录不存在: {}", path.display()));
    }
    Command::new("open")
        .arg(path.as_os_str())
        .status()
        .map_err(|e| format!("打开 Finder 失败: {}", e))?;
    Ok(CodexEnvActionResult {
        ok: true,
        message: format!("已在 Finder 中打开 {}", row.config_dir),
        environment: None,
    })
}

pub fn open_config(id: String) -> Result<CodexEnvActionResult, String> {
    let row = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let dir = PathBuf::from(&row.config_dir);
    if !dir.is_dir() {
        return Err(format!("环境目录不存在: {}", dir.display()));
    }
    let config = dir.join("config.toml");
    let created = if !config.is_file() {
        fs::write(&config, "# Codex config managed by AgentBuddy\n")
            .map_err(|e| format!("创建 config.toml 失败: {}", e))?;
        let _ = fs::set_permissions(&config, fs::Permissions::from_mode(0o600));
        true
    } else {
        false
    };

    let status = Command::new("open")
        .arg(config.as_os_str())
        .status()
        .map_err(|e| format!("打开 config.toml 失败: {}", e))?;
    if !status.success() {
        return Err(format!(
            "打开 config.toml 失败（退出码: {:?}）",
            status.code()
        ));
    }

    Ok(CodexEnvActionResult {
        ok: true,
        message: if created {
            format!(
                "已创建并打开 {}/config.toml",
                display_path_for_msg(&row.config_dir)
            )
        } else {
            format!(
                "已用系统默认应用打开 {}/config.toml",
                display_path_for_msg(&row.config_dir)
            )
        },
        environment: None,
    })
}

pub fn get_env_secret(id: String) -> Result<String, String> {
    let row = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let dir = PathBuf::from(&row.config_dir);
    if !dir.is_dir() {
        return Ok(String::new());
    }
    let managed = read_managed_config(&dir);
    Ok(managed.bearer_token)
}

pub fn sync_mcp_to_environment(id: String) -> Result<CodexEnvMcpSyncResult, String> {
    ensure_default_environment()?;
    let row = db::get_codex_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;

    let src = shared_config_path()?;
    let (global_name_set, _) = read_mcp_server_names(&src);
    let mut global_names: Vec<String> = global_name_set.iter().cloned().collect();
    global_names.sort();
    let global_count = global_names.len() as u32;

    if row.is_default {
        return Ok(CodexEnvMcpSyncResult {
            ok: true,
            message: "默认环境已直接使用 ~/.codex/config.toml，无需同步".into(),
            global_server_count: global_count,
            global_server_names: global_names,
            results: vec![CodexEnvMcpSyncItem {
                id: row.id,
                name: row.name,
                ok: true,
                status: "default".into(),
                server_count: global_count,
                message: "已使用默认 ~/.codex/config.toml".into(),
            }],
        });
    }

    let dir = PathBuf::from(&row.config_dir);
    match sync_mcp_servers_to_dir(&dir) {
        Ok((count, names)) => Ok(CodexEnvMcpSyncResult {
            ok: true,
            message: format!("已将默认环境 MCP（{} 个）同步到「{}」", count, row.name),
            global_server_count: global_count,
            global_server_names: global_names.clone(),
            results: vec![CodexEnvMcpSyncItem {
                id: row.id,
                name: row.name,
                ok: true,
                status: "in_sync".into(),
                server_count: count,
                message: if names.is_empty() {
                    "已同步（默认环境无 mcp_servers）".into()
                } else {
                    format!("已同步: {}", names.join(", "))
                },
            }],
        }),
        Err(e) => Ok(CodexEnvMcpSyncResult {
            ok: false,
            message: format!("同步「{}」失败：{}", row.name, e),
            global_server_count: global_count,
            global_server_names: global_names,
            results: vec![CodexEnvMcpSyncItem {
                id: row.id,
                name: row.name,
                ok: false,
                status: "error".into(),
                server_count: 0,
                message: e,
            }],
        }),
    }
}

pub fn sync_mcp_to_all_environments() -> Result<CodexEnvMcpSyncResult, String> {
    ensure_default_environment()?;
    let src = shared_config_path()?;
    let (global_name_set, _) = read_mcp_server_names(&src);
    let mut global_names: Vec<String> = global_name_set.iter().cloned().collect();
    global_names.sort();
    let global_count = global_names.len() as u32;

    let rows = db::load_codex_environment_rows()?;
    let mut results = Vec::new();
    let mut ok_n = 0u32;
    let mut fail_n = 0u32;
    let mut skip_n = 0u32;

    for row in rows {
        if row.is_default {
            continue;
        }
        let dir = PathBuf::from(&row.config_dir);
        if !dir.is_dir() {
            skip_n += 1;
            results.push(CodexEnvMcpSyncItem {
                id: row.id,
                name: row.name,
                ok: false,
                status: "missing".into(),
                server_count: 0,
                message: "目录不存在，已跳过".into(),
            });
            continue;
        }
        match sync_mcp_servers_to_dir(&dir) {
            Ok((count, names)) => {
                ok_n += 1;
                results.push(CodexEnvMcpSyncItem {
                    id: row.id,
                    name: row.name,
                    ok: true,
                    status: "in_sync".into(),
                    server_count: count,
                    message: if names.is_empty() {
                        "已同步（默认环境无 mcp_servers）".into()
                    } else {
                        format!("已同步: {}", names.join(", "))
                    },
                });
            }
            Err(e) => {
                fail_n += 1;
                results.push(CodexEnvMcpSyncItem {
                    id: row.id,
                    name: row.name,
                    ok: false,
                    status: "error".into(),
                    server_count: 0,
                    message: e,
                });
            }
        }
    }

    if results.is_empty() {
        return Ok(CodexEnvMcpSyncResult {
            ok: true,
            message: "没有可同步的自定义环境".into(),
            global_server_count: global_count,
            global_server_names: global_names,
            results,
        });
    }

    let ok = fail_n == 0;
    let message = if ok {
        format!(
            "已同步 {} 个环境的 MCP（默认环境 {} 个 server）{}",
            ok_n,
            global_count,
            if skip_n > 0 {
                format!("，跳过 {} 个", skip_n)
            } else {
                String::new()
            }
        )
    } else {
        format!(
            "部分失败：成功 {}，失败 {}，跳过 {}",
            ok_n, fail_n, skip_n
        )
    };

    Ok(CodexEnvMcpSyncResult {
        ok,
        message,
        global_server_count: global_count,
        global_server_names: global_names,
        results,
    })
}

/* ===== Tests ===== */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_block_append_and_replace() {
        let block1 = render_marker_block(&[
            r#"alias codex-work="CODEX_HOME=$HOME/.codex-work codex""#.into(),
        ]);
        let base = "export PATH=/usr/bin\n";
        let with = apply_marker_block(base, &block1);
        assert!(with.contains(MARKER_BEGIN));
        assert!(with.contains("codex-work"));

        let block2 = render_marker_block(&[
            r#"alias codex-personal="CODEX_HOME=$HOME/.codex-personal codex""#.into(),
        ]);
        let replaced = apply_marker_block(&with, &block2);
        assert_eq!(replaced.matches(MARKER_BEGIN).count(), 1);
        assert!(replaced.contains("codex-personal"));
        assert!(!replaced.contains("codex-work"));
    }

    #[test]
    fn marker_block_remove() {
        let block = render_marker_block(&[
            r#"alias codex-work="CODEX_HOME=$HOME/.codex-work codex""#.into(),
        ]);
        let content = format!("before\n\n{}after\n", block);
        let (next, removed) = remove_marker_block(&content);
        assert!(removed);
        assert!(!next.contains(MARKER_BEGIN));
        assert!(next.contains("before"));
        assert!(next.contains("after"));
    }

    #[test]
    fn validate_slug_and_alias() {
        assert!(validate_slug("work").is_ok());
        assert!(validate_slug("default").is_err());
        assert!(validate_slug("-bad").is_err());
        assert!(validate_alias("codex-work", false).is_ok());
        assert!(validate_alias("codex", false).is_err());
        assert!(validate_alias("codex", true).is_ok());
    }

    #[test]
    fn parse_aliases() {
        let block = render_marker_block(&[
            r#"alias codex-work="CODEX_HOME=$HOME/.codex-work codex""#.into(),
            r#"alias codex-personal="CODEX_HOME=$HOME/.codex-personal codex""#.into(),
        ]);
        let names = parse_aliases_from_block(&block);
        assert_eq!(names, vec!["codex-work", "codex-personal"]);
    }

    #[test]
    fn mcp_servers_replace_preserves_other_keys() {
        let dir = std::env::temp_dir().join(format!("agentbuddy-codex-mcp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(
            &path,
            r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "http://127.0.0.1:1/v1"

[mcp_servers.old]
command = "echo"
"#,
        )
        .unwrap();

        let mut src_doc = toml_edit::DocumentMut::new();
        let mut table = toml_edit::Table::new();
        let mut server = toml_edit::Table::new();
        server["command"] = toml_edit::value("npx");
        table.insert("new", toml_edit::Item::Table(server));
        src_doc["mcp_servers"] = toml_edit::Item::Table(table);
        let item = src_doc.get("mcp_servers").cloned();

        write_mcp_servers_item(&path, item.as_ref()).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("model = \"gpt-5.5\"") || raw.contains("model=\"gpt-5.5\"") || raw.contains("gpt-5.5"));
        assert!(raw.contains("new"));
        assert!(!raw.contains("old") || raw.find("mcp_servers").is_some());
        let doc: toml_edit::DocumentMut = raw.parse().unwrap();
        let names: Vec<_> = doc
            .get("mcp_servers")
            .and_then(|i| i.as_table())
            .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default();
        assert_eq!(names, vec!["new".to_string()]);
        assert_eq!(toml_str(doc.get("model")), "gpt-5.5");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auth_json_write_merge_and_clear() {
        let dir = std::env::temp_dir().join(format!(
            "agentbuddy-codex-auth-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // 首次写入
        assert!(apply_auth_json_edit(&dir, Some("sk-test-1")).unwrap());
        let auth = dir.join("auth.json");
        assert!(auth.is_file());
        let raw = fs::read_to_string(&auth).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["OPENAI_API_KEY"], "sk-test-1");
        assert_eq!(read_auth_openai_api_key(&dir), "sk-test-1");

        // 合并保留其它键
        fs::write(
            &auth,
            r#"{
  "OPENAI_API_KEY": "sk-test-1",
  "other": "keep-me"
}
"#,
        )
        .unwrap();
        assert!(apply_auth_json_edit(&dir, Some("sk-test-2")).unwrap());
        let raw = fs::read_to_string(&auth).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["OPENAI_API_KEY"], "sk-test-2");
        assert_eq!(v["other"], "keep-me");

        // 相同值不写
        assert!(!apply_auth_json_edit(&dir, Some("sk-test-2")).unwrap());

        // 清除 OPENAI_API_KEY 但保留其它键
        assert!(apply_auth_json_edit(&dir, Some("")).unwrap());
        let raw = fs::read_to_string(&auth).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("OPENAI_API_KEY").is_none());
        assert_eq!(v["other"], "keep-me");
        assert_eq!(read_auth_openai_api_key(&dir), "");

        // 再清一次：无键则不变
        assert!(!apply_auth_json_edit(&dir, Some("")).unwrap());

        // 仅剩 OPENAI_API_KEY 时清除会删除文件
        assert!(apply_auth_json_edit(&dir, Some("only-key")).unwrap());
        // 去掉 other
        fs::write(&auth, r#"{"OPENAI_API_KEY":"only-key"}"#).unwrap();
        assert!(apply_auth_json_edit(&dir, Some("")).unwrap());
        assert!(!auth.is_file());

        // managed edit 同时写 token，不强制自定义 provider
        let changed = apply_managed_config_edit(
            &dir,
            Some("gpt-test"),
            Some("openai"),
            None,
            Some("sk-via-managed"),
        )
        .unwrap();
        assert!(changed.contains(&"Token".to_string()));
        assert!(changed.contains(&"模型".to_string()) || changed.contains(&"Provider".to_string()));
        assert_eq!(read_auth_openai_api_key(&dir), "sk-via-managed");
        let managed = read_managed_config(&dir);
        assert_eq!(managed.bearer_token, "sk-via-managed");
        assert!(managed.has_auth_file);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auth_json_prefers_over_legacy_bearer() {
        let dir = std::env::temp_dir().join(format!(
            "agentbuddy-codex-auth-legacy-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("config.toml"),
            r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
experimental_bearer_token = "legacy-token"
"#,
        )
        .unwrap();

        let managed = read_managed_config(&dir);
        assert_eq!(managed.bearer_token, "legacy-token");

        apply_auth_json_edit(&dir, Some("auth-token")).unwrap();
        let managed = read_managed_config(&dir);
        assert_eq!(managed.bearer_token, "auth-token");

        let _ = fs::remove_dir_all(&dir);
    }
}
