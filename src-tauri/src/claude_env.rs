//! Multi-environment manager for Claude Code via `CLAUDE_CONFIG_DIR`.
//!
//! Default root stays at `~/.claude`. Extra environments live under
//! `$HOME/.claude-<slug>` (or a user-chosen path still inside `$HOME`).
//! Shell aliases are written into a managed marker block in `~/.zshrc`.

use crate::db;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ENV_ID: &str = "default";
const MARKER_BEGIN: &str = "# >>> AgentBuddy Claude Env (managed) >>>";
const MARKER_END: &str = "# <<< AgentBuddy Claude Env (managed) <<<";

const CORE_FILES: &[&str] = &["settings.json", "CLAUDE.md"];
const CORE_DIRS: &[&str] = &["skills", "agents"];

/* ===== DTOs ===== */

/// Public list item returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvironment {
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
    pub has_settings: bool,
    pub has_skills: bool,
    pub has_agents: bool,
    /// MCP sync status vs global ~/.claude.json top-level mcpServers.
    /// default | in_sync | out_of_sync | missing | no_global
    pub mcp_sync_status: String,
    pub mcp_server_count: u32,
    pub global_mcp_server_count: u32,
    /// settings.json → env.ANTHROPIC_BASE_URL（实时读取，不入库）。缺失为空串。
    pub base_url: String,
    /// settings.json → env.ANTHROPIC_AUTH_TOKEN。出于安全，列表接口不回传明文，
    /// 此字段恒为空串；前端编辑时用 `get_claude_env_secret` 按需拉取真值。
    pub api_key: String,
    /// 该环境 settings.json 是否已设置 ANTHROPIC_AUTH_TOKEN（供 UI 展示「已配置」）。
    pub has_api_key: bool,
    /// settings.json → env.ANTHROPIC_MODEL（实时读取，不入库）。缺失为空串。
    /// 写入时会同步整组 DEFAULT_* 模型键；读取仍只看主键。
    pub model: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Internal DB row (no runtime probes).
