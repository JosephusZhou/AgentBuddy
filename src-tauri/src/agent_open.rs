//! Open an agent's config directory / config files in the system file manager
//! or default app.
//!
//! Commands only accept agent `name` + file kind — never an arbitrary path from the UI —
//! so the frontend cannot coerce the shell into opening untrusted locations.

use crate::platform;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// What the Agent 页 card can open for a given agent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOpenTargets {
    /// Primary config root (may be absent for agents with no known dir).
    pub config_dir: Option<String>,
    /// MCP config file resolved via `mcp_config` (registry agents only).
    pub mcp_file: Option<String>,
    /// Main settings file when it is distinct from the MCP file (today: claude-code).
    pub settings_file: Option<String>,
    /// Global prompt file (CLAUDE.md / AGENTS.md / GEMINI.md) inside the config dir.
    /// None for agents that don't support global prompt editing.
    pub prompt_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOpenResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFileKind {
    Mcp,
    Settings,
}

impl ConfigFileKind {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "mcp" => Ok(Self::Mcp),
            "settings" => Ok(Self::Settings),
            other => Err(format!("未知配置文件类型: {other}（期望 mcp 或 settings）")),
        }
    }
}

fn home_dir() -> Result<PathBuf, String> {
    platform::home_dir()
}

fn expand_tilde(path: &str) -> String {
    platform::expand_tilde_lossy(path)
}

fn display_path(abs: &str) -> String {
    platform::display_path(abs)
}

fn path_key(path: &Path) -> String {
    // Best-effort normalize for equality: expand + canonicalize when possible.
    let s = path.to_string_lossy().to_string();
    if let Ok(c) = path.canonicalize() {
        return c.to_string_lossy().to_string();
    }
    s
}

/// Resolve the preferred config directory for `name`.
///
/// Priority:
/// 1. Cached sniff `config_dirs[0]` (covers Claude Desktop scan + manual agents)
/// 2. Registry `config_paths[0]` expanded (even if the dir does not exist yet — open will fail later)
/// 3. Parent of the resolved MCP file (Claude Desktop when cache is empty)
fn resolve_config_dir(name: &str) -> Option<PathBuf> {
    if let Ok(agents) = crate::db::load_agents() {
        if let Some(row) = agents.iter().find(|a| a.name == name) {
            if let Some(dir) = row.config_dirs.first() {
                let expanded = expand_tilde(dir);
                if !expanded.is_empty() {
                    return Some(PathBuf::from(expanded));
                }
            }
        }
    }

    if let Some(spec) = crate::agents::find(name) {
        if let Some(p) = spec.config_paths.first() {
            return Some(PathBuf::from(expand_tilde(p)));
        }
        // Claude Desktop has empty config_paths and relies on Application Support scan.
        if spec.scan_app_support {
            if let Ok(mcp) = crate::mcp_config::resolve_mcp_path(name) {
                if let Some(parent) = mcp.parent() {
                    return Some(parent.to_path_buf());
                }
            }
        }
    }

    None
}

/// Main settings path when it is intentionally separate from the MCP file.
/// Only Claude Code today: MCP lives at `~/.claude.json`, settings at `~/.claude/settings.json`.
fn resolve_settings_file(name: &str) -> Option<PathBuf> {
    if name == "claude-code" {
        return home_dir()
            .ok()
            .map(|h| h.join(".claude").join("settings.json"));
    }
    None
}

