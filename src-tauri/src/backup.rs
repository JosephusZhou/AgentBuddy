//! Backup management: probe sources, pack curated configs into zip (optional passphrase
//! encryption), upload to one or more WebDAV targets.
//!
//! Spec: project root `BACKUP_MANAGE_PLAN.md`.

use crate::agents::{self, AgentSpec};
use crate::config::{self, BackupSettings};
use crate::crypto;
use crate::db;
use crate::mcp_config;
use crate::sniff;
use crate::webdav;
use chrono::Local;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// ===== Public DTOs =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUnitNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub available: bool,
    pub selected_by_default: bool,
    pub contains_secrets: bool,
    pub estimated_bytes: u64,
    pub path_summary: String,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub children: Vec<BackupUnitNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRunPayload {
    pub unit_ids: Vec<String>,
    pub webdav_connection_ids: Vec<String>,
    pub passphrase: Option<String>,
    pub remote_prefix: Option<String>,
    #[serde(default)]
    pub acknowledge_plaintext_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUploadTargetResult {
    pub connection_id: String,
    pub name: String,
    pub ok: bool,
    pub message: String,
    pub remote_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRunResult {
    pub ok: bool,
    pub archive_file_name: String,
    pub archive_bytes: u64,
    pub encrypted: bool,
    pub targets: Vec<BackupUploadTargetResult>,
    pub warnings: Vec<String>,
    pub message: String,
}

/// Remote archive entry for restore UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBackupItem {
    pub name: String,
    pub bytes: u64,
    pub last_modified: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRemoteBackupsPayload {
    pub connection_id: String,
    pub remote_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreBackupPayload {
    pub connection_id: String,
    pub file_name: String,
    pub remote_prefix: Option<String>,
    /// Required when archive is `.abenc`.
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreBackupResult {
    pub ok: bool,
    pub message: String,
    pub restored_files: u32,
    pub skipped_files: u32,
    pub warnings: Vec<String>,
}

/// Progress event emitted on channel `backup-progress` during `run_backup_upload`.
///
/// `phase`: collect | zip | encrypt | upload | finalize
/// `current` / `total`: overall step counters (1-based current when phase starts).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgressEvent {
    pub phase: String,
    pub current: u32,
    pub total: u32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
}

fn emit_backup_progress(app: &AppHandle, event: BackupProgressEvent) {
    // Best-effort: UI may have no listener; never fail the backup for emit errors.
    let _ = app.emit("backup-progress", &event);
}

// ===== Manifest (internal + serialized into zip) =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format_version: u32,
    app_version: String,
    created_at: String,
    platform: String,
    encrypted: bool,
    unit_ids: Vec<String>,
    contains_secrets: bool,
    sources: Vec<ManifestSource>,
    exclusions: Vec<String>,
    warnings: Vec<String>,
    checksums: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSource {
    id: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env_id: Option<String>,
    items: Vec<ManifestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestItem {
    path: String,
    origin: String,
    bytes: u64,
}

#[derive(Debug, Clone)]
struct FileEntry {
    /// Absolute path on disk
    source: PathBuf,
    /// Path inside the zip (and manifest)
    archive_path: String,
    /// Display origin (tilde-expanded style)
    origin: String,
    unit_id: String,
    bytes: u64,
    secrets: bool,
}

// ===== Settings passthrough =====

pub fn get_backup_settings() -> Result<BackupSettings, String> {
    config::load_backup_settings()
}

pub fn update_backup_settings(settings: BackupSettings) -> Result<BackupSettings, String> {
    config::save_backup_settings(settings)
}

// ===== List units =====

pub fn list_backup_units() -> Result<Vec<BackupUnitNode>, String> {
    let settings = config::load_backup_settings()?;
    let mut roots = Vec::new();

    roots.push(build_app_unit());
    roots.push(build_cliproxy_unit(&settings));
    roots.push(build_sub2api_unit(&settings));
    roots.push(build_agents_group());

    Ok(roots)
}

fn build_app_unit() -> BackupUnitNode {
    let dir = match config::app_dir() {
        Ok(d) => d,
        Err(e) => {
            return BackupUnitNode {
                id: "app:agentbuddy".into(),
                label: "AgentBuddy 应用数据".into(),
                kind: "app".into(),
                available: false,
                selected_by_default: false,
                contains_secrets: true,
                estimated_bytes: 0,
                path_summary: String::new(),
                warnings: vec![e],
                children: vec![],
            };
        }
    };

    let config_p = dir.join("config.json");
    let db_p = dir.join("agents.db");
    let skills_p = dir.join("skills");

    let mut children = vec![
        leaf_unit(
            "app:agentbuddy:config",
            "config.json（含 secretsKey）",
            "app",
            config_p.exists(),
            true,
            true,
            file_size(&config_p),
            display_path(&config_p),
            if config_p.exists() {
                vec!["包含加密主密钥 secretsKey".into()]
            } else {
                vec![]
            },
        ),
        leaf_unit(
            "app:agentbuddy:db",
            "agents.db（MCP / 环境 / WebDAV 元数据）",
            "app",
            db_p.exists(),
            true,
            true,
            file_size(&db_p),
            display_path(&db_p),
            vec![],
        ),
        leaf_unit(
            "app:agentbuddy:skills",
            "Skills 库",
            "app",
            skills_p.is_dir(),
            true,
            false,
            dir_size_capped(&skills_p),
            display_path(&skills_p),
            vec![],
        ),
    ];

    let available = children.iter().any(|c| c.available);
    let estimated_bytes: u64 = children.iter().map(|c| c.estimated_bytes).sum();
    let contains_secrets = children.iter().any(|c| c.contains_secrets);
    // Parent selected_by_default if any child is
    for c in &mut children {
        if !c.available {
            c.selected_by_default = false;
        }
    }

    BackupUnitNode {
        id: "app:agentbuddy".into(),
        label: "AgentBuddy 应用数据".into(),
        kind: "app".into(),
        available,
        selected_by_default: available,
        contains_secrets,
        estimated_bytes,
        path_summary: display_path(&dir),
        warnings: if available {
            vec!["备份 config.json 会导出 secretsKey，请优先使用口令加密".into()]
        } else {
            vec![]
        },
        children,
    }
}

fn build_cliproxy_unit(settings: &BackupSettings) -> BackupUnitNode {
    let conf = resolve_cliproxy_conf(settings);
    let auth_dir = conf
        .as_ref()
        .and_then(|p| read_auth_dir_from_conf(p))
        .or_else(|| {
            dirs::home_dir().map(|h| h.join(".cli-proxy-api"))
        });

    let conf_exists = conf.as_ref().map(|p| p.is_file()).unwrap_or(false);
    let auth_exists = auth_dir.as_ref().map(|p| p.is_dir()).unwrap_or(false);
    let available = conf_exists || auth_exists;

    let mut warnings = Vec::new();
    let mut bytes = 0u64;
    let mut summary_parts = Vec::new();

    if let Some(ref p) = conf {
        summary_parts.push(format!("conf: {}", display_path(p)));
        if conf_exists {
            bytes = bytes.saturating_add(file_size(p));
        } else {
            warnings.push(format!("配置文件不存在: {}", display_path(p)));
        }
    } else {
        warnings.push("未找到 cliproxyapi 配置文件".into());
    }
    if let Some(ref p) = auth_dir {
        summary_parts.push(format!("auth-dir: {}", display_path(p)));
        if auth_exists {
            bytes = bytes.saturating_add(dir_size_capped(p));
        }
    }

    BackupUnitNode {
        id: "tool:cliproxyapi".into(),
        label: "CLIProxyAPI (cliproxyapi)".into(),
        kind: "tool".into(),
        available,
        selected_by_default: available,
        contains_secrets: true,
        estimated_bytes: bytes,
        path_summary: summary_parts.join(" · "),
        warnings,
        children: vec![],
    }
}

fn build_sub2api_unit(settings: &BackupSettings) -> BackupUnitNode {
    let root = resolve_sub2api_root(settings);
    let available = root.as_ref().map(|p| p.exists()).unwrap_or(false);
    let mut warnings = Vec::new();
    let mut bytes = 0u64;
    let summary = if let Some(ref p) = root {
        let conf = p.join("config.yaml");
        if conf.is_file() {
            bytes = bytes.saturating_add(file_size(&conf));
        } else if p.join("config.yml").is_file() {
            bytes = bytes.saturating_add(file_size(&p.join("config.yml")));
        } else {
            warnings.push("根目录下未找到 config.yaml".into());
        }
        let data = p.join("data");
        if data.is_dir() {
            bytes = bytes.saturating_add(dir_size_capped(&data));
        }
        display_path(p)
    } else {
        warnings.push("未检测到 sub2api 安装目录（可在下方自定义路径）".into());
        String::new()
    };

    BackupUnitNode {
        id: "tool:sub2api".into(),
        label: "sub2api".into(),
        kind: "tool".into(),
        available,
        selected_by_default: available,
        contains_secrets: true,
        estimated_bytes: bytes,
        path_summary: summary,
        warnings,
        children: vec![],
    }
}

fn build_agents_group() -> BackupUnitNode {
    let agents_list = db::load_agents().unwrap_or_else(|_| sniff::sniff_agents());
    let found: HashMap<String, sniff::SniffResult> = agents_list
        .into_iter()
        .map(|a| (a.name.clone(), a))
        .collect();

    let claude_envs = db::load_claude_environment_rows().unwrap_or_default();
    let codex_envs = db::load_codex_environment_rows().unwrap_or_default();

    let mut children = Vec::new();
    let mut seen_shared: HashSet<String> = HashSet::new();

    for spec in agents::agents() {
        // 共享物理根的 agent 只建一个单元（shared_root 相同者跳过后续）
        if let Some(shared) = spec.shared_root {
            if !seen_shared.insert(shared.to_string()) {
                continue;
            }
        }

        let sniff = found.get(spec.name);
        let is_found = sniff.map(|s| s.found).unwrap_or(false)
            || sniff
                .map(|s| !s.config_dirs.is_empty())
                .unwrap_or(false);

        match spec.name {
            "claude-code" => {
                children.push(build_claude_agent_unit(spec, is_found, &claude_envs));
            }
            "codex" => {
                children.push(build_codex_agent_unit(spec, is_found, &codex_envs));
            }
            _ => {
                children.push(build_generic_agent_unit(
                    spec.name,
                    spec.display_name,
                    spec,
                    is_found,
                    sniff,
                ));
            }
        }
    }

    let available = children.iter().any(|c| c.available);
    let estimated_bytes: u64 = children.iter().map(|c| c.estimated_bytes).sum();

    BackupUnitNode {
        id: "agents".into(),
        label: "Agent 配置".into(),
        kind: "group".into(),
        available,
        selected_by_default: available,
        contains_secrets: children.iter().any(|c| c.contains_secrets),
        estimated_bytes,
        path_summary: format!("{} 个可备份", children.iter().filter(|c| c.available).count()),
        warnings: vec![],
        children,
    }
}

fn build_claude_agent_unit(
    spec: &AgentSpec,
    is_found: bool,
    envs: &[crate::claude_env::ClaudeEnvironmentRow],
) -> BackupUnitNode {
    let mut children = Vec::new();
    let home = dirs::home_dir();

    // default env
    let default_dir = home.as_ref().map(|h| h.join(".claude"));
    let global_mcp = home.as_ref().map(|h| h.join(".claude.json"));
    let def_bytes = estimate_claude_env_bytes(
        default_dir.as_deref(),
        global_mcp.as_deref(),
        true,
    );
    children.push(leaf_unit(
        "agent:claude-code:env:default",
        "默认环境 (~/.claude + ~/.claude.json)",
        "agent-env",
        is_found || default_dir.as_ref().map(|p| p.exists()).unwrap_or(false),
        true,
        true,
        def_bytes,
        default_dir
            .as_ref()
            .map(|p| display_path(p))
            .unwrap_or_default(),
        vec![],
    ));

    for env in envs {
        if env.id == "default" || env.is_default {
            continue;
        }
        let dir = PathBuf::from(&env.config_dir);
        let exists = dir.is_dir();
        let bytes = estimate_claude_env_bytes(Some(&dir), Some(&dir.join(".claude.json")), false);
        children.push(leaf_unit(
            &format!("agent:claude-code:env:{}", env.id),
            &format!("{} ({})", env.name, display_path(&dir)),
            "agent-env",
            exists,
            exists,
            true,
            bytes,
            display_path(&dir),
            if exists {
                vec![]
            } else {
                vec!["环境目录不存在".into()]
            },
        ));
    }

    let available = children.iter().any(|c| c.available);
    BackupUnitNode {
        id: "agent:claude-code".into(),
        label: spec.display_name.to_string(),
        kind: "agent".into(),
        available,
        selected_by_default: available,
        contains_secrets: true,
        estimated_bytes: children.iter().map(|c| c.estimated_bytes).sum(),
        path_summary: "~/.claude · ~/.claude.json".into(),
        warnings: if is_found {
            vec![]
        } else {
            vec!["未扫描到安装路径，仍可备份已存在的配置目录".into()]
        },
        children,
    }
}

fn build_codex_agent_unit(
    spec: &AgentSpec,
    is_found: bool,
    envs: &[crate::codex_env::CodexEnvironmentRow],
) -> BackupUnitNode {
    let mut children = Vec::new();
    let home = dirs::home_dir();
    let default_dir = home.as_ref().map(|h| h.join(".codex"));

    children.push(leaf_unit(
        "agent:codex:env:default",
        "默认环境 (~/.codex)",
        "agent-env",
        is_found || default_dir.as_ref().map(|p| p.exists()).unwrap_or(false),
        true,
        true,
        estimate_codex_env_bytes(default_dir.as_deref()),
        default_dir
            .as_ref()
            .map(|p| display_path(p))
            .unwrap_or_default(),
        vec![],
    ));

    for env in envs {
        if env.id == "default" || env.is_default {
            continue;
        }
        let dir = PathBuf::from(&env.config_dir);
        let exists = dir.is_dir();
        children.push(leaf_unit(
            &format!("agent:codex:env:{}", env.id),
            &format!("{} ({})", env.name, display_path(&dir)),
            "agent-env",
            exists,
            exists,
            true,
            estimate_codex_env_bytes(Some(&dir)),
            display_path(&dir),
            if exists {
                vec![]
            } else {
                vec!["环境目录不存在".into()]
            },
        ));
    }

    let available = children.iter().any(|c| c.available);
    BackupUnitNode {
        id: "agent:codex".into(),
        label: spec.display_name.to_string(),
        kind: "agent".into(),
        available,
        selected_by_default: available,
        contains_secrets: true,
        estimated_bytes: children.iter().map(|c| c.estimated_bytes).sum(),
        path_summary: "~/.codex".into(),
        warnings: vec![],
        children,
    }
}

fn build_generic_agent_unit(
    id_name: &str,
    label: &str,
    spec: &AgentSpec,
    is_found: bool,
    sniff: Option<&sniff::SniffResult>,
) -> BackupUnitNode {
    let mut paths = Vec::new();
    let mut bytes = 0u64;
    let mut secrets = false;

    // MCP file
    if let Ok(mcp) = mcp_config::resolve_mcp_path(spec.name) {
        if mcp.is_file() {
            bytes = bytes.saturating_add(file_size(&mcp));
            paths.push(display_path(&mcp));
            secrets = true;
        }
    }

    // config dirs + skills
    let config_dirs: Vec<PathBuf> = if let Some(s) = sniff {
        s.config_dirs.iter().map(PathBuf::from).collect()
    } else {
        spec.config_paths
            .iter()
            .filter_map(|p| expand_home(p))
            .collect()
    };

    for root in &config_dirs {
        if !root.exists() {
            continue;
        }
        paths.push(display_path(root));
        for skill_rel in spec.skills_roots {
            if let Some(sk) = expand_home(skill_rel) {
                if sk.is_dir() {
                    bytes = bytes.saturating_add(dir_size_capped(&sk));
                }
            }
        }
        // OpenCode / DevEco auth
        if matches!(spec.name, "opencode" | "deveco-code") {
            if let Some(auth) = opencode_auth_path(spec.name) {
                if auth.is_file() {
                    bytes = bytes.saturating_add(file_size(&auth));
                    paths.push(display_path(&auth));
                    secrets = true;
                }
            }
        }
    }

    // claude-desktop special
    if spec.name == "claude-desktop" {
        if let Ok(mcp) = mcp_config::resolve_mcp_path("claude-desktop") {
            if mcp.is_file() {
                bytes = bytes.saturating_add(file_size(&mcp));
                paths.push(display_path(&mcp));
                secrets = true;
            }
        }
    }

    let path_exists = mcp_config::resolve_mcp_path(spec.name)
        .map(|p| p.exists())
        .unwrap_or(false)
        || config_dirs.iter().any(|d| d.exists())
        || !paths.is_empty();
    let available = path_exists;
    BackupUnitNode {
        id: format!("agent:{}", id_name),
        label: label.to_string(),
        kind: "agent".into(),
        available,
        selected_by_default: is_found && available,
        contains_secrets: secrets || matches!(spec.name, "opencode" | "deveco-code" | "codex"),
        estimated_bytes: bytes,
        path_summary: paths.into_iter().take(3).collect::<Vec<_>>().join(" · "),
        warnings: if is_found {
            vec![]
        } else if available {
            vec!["未在 Agent 管理中标记为已安装，但检测到配置文件".into()]
        } else {
            vec!["未检测到配置".into()]
        },
        children: vec![],
    }
}

fn leaf_unit(
    id: &str,
    label: &str,
    kind: &str,
    available: bool,
    selected: bool,
    secrets: bool,
    bytes: u64,
    path_summary: String,
    warnings: Vec<String>,
) -> BackupUnitNode {
    BackupUnitNode {
        id: id.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
        available,
        selected_by_default: selected && available,
        contains_secrets: secrets,
        estimated_bytes: bytes,
        path_summary,
        warnings,
        children: vec![],
    }
}

// ===== Path resolvers =====

fn expand_home(path: &str) -> Option<PathBuf> {
    if path.starts_with("~/") {
        let home = dirs::home_dir()?;
        return Some(home.join(&path[2..]));
    }
    if path == "~" {
        return dirs::home_dir();
    }
    Some(PathBuf::from(path))
}

fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

fn resolve_cliproxy_conf(settings: &BackupSettings) -> Option<PathBuf> {
    if !settings.cliproxyapi_conf_path.trim().is_empty() {
        return Some(PathBuf::from(settings.cliproxyapi_conf_path.trim()));
    }
    let candidates = [
        "/usr/local/etc/cliproxyapi.conf",
        "/opt/homebrew/etc/cliproxyapi.conf",
        "/etc/cliproxyapi.conf",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    // still return first default for display even if missing
    Some(PathBuf::from(candidates[0]))
}

fn read_auth_dir_from_conf(conf: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(conf).ok()?;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        // auth-dir: "..."
        if let Some(rest) = t.strip_prefix("auth-dir:") {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return expand_home(v);
            }
        }
    }
    None
}

fn resolve_sub2api_root(settings: &BackupSettings) -> Option<PathBuf> {
    if !settings.sub2api_root_path.trim().is_empty() {
        let p = PathBuf::from(settings.sub2api_root_path.trim());
        return Some(p);
    }
    let home = dirs::home_dir()?;
    let candidates = [
        home.join("Downloads/sub2api"),
        PathBuf::from("/opt/sub2api"),
        PathBuf::from("/etc/sub2api"),
        home.join(".config/sub2api"),
        home.join(".sub2api"),
    ];
    for c in &candidates {
        if c.join("config.yaml").is_file()
            || c.join("config.yml").is_file()
            || c.join("sub2api").is_file()
            || c.is_dir() && c.join("data").exists()
        {
            return Some(c.clone());
        }
    }
    None
}

fn opencode_auth_path(agent_name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let share = match agent_name {
        "opencode" => "opencode",
        "deveco-code" => "deveco",
        _ => return None,
    };
    Some(home.join(".local/share").join(share).join("auth.json"))
}

// ===== Size helpers =====

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_size_capped(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if is_excluded(entry.path()) {
                continue;
            }
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if len > MAX_FILE_BYTES {
                continue;
            }
            total = total.saturating_add(len);
        }
    }
    total
}

fn estimate_claude_env_bytes(
    config_dir: Option<&Path>,
    mcp_json: Option<&Path>,
    include_global_mcp: bool,
) -> u64 {
    let mut n = 0u64;
    if let Some(d) = config_dir {
        n = n.saturating_add(file_size(&d.join("settings.json")));
        n = n.saturating_add(dir_size_capped(&d.join("skills")));
        if !include_global_mcp {
            n = n.saturating_add(file_size(&d.join(".claude.json")));
        }
    }
    if include_global_mcp {
        if let Some(m) = mcp_json {
            n = n.saturating_add(file_size(m));
        }
    }
    n
}

fn estimate_codex_env_bytes(config_dir: Option<&Path>) -> u64 {
    let Some(d) = config_dir else { return 0 };
    let mut n = 0u64;
    for name in ["config.toml", "AGENTS.md", "auth.json"] {
        n = n.saturating_add(file_size(&d.join(name)));
    }
    n = n.saturating_add(dir_size_capped(&d.join("skills")));
    n
}

// ===== Exclusion =====

fn is_excluded(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let lower = s.to_ascii_lowercase();
    if lower.ends_with(".ds_store") {
        return true;
    }
    if lower.ends_with(".log") {
        return true;
    }
    if lower.contains("/logs/") || lower.ends_with("/logs") {
        return true;
    }
    if lower.contains("/cache/") || lower.ends_with("/cache") {
        return true;
    }
    if lower.contains("/sessions/") || lower.ends_with("/sessions") {
        return true;
    }
    if lower.contains("/ipc/") || lower.ends_with("/ipc") {
        return true;
    }
    if lower.contains("/.tmp/") || lower.contains("/tmp/") && lower.contains("agentbuddy-backup") {
        // don't exclude our own staging via generic tmp
    }
    if lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
        || lower.contains(".sqlite-")
        || lower.ends_with(".db-wal")
        || lower.ends_with(".db-shm")
    {
        // agents.db under app is explicit include — only exclude when path has sessions etc.
        // For agent trees we exclude sqlite; app db is added as a single file not via walk.
        // App SQLite is added as a single explicit file, not via agent-tree walk.
        // Match both legacy ~/.agentbuddy and platform app-data locations.
        let is_app_db = s.contains(".agentbuddy/agents.db")
            || s.contains(".agentbuddy\\agents.db")
            || s.ends_with("AgentBuddy/agents.db")
            || s.ends_with("AgentBuddy\\agents.db")
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "agents.db")
                .unwrap_or(false)
                && s.to_ascii_lowercase().contains("agentbuddy");
        if !is_app_db {
            // walking agent dirs: exclude
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".sqlite")
                || name.ends_with(".sqlite3")
                || name.contains(".sqlite")
                || name == "history.jsonl"
            {
                return true;
            }
        }
    }
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "history.jsonl")
        .unwrap_or(false)
    {
        return true;
    }
    // exclude logs directory name components
    for comp in path.components() {
        if let std::path::Component::Normal(os) = comp {
            let n = os.to_string_lossy().to_ascii_lowercase();
            if matches!(
                n.as_str(),
                "logs" | "cache" | "sessions" | "ipc" | ".tmp" | "node_modules"
            ) {
                return true;
            }
        }
    }
    false
}