#[derive(Debug, Clone)]
pub struct ClaudeEnvironmentRow {
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
pub struct ClaudeEnvUpsertPayload {
    /// None / empty = create (not used for default bootstrap).
    pub id: Option<String>,
    pub name: String,
    pub slug: String,
    pub config_dir: String,
    pub alias_name: String,
    pub notes: Option<String>,
    /// settings.json → env.ANTHROPIC_BASE_URL。Some("") 表示删除该键，None 表示不改动。
    pub base_url: Option<String>,
    /// settings.json → env.ANTHROPIC_AUTH_TOKEN。Some("") 表示删除该键，None 表示不改动。
    pub api_key: Option<String>,
    /// settings.json → env.ANTHROPIC_MODEL 及 DEFAULT_* 模型键族。
    /// Some("") 表示删除整组，None 表示不改动。
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvClonePayload {
    pub source_id: String,
    pub name: String,
    pub slug: String,
    pub config_dir: String,
    pub alias_name: String,
    pub notes: Option<String>,
    /// Optional override for settings.json → env.ANTHROPIC_BASE_URL.
    /// Empty / omitted keeps the value copied from the source environment.
    pub base_url: Option<String>,
    /// Optional override for settings.json → env.ANTHROPIC_AUTH_TOKEN.
    /// Empty / omitted keeps the value copied from the source environment.
    pub api_key: Option<String>,
    /// Optional override for settings.json → env.ANTHROPIC_MODEL 及 DEFAULT_* 键族。
    /// Empty / omitted keeps whatever the source environment had (usually none).
    pub model: Option<String>,
    /// When true (default if omitted), copy top-level mcpServers from ~/.claude.json
    /// into the new environment's `$config_dir/.claude.json`.
    pub sync_mcp: Option<bool>,
    /// When true, immediately write the shell alias into ~/.zshrc after creation.
    /// Defaults to false — alias管理默认保持独立一步，避免擅自改写 ~/.zshrc。
    pub install_alias: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvImportPayload {
    pub config_dir: String,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub alias_name: Option<String>,
    pub notes: Option<String>,
    /// When true, immediately write the shell alias into ~/.zshrc after import.
    /// Defaults to false — 与复制保持一致，别名写入默认为独立操作。
    pub install_alias: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvCandidate {
    pub path: String,
    pub suggested_name: String,
    pub suggested_slug: String,
    pub suggested_alias: String,
    pub has_settings: bool,
    pub has_skills: bool,
    pub has_agents: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvSniffResult {
    pub candidates: Vec<ClaudeEnvCandidate>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvShellStatus {
    pub zshrc_path: String,
    pub zshrc_exists: bool,
    pub block_present: bool,
    pub aliases: Vec<String>,
    pub preview: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvActionResult {
    pub ok: bool,
    pub message: String,
    pub environment: Option<ClaudeEnvironment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvMcpSyncItem {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub status: String,
    pub server_count: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvMcpSyncResult {
    pub ok: bool,
    pub message: String,
    pub global_server_count: u32,
    pub global_server_names: Vec<String>,
    pub results: Vec<ClaudeEnvMcpSyncItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvMcpStatusResult {
    pub global_path: String,
    pub global_exists: bool,
    pub global_server_count: u32,
    pub global_server_names: Vec<String>,
    pub environments: Vec<ClaudeEnvironment>,
    pub message: String,
}

/* ===== Helpers ===== */

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
    let home_canon = home
        .canonicalize()
        .unwrap_or_else(|_| home.clone());
    // If path does not exist yet, compare prefix against home string form.
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
        // Reject path traversal segments.
        if abs.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
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
    if !re_ok
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
    {
        return Err("slug 仅允许小写字母、数字与单个连字符，且不能首尾为连字符".into());
    }
    if slug == "default" {
        return Err("slug「default」已保留给默认环境".into());
    }
    Ok(slug)
}

fn validate_alias(alias: &str, allow_claude: bool) -> Result<String, String> {
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
    if !allow_claude && alias == "claude" {
        return Err("非默认环境不能使用别名「claude」，以免覆盖原生命令".into());
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
    let has_settings = path.join("settings.json").is_file();
    let has_skills = path.join("skills").is_dir();
    let has_agents = path.join("agents").is_dir();
    (true, has_settings, has_skills, has_agents)
}

fn row_to_public(row: ClaudeEnvironmentRow) -> ClaudeEnvironment {
    let path = PathBuf::from(&row.config_dir);
    let (dir_exists, has_settings, has_skills, has_agents) = probe_dir(&path);
    let (global_names, _) = read_mcp_server_names(&shared_mcp_path());
    let global_count = global_names.len() as u32;
    let (status, local_count) = mcp_status_for_row(&row, dir_exists, &global_names);
    let (base_url, api_key_plain, model) = if dir_exists {
        read_settings_env(&path)
    } else {
        (String::new(), String::new(), String::new())
    };
    let has_api_key = !api_key_plain.is_empty();
    ClaudeEnvironment {
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
        has_settings,
        has_skills,
        has_agents,
        mcp_sync_status: status,
        mcp_server_count: local_count,
        global_mcp_server_count: global_count,
        base_url,
        // 不回传明文 token：列表只给「是否已设置」，编辑时按需拉取真值。
        api_key: String::new(),
        has_api_key,
        model,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/* ===== MCP share (top-level mcpServers only) ===== */

fn shared_mcp_path() -> PathBuf {
    match home_dir() {
        Ok(h) => h.join(".claude.json"),
        Err(_) => PathBuf::from(".claude.json"),
    }
}

fn env_mcp_path(config_dir: &Path) -> PathBuf {
    config_dir.join(".claude.json")
}

/// Read top-level mcpServers object. Missing file / key → empty map.
fn read_mcp_servers(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.is_file() {
        return Ok(Map::new());
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    if raw.trim().is_empty() {
        return Ok(Map::new());
    }
    let doc: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))?;
    let Some(obj) = doc.as_object() else {
        return Err(format!("{} 根节点必须是 JSON 对象", path.display()));
    };
    match obj.get("mcpServers") {
        Some(v) if v.is_object() => Ok(v.as_object().cloned().unwrap_or_default()),
        Some(_) => Err(format!("{} 的 mcpServers 必须是对象", path.display())),
        None => Ok(Map::new()),
    }
}

fn read_mcp_server_names(path: &Path) -> (BTreeSet<String>, bool) {
    match read_mcp_servers(path) {
        Ok(map) => (map.keys().cloned().collect(), path.is_file()),
        Err(_) => (BTreeSet::new(), path.is_file()),
    }
}

#[allow(dead_code)]
fn mcp_server_names(map: &Map<String, Value>) -> BTreeSet<String> {
    map.keys().cloned().collect()
}

fn mcp_status_for_row(
    row: &ClaudeEnvironmentRow,
    dir_exists: bool,
    global_names: &BTreeSet<String>,
) -> (String, u32) {
    if row.is_default {
        return ("default".into(), global_names.len() as u32);
    }
    if !dir_exists {
        return ("missing".into(), 0);
    }
    if global_names.is_empty() && !shared_mcp_path().is_file() {
        // No global file at all
        let local_path = env_mcp_path(Path::new(&row.config_dir));
        let (local_names, _) = read_mcp_server_names(&local_path);
        return ("no_global".into(), local_names.len() as u32);
    }
    let local_path = env_mcp_path(Path::new(&row.config_dir));
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
    } else if global_names.is_empty() && local_names.is_empty() {
        ("in_sync".into(), 0)
    } else {
        ("out_of_sync".into(), count)
    }
}

/// Write top-level mcpServers, preserving all other keys. Creates file if missing.
fn write_mcp_servers(path: &Path, servers: &Map<String, Value>) -> Result<(), String> {
    let mut doc: Value = if path.is_file() {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
        if raw.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(&raw)
                .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))?
        }
    } else {
        Value::Object(Map::new())
    };

    if !doc.is_object() {
        return Err(format!("{} 根节点必须是 JSON 对象", path.display()));
    }
    {
        let obj = doc.as_object_mut().unwrap();
        obj.insert("mcpServers".into(), Value::Object(servers.clone()));
    }

    let pretty = serde_json::to_string_pretty(&doc)
        .map_err(|e| format!("序列化 {} 失败: {}", path.display(), e))?;
    let content = format!("{}\n", pretty);

    // Prefer restrictive perms when creating a new secrets-bearing file.
    let create_mode = if path.is_file() {
        fs::metadata(path).ok().map(|m| m.permissions().mode())
    } else {
        None
    };

    atomic_write(path, &content)?;

    if let Some(mode) = create_mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    } else {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn sync_mcp_servers_to_dir(config_dir: &Path) -> Result<(u32, Vec<String>), String> {
    if !config_dir.is_dir() {
        return Err(format!("环境目录不存在: {}", config_dir.display()));
    }
    let src = shared_mcp_path();
    let servers = read_mcp_servers(&src)?;
    let names: Vec<String> = {
        let mut v: Vec<String> = servers.keys().cloned().collect();
        v.sort();
        v
    };
    let count = names.len() as u32;
    let dst = env_mcp_path(config_dir);

    // Skip if same path (shouldn't happen for non-default)
    if let (Ok(a), Ok(b)) = (src.canonicalize(), dst.canonicalize()) {
        if a == b {
            return Ok((count, names));
        }
    }

    write_mcp_servers(&dst, &servers)?;
    Ok((count, names))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("创建目录 {} 失败: {}", dst.display(), e))?;
    for entry in fs::read_dir(src)
        .map_err(|e| format!("读取目录 {} 失败: {}", src.display(), e))?
    {
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
        // skip symlinks / other
    }
    Ok(())
}

fn copy_core(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源环境目录不存在: {}", src.display()));
    }
    fs::create_dir_all(dst)
        .map_err(|e| format!("创建目标目录 {} 失败: {}", dst.display(), e))?;

    for name in CORE_FILES {
        let from = src.join(name);
        if from.is_file() {
            let to = dst.join(name);
            fs::copy(&from, &to).map_err(|e| {
                format!(
                    "复制 {} 失败: {}",
                    name,
                    e
                )
            })?;
        }
    }
    for name in CORE_DIRS {
        let from = src.join(name);
        if from.is_dir() {
            let to = dst.join(name);
            copy_dir_recursive(&from, &to)?;
        }
    }
    Ok(())
}

/// 目录迁移方式，用于区分返回给用户的提示文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvMoveOutcome {
    /// 原目录不存在或为空：没有内容被搬运，仅记录路径变化。
    NoMove,
    /// `fs::rename` 原子重命名成功。
    Renamed,
    /// 跨卷回退：完整复制后删除旧目录。
    Copied,
}

/// 严格递归复制：遇到 symlink 或特殊文件直接报错。
///
/// 与用于 clone 的 [`copy_dir_recursive`]（静默跳过 symlink）刻意分开：clone 只需核心
/// 文件的近似副本，而"重命名/迁移"要求内容零丢失，静默跳过 symlink 会违反预期，
/// 因此这里选择显式失败让用户手工处理。
fn copy_dir_strict(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("创建目录 {} 失败: {}", dst.display(), e))?;
    for entry in fs::read_dir(src)
        .map_err(|e| format!("读取目录 {} 失败: {}", src.display(), e))?
    {
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

/// 真正迁移环境目录：优先 `fs::rename`（原子、保留 symlink 与全部内容），
/// 跨卷失败时回退到严格复制 + 删除旧目录。
///
/// 契约：
/// - 原目录不存在或为空 → 返回 [`EnvMoveOutcome::NoMove`]，不做任何磁盘操作；
/// - 目标目录必须不存在或为空，否则报错且不改动任何目录；
/// - 任一步失败都会清理已创建的新目录，确保源目录保持完整可用。
fn move_environment_dir(src: &Path, dst: &Path) -> Result<EnvMoveOutcome, String> {
    // 原目录不存在但存在同名非目录文件：拒绝，避免覆盖不明文件。
    if src.exists() && !src.is_dir() {
        return Err(format!("原路径存在但不是目录：{}", src.display()));
    }
    // 没有内容需要搬运（原目录缺失或为空）：视为仅改路径。
    if !src.is_dir() || is_dir_empty(src)? {
        return Ok(EnvMoveOutcome::NoMove);
    }
    // 目标必须不存在或为空（is_dir_empty 对"存在但非目录"会报错）。
    if !is_dir_empty(dst)? {
        return Err(format!("新路径已存在且非空：{}", dst.display()));
    }
    // rename 要求目标不存在；若是已存在的空目录先移除，让 rename 能落地。
    if dst.exists() {
        fs::remove_dir(dst)
            .map_err(|e| format!("清理空目标目录 {} 失败: {}", dst.display(), e))?;
    }
    // 确保目标父目录存在（新路径可能嵌套在尚未创建的目录下），避免同卷时也无谓走复制回退。
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标父目录 {} 失败: {}", parent.display(), e))?;
    }
    // 优先原子重命名（同卷时保留 symlink、权限、时间戳等一切内容）。
    match fs::rename(src, dst) {
        Ok(()) => Ok(EnvMoveOutcome::Renamed),
        Err(rename_err) => {
            // 跨文件系统等原因失败：回退到严格复制。
            if let Err(copy_err) = copy_dir_strict(src, dst) {
                // 清理半成品副本，保留源目录不变。
                let _ = fs::remove_dir_all(dst);
                return Err(format!(
                    "迁移目录失败（重命名：{}；复制回退：{}）",
                    rename_err, copy_err
                ));
            }
            // 复制成功后删除旧目录。删除失败则回滚新副本，让源目录继续作为唯一可用副本，
            // 避免两处残留被误当成迁移成功。
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

/// 自定义模型写入 settings.json → env 时同步维护的一组键。
/// 主键 `ANTHROPIC_MODEL` 仍是 UI/DTO 读写的唯一入口；其余为 Claude Code 各档默认模型
/// 覆盖（模型 id 与展示名共用同一用户输入值）。
const MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
];

/// clone 时写入 settings.json 的受管 env 键。语义：留空表示「沿用源值」，
/// 因此把空字段转成 `None`（不改动）后复用统一实现 `apply_settings_env_edit`，
/// 既消除重复，也让 clone 出的 settings.json 一并享有 0600 权限。
fn apply_settings_overrides(
    config_dir: &Path,
    base_url: Option<&str>,
    api_key: Option<&str>,
    model: Option<&str>,
) -> Result<Vec<String>, String> {
    // 用内部 fn 而非闭包：自由函数有生命周期省略，会把输入 &str 与输出 &str 绑定同一生命周期。
    fn non_empty(o: Option<&str>) -> Option<&str> {
        o.map(str::trim).filter(|s| !s.is_empty())
    }
    apply_settings_env_edit(
        config_dir,
        non_empty(base_url),
        non_empty(api_key),
        non_empty(model),
    )
}

/// Read the managed env keys from `<config_dir>/settings.json`.
/// Missing file / key / non-string value → empty string. Never fails hard:
/// a malformed settings.json just yields empties so listing keeps working.
/// `model` 只读主键 `ANTHROPIC_MODEL`（列表/编辑表单的唯一来源）。
fn read_settings_env(config_dir: &Path) -> (String, String, String) {
    let path = config_dir.join("settings.json");
    if !path.is_file() {
        return (String::new(), String::new(), String::new());
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return (String::new(), String::new(), String::new());
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (String::new(), String::new(), String::new());
    };
    let get = |key: &str| -> String {
        root.get("env")
            .and_then(|e| e.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    (
        get("ANTHROPIC_BASE_URL"),
        get("ANTHROPIC_AUTH_TOKEN"),
        get("ANTHROPIC_MODEL"),
    )
}

/// Apply edits to the managed env keys in `<config_dir>/settings.json`.
///
/// Per-field semantics:
/// - `None`        → leave the key(s) untouched.
/// - `Some("...")` → set the key (trimmed) to the given value.
/// - `Some("")`    → delete the key from `env` if present.
///
/// `model` 会同步写入/删除整组 [`MODEL_ENV_KEYS`]（同一用户值），避免只改主键
/// 导致各档 DEFAULT_* 残留旧值。
///
/// Returns the list of human-readable field labels that actually changed.
/// No net change → no write (keeps mtime / avoids needless disk churn).
/// Preserves all other keys; env-scope secrets file kept at 0600.
fn apply_settings_env_edit(
    config_dir: &Path,
    base_url: Option<&str>,
    api_key: Option<&str>,
    model: Option<&str>,
) -> Result<Vec<String>, String> {
    // Normalize: None stays None; Some is trimmed.
    let base = base_url.map(|s| s.trim().to_string());
    let key = api_key.map(|s| s.trim().to_string());
    let model = model.map(|s| s.trim().to_string());
    if base.is_none() && key.is_none() && model.is_none() {
        return Ok(Vec::new());
    }

    let path = config_dir.join("settings.json");
    let mut root: serde_json::Value = if path.is_file() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("读取 settings.json 失败: {}", e))?;
        if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw)
                .map_err(|e| format!("解析 settings.json 失败: {}", e))?
        }
    } else {
        serde_json::json!({})
    };
    if !root.is_object() {
        return Err("settings.json 根节点必须是 JSON 对象".into());
    }

    // Apply each edit, tracking whether the document actually changed.
    // 统一用 apply_keys：单键（Base URL / API Key）与模型键族共用同一路径，
    // 避免两个 mut 闭包同时借用 root/changed。
    let mut changed = Vec::new();
    let mut apply_keys = |field: Option<String>, json_keys: &[&str], label: &str| -> Result<(), String> {
        let Some(val) = field else { return Ok(()) };
        let map = root.as_object_mut().unwrap();
        let mut any = false;
        if val.is_empty() {
            // Delete keys from env if present.
            if let Some(env) = map.get_mut("env").and_then(|v| v.as_object_mut()) {
                for k in json_keys {
                    if env.remove(*k).is_some() {
                        any = true;
                    }
                }
            }
        } else {
            // Set keys, creating env object if needed.
            if !map.get("env").map(|v| v.is_object()).unwrap_or(false) {
                map.insert("env".into(), serde_json::json!({}));
            }
            let env = map
                .get_mut("env")
                .and_then(|v| v.as_object_mut())
                .ok_or_else(|| "无法写入 settings.json 的 env 字段".to_string())?;
            for k in json_keys {
                let prev = env.get(*k).and_then(|v| v.as_str());
                if prev != Some(val.as_str()) {
                    env.insert((*k).into(), serde_json::Value::String(val.clone()));
                    any = true;
                }
            }
        }
        if any {
            changed.push(label.to_string());
        }
        Ok(())
    };
    apply_keys(base, &["ANTHROPIC_BASE_URL"], "Base URL")?;
    apply_keys(key, &["ANTHROPIC_AUTH_TOKEN"], "API Key")?;
    apply_keys(model, MODEL_ENV_KEYS, "模型")?;

    if changed.is_empty() {
        return Ok(changed);
    }

    // Drop an empty `env` object we may have left behind, to avoid noise.
    if let Some(map) = root.as_object_mut() {
        if map.get("env").map(|v| v.as_object().map(|o| o.is_empty()).unwrap_or(false)).unwrap_or(false) {
            map.remove("env");
        }
    }

    let pretty = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("序列化 settings.json 失败: {}", e))?;
    let content = format!("{}\n", pretty);
    // Preserve existing perms; default to 0600 for this secrets-bearing file.
    let create_mode = fs::metadata(&path).ok().map(|m| m.permissions().mode());
    atomic_write(&path, &content)?;
    if let Some(mode) = create_mode {
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode));
    } else {
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(changed)
}

/// 老环境可能只有主键 `ANTHROPIC_MODEL`。若主键非空且伴生 DEFAULT_* 键缺失或
/// 与主键不一致，则按主键值补齐整组 [`MODEL_ENV_KEYS`]。主键缺失/空串时不改动。
/// 复用 `apply_settings_env_edit`：已齐全则 no-op，不全才写盘。
fn ensure_model_env_companions(config_dir: &Path) -> Result<Vec<String>, String> {
    let (_, _, model) = read_settings_env(config_dir);
    if model.is_empty() {
        return Ok(Vec::new());
    }
    apply_settings_env_edit(config_dir, None, None, Some(model.as_str()))
}

fn ensure_unique_fields(
    id: &str,
    slug: &str,
    config_dir: &str,
    alias_name: &str,
) -> Result<(), String> {
    let rows = db::load_claude_environment_rows()?;
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
    let rows = db::load_claude_environment_rows()?;
    if rows.iter().any(|r| r.is_default) {
        return Ok(());
    }
    let home = home_dir()?;
    let config_dir = home.join(".claude");
    let now = now_secs();
    let row = ClaudeEnvironmentRow {
        id: DEFAULT_ENV_ID.into(),
        name: "默认环境".into(),
        slug: "default".into(),
        config_dir: config_dir.to_string_lossy().to_string(),
        alias_name: "claude".into(),
        is_default: true,
        source: "default".into(),
        notes: "直接运行 claude 使用此环境（不写入 shell 别名块）".into(),
        alias_installed: false,
        created_at: now,
        updated_at: now,
    };
    db::upsert_claude_environment_row(&row)
}

fn slug_from_dirname(dirname: &str) -> String {
    // .claude-work -> work ; .claude -> default
    if dirname == ".claude" {
        return "default".into();
    }
    let rest = dirname
        .strip_prefix(".claude-")
        .unwrap_or(dirname);
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
    // Fallback absolute (should be rare given home restriction)
    Ok(config_dir.to_string())
}

/* ===== Shell marker block ===== */

/// 目标 shell 类型（决定 rc 文件与别名语法）。
#[derive(Clone, Copy, PartialEq)]
enum ShellKind {
    Zsh,
    Bash,
    Fish,
}

/// 依据 $SHELL 猜测当前登录 shell。未知一律按 zsh 处理（macOS 默认）。
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

/// 当前 shell 对应的 rc 文件路径与类型：
/// - zsh  → ~/.zshrc
/// - bash → ~/.bash_profile（优先已存在的，其次 ~/.bashrc）
/// - fish → ~/.config/fish/config.fish
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

/// 供提示消息使用的 rc 路径（~ 形式）。失败兜底 ~/.zshrc。
fn rc_hint() -> String {
    match shell_rc() {
        Ok((path, _)) => display_path_for_msg(&path.to_string_lossy()),
        Err(_) => "~/.zshrc".to_string(),
    }
}

/// Only envs marked `alias_installed` (and non-default with existing dir) enter the shell block.
fn build_alias_lines(
    rows: &[ClaudeEnvironmentRow],
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
        // fish 不支持 `VAR=val cmd` 前缀语法，用 env 包一层；zsh/bash 通用。
        let line = if shell == ShellKind::Fish {
            format!(
                "alias {}=\"env CLAUDE_CONFIG_DIR={} claude\"",
                r.alias_name, shell_path
            )
        } else {
            format!(
                "alias {}=\"CLAUDE_CONFIG_DIR={} claude\"",
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

/// Replace or append the managed marker block. Pure string transform (unit-testable).
pub fn apply_marker_block(content: &str, block: &str) -> String {
    if let Some(start) = content.find(MARKER_BEGIN) {
        if let Some(end_rel) = content[start..].find(MARKER_END) {
            let end = start + end_rel + MARKER_END.len();
            // Consume a single trailing newline after the end marker if present.
            let mut end_adj = end;
            if content[end_adj..].starts_with('\n') {
                end_adj += 1;
            }
            let mut new_content = String::with_capacity(content.len() + block.len());
            new_content.push_str(&content[..start]);
            // Avoid double blank lines before block
            while new_content.ends_with("\n\n\n") {
                new_content.pop();
            }
            new_content.push_str(block);
            new_content.push_str(&content[end_adj..]);
            return new_content;
        }
    }
    // Append
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
            // Collapse excessive blank lines around the cut
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
    // Per-call atomic sequence keeps the temp path unique even for concurrent writers
    // in the same process, so they never share a temp file and corrupt each other's write.
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
        let mut f = fs::File::create(&tmp)
            .map_err(|e| format!("创建临时文件失败: {}", e))?;
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

/* ===== Public API ===== */

pub fn list_environments() -> Result<Vec<ClaudeEnvironment>, String> {
    // Highest priority: Claude Code CLI must be installed. Config dir alone
    // (e.g. leftover ~/.claude) is not enough — same rule as agent sniff.
    // When missing, return empty so the UI shows the not-installed empty state
    // instead of a synthetic "default" card.
    if !crate::sniff::is_agent_installed("claude-code") {
        return Ok(Vec::new());
    }
    ensure_default_environment()?;
    let rows = db::load_claude_environment_rows()?;
    Ok(rows.into_iter().map(row_to_public).collect())
}

pub fn sniff_environments() -> Result<ClaudeEnvSniffResult, String> {
    if !crate::sniff::is_agent_installed("claude-code") {
        return Ok(ClaudeEnvSniffResult {
            candidates: Vec::new(),
            message: "未检测到 Claude Code CLI，请先安装后再扫描环境".into(),
        });
    }
    ensure_default_environment()?;
    let home = home_dir()?;
    let registered: std::collections::HashSet<String> = db::load_claude_environment_rows()?
        .into_iter()
        .map(|r| r.config_dir)
        .collect();

    let mut candidates = Vec::new();
    let entries = fs::read_dir(&home)
        .map_err(|e| format!("无法读取主目录: {}", e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".claude" {
            continue;
        }
        // Match .claude-<suffix>
        if !name.starts_with(".claude-") {
            continue;
        }
        let suffix = &name[".claude-".len()..];
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
        let (dir_exists, has_settings, has_skills, has_agents) = probe_dir(&path);
        if !dir_exists {
            continue;
        }
        candidates.push(ClaudeEnvCandidate {
            path: path_str,
            suggested_name: format!("Claude · {}", slug),
            suggested_slug: slug.clone(),
            suggested_alias: format!("claude-{}", slug),
            has_settings,
            has_skills,
            has_agents,
        });
    }
    candidates.sort_by(|a, b| a.suggested_slug.cmp(&b.suggested_slug));
    let count = candidates.len();
    Ok(ClaudeEnvSniffResult {
        candidates,
        message: if count == 0 {
            "未发现未登记的 .claude-* 目录".into()
        } else {
            format!("发现 {} 个可导入目录", count)
        },
    })
}

pub fn import_environment(payload: ClaudeEnvImportPayload) -> Result<ClaudeEnvActionResult, String> {
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
        validate_alias(&format!("claude-{}", slug), false)?
    };
    let name = if let Some(n) = payload.name.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        validate_name(n)?
    } else {
        validate_name(&format!("Claude · {}", slug))?
    };
    let notes = payload.notes.unwrap_or_default().trim().to_string();
    let id = format!("env-{}-{}", slug, now_secs());

    ensure_unique_fields(&id, &slug, &config_dir_str, &alias)?;

    let now = now_secs();
    let mut row = ClaudeEnvironmentRow {
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
    db::upsert_claude_environment_row(&row)?;

    let mut message = format!("已导入环境「{}」", name);
    let (alias_installed, alias_msg) =
        install_alias_after_create(&row, payload.install_alias.unwrap_or(false));
    row.alias_installed = alias_installed;
    message.push_str(&alias_msg);

    let env = row_to_public(row);
    Ok(ClaudeEnvActionResult {
        ok: true,
        message,
        environment: Some(env),
    })
}

pub fn clone_environment(payload: ClaudeEnvClonePayload) -> Result<ClaudeEnvActionResult, String> {
    ensure_default_environment()?;
    let source = db::get_claude_environment_row(&payload.source_id)?
        .ok_or_else(|| format!("源环境不存在: {}", payload.source_id))?;
    let src_path = PathBuf::from(&source.config_dir);
    if !src_path.is_dir() {
        return Err(format!(
            "源环境目录不存在: {}。请确认 Claude Code 已初始化该环境。",
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
        // best-effort cleanup
        let _ = fs::remove_dir_all(&dst);
        return Err(e);
    }

    let override_fields = match apply_settings_overrides(
        &dst,
        payload.base_url.as_deref(),
        payload.api_key.as_deref(),
        payload.model.as_deref(),
    ) {
        Ok(fields) => fields,
        Err(e) => {
            let _ = fs::remove_dir_all(&dst);
            return Err(e);
        }
    };
    // 源环境若是老配置（仅有 ANTHROPIC_MODEL），克隆后补齐伴生键。
    let backfilled = match ensure_model_env_companions(&dst) {
        Ok(fields) => fields,
        Err(e) => {
            let _ = fs::remove_dir_all(&dst);
            return Err(e);
        }
    };

    let now = now_secs();
    let mut row = ClaudeEnvironmentRow {
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
    db::upsert_claude_environment_row(&row)?;

    let mut message = format!(
        "已从「{}」复制核心配置到「{}」。",
        source.name, name
    );
    if override_fields.is_empty() {
        message.push_str("Base URL / API Key 沿用源环境。");
    } else {
        message.push_str(&format!(
            "已覆盖 settings.json 中的 {}。",
            override_fields.join("、")
        ));
    }
    if !backfilled.is_empty() {
        message.push_str("已补齐 DEFAULT_* 模型键。");
    }

    let sync_mcp = payload.sync_mcp.unwrap_or(true);
    if sync_mcp {
        match sync_mcp_servers_to_dir(&dst) {
            Ok((count, _)) => {
                if count == 0 {
                    message.push_str(" 已尝试同步全局 MCP（当前全局无 mcpServers）。");
                } else {
                    message.push_str(&format!(" 已同步全局 MCP（{} 个 server）。", count));
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

    message.push_str("新环境不含会话历史，首次启动可能需要重新登录。");

    let env = row_to_public(row);
    Ok(ClaudeEnvActionResult {
        ok: true,
        message,
        environment: Some(env),
    })
}

pub fn upsert_environment(payload: ClaudeEnvUpsertPayload) -> Result<ClaudeEnvActionResult, String> {
    ensure_default_environment()?;
    let name = validate_name(&payload.name)?;
    let notes = payload.notes.unwrap_or_default().trim().to_string();
    let id = payload
        .id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "更新环境需要提供 id；新建请使用「从现有环境复制」".to_string())?;

    let existing = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;

    // 提前留存旧路径：默认分支会把 existing.config_dir move 进 tuple，
    // 之后判断"是否发生迁移"和回滚都需要它，故先克隆一份稳定副本。
    let previous_config_dir = existing.config_dir.clone();

    let (slug, config_dir, alias_name, is_default, source) = if existing.is_default {
        // Default: only name / notes / display alias_name (kept as claude)
        let _ = validate_alias("claude", true)?;
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
        // 仅做前置校验；真正的目录迁移放到 ensure_unique_fields 之后执行，
        // 避免"搬完才发现新路径已被其它环境占用"。
        if requested_str != previous_config_dir && !is_dir_empty(&requested)? {
            return Err(format!("新路径已存在且非空: {}", requested.display()));
        }
        (slug, requested_str, alias, false, existing.source)
    };

    ensure_unique_fields(&id, &slug, &config_dir, &alias_name)?;

    // 目录迁移：仅非默认环境、且路径确实变化时执行。放在唯一性校验之后，
    // 确保不会把内容搬到一个已被别的环境占用的路径。
    let mut move_outcome = EnvMoveOutcome::NoMove;
    if !is_default && config_dir != previous_config_dir {
        let src = PathBuf::from(&previous_config_dir);
        let dst = PathBuf::from(&config_dir);
        move_outcome = move_environment_dir(&src, &dst)?;
    }

    let now = now_secs();
    let row = ClaudeEnvironmentRow {
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
    db::upsert_claude_environment_row(&row).map_err(|e| {
        // DB 写入失败：尽力把目录迁回旧路径，保持磁盘与数据库一致。
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
    // Alias name / config_dir may have changed while still installed — refresh shell block.
    // alias body 从 DB 的 config_dir 重建，因此目录迁移后此处会自动指向新路径。
    if !is_default && existing.alias_installed {
        let _ = rewrite_shell_block_from_db();
    }

    // Edit managed settings.json env keys (non-default only). 留空即删除语义。
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
            match apply_settings_env_edit(
                &dir,
                payload.base_url.as_deref(),
                payload.api_key.as_deref(),
                payload.model.as_deref(),
            ) {
                Ok(changed) if !changed.is_empty() => {
                    message.push_str(&format!("；已更新 settings.json 的 {}", changed.join("、")));
                }
                Ok(_) => {}
                Err(e) => {
                    message.push_str(&format!("；但 settings.json 更新失败：{}", e));
                }
            }
            // 即使 model 字段未改（payload 为 None），也检查主键与伴生键是否齐全并补齐。
            // 用户显式改过/清空模型时 apply 已处理整组，此处多为 no-op。
            match ensure_model_env_companions(&dir) {
                Ok(changed) if !changed.is_empty() => {
                    message.push_str("；已补齐 DEFAULT_* 模型键");
                }
                Ok(_) => {}
                Err(e) => {
                    message.push_str(&format!("；但补齐 DEFAULT_* 模型键失败：{}", e));
                }
            }
        } else if payload.base_url.is_some()
            || payload.api_key.is_some()
            || payload.model.is_some()
        {
            message.push_str("；环境目录不存在，未写入 Base URL / API Key / 模型");
        }
    }

    let env = row_to_public(row);
    Ok(ClaudeEnvActionResult {
        ok: true,
        message,
        environment: Some(env),
    })
}

pub fn delete_environment(id: String, delete_files: bool) -> Result<ClaudeEnvActionResult, String> {
    ensure_default_environment()?;
    let existing = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    if existing.is_default {
        return Err("不能删除默认环境".into());
    }

    if delete_files {
        let path = PathBuf::from(&existing.config_dir);
        path_inside_home(&path)?;
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| {
                format!("删除目录 {} 失败: {}", path.display(), e)
            })?;
        }
    }

    db::delete_claude_environment_row(&id)?;

    // Refresh shell block from remaining installed flags.
    let _ = rewrite_shell_block_from_db();

    Ok(ClaudeEnvActionResult {
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

/// Rewrite ~/.zshrc managed block from rows with `alias_installed = true`.
fn rewrite_shell_block_from_db() -> Result<ClaudeEnvShellStatus, String> {
    ensure_default_environment()?;
    let rows = db::load_claude_environment_rows()?;
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
            return Ok(ClaudeEnvShellStatus {
                zshrc_path: zshrc.to_string_lossy().to_string(),
                zshrc_exists: true,
                block_present: false,
                aliases: vec![],
                preview: String::new(),
                message: format!(
                    "已清除 {} 中的 AgentBuddy 标记块（当前没有启用的别名）",
                    display_path_for_msg(&zshrc.to_string_lossy())
                ),
            });
        }
        return Ok(ClaudeEnvShellStatus {
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

    Ok(ClaudeEnvShellStatus {
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

/// Optionally install the shell alias right after a create/import.
///
/// Returns `(installed, message_suffix)`. The suffix is a human-readable
/// description of what happened (empty when `want` is false / no-op). Never
/// fails hard: the environment is already persisted, so alias problems only
/// degrade the trailing message, not the whole op.
fn install_alias_after_create(row: &ClaudeEnvironmentRow, want: bool) -> (bool, String) {
    if !want || row.is_default {
        return (false, String::new());
    }
    let path = PathBuf::from(&row.config_dir);
    if !path.is_dir() {
        return (false, " 未自动写入别名：环境目录不存在。".into());
    }
    if let Err(e) = db::set_claude_env_alias_installed(&row.id, true) {
        return (false, format!(" 别名写入失败：{}。", e));
    }
    match rewrite_shell_block_from_db() {
        Ok(_) => (
            true,
            format!(
                " 已写入 shell 别名 {}（执行 source {} 或新开终端后生效）。",
                row.alias_name, rc_hint()
            ),
        ),
        Err(e) => {
            // Roll back the DB flag so state stays consistent with ~/.zshrc.
            let _ = db::set_claude_env_alias_installed(&row.id, false);
            (false, format!(" 别名写入失败：{}。", e))
        }
    }
}

/// Enable shell alias for one non-default environment and rewrite the marker block.
pub fn install_env_alias(id: String) -> Result<ClaudeEnvShellStatus, String> {
    ensure_default_environment()?;
    let existing = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    if existing.is_default {
        return Err("默认环境不支持写入 shell 别名，请直接运行 claude".into());
    }
    let path = PathBuf::from(&existing.config_dir);
    if !path.is_dir() {
        return Err(format!(
            "环境目录不存在，无法写入别名: {}",
            existing.config_dir
        ));
    }
    db::set_claude_env_alias_installed(&id, true)?;
    let mut status = rewrite_shell_block_from_db()?;
    status.message = format!(
        "已为「{}」写入别名 {}。请执行 source {} 或新开终端后生效。",
        existing.name, existing.alias_name, rc_hint()
    );
    Ok(status)
}

/// Disable shell alias for one environment and rewrite the marker block.
pub fn remove_env_alias(id: String) -> Result<ClaudeEnvShellStatus, String> {
    ensure_default_environment()?;
    let existing = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    if existing.is_default {
        return Err("默认环境没有 shell 别名可移除".into());
    }
    db::set_claude_env_alias_installed(&id, false)?;
    let mut status = rewrite_shell_block_from_db()?;
    status.message = format!(
        "已移除「{}」的别名 {}。请执行 source {} 或新开终端后生效。",
        existing.name, existing.alias_name, rc_hint()
    );
    Ok(status)
}

/// Remove the whole managed block and clear all alias_installed flags.
pub fn remove_all_aliases() -> Result<ClaudeEnvShellStatus, String> {
    let (zshrc, _) = shell_rc()?;
    let zshrc_exists = zshrc.is_file();
    if !zshrc_exists {
        db::set_claude_env_alias_installed_all(false)?;
        return Ok(ClaudeEnvShellStatus {
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
    db::set_claude_env_alias_installed_all(false)?;
    Ok(ClaudeEnvShellStatus {
        zshrc_path: zshrc.to_string_lossy().to_string(),
        zshrc_exists: true,
        block_present: false,
        aliases: vec![],
        preview: String::new(),
        message: if removed {
            format!(
                "已从 {} 移除 AgentBuddy Claude 环境标记块",
                display_path_for_msg(&zshrc.to_string_lossy())
            )
        } else {
            format!(
                "{} 中未找到 AgentBuddy 标记块",
                display_path_for_msg(&zshrc.to_string_lossy())
            )
        },
    })
}

pub fn get_shell_status() -> Result<ClaudeEnvShellStatus, String> {
    ensure_default_environment()?;
    let rows = db::load_claude_environment_rows()?;
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

    Ok(ClaudeEnvShellStatus {
        zshrc_path: zshrc.to_string_lossy().to_string(),
        zshrc_exists,
        block_present,
        aliases,
        preview,
        message: if block_present {
            format!(
                "已在 {} 中检测到 AgentBuddy 标记块",
                display_path_for_msg(&zshrc.to_string_lossy())
            )
        } else {
            "尚未写入 shell 别名".to_string()
        },
    })
}

pub fn reveal_dir(id: String) -> Result<ClaudeEnvActionResult, String> {
    let row = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let path = PathBuf::from(&row.config_dir);
    if !path.exists() {
        return Err(format!("目录不存在: {}", path.display()));
    }
    Command::new("open")
        .arg(path.as_os_str())
        .status()
        .map_err(|e| format!("打开 Finder 失败: {}", e))?;
    Ok(ClaudeEnvActionResult {
        ok: true,
        message: format!("已在 Finder 中打开 {}", row.config_dir),
        environment: None,
    })
}

/// Open `<config_dir>/settings.json` with the system default app for editing.
/// Creates an empty JSON object file if the directory exists but the file is missing.
pub fn open_settings(id: String) -> Result<ClaudeEnvActionResult, String> {
    let row = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let dir = PathBuf::from(&row.config_dir);
    if !dir.is_dir() {
        return Err(format!("环境目录不存在: {}", dir.display()));
    }
    let settings = dir.join("settings.json");
    let created = if !settings.is_file() {
        fs::write(&settings, "{}\n")
            .map_err(|e| format!("创建 settings.json 失败: {}", e))?;
        true
    } else {
        false
    };

    let status = Command::new("open")
        .arg(settings.as_os_str())
        .status()
        .map_err(|e| format!("打开 settings.json 失败: {}", e))?;
    if !status.success() {
        return Err(format!(
            "打开 settings.json 失败（退出码: {:?}）",
            status.code()
        ));
    }

    Ok(ClaudeEnvActionResult {
        ok: true,
        message: if created {
            format!(
                "已创建并打开 {}/settings.json",
                display_path_for_msg(&row.config_dir)
            )
        } else {
            format!(
                "已用系统默认应用打开 {}/settings.json",
                display_path_for_msg(&row.config_dir)
            )
        },
        environment: None,
    })
}

/// 按需读取某环境 settings.json 的 ANTHROPIC_AUTH_TOKEN 明文，供编辑弹窗预填。
/// 列表接口不回传明文，只有用户主动编辑该环境时才调用此命令，收紧暴露面。
pub fn get_env_secret(id: String) -> Result<String, String> {
    let row = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;
    let dir = PathBuf::from(&row.config_dir);
    if !dir.is_dir() {
        return Ok(String::new());
    }
    let (_, api_key, _) = read_settings_env(&dir);
    Ok(api_key)
}

/// Sync global ~/.claude.json top-level mcpServers into one custom environment.
pub fn sync_mcp_to_environment(id: String) -> Result<ClaudeEnvMcpSyncResult, String> {
    ensure_default_environment()?;
    let row = db::get_claude_environment_row(&id)?
        .ok_or_else(|| format!("环境不存在: {}", id))?;

    let src = shared_mcp_path();
    let global_servers = read_mcp_servers(&src)?;
    let mut global_names: Vec<String> = global_servers.keys().cloned().collect();
    global_names.sort();
    let global_count = global_names.len() as u32;

    if row.is_default {
        return Ok(ClaudeEnvMcpSyncResult {
            ok: true,
            message: "默认环境已直接使用全局 ~/.claude.json，无需同步".into(),
            global_server_count: global_count,
            global_server_names: global_names,
            results: vec![ClaudeEnvMcpSyncItem {
                id: row.id,
                name: row.name,
                ok: true,
                status: "default".into(),
                server_count: global_count,
                message: "已使用全局 ~/.claude.json".into(),
            }],
        });
    }

    let dir = PathBuf::from(&row.config_dir);
    match sync_mcp_servers_to_dir(&dir) {
        Ok((count, names)) => Ok(ClaudeEnvMcpSyncResult {
            ok: true,
            message: format!(
                "已将全局 MCP（{} 个）同步到「{}」",
                count, row.name
            ),
            global_server_count: global_count,
            global_server_names: global_names.clone(),
            results: vec![ClaudeEnvMcpSyncItem {
                id: row.id,
                name: row.name,
                ok: true,
                status: "in_sync".into(),
                server_count: count,
                message: if names.is_empty() {
                    "已同步（全局无 mcpServers）".into()
                } else {
                    format!("已同步: {}", names.join(", "))
                },
            }],
        }),
        Err(e) => Ok(ClaudeEnvMcpSyncResult {
            ok: false,
            message: format!("同步「{}」失败：{}", row.name, e),
            global_server_count: global_count,
            global_server_names: global_names,
            results: vec![ClaudeEnvMcpSyncItem {
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

/// Sync global mcpServers into every non-default environment with an existing dir.
pub fn sync_mcp_to_all_environments() -> Result<ClaudeEnvMcpSyncResult, String> {
    ensure_default_environment()?;
    let src = shared_mcp_path();
    let global_servers = read_mcp_servers(&src)?;
    let mut global_names: Vec<String> = global_servers.keys().cloned().collect();
    global_names.sort();
    let global_count = global_names.len() as u32;

    let rows = db::load_claude_environment_rows()?;
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
            results.push(ClaudeEnvMcpSyncItem {
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
                results.push(ClaudeEnvMcpSyncItem {
                    id: row.id,
                    name: row.name,
                    ok: true,
                    status: "in_sync".into(),
                    server_count: count,
                    message: if names.is_empty() {
                        "已同步（全局无 mcpServers）".into()
                    } else {
                        format!("已同步: {}", names.join(", "))
                    },
                });
            }
            Err(e) => {
                fail_n += 1;
                results.push(ClaudeEnvMcpSyncItem {
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
        return Ok(ClaudeEnvMcpSyncResult {
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
            "已同步 {} 个环境的 MCP（全局 {} 个 server）{}",
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

    Ok(ClaudeEnvMcpSyncResult {
        ok,
        message,
        global_server_count: global_count,
        global_server_names: global_names,
        results,
    })
}

pub fn get_mcp_sync_status() -> Result<ClaudeEnvMcpStatusResult, String> {
    ensure_default_environment()?;
    let global_path = shared_mcp_path();
    let global_exists = global_path.is_file();
    let (global_name_set, _) = read_mcp_server_names(&global_path);
    let mut global_server_names: Vec<String> = global_name_set.iter().cloned().collect();
    global_server_names.sort();
    let global_server_count = global_server_names.len() as u32;

    let environments = list_environments()?;
    let out_of_sync = environments
        .iter()
        .filter(|e| !e.is_default && (e.mcp_sync_status == "out_of_sync" || e.mcp_sync_status == "missing"))
        .count();

    let message = if !global_exists {
        "未找到全局 ~/.claude.json".into()
    } else if out_of_sync == 0 {
        format!(
            "全局 MCP {} 个 server；自定义环境均已对齐",
            global_server_count
        )
    } else {
        format!(
            "全局 MCP {} 个 server；{} 个自定义环境未对齐",
            global_server_count, out_of_sync
        )
    };

    Ok(ClaudeEnvMcpStatusResult {
        global_path: global_path.to_string_lossy().to_string(),
        global_exists,
        global_server_count,
        global_server_names,
        environments,
        message,
    })
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

/* ===== Tests ===== */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_block_append_and_replace() {
        let block1 = render_marker_block(&[
            r#"alias claude-work="CLAUDE_CONFIG_DIR=$HOME/.claude-work claude""#.into(),
        ]);
        let content = "# my zshrc\nexport FOO=1\n";
        let with = apply_marker_block(content, &block1);
        assert!(with.contains(MARKER_BEGIN));
        assert!(with.contains("claude-work"));
        assert!(with.contains("export FOO=1"));

        let block2 = render_marker_block(&[
            r#"alias claude-personal="CLAUDE_CONFIG_DIR=$HOME/.claude-personal claude""#.into(),
        ]);
        let replaced = apply_marker_block(&with, &block2);
        assert!(replaced.contains("claude-personal"));
        assert!(!replaced.contains("claude-work"));
        assert_eq!(replaced.matches(MARKER_BEGIN).count(), 1);
        assert!(replaced.contains("export FOO=1"));
    }

    #[test]
    fn marker_block_remove() {
        let block = render_marker_block(&[
            r#"alias claude-work="CLAUDE_CONFIG_DIR=$HOME/.claude-work claude""#.into(),
        ]);
        let content = format!("# head\n\n{}# tail\n", block);
        let (next, removed) = remove_marker_block(&content);
        assert!(removed);
        assert!(!next.contains(MARKER_BEGIN));
        assert!(next.contains("# head"));
        assert!(next.contains("# tail"));
    }

    #[test]
    fn validate_slug_and_alias() {
        assert!(validate_slug("work").is_ok());
        assert!(validate_slug("my-work").is_ok());
        assert!(validate_slug("Default").is_err());
        assert!(validate_slug("-bad").is_err());
        assert!(validate_slug("default").is_err());

        assert!(validate_alias("claude-work", false).is_ok());
        assert!(validate_alias("claude", false).is_err());
        assert!(validate_alias("claude", true).is_ok());
        assert!(validate_alias("1bad", false).is_err());
    }

    #[test]
    fn parse_aliases() {
        let block = render_marker_block(&[
            r#"alias claude-work="CLAUDE_CONFIG_DIR=$HOME/.claude-work claude""#.into(),
            r#"alias claude-personal="CLAUDE_CONFIG_DIR=$HOME/.claude-personal claude""#.into(),
        ]);
        let aliases = parse_aliases_from_block(&block);
        assert_eq!(aliases, vec!["claude-work", "claude-personal"]);
    }

    #[test]
    fn slug_from_dirname_works() {
        assert_eq!(slug_from_dirname(".claude-work"), "work");
        assert_eq!(slug_from_dirname(".claude-my_env"), "my-env");
        assert_eq!(slug_from_dirname(".claude"), "default");
    }

    #[test]
    fn settings_overrides_patch_env_keys() {
        let dir = std::env::temp_dir().join(format!(
            "agentbuddy-claude-env-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "http://old.example",
    "ANTHROPIC_AUTH_TOKEN": "old-token",
    "KEEP_ME": "yes"
  },
  "effortLevel": "high"
}
"#,
        )
        .unwrap();

        // Empty overrides → no change
        let changed = apply_settings_overrides(&dir, Some("  "), Some(""), None).unwrap();
        assert!(changed.is_empty());
        let raw = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(raw.contains("http://old.example"));
        assert!(raw.contains("old-token"));

        // Patch all three
        let changed = apply_settings_overrides(
            &dir,
            Some("https://new.example/v1"),
            Some("new-secret-token"),
            Some("claude-opus-4-8"),
        )
        .unwrap();
        assert_eq!(
            changed,
            vec![
                "Base URL".to_string(),
                "API Key".to_string(),
                "模型".to_string()
            ]
        );
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            v["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("https://new.example/v1")
        );
        assert_eq!(
            v["env"]["ANTHROPIC_AUTH_TOKEN"].as_str(),
            Some("new-secret-token")
        );
        assert_eq!(
            v["env"]["ANTHROPIC_MODEL"].as_str(),
            Some("claude-opus-4-8")
        );
        // Companion DEFAULT_* keys share the same custom model value.
        for k in MODEL_ENV_KEYS {
            assert_eq!(
                v["env"][k].as_str(),
                Some("claude-opus-4-8"),
                "missing or wrong companion key {}",
                k
            );
        }
        // Unrelated keys preserved
        assert_eq!(v["env"]["KEEP_ME"].as_str(), Some("yes"));
        assert_eq!(v["effortLevel"].as_str(), Some("high"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_env_edit_set_delete_and_noop() {
        let dir = std::env::temp_dir().join(format!(
            "agentbuddy-claude-env-edit-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "http://old.example",
    "ANTHROPIC_AUTH_TOKEN": "old-token",
    "KEEP_ME": "yes"
  },
  "effortLevel": "high"
}
"#,
        )
        .unwrap();

        // None on every field → no-op, no change reported.
        let changed = apply_settings_env_edit(&dir, None, None, None).unwrap();
        assert!(changed.is_empty());

        // Set base+model, delete api key (Some("")).
        let changed = apply_settings_env_edit(
            &dir,
            Some("https://new.example/v1"),
            Some(""),
            Some("claude-opus-4-8"),
        )
        .unwrap();
        assert_eq!(
            changed,
            vec![
                "Base URL".to_string(),
                "API Key".to_string(),
                "模型".to_string()
            ]
        );
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            v["env"]["ANTHROPIC_BASE_URL"].as_str(),
            Some("https://new.example/v1")
        );
        assert!(v["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
        for k in MODEL_ENV_KEYS {
            assert_eq!(
                v["env"][k].as_str(),
                Some("claude-opus-4-8"),
                "missing or wrong companion key {}",
                k
            );
        }
        // Unrelated keys preserved.
        assert_eq!(v["env"]["KEEP_ME"].as_str(), Some("yes"));
        assert_eq!(v["effortLevel"].as_str(), Some("high"));

        // Setting the same model again (all companions already match) → no change.
        let changed = apply_settings_env_edit(&dir, None, None, Some("claude-opus-4-8")).unwrap();
        assert!(changed.is_empty());

        // Setting the same value again → no change reported.
        let changed =
            apply_settings_env_edit(&dir, Some("https://new.example/v1"), None, None).unwrap();
        assert!(changed.is_empty());

        // Deleting an absent key → no change reported.
        let changed = apply_settings_env_edit(&dir, None, Some(""), None).unwrap();
        assert!(changed.is_empty());

        // Delete model → clears primary + all DEFAULT_* companions.
        let changed = apply_settings_env_edit(&dir, None, None, Some("")).unwrap();
        assert_eq!(changed, vec!["模型".to_string()]);
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        for k in MODEL_ENV_KEYS {
            assert!(
                v["env"].get(k).is_none(),
                "companion key {} should be removed",
                k
            );
        }
        assert_eq!(v["env"]["KEEP_ME"].as_str(), Some("yes"));

        // Deleting model again when already absent → no change.
        let changed = apply_settings_env_edit(&dir, None, None, Some("")).unwrap();
        assert!(changed.is_empty());

        // Backfill: primary already set, missing companions → write companions only.
        {
            let map = v.as_object().unwrap();
            let mut env = map.get("env").and_then(|e| e.as_object()).cloned().unwrap();
            env.insert(
                "ANTHROPIC_MODEL".into(),
                serde_json::Value::String("only-primary".into()),
            );
            let mut root = serde_json::Map::new();
            root.insert("env".into(), serde_json::Value::Object(env));
            root.insert(
                "effortLevel".into(),
                serde_json::Value::String("high".into()),
            );
            fs::write(
                dir.join("settings.json"),
                serde_json::to_string_pretty(&serde_json::Value::Object(root)).unwrap(),
            )
            .unwrap();
        }
        let changed = apply_settings_env_edit(&dir, None, None, Some("only-primary")).unwrap();
        assert_eq!(changed, vec!["模型".to_string()]);
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        for k in MODEL_ENV_KEYS {
            assert_eq!(v["env"][k].as_str(), Some("only-primary"));
        }

        // read_settings_env round-trips the current values (primary only).
        let (base, key, model) = read_settings_env(&dir);
        assert_eq!(base, "https://new.example/v1");
        assert_eq!(key, "");
        assert_eq!(model, "only-primary");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_model_companions_backfills_without_payload() {
        let dir = std::env::temp_dir().join(format!(
            "agentbuddy-claude-env-backfill-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Legacy: only primary key.
        fs::write(
            dir.join("settings.json"),
            r#"{
  "env": {
    "ANTHROPIC_MODEL": "legacy-model",
    "KEEP_ME": "yes"
  }
}
"#,
        )
        .unwrap();

        // Save without model payload → companions filled from primary.
        let changed = ensure_model_env_companions(&dir).unwrap();
        assert_eq!(changed, vec!["模型".to_string()]);
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        for k in MODEL_ENV_KEYS {
            assert_eq!(
                v["env"][k].as_str(),
                Some("legacy-model"),
                "companion {} not backfilled",
                k
            );
        }
        assert_eq!(v["env"]["KEEP_ME"].as_str(), Some("yes"));

        // Second save → already complete, no write.
        let changed = ensure_model_env_companions(&dir).unwrap();
        assert!(changed.is_empty());

        // No primary → no-op even if companions missing.
        fs::write(
            dir.join("settings.json"),
            r#"{ "env": { "KEEP_ME": "yes" } }"#,
        )
        .unwrap();
        let changed = ensure_model_env_companions(&dir).unwrap();
        assert!(changed.is_empty());
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert!(v["env"].get("ANTHROPIC_MODEL").is_none());
        assert_eq!(v["env"]["KEEP_ME"].as_str(), Some("yes"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_servers_write_preserves_other_keys_and_syncs_names() {
        let dir = std::env::temp_dir().join(format!(
            "agentbuddy-claude-mcp-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Target with extra keys, no mcpServers
        let target = dir.join(".claude.json");
        fs::write(
            &target,
            r#"{
  "numStartups": 3,
  "projects": { "/tmp/x": { "allowedTools": [] } }
}
"#,
        )
        .unwrap();

        let mut servers = Map::new();
        servers.insert(
            "demo".into(),
            serde_json::json!({"command": "npx", "args": ["-y", "demo"]}),
        );
        servers.insert(
            "alpha".into(),
            serde_json::json!({"type": "http", "url": "https://example.com"}),
        );
        write_mcp_servers(&target, &servers).unwrap();

        let v: Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(v["numStartups"], 3);
        assert!(v["projects"]["/tmp/x"].is_object());
        assert!(v["mcpServers"]["demo"].is_object());
        assert!(v["mcpServers"]["alpha"].is_object());

        let names = mcp_server_names(v["mcpServers"].as_object().unwrap());
        assert_eq!(
            names,
            BTreeSet::from(["alpha".to_string(), "demo".to_string()])
        );

        // Empty map clears servers but keeps other keys
        write_mcp_servers(&target, &Map::new()).unwrap();
        let v2: Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(v2["numStartups"], 3);
        assert_eq!(v2["mcpServers"], serde_json::json!({}));

        // Missing file → create
        let target2 = dir.join("nested").join(".claude.json");
        fs::create_dir_all(target2.parent().unwrap()).unwrap();
        write_mcp_servers(&target2, &servers).unwrap();
        assert!(target2.is_file());
        let v3: Value =
            serde_json::from_str(&fs::read_to_string(&target2).unwrap()).unwrap();
        assert!(v3["mcpServers"]["demo"].is_object());

        let _ = fs::remove_dir_all(&dir);
    }
}