/// Compute open targets. Paths are absolute strings; missing files still appear so the UI
/// can show the button and let open return a clear "does not exist" error.
pub fn open_targets(name: &str) -> AgentOpenTargets {
    let config_dir = resolve_config_dir(name).map(|p| p.to_string_lossy().to_string());

    let mcp_file = if crate::agents::find(name).is_some() {
        crate::mcp_config::resolve_mcp_path(name)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    let settings_file = resolve_settings_file(name).and_then(|settings| {
        // Collapse when settings and MCP resolve to the same physical path.
        if let Some(ref mcp) = mcp_file {
            if path_key(Path::new(mcp)) == path_key(&settings) {
                return None;
            }
        }
        Some(settings.to_string_lossy().to_string())
    });

    // Global prompt file: config_dir + prompt_file name from registry.
    let prompt_file = config_dir.as_ref().and_then(|dir| {
        crate::agents::find(name).and_then(|spec| {
            spec.prompt_file
                .map(|fname| Path::new(dir).join(fname).to_string_lossy().to_string())
        })
    });

    AgentOpenTargets {
        config_dir,
        mcp_file,
        settings_file,
        prompt_file,
    }
}

fn open_existing_path(path: &Path, kind_label: &str) -> Result<AgentOpenResult, String> {
    if !path.exists() {
        return Err(format!(
            "{kind_label}不存在: {}",
            display_path(&path.to_string_lossy())
        ));
    }

    platform::open_path(path).map_err(|e| format!("打开{kind_label}失败: {e}"))?;

    Ok(AgentOpenResult {
        ok: true,
        message: format!(
            "已打开 {}: {}",
            kind_label,
            display_path(&path.to_string_lossy())
        ),
    })
}

pub fn reveal_config_dir(name: String) -> Result<AgentOpenResult, String> {
    let dir =
        resolve_config_dir(&name).ok_or_else(|| format!("Agent「{name}」没有可打开的配置目录"))?;
    open_existing_path(&dir, "配置目录")
}

pub fn open_config_file(name: String, kind: String) -> Result<AgentOpenResult, String> {
    let kind = ConfigFileKind::parse(&kind)?;
    let targets = open_targets(&name);
    let (path, label) = match kind {
        ConfigFileKind::Mcp => {
            let p = targets
                .mcp_file
                .ok_or_else(|| format!("Agent「{name}」没有 MCP 配置文件"))?;
            (PathBuf::from(p), "MCP 配置文件")
        }
        ConfigFileKind::Settings => {
            let p = targets
                .settings_file
                .ok_or_else(|| format!("Agent「{name}」没有独立的主配置文件"))?;
            (PathBuf::from(p), "主配置文件")
        }
    };
    open_existing_path(&path, label)
}

/// Resolve the global prompt file path for `name`.
/// Returns Err when the agent has no known prompt file.
fn resolve_prompt_path(name: &str) -> Result<PathBuf, String> {
    let targets = open_targets(name);
    let path = targets
        .prompt_file
        .ok_or_else(|| format!("Agent「{name}」不支持全局提示词编辑"))?;
    Ok(PathBuf::from(path))
}

/// Read the global prompt file content for `name`.
/// Returns empty string when the file does not exist yet.
pub fn read_agent_prompt(name: &str) -> Result<String, String> {
    let path = resolve_prompt_path(name)?;
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(|e| format!("读取全局提示词失败: {e}"))
}

/// Write the global prompt file content for `name`.
/// Creates parent dirs and the file itself if missing.
/// Uses temp-file + rename for atomic write.
pub fn write_agent_prompt(name: &str, content: &str) -> Result<AgentOpenResult, String> {
    let path = resolve_prompt_path(name)?;

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }

    // Atomic write: temp file in same dir + rename.
    let tmp = path.with_extension(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{e}.tmp"))
            .unwrap_or_else(|| "tmp".to_string()),
    );
    std::fs::write(&tmp, content).map_err(|e| format!("写入临时文件失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换提示词文件失败: {e}"))?;

    Ok(AgentOpenResult {
        ok: true,
        message: format!(
            "已保存全局提示词: {}",
            display_path(&path.to_string_lossy())
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_kind_parse() {
        assert_eq!(ConfigFileKind::parse("mcp").unwrap(), ConfigFileKind::Mcp);
        assert_eq!(
            ConfigFileKind::parse("Settings").unwrap(),
            ConfigFileKind::Settings
        );
        assert!(ConfigFileKind::parse("other").is_err());
    }

    #[test]
    fn claude_code_has_distinct_settings() {
        // 持 HOME 测试锁：home_dir 与 open_targets 都依赖 HOME 派生路径，
        // 两次读取之间不能被其它换 HOME 的测试穿插（约定见 config.rs TEST_HOME_LOCK）。
        let _home_guard = crate::config::lock_home_for_test();
        let home = dirs::home_dir().expect("home");
        let targets = open_targets("claude-code");
        assert_eq!(
            targets.mcp_file.as_deref(),
            Some(home.join(".claude.json").to_string_lossy().as_ref())
        );
        assert_eq!(
            targets.settings_file.as_deref(),
            Some(
                home.join(".claude")
                    .join("settings.json")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            targets.config_dir.as_deref(),
            Some(home.join(".claude").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn codex_settings_collapsed_into_mcp() {
        let home = dirs::home_dir().expect("home");
        let targets = open_targets("codex");
        assert_eq!(
            targets.mcp_file.as_deref(),
            Some(
                home.join(".codex")
                    .join("config.toml")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert!(targets.settings_file.is_none());
        assert_eq!(
            targets.config_dir.as_deref(),
            Some(home.join(".codex").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn unknown_manual_agent_has_no_mcp() {
        let targets = open_targets("my-custom-agent-xyz");
        assert!(targets.mcp_file.is_none());
        assert!(targets.settings_file.is_none());
    }
}