fn path_allowed_for_custom(path: &Path) -> bool {
    let Ok(canon) = path.canonicalize() else {
        // may not exist yet for conf path display; allow absolute / home-relative forms
        let s = path.to_string_lossy();
        if s.starts_with('~') {
            return true;
        }
        #[cfg(windows)]
        {
            // Drive-letter absolute path, e.g. C:\...
            let bytes = s.as_bytes();
            if bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && (bytes[2] == b'\\' || bytes[2] == b'/')
            {
                // Still block obvious system roots even before canonicalize
                let lower = s.to_ascii_lowercase().replace('/', "\\");
                if lower.starts_with("c:\\windows")
                    || lower.starts_with("c:\\program files")
                    || lower.starts_with("c:\\programdata")
                {
                    return false;
                }
                return true;
            }
            return false;
        }
        #[cfg(not(windows))]
        {
            return s.starts_with('/');
        }
    };
    if let Some(home) = dirs::home_dir() {
        if canon.starts_with(&home) {
            return true;
        }
    }
    // AgentBuddy app data (may sit outside home on Windows LOCALAPPDATA)
    if let Ok(app) = crate::config::app_dir() {
        if canon.starts_with(&app) {
            return true;
        }
    }
    #[cfg(windows)]
    {
        // Default deny system directories; allow nothing outside home/appdata unless
        // it is clearly under the user profile via env-expanded forms already covered.
        let s = canon.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
        if s.starts_with("c:\\windows")
            || s.contains("\\windows\\")
            || s.starts_with("c:\\program files")
            || s.starts_with("c:\\programdata")
        {
            return false;
        }
        // Allow LocalAppData / AppData trees for agent configs that live there.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if canon.starts_with(Path::new(&local)) {
                return true;
            }
        }
        if let Ok(roam) = std::env::var("APPDATA") {
            if canon.starts_with(Path::new(&roam)) {
                return true;
            }
        }
        return false;
    }
    #[cfg(not(windows))]
    {
        let allowed_prefixes = ["/opt/", "/usr/local/", "/etc/", "/private/etc/"];
        let s = canon.to_string_lossy();
        return allowed_prefixes.iter().any(|p| s.starts_with(p));
    }
}

// ===== Collect + run =====

pub fn run_backup_upload(
    app: AppHandle,
    payload: BackupRunPayload,
) -> Result<BackupRunResult, String> {
    let settings = config::load_backup_settings()?;
    let unit_ids: HashSet<String> = payload
        .unit_ids
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if unit_ids.is_empty() {
        return Err("请至少选择一个备份内容".to_string());
    }
    if payload.webdav_connection_ids.is_empty() {
        return Err("请至少选择一个 WebDAV 目标".to_string());
    }

    let upload_ids: Vec<String> = payload
        .webdav_connection_ids
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if upload_ids.is_empty() {
        return Err("请至少选择一个 WebDAV 目标".to_string());
    }

    let passphrase_preview = payload
        .passphrase
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // Overall steps: collect + zip + [encrypt] + each upload + finalize
    let total_steps: u32 = 2
        + if passphrase_preview.is_some() { 1 } else { 0 }
        + upload_ids.len() as u32
        + 1;
    let mut step: u32 = 0;
    let mut advance = |app: &AppHandle,
                       phase: &str,
                       message: String,
                       connection_id: Option<String>| {
        step = step.saturating_add(1);
        emit_backup_progress(
            app,
            BackupProgressEvent {
                phase: phase.to_string(),
                current: step.min(total_steps),
                total: total_steps,
                message,
                connection_id,
            },
        );
    };

    advance(
        &app,
        "collect",
        "正在收集备份文件…".into(),
        None,
    );

    let mut warnings = Vec::new();
    let entries = collect_entries(&unit_ids, &settings, &mut warnings)?;
    if entries.is_empty() {
        return Err("所选范围内没有可备份的文件".to_string());
    }

    let contains_secrets = entries.iter().any(|e| e.secrets)
        || unit_ids.iter().any(|id| {
            id.starts_with("app:agentbuddy")
                || id.starts_with("tool:")
                || id.starts_with("agent:")
        });

    let passphrase = passphrase_preview;

    if passphrase.is_none() && contains_secrets && !payload.acknowledge_plaintext_secrets {
        return Err(
            "备份包可能包含密钥（API Key / OAuth / secretsKey）。请设置口令加密，或勾选「我已知晓风险」后继续"
                .to_string(),
        );
    }

    // 文件名时间戳：yyyyMMddHHmmss（无分隔符，便于排序与去重）
    let stamp = Local::now().format("%Y%m%d%H%M%S").to_string();
    let base_name = format!("agentbuddy-backup-{}", stamp);
    let encrypted = passphrase.is_some();
    let archive_file_name = if encrypted {
        format!("{}.abenc", base_name)
    } else {
        format!("{}.zip", base_name)
    };

    advance(
        &app,
        "zip",
        format!(
            "正在打包 {} 个文件…",
            entries.len()
        ),
        None,
    );

    // staging
    let staging = std::env::temp_dir().join(format!(
        "agentbuddy-backup-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&staging, fs::Permissions::from_mode(0o700));
    }

    let zip_path = staging.join(format!("{}.zip", base_name));
    let unit_list: Vec<String> = unit_ids.iter().cloned().collect();
    build_zip(
        &zip_path,
        &entries,
        &unit_list,
        contains_secrets,
        encrypted,
        &warnings,
    )?;

    let final_path = if let Some(ref pass) = passphrase {
        advance(
            &app,
            "encrypt",
            "正在加密备份包…".into(),
            None,
        );
        let zip_bytes = fs::read(&zip_path).map_err(|e| format!("读取 zip 失败: {}", e))?;
        let enc = crypto::encrypt_backup_blob(pass, &zip_bytes)?;
        let out = staging.join(&archive_file_name);
        fs::write(&out, &enc).map_err(|e| format!("写入加密备份失败: {}", e))?;
        let _ = fs::remove_file(&zip_path);
        out
    } else {
        zip_path
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&final_path, fs::Permissions::from_mode(0o600));
    }

    let archive_bytes = file_size(&final_path);

    // remote dir: 用户指定的上传目录（默认 AgentBuddy）；不存在时由 WebDAV MKCOL 逐级创建。
    // 备份文件直接落在该目录下，不再分子目录。
    let prefix = payload
        .remote_prefix
        .as_ref()
        .map(|s| s.trim().trim_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let s = settings.default_remote_dir.trim().trim_matches('/').to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
        .unwrap_or_else(|| "AgentBuddy".to_string());
    // 拒绝路径穿越
    if prefix.split('/').any(|p| p == "..") {
        return Err("上传目录不允许包含 ..".to_string());
    }
    let rel_dir = prefix;

    let mut targets = Vec::new();
    for (idx, cid) in upload_ids.iter().enumerate() {
        let display_name = db::get_webdav_connection_row(cid)
            .ok()
            .flatten()
            .map(|r| r.name)
            .unwrap_or_else(|| cid.clone());
        advance(
            &app,
            "upload",
            format!(
                "正在上传到 {}（{}/{}）…",
                display_name,
                idx + 1,
                upload_ids.len()
            ),
            Some(cid.clone()),
        );
        match webdav::upload_file_for_connection(cid, &rel_dir, &archive_file_name, &final_path) {
            Ok((name, remote)) => {
                // Keep only the newest N archives in this remote dir.
                match webdav::prune_old_backups_for_connection(
                    cid,
                    &rel_dir,
                    webdav::BACKUP_REMOTE_KEEP,
                ) {
                    Ok(prune_warns) => {
                        for w in prune_warns {
                            warnings.push(format!("[{}] {}", name, w));
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("[{}] 清理旧备份失败: {}", name, e));
                    }
                }
                targets.push(BackupUploadTargetResult {
                    connection_id: cid.to_string(),
                    name,
                    ok: true,
                    message: format!(
                        "上传成功（远程仅保留最新 {} 份）",
                        webdav::BACKUP_REMOTE_KEEP
                    ),
                    remote_path: remote,
                });
            }
            Err(e) => {
                targets.push(BackupUploadTargetResult {
                    connection_id: cid.to_string(),
                    name: display_name,
                    ok: false,
                    message: e,
                    remote_path: String::new(),
                });
            }
        }
    }

    let any_ok = targets.iter().any(|t| t.ok);

    advance(
        &app,
        "finalize",
        "正在清理临时文件…".into(),
        None,
    );

    // Always drop staging — never keep a local copy of the archive.
    let _ = fs::remove_dir_all(&staging);

    let success_n = targets.iter().filter(|t| t.ok).count();
    let fail_n = targets.len() - success_n;
    let message = if any_ok && fail_n == 0 {
        format!(
            "备份完成：已上传到 {} 个 WebDAV（{}，{}；每目标最多保留 {} 份）",
            success_n,
            archive_file_name,
            format_bytes(archive_bytes),
            webdav::BACKUP_REMOTE_KEEP
        )
    } else if any_ok {
        format!(
            "备份部分成功：{}/{} 个目标上传成功（{}）",
            success_n,
            targets.len(),
            archive_file_name
        )
    } else {
        format!("备份已打包但全部上传失败（{}）", archive_file_name)
    };

    // Final tick: ensure bar reaches 100% with summary text
    emit_backup_progress(
        &app,
        BackupProgressEvent {
            phase: "finalize".into(),
            current: total_steps,
            total: total_steps,
            message: message.clone(),
            connection_id: None,
        },
    );

    Ok(BackupRunResult {
        ok: any_ok,
        archive_file_name,
        archive_bytes,
        encrypted,
        targets,
        warnings,
        message,
    })
}

// ===== Remote list + restore =====

fn resolve_remote_prefix(
    payload_prefix: &Option<String>,
    settings: &BackupSettings,
) -> Result<String, String> {
    let prefix = payload_prefix
        .as_ref()
        .map(|s| s.trim().trim_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let s = settings.default_remote_dir.trim().trim_matches('/').to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
        .unwrap_or_else(|| "AgentBuddy".to_string());
    if prefix.split('/').any(|p| p == "..") {
        return Err("上传目录不允许包含 ..".to_string());
    }
    Ok(prefix)
}

pub fn list_remote_backups(
    payload: ListRemoteBackupsPayload,
) -> Result<Vec<RemoteBackupItem>, String> {
    let settings = config::load_backup_settings()?;
    let rel_dir = resolve_remote_prefix(&payload.remote_prefix, &settings)?;
    let cid = payload.connection_id.trim();
    if cid.is_empty() {
        return Err("请选择 WebDAV 连接".to_string());
    }
    let entries = webdav::list_remote_dir_for_connection(cid, &rel_dir)?;
    let mut items: Vec<RemoteBackupItem> = entries
        .into_iter()
        .filter(|e| !e.is_collection && webdav::is_agentbuddy_backup_name(&e.name))
        .map(|e| RemoteBackupItem {
            encrypted: e.name.ends_with(".abenc"),
            name: e.name,
            bytes: e.bytes,
            last_modified: e.last_modified,
        })
        .collect();
    // Newest first (filename stamp yyyyMMddHHmmss)
    items.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(items)
}

/// Expand `~/…` and leave absolute paths as-is.
fn expand_origin_path(origin: &str) -> Result<PathBuf, String> {
    let o = origin.trim();
    if o.is_empty() {
        return Err("manifest 条目 origin 为空".to_string());
    }
    if let Some(rest) = o.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| "无法解析主目录".to_string())?;
        return Ok(home.join(rest));
    }
    if o == "~" {
        return dirs::home_dir().ok_or_else(|| "无法解析主目录".to_string());
    }
    Ok(PathBuf::from(o))
}

/// Safety: restored absolute paths must stay under $HOME / /opt / /usr/local / /etc.
fn restore_path_allowed(path: &Path) -> bool {
    // Prefer checking the would-be absolute path; if parent does not exist yet,
    // walk up to an existing ancestor and re-join the relative tail.
    let candidate = match path.parent() {
        None => path.to_path_buf(),
        Some(parent) if parent.as_os_str().is_empty() => path.to_path_buf(),
        Some(parent) if parent.exists() => path.to_path_buf(),
        Some(parent) => {
            let mut cur = parent.to_path_buf();
            let mut tail = Vec::new();
            while !cur.exists() {
                if let Some(name) = cur.file_name().map(|s| s.to_os_string()) {
                    tail.push(name);
                    cur.pop();
                } else {
                    break;
                }
            }
            if let Ok(base) = cur.canonicalize() {
                let mut out = base;
                for part in tail.into_iter().rev() {
                    out.push(part);
                }
                if let Some(name) = path.file_name() {
                    out.push(name);
                }
                out
            } else {
                path.to_path_buf()
            }
        }
    };
    path_allowed_for_custom(&candidate)
}

pub fn restore_remote_backup(
    app: AppHandle,
    payload: RestoreBackupPayload,
) -> Result<RestoreBackupResult, String> {
    let settings = config::load_backup_settings()?;
    let rel_dir = resolve_remote_prefix(&payload.remote_prefix, &settings)?;
    let cid = payload.connection_id.trim().to_string();
    if cid.is_empty() {
        return Err("请选择 WebDAV 连接".to_string());
    }
    let file_name = payload.file_name.trim().to_string();
    if !webdav::is_agentbuddy_backup_name(&file_name) {
        return Err("不是有效的 AgentBuddy 备份文件名".to_string());
    }
    let is_enc = file_name.ends_with(".abenc");
    let passphrase = payload
        .passphrase
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if is_enc && passphrase.is_none() {
        return Err("该备份已加密，请填写备份口令".to_string());
    }

    let total_steps = if is_enc { 4u32 } else { 3u32 };
    let mut step = 0u32;
    let mut advance = |phase: &str, message: String| {
        step = step.saturating_add(1);
        emit_backup_progress(
            &app,
            BackupProgressEvent {
                phase: phase.to_string(),
                current: step.min(total_steps),
                total: total_steps,
                message,
                connection_id: Some(cid.clone()),
            },
        );
    };

    advance("download", format!("正在下载 {}…", file_name));

    let staging = std::env::temp_dir().join(format!(
        "agentbuddy-restore-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&staging, fs::Permissions::from_mode(0o700));
    }

    let archive_path = staging.join(&file_name);
    let download_result = (|| {
        webdav::download_file_for_connection(&cid, &rel_dir, &file_name, &archive_path)?;
        let zip_path = if is_enc {
            advance("decrypt", "正在解密备份包…".into());
            let pass = passphrase.as_ref().unwrap();
            let blob = fs::read(&archive_path).map_err(|e| format!("读取加密备份失败: {}", e))?;
            let plain = crypto::decrypt_backup_blob(pass, &blob)
                .map_err(|e| format!("解密失败（口令可能不正确）: {}", e))?;
            let zp = staging.join("restored.zip");
            fs::write(&zp, &plain).map_err(|e| format!("写入解密 zip 失败: {}", e))?;
            zp
        } else {
            archive_path.clone()
        };

        advance("restore", "正在还原文件…".into());
        let (restored, skipped, mut warnings) = extract_backup_zip(&zip_path)?;
        advance("finalize", "正在清理临时文件…".into());
        let _ = fs::remove_dir_all(&staging);

        let message = if restored == 0 {
            "未还原任何文件（包内可能无有效条目）".to_string()
        } else {
            format!(
                "恢复完成：已还原 {} 个文件{}",
                restored,
                if skipped > 0 {
                    format!("，跳过 {}", skipped)
                } else {
                    String::new()
                }
            )
        };
        if skipped > 0 {
            warnings.push(format!("有 {} 个文件因路径安全限制或写失败被跳过", skipped));
        }
        emit_backup_progress(
            &app,
            BackupProgressEvent {
                phase: "finalize".into(),
                current: total_steps,
                total: total_steps,
                message: message.clone(),
                connection_id: Some(cid.clone()),
            },
        );
        Ok(RestoreBackupResult {
            ok: restored > 0,
            message,
            restored_files: restored,
            skipped_files: skipped,
            warnings,
        })
    })();

    if download_result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    download_result
}

fn extract_backup_zip(zip_path: &Path) -> Result<(u32, u32, Vec<String>), String> {
    let file = File::open(zip_path).map_err(|e| format!("打开备份 zip 失败: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {}", e))?;

    // Prefer manifest origins when present.
    let mut origin_map: HashMap<String, String> = HashMap::new();
    if let Ok(mut mf) = archive.by_name("manifest.json") {
        let mut s = String::new();
        if mf.read_to_string(&mut s).is_ok() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(sources) = v.get("sources").and_then(|x| x.as_array()) {
                    for src in sources {
                        if let Some(items) = src.get("items").and_then(|x| x.as_array()) {
                            for item in items {
                                let path = item
                                    .get("path")
                                    .and_then(|p| p.as_str())
                                    .unwrap_or("");
                                let origin = item
                                    .get("origin")
                                    .and_then(|p| p.as_str())
                                    .unwrap_or("");
                                if !path.is_empty() && !origin.is_empty() {
                                    origin_map.insert(path.to_string(), origin.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut restored = 0u32;
    let mut skipped = 0u32;
    let mut warnings = Vec::new();

    // Re-open archive after reading manifest (ZipFile borrows).
    let file = File::open(zip_path).map_err(|e| format!("打开备份 zip 失败: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let name = entry.name().to_string();
        if name == "manifest.json" || name.ends_with('/') {
            continue;
        }
        // Only restore files/* layout
        if !name.starts_with("files/") && !origin_map.contains_key(&name) {
            // still try if origin_map has it
            if !origin_map.contains_key(&name) {
                continue;
            }
        }

        let dest = if let Some(origin) = origin_map.get(&name) {
            match expand_origin_path(origin) {
                Ok(p) => p,
                Err(e) => {
                    skipped += 1;
                    warnings.push(format!("跳过 {}: {}", name, e));
                    continue;
                }
            }
        } else {
            // Fallback: strip files/ prefix and place under home (should be rare)
            skipped += 1;
            warnings.push(format!("跳过 {}：manifest 中无 origin 映射", name));
            continue;
        };

        if !restore_path_allowed(&dest) {
            skipped += 1;
            warnings.push(format!(
                "跳过 {}：目标路径不在允许范围内（{}）",
                name,
                dest.display()
            ));
            continue;
        }

        if let Some(parent) = dest.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                skipped += 1;
                warnings.push(format!("跳过 {}：创建目录失败 {}", name, e));
                continue;
            }
        }

        let mut out = match File::create(&dest) {
            Ok(f) => f,
            Err(e) => {
                skipped += 1;
                warnings.push(format!("跳过 {}：写入失败 {}", name, e));
                continue;
            }
        };
        if std::io::copy(&mut entry, &mut out).is_err() {
            skipped += 1;
            warnings.push(format!("跳过 {}：复制数据失败", name));
            let _ = fs::remove_file(&dest);
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Prefer private mode for likely secret files
            let mode = if dest.ends_with("auth.json")
                || dest.ends_with("config.json")
                || dest.ends_with(".abenc")
            {
                0o600
            } else {
                0o644
            };
            let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(mode));
        }
        restored += 1;
    }

    Ok((restored, skipped, warnings))
}

fn collect_entries(
    unit_ids: &HashSet<String>,
    settings: &BackupSettings,
    warnings: &mut Vec<String>,
) -> Result<Vec<FileEntry>, String> {
    let mut out = Vec::new();
    let expanded = expand_unit_ids(unit_ids);

    for id in &expanded {
        match id.as_str() {
            "app:agentbuddy:config" | "app:agentbuddy" if unit_ids.contains("app:agentbuddy") || unit_ids.contains("app:agentbuddy:config") => {
                // handled below via flags
            }
            _ => {}
        }
    }

    // App
    if expanded.iter().any(|id| id.starts_with("app:agentbuddy")) {
        let dir = config::app_dir()?;
        if unit_selected(&expanded, "app:agentbuddy")
            || unit_selected(&expanded, "app:agentbuddy:config")
        {
            push_file(
                &mut out,
                &dir.join("config.json"),
                "files/app/agentbuddy/config.json",
                "app:agentbuddy:config",
                true,
                warnings,
            );
        }
        if unit_selected(&expanded, "app:agentbuddy")
            || unit_selected(&expanded, "app:agentbuddy:db")
        {
            push_file(
                &mut out,
                &dir.join("agents.db"),
                "files/app/agentbuddy/agents.db",
                "app:agentbuddy:db",
                true,
                warnings,
            );
        }
        if unit_selected(&expanded, "app:agentbuddy")
            || unit_selected(&expanded, "app:agentbuddy:skills")
        {
            push_dir(
                &mut out,
                &dir.join("skills"),
                "files/app/agentbuddy/skills",
                "app:agentbuddy:skills",
                false,
                warnings,
            );
        }
    }

    // cliproxy
    if unit_selected(&expanded, "tool:cliproxyapi") {
        if let Some(conf) = resolve_cliproxy_conf(settings) {
            if !path_allowed_for_custom(&conf) && conf.exists() {
                warnings.push(format!("cliproxy 配置路径不在允许范围内，已跳过: {}", display_path(&conf)));
            } else {
                push_file(
                    &mut out,
                    &conf,
                    "files/tool/cliproxyapi/cliproxyapi.conf",
                    "tool:cliproxyapi",
                    true,
                    warnings,
                );
                let auth = read_auth_dir_from_conf(&conf)
                    .or_else(|| dirs::home_dir().map(|h| h.join(".cli-proxy-api")));
                if let Some(auth_dir) = auth {
                    if auth_dir.is_dir() {
                        push_dir(
                            &mut out,
                            &auth_dir,
                            "files/tool/cliproxyapi/auth-dir",
                            "tool:cliproxyapi",
                            true,
                            warnings,
                        );
                    }
                }
            }
        }
    }

    // sub2api
    if unit_selected(&expanded, "tool:sub2api") {
        if let Some(root) = resolve_sub2api_root(settings) {
            if !path_allowed_for_custom(&root) && root.exists() {
                warnings.push(format!("sub2api 路径不在允许范围内，已跳过: {}", display_path(&root)));
            } else {
                let conf = if root.join("config.yaml").is_file() {
                    root.join("config.yaml")
                } else if root.join("config.yml").is_file() {
                    root.join("config.yml")
                } else {
                    root.join("config.yaml")
                };
                push_file(
                    &mut out,
                    &conf,
                    "files/tool/sub2api/config.yaml",
                    "tool:sub2api",
                    true,
                    warnings,
                );
                let data = root.join("data");
                if data.is_dir() {
                    push_dir(
                        &mut out,
                        &data,
                        "files/tool/sub2api/data",
                        "tool:sub2api",
                        false,
                        warnings,
                    );
                }
            }
        } else {
            warnings.push("sub2api 未找到可备份路径".into());
        }
    }

    // Agents
    collect_agent_entries(&expanded, &mut out, warnings)?;

    // Dedupe by archive_path
    let mut seen = HashSet::new();
    out.retain(|e| seen.insert(e.archive_path.clone()));
    Ok(out)
}

fn unit_selected(expanded: &HashSet<String>, id: &str) -> bool {
    expanded.contains(id)
}

fn expand_unit_ids(unit_ids: &HashSet<String>) -> HashSet<String> {
    let mut out = unit_ids.clone();
    // Parent selects all children semantics for collection
    if unit_ids.contains("app:agentbuddy") {
        out.insert("app:agentbuddy:config".into());
        out.insert("app:agentbuddy:db".into());
        out.insert("app:agentbuddy:skills".into());
    }
    if unit_ids.contains("agent:claude-code") {
        // mark all env ids that exist — collect will also check agent:claude-code:env:*
        out.insert("agent:claude-code:env:default".into());
        if let Ok(envs) = db::load_claude_environment_rows() {
            for e in envs {
                if e.id != "default" && !e.is_default {
                    out.insert(format!("agent:claude-code:env:{}", e.id));
                }
            }
        }
    }
    if unit_ids.contains("agent:codex") {
        out.insert("agent:codex:env:default".into());
        if let Ok(envs) = db::load_codex_environment_rows() {
            for e in envs {
                if e.id != "default" && !e.is_default {
                    out.insert(format!("agent:codex:env:{}", e.id));
                }
            }
        }
    }
    if unit_ids.contains("agents") {
        for spec in agents::agents() {
            out.insert(format!("agent:{}", spec.name));
        }
        // expand multi-env parents
        out.insert("agent:claude-code".into());
        out.insert("agent:codex".into());
        return expand_unit_ids(&out);
    }
    out
}

fn collect_agent_entries(
    expanded: &HashSet<String>,
    out: &mut Vec<FileEntry>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    // Claude envs
    for id in expanded.iter().filter(|id| id.starts_with("agent:claude-code:env:")) {
        let env_id = id.strip_prefix("agent:claude-code:env:").unwrap_or("default");
        collect_claude_env(env_id, out, warnings);
    }

    // Codex envs
    for id in expanded.iter().filter(|id| id.starts_with("agent:codex:env:")) {
        let env_id = id.strip_prefix("agent:codex:env:").unwrap_or("default");
        collect_codex_env(env_id, out, warnings);
    }

    // Generic agents (non multi-env parents without :env:)
    for id in expanded.iter().filter(|id| {
        id.starts_with("agent:")
            && !id.contains(":env:")
            && *id != "agent:claude-code"
            && *id != "agent:codex"
    }) {
        let name = id.strip_prefix("agent:").unwrap_or("");
        if name.is_empty() {
            continue;
        }
        collect_generic_agent(name, out, warnings);
    }

    Ok(())
}

fn collect_claude_env(env_id: &str, out: &mut Vec<FileEntry>, warnings: &mut Vec<String>) {
    let unit = format!("agent:claude-code:env:{}", env_id);
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };

    if env_id == "default" {
        let config_dir = home.join(".claude");
        push_file(
            out,
            &home.join(".claude.json"),
            "files/agent/claude-code/default/claude.json",
            &unit,
            true,
            warnings,
        );
        push_file(
            out,
            &config_dir.join("settings.json"),
            "files/agent/claude-code/default/settings.json",
            &unit,
            true,
            warnings,
        );
        push_dir(
            out,
            &config_dir.join("skills"),
            "files/agent/claude-code/default/skills",
            &unit,
            false,
            warnings,
        );
        return;
    }

    let row = db::get_claude_environment_row(env_id).ok().flatten();
    let config_dir = match row {
        Some(r) => PathBuf::from(r.config_dir),
        None => {
            warnings.push(format!("Claude 环境 {} 不存在", env_id));
            return;
        }
    };
    let slug = env_id;
    push_file(
        out,
        &config_dir.join(".claude.json"),
        &format!("files/agent/claude-code/{}/claude.json", slug),
        &unit,
        true,
        warnings,
    );
    push_file(
        out,
        &config_dir.join("settings.json"),
        &format!("files/agent/claude-code/{}/settings.json", slug),
        &unit,
        true,
        warnings,
    );
    push_dir(
        out,
        &config_dir.join("skills"),
        &format!("files/agent/claude-code/{}/skills", slug),
        &unit,
        false,
        warnings,
    );
}

fn collect_codex_env(env_id: &str, out: &mut Vec<FileEntry>, warnings: &mut Vec<String>) {
    let unit = format!("agent:codex:env:{}", env_id);
    let config_dir = if env_id == "default" {
        match dirs::home_dir() {
            Some(h) => h.join(".codex"),
            None => return,
        }
    } else {
        match db::get_codex_environment_row(env_id).ok().flatten() {
            Some(r) => PathBuf::from(r.config_dir),
            None => {
                warnings.push(format!("Codex 环境 {} 不存在", env_id));
                return;
            }
        }
    };
    let slug = env_id;
    for name in ["config.toml", "AGENTS.md", "auth.json"] {
        push_file(
            out,
            &config_dir.join(name),
            &format!("files/agent/codex/{}/{}", slug, name),
            &unit,
            name == "auth.json" || name == "config.toml",
            warnings,
        );
    }
    push_dir(
        out,
        &config_dir.join("skills"),
        &format!("files/agent/codex/{}/skills", slug),
        &unit,
        false,
        warnings,
    );
}

fn collect_generic_agent(name: &str, out: &mut Vec<FileEntry>, warnings: &mut Vec<String>) {
    let unit = format!("agent:{}", name);
    // 历史兼容：旧备份/配置可能引用已移除的 codebuddy（与 codebuddy-cn 同一物理根 ~/.codebuddy）
    let Some(spec) = agents::find(name).or_else(|| {
        if name == "codebuddy" {
            agents::find("codebuddy-cn")
        } else {
            None
        }
    }) else {
        warnings.push(format!("未知 Agent: {}", name));
        return;
    };

    let archive_root = format!("files/agent/{}", name);

    if let Ok(mcp) = mcp_config::resolve_mcp_path(spec.name) {
        let file_name = mcp
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mcp.json");
        push_file(
            out,
            &mcp,
            &format!("{}/{}", archive_root, file_name),
            &unit,
            true,
            warnings,
        );
    }

    for skill_rel in spec.skills_roots {
        if let Some(sk) = expand_home(skill_rel) {
            if sk.is_dir() {
                push_dir(
                    out,
                    &sk,
                    &format!("{}/skills", archive_root),
                    &unit,
                    false,
                    warnings,
                );
                break; // first skills root only for write-shaped backup
            }
        }
    }

    // OpenCode config dir extras
    if matches!(spec.name, "opencode" | "deveco-code") {
        if let Some(auth) = opencode_auth_path(spec.name) {
            push_file(
                out,
                &auth,
                &format!("{}/auth.json", archive_root),
                &unit,
                true,
                warnings,
            );
        }
        // also pack full opencode.json if different from mcp resolve
        if let Ok(cfg) = mcp_config::resolve_mcp_path(spec.name) {
            // already added as mcp file name
            let _ = cfg;
        }
    }

    // antigravity skills second root — already handled by first existing skills_roots
}

fn push_file(
    out: &mut Vec<FileEntry>,
    source: &Path,
    archive_path: &str,
    unit_id: &str,
    secrets: bool,
    warnings: &mut Vec<String>,
) {
    if !source.is_file() {
        return;
    }
    if is_excluded(source) {
        return;
    }
    let meta = match fs::metadata(source) {
        Ok(m) => m,
        Err(e) => {
            warnings.push(format!("无法读取 {}: {}", display_path(source), e));
            return;
        }
    };
    if meta.len() > MAX_FILE_BYTES {
        warnings.push(format!(
            "跳过过大文件（>{}）: {}",
            format_bytes(MAX_FILE_BYTES),
            display_path(source)
        ));
        return;
    }
    // skip symlink files (fs::metadata follows links; use symlink_metadata)
    if fs::symlink_metadata(source)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        warnings.push(format!("跳过符号链接: {}", display_path(source)));
        return;
    }
    out.push(FileEntry {
        source: source.to_path_buf(),
        archive_path: archive_path.to_string(),
        origin: display_path(source),
        unit_id: unit_id.to_string(),
        bytes: meta.len(),
        secrets,
    });
}

fn push_dir(
    out: &mut Vec<FileEntry>,
    source_dir: &Path,
    archive_prefix: &str,
    unit_id: &str,
    secrets: bool,
    warnings: &mut Vec<String>,
) {
    if !source_dir.is_dir() {
        return;
    }
    for entry in WalkDir::new(source_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_excluded(path) {
            continue;
        }
        let rel = match path.strip_prefix(source_dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        let archive_path = format!("{}/{}", archive_prefix.trim_end_matches('/'), rel_s);
        push_file(out, path, &archive_path, unit_id, secrets, warnings);
    }
}

fn build_zip(
    zip_path: &Path,
    entries: &[FileEntry],
    unit_ids: &[String],
    contains_secrets: bool,
    encrypted_flag: bool,
    warnings: &[String],
) -> Result<(), String> {
    let file = File::create(zip_path).map_err(|e| format!("创建 zip 失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(zip_path, fs::Permissions::from_mode(0o600));
    }

    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut checksums = HashMap::new();
    let mut sources_map: HashMap<String, ManifestSource> = HashMap::new();

    for entry in entries {
        let mut f = match File::open(&entry.source) {
            Ok(f) => f,
            Err(e) => {
                // skip unreadable
                let _ = e;
                continue;
            }
        };
        let mut data = Vec::new();
        if f.read_to_end(&mut data).is_err() {
            continue;
        }
        let hash = Sha256::digest(&data);
        checksums.insert(
            entry.archive_path.clone(),
            format!("sha256:{:x}", hash),
        );

        zip.start_file(&entry.archive_path, opts)
            .map_err(|e| format!("写入 zip 条目失败: {}", e))?;
        zip.write_all(&data)
            .map_err(|e| format!("写入 zip 数据失败: {}", e))?;

        let src = sources_map
            .entry(entry.unit_id.clone())
            .or_insert_with(|| ManifestSource {
                id: entry.unit_id.clone(),
                label: entry.unit_id.clone(),
                agent_name: entry
                    .unit_id
                    .strip_prefix("agent:")
                    .map(|s| s.split(':').next().unwrap_or(s).to_string()),
                env_id: entry.unit_id.split(":env:").nth(1).map(|s| s.to_string()),
                items: vec![],
            });
        src.items.push(ManifestItem {
            path: entry.archive_path.clone(),
            origin: entry.origin.clone(),
            bytes: entry.bytes,
        });
    }

    let manifest = Manifest {
        format_version: 1,
        app_version: APP_VERSION.to_string(),
        created_at: Local::now().to_rfc3339(),
        platform: "macos".into(),
        encrypted: encrypted_flag,
        unit_ids: unit_ids.to_vec(),
        contains_secrets,
        sources: sources_map.into_values().collect(),
        exclusions: vec![
            "**/logs/**".into(),
            "**/cache/**".into(),
            "**/sessions/**".into(),
            "**/*.sqlite*".into(),
            "**/history.jsonl".into(),
        ],
        warnings: warnings.to_vec(),
        checksums,
    };

    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("序列化 manifest 失败: {}", e))?;
    zip.start_file("manifest.json", opts)
        .map_err(|e| format!("写入 manifest 失败: {}", e))?;
    zip.write_all(&manifest_json)
        .map_err(|e| format!("写入 manifest 失败: {}", e))?;

    zip.finish()
        .map_err(|e| format!("完成 zip 失败: {}", e))?;
    Ok(())
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let f = n as f64;
    if f >= MB {
        format!("{:.1} MB", f / MB)
    } else if f >= KB {
        format!("{:.1} KB", f / KB)
    } else {
        format!("{} B", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_logs_and_sessions() {
        assert!(is_excluded(Path::new("/tmp/foo/logs/a.txt")));
        assert!(is_excluded(Path::new("/Users/x/.codex/sessions/1.jsonl")));
        assert!(is_excluded(Path::new("/Users/x/.codex/history.jsonl")));
        assert!(!is_excluded(Path::new("/Users/x/.codex/config.toml")));
    }

    #[test]
    fn expand_parent_app() {
        let mut s = HashSet::new();
        s.insert("app:agentbuddy".into());
        let e = expand_unit_ids(&s);
        assert!(e.contains("app:agentbuddy:config"));
        assert!(e.contains("app:agentbuddy:db"));
        assert!(e.contains("app:agentbuddy:skills"));
    }
}
