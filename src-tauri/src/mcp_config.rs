//! Apply / remove MCP server entries in each agent's config file.
//! Spec: AGENT_MCP_SKILLS_MAP.md

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

// 方言/路径规格来自集中注册表；别名 `Dialect` 让下方现有分派保持不变。
use crate::agents::{McpDialect as Dialect, McpPath};

/* ===== Public types ===== */

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDraft {
    pub title: String,
    /// "stdio" | "http" | "sse"
    #[serde(rename = "type")]
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpResult {
    pub agent: String,
    pub path: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpBatchResult {
    pub results: Vec<AgentMcpResult>,
    pub all_ok: bool,
}

/// Unified MCP server record for UI / DB (matches frontend McpServer fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerRecord {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub applied_agents: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSniffResult {
    pub servers: Vec<McpServerRecord>,
    pub scanned_agents: usize,
    pub found_entries: usize,
    pub message: String,
}

/// Result of a runtime connectivity probe against one MCP draft.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTestResult {
    pub ok: bool,
    pub message: String,
    /// Short excerpt of the server's response / error, for the UI to show.
    pub detail: String,
}

/* ===== Public API ===== */

pub fn apply_mcp_to_agents(draft: &McpDraft, agents: &[String]) -> McpBatchResult {
    let title = draft.title.trim();
    if title.is_empty() {
        return McpBatchResult {
            results: vec![AgentMcpResult {
                agent: "*".into(),
                path: String::new(),
                ok: false,
                message: "MCP 标题不能为空".into(),
            }],
            all_ok: false,
        };
    }

    let targets = dedupe_write_targets(agents);
    let mut results = Vec::new();

    for target in targets {
        let res = match apply_one(&target.agent, title, draft) {
            Ok(path) => AgentMcpResult {
                agent: target.agent,
                path: path.display().to_string(),
                ok: true,
                message: "已写入".into(),
            },
            Err(e) => AgentMcpResult {
                agent: target.agent,
                path: e.path.unwrap_or_default(),
                ok: false,
                message: e.message,
            },
        };
        results.push(res);
    }

    let all_ok = results.iter().all(|r| r.ok);
    McpBatchResult { results, all_ok }
}

pub fn remove_mcp_from_agents(title: &str, agents: &[String]) -> McpBatchResult {
    let title = title.trim();
    if title.is_empty() {
        return McpBatchResult {
            results: vec![AgentMcpResult {
                agent: "*".into(),
                path: String::new(),
                ok: false,
                message: "MCP 标题不能为空".into(),
            }],
            all_ok: false,
        };
    }

    let targets = dedupe_write_targets(agents);
    let mut results = Vec::new();

    for target in targets {
        let res = match remove_one(&target.agent, title) {
            Ok(path) => AgentMcpResult {
                agent: target.agent,
                path: path.display().to_string(),
                ok: true,
                message: "已删除".into(),
            },
            Err(e) => AgentMcpResult {
                agent: target.agent,
                path: e.path.unwrap_or_default(),
                ok: false,
                message: e.message,
            },
        };
        results.push(res);
    }

    let all_ok = results.iter().all(|r| r.ok);
    McpBatchResult { results, all_ok }
}

/// Scan all known agents' MCP configs and return unified records (grouped by title).
pub fn sniff_mcp_servers() -> McpSniffResult {
    // 扫描去重后的物理根：共享根（shared_root 标识相同者）只扫一次，名字由 agents_for_shared_root 归并。
    let mut seen_roots: BTreeSet<&str> = BTreeSet::new();
    let scan_agents: Vec<&str> = crate::agents::agents()
        .iter()
        .filter(|s| {
            let key = s.shared_root.unwrap_or(s.name);
            seen_roots.insert(key)
        })
        .map(|s| s.name)
        .collect();

    // title_lower -> (record, agent set)
    let mut by_title: HashMap<String, (McpServerRecord, BTreeSet<String>)> = HashMap::new();
    let mut found_entries = 0usize;
    let mut scanned = 0usize;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        * 1000;

    for agent in scan_agents {
        scanned += 1;
        let entries = match read_agent_mcp_entries(agent) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for (title, draft) in entries {
            // Skip internal smoke-test leftovers
            if title.starts_with("__agentbuddy") {
                continue;
            }
            found_entries += 1;
            let key = title.to_lowercase();
            let agents_for_title = agents_for_shared_root(agent);

            if let Some((rec, set)) = by_title.get_mut(&key) {
                for a in agents_for_title {
                    set.insert(a);
                }
                // Fill empty fields from later sightings if useful
                if rec.command.is_empty() && !draft.command.is_empty() {
                    rec.command = draft.command;
                    rec.args = draft.args;
                    rec.env = draft.env;
                }
                if rec.url.is_empty() && !draft.url.is_empty() {
                    rec.url = draft.url;
                    rec.headers = draft.headers;
                }
                if rec.transport.is_empty() {
                    rec.transport = draft.transport;
                }
            } else {
                let mut set = BTreeSet::new();
                for a in agents_for_title {
                    set.insert(a);
                }
                let rec = McpServerRecord {
                    id: format!("mcp-sniff-{}-{}", now, by_title.len() + 1),
                    title: title.clone(),
                    transport: draft.transport,
                    command: draft.command,
                    args: draft.args,
                    env: draft.env,
                    url: draft.url,
                    headers: draft.headers,
                    applied_agents: vec![],
                    created_at: now,
                };
                by_title.insert(key, (rec, set));
            }
        }
    }

    let mut servers: Vec<McpServerRecord> = by_title
        .into_values()
        .map(|(mut rec, set)| {
            rec.applied_agents = set.into_iter().collect();
            rec
        })
        .collect();
    servers.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let message = format!(
        "嗅探完成 — 扫描 {} 个 Agent，发现 {} 条配置项，合并为 {} 个 MCP",
        scanned,
        found_entries,
        servers.len()
    );

    McpSniffResult {
        servers,
        scanned_agents: scanned,
        found_entries,
        message,
    }
}

/// Merge sniffed servers into existing list (by title, case-insensitive).
pub fn merge_sniffed_servers(
    existing: &[McpServerRecord],
    sniffed: &[McpServerRecord],
) -> Vec<McpServerRecord> {
    let mut map: HashMap<String, McpServerRecord> = HashMap::new();
    for s in existing {
        // Definitions created in the app stay listed even if no agent currently has
        // them, but disk discovery is authoritative for their applied-agent state.
        let mut record = s.clone();
        record.applied_agents.clear();
        map.insert(record.title.to_lowercase(), record);
    }
    for s in sniffed {
        let key = s.title.to_lowercase();
        if let Some(prev) = map.get_mut(&key) {
            // Disk is source of truth for applied agents of this title
            prev.applied_agents = s.applied_agents.clone();
            prev.transport = s.transport.clone();
            prev.command = s.command.clone();
            prev.args = s.args.clone();
            prev.env = s.env.clone();
            prev.url = s.url.clone();
            prev.headers = s.headers.clone();
            // keep prev.id and created_at
        } else {
            map.insert(key, s.clone());
        }
    }
    let mut out: Vec<McpServerRecord> = map.into_values().collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

/* ===== Internals ===== */

fn agents_for_shared_root(agent: &str) -> Vec<String> {
    match crate::agents::find(agent).and_then(|s| s.shared_root) {
        Some(key) => crate::agents::agents()
            .iter()
            .filter(|s| s.shared_root == Some(key))
            .map(|s| s.name.to_string())
            .collect(),
        None => vec![agent.to_string()],
    }
}

/// Count MCP entries in one agent's config file (0 for unknown agents or unreadable/missing files).
pub(crate) fn count_agent_mcp_entries(agent: &str) -> usize {
    read_agent_mcp_entries(agent).map(|v| v.len()).unwrap_or(0)
}

/// Read all MCP entries from one agent config as (title, draft).
fn read_agent_mcp_entries(agent: &str) -> Result<Vec<(String, McpDraft)>, OpError> {
    if matches!(
        crate::agents::find(agent).map(|spec| spec.mcp.path),
        Some(McpPath::ClaudeDesktopScan)
    ) {
        return read_claude_desktop_mcp_entries(&claude_desktop_config_paths(&home()?));
    }

    let target = resolve_target(agent)?;
    if !target.path.exists() {
        return Ok(vec![]);
    }
    match target.dialect {
        Dialect::TomlMcpServers => read_toml_entries(&target.path),
        Dialect::JsonMcpServers | Dialect::ClaudeJsonUser => {
            read_json_map_entries(&target.path, "mcpServers", false, target.jsonc)
        }
        Dialect::JsonMcp => read_json_map_entries(&target.path, "mcp", true, target.jsonc),
        Dialect::JsonGeminiMixed => read_json_map_entries(&target.path, "mcpServers", false, false),
    }
}

/// Read Claude Desktop MCP entries across all discovered configuration files.
/// Configuration directories are ordered deterministically, so the first matching
/// title is authoritative when the same MCP is present in multiple directories.
fn read_claude_desktop_mcp_entries(paths: &[PathBuf]) -> Result<Vec<(String, McpDraft)>, OpError> {
    let mut entries = Vec::new();
    let mut seen_titles = BTreeSet::new();
    for path in paths {
        for (title, draft) in read_json_map_entries(path, "mcpServers", false, false)? {
            if seen_titles.insert(title.to_lowercase()) {
                entries.push((title, draft));
            }
        }
    }
    Ok(entries)
}

fn read_json_map_entries(
    path: &Path,
    root_key: &str,
    opencode_style: bool,
    jsonc: bool,
) -> Result<Vec<(String, McpDraft)>, OpError> {
    let doc = read_json_value(path, jsonc)?;
    let Some(map) = doc.get(root_key).and_then(|v| v.as_object()) else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    for (title, entry) in map {
        if title.trim().is_empty() {
            continue;
        }
        if let Some(draft) = parse_entry_value(title, entry, opencode_style, false) {
            out.push((title.clone(), draft));
        }
    }
    Ok(out)
}

fn parse_entry_value(
    title: &str,
    entry: &Value,
    opencode_style: bool,
    _gemini: bool,
) -> Option<McpDraft> {
    let obj = entry.as_object()?;

    // OpenCode local/remote
    if opencode_style {
        let t = obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        if t == "local" || (obj.get("command").is_some() && t != "remote") {
            let (command, args) = parse_command_field(obj.get("command"), obj.get("args"));
            let env = parse_string_map(obj.get("environment").or_else(|| obj.get("env")));
            return Some(McpDraft {
                title: title.to_string(),
                transport: "stdio".into(),
                command,
                args,
                env,
                url: String::new(),
                headers: HashMap::new(),
            });
        }
        if t == "remote" || obj.get("url").is_some() {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let headers = parse_string_map(obj.get("headers"));
            return Some(McpDraft {
                title: title.to_string(),
                transport: "http".into(),
                command: String::new(),
                args: vec![],
                env: HashMap::new(),
                url,
                headers,
            });
        }
    }

    // Gemini httpUrl
    if let Some(url) = obj.get("httpUrl").and_then(|v| v.as_str()) {
        return Some(McpDraft {
            title: title.to_string(),
            transport: "http".into(),
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            url: url.to_string(),
            headers: parse_string_map(obj.get("headers")),
        });
    }

    let type_str = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    if type_str == "http" || type_str == "sse" || obj.get("url").is_some() {
        let transport = if type_str == "sse" { "sse" } else { "http" };
        return Some(McpDraft {
            title: title.to_string(),
            transport: transport.into(),
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            url: obj
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            headers: parse_string_map(obj.get("headers")),
        });
    }

    // stdio default when command present
    if obj.get("command").is_some() || type_str == "stdio" || type_str.is_empty() {
        let (command, args) = parse_command_field(obj.get("command"), obj.get("args"));
        if command.is_empty() {
            return None;
        }
        return Some(McpDraft {
            title: title.to_string(),
            transport: "stdio".into(),
            command,
            args,
            env: parse_string_map(obj.get("env")),
            url: String::new(),
            headers: HashMap::new(),
        });
    }

    None
}

fn parse_command_field(command: Option<&Value>, args: Option<&Value>) -> (String, Vec<String>) {
    if let Some(Value::Array(arr)) = command {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if parts.is_empty() {
            return (String::new(), vec![]);
        }
        let cmd = parts[0].clone();
        let rest = parts[1..].to_vec();
        return (cmd, rest);
    }
    let cmd = command
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let arg_list = match args {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => s.split_whitespace().map(|s| s.to_string()).collect(),
        _ => vec![],
    };
    (cmd, arg_list)
}

fn parse_string_map(v: Option<&Value>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(Value::Object(map)) = v {
        for (k, val) in map {
            if let Some(s) = val.as_str() {
                out.insert(k.clone(), s.to_string());
            } else if !val.is_null() {
                out.insert(k.clone(), val.to_string());
            }
        }
    }
    out
}

fn read_toml_entries(path: &Path) -> Result<Vec<(String, McpDraft)>, OpError> {
    let raw = fs::read_to_string(path)
        .map_err(|e| err(Some(path.to_path_buf()), format!("读取失败: {e}")))?;
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }
    let doc = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| err(Some(path.to_path_buf()), format!("TOML 解析失败: {e}")))?;
    let Some(mcp_servers) = doc.get("mcp_servers").and_then(|i| i.as_table()) else {
        return Ok(vec![]);
    };

    let mut out = Vec::new();
    for (title, item) in mcp_servers.iter() {
        let Some(table) = item.as_table() else {
            continue;
        };
        let type_str = table
            .get("type")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_lowercase();
        let command = table
            .get("command")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();
        let url = table
            .get("url")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        let mut args = Vec::new();
        if let Some(arr) = table.get("args").and_then(|i| i.as_array()) {
            for v in arr.iter() {
                if let Some(s) = v.as_str() {
                    args.push(s.to_string());
                }
            }
        }

        let mut env = HashMap::new();
        if let Some(env_table) = table.get("env").and_then(|i| i.as_table()) {
            for (k, v) in env_table.iter() {
                if let Some(s) = v.as_str() {
                    env.insert(k.to_string(), s.to_string());
                }
            }
        }

        let mut headers = HashMap::new();
        if let Some(h) = table.get("http_headers").and_then(|i| i.as_inline_table()) {
            for (k, v) in h.iter() {
                if let Some(s) = v.as_str() {
                    headers.insert(k.to_string(), s.to_string());
                }
            }
        }

        let transport = if type_str == "http" || type_str == "sse" || !url.is_empty() {
            if type_str == "sse" {
                "sse"
            } else {
                "http"
            }
        } else if !command.is_empty() || type_str == "stdio" {
            "stdio"
        } else {
            continue;
        };

        out.push((
            title.to_string(),
            McpDraft {
                title: title.to_string(),
                transport: transport.into(),
                command,
                args,
                env,
                url,
                headers,
            },
        ));
    }
    Ok(out)
}

struct WriteTarget {
    agent: String,
}

#[derive(Debug)]
struct OpError {
    path: Option<String>,
    message: String,
}

fn err(path: Option<PathBuf>, message: impl Into<String>) -> OpError {
    OpError {
        path: path.map(|p| p.display().to_string()),
        message: message.into(),
    }
}

/// CodeBuddy / CodeBuddy CN share one config root — write once.
fn dedupe_write_targets(agents: &[String]) -> Vec<WriteTarget> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();

    for a in agents {
        let name = a.trim();
        if name.is_empty() {
            continue;
        }
        // 物理根 key：共享根用其标识，否则用 agent 名自身。
        let path_key = crate::agents::find(name)
            .and_then(|s| s.shared_root)
            .map(|k| k.to_string())
            .unwrap_or_else(|| name.to_string());
        if !seen.insert(path_key.clone()) {
            continue;
        }
        // canonical：同一物理根下，注册表中最靠前且本次被选中的 agent。
        let canonical = crate::agents::agents()
            .iter()
            .find(|s| {
                let k = s
                    .shared_root
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| s.name.to_string());
                k == path_key && agents.iter().any(|x| x.trim() == s.name)
            })
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| name.to_string());
        out.push(WriteTarget { agent: canonical });
    }
    out
}

fn home() -> Result<PathBuf, OpError> {
    dirs::home_dir().ok_or_else(|| err(None, "无法解析用户主目录"))
}

// `Dialect` 现由 `agents::McpDialect` 提供（见顶部 use 别名）。

struct ResolvedTarget {
    path: PathBuf,
    dialect: Dialect,
    /// Prefer json5 (JSONC) parse/write pretty JSON without comments preserved
    jsonc: bool,
}

fn resolve_target(agent: &str) -> Result<ResolvedTarget, OpError> {
    let h = home()?;
    let spec = crate::agents::find(agent)
        .ok_or_else(|| err(None, format!("未知 Agent: {agent}")))?;
    let dialect = spec.mcp.dialect;
    match spec.mcp.path {
        McpPath::Fixed(rel) => Ok(ResolvedTarget {
            path: h.join(rel),
            dialect,
            jsonc: spec.mcp.jsonc,
        }),
        McpPath::OpencodeConfig => {
            let base = h.join(".config/opencode");
            let jsonc_path = base.join("opencode.jsonc");
            let json_path = base.join("opencode.json");
            let path = if jsonc_path.exists() {
                jsonc_path
            } else {
                json_path
            };
            let is_jsonc = path.extension().and_then(|e| e.to_str()) == Some("jsonc");
            Ok(ResolvedTarget {
                path,
                dialect,
                jsonc: is_jsonc,
            })
        }
        McpPath::CodebuddyMcp => {
            let base = h.join(".codebuddy");
            let dot = base.join(".mcp.json");
            let plain = base.join("mcp.json");
            // Prefer existing file; default to .mcp.json when neither exists
            let path = if dot.exists() {
                dot
            } else if plain.exists() {
                plain
            } else {
                dot
            };
            Ok(ResolvedTarget {
                path,
                dialect,
                jsonc: false,
            })
        }
        McpPath::ClaudeDesktopScan => {
            let path = resolve_claude_desktop_config(&h)?;
            Ok(ResolvedTarget {
                path,
                dialect,
                jsonc: false,
            })
        }
    }
}

/// Resolve the on-disk MCP config file path for an agent (no I/O beyond existence probes
/// already used by dialect-specific path selection). Used by agent open/reveal helpers.
pub(crate) fn resolve_mcp_path(agent: &str) -> Result<PathBuf, String> {
    resolve_target(agent)
        .map(|t| t.path)
        .map_err(|e| e.message)
}

fn resolve_claude_desktop_config(home: &Path) -> Result<PathBuf, OpError> {
    Ok(claude_desktop_config_paths(home)
        .into_iter()
        .next()
        .unwrap_or_else(|| home.join("Library/Application Support/Claude/claude_desktop_config.json")))
}

/// Return every existing Claude Desktop MCP configuration in a deterministic order.
/// The default `Claude` directory sorts before `Claude-*` siblings.
fn claude_desktop_config_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for root in claude_desktop_config_roots(home) {
        let default = root.join("Claude").join("claude_desktop_config.json");
        if default.exists() {
            paths.insert(default);
        }
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("Claude-") {
                    let cfg = entry.path().join("claude_desktop_config.json");
                    if cfg.exists() {
                        paths.insert(cfg);
                    }
                }
            }
        }
    }

    let mut paths: Vec<_> = paths.into_iter().collect();
    paths.sort_by_key(|path| {
        let is_default = path
            .parent()
            .and_then(|parent| parent.file_name())
            .is_some_and(|name| name == "Claude");
        (!is_default, path.clone())
    });
    paths
}

fn claude_desktop_config_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    {
        roots.push(home.join("Library/Application Support"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::config_dir() {
            roots.push(appdata);
        } else if let Ok(v) = std::env::var("APPDATA") {
            roots.push(PathBuf::from(v));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(cfg) = dirs::config_dir() {
            roots.push(cfg);
        }
    }
    if roots.is_empty() {
        if let Some(cfg) = dirs::config_dir() {
            roots.push(cfg);
        } else {
            roots.push(home.join("Library/Application Support"));
        }
    }
    roots
}

fn apply_one(agent: &str, title: &str, draft: &McpDraft) -> Result<PathBuf, OpError> {
    if matches!(
        crate::agents::find(agent).map(|spec| spec.mcp.path),
        Some(McpPath::ClaudeDesktopScan)
    ) {
        let h = home()?;
        let paths = claude_desktop_config_paths(&h);
        let paths = if paths.is_empty() {
            vec![resolve_claude_desktop_config(&h)?]
        } else {
            paths
        };
        return apply_to_paths(&paths, |path| {
            apply_json_object_key(path, "mcpServers", title, draft, false, false)
        });
    }

    let target = resolve_target(agent)?;
    match target.dialect {
        Dialect::TomlMcpServers => apply_toml_mcp_servers(&target.path, title, draft),
        Dialect::JsonMcpServers => {
            apply_json_object_key(&target.path, "mcpServers", title, draft, false, target.jsonc)
        }
        Dialect::JsonMcp => {
            apply_json_object_key(&target.path, "mcp", title, draft, true, target.jsonc)
        }
        Dialect::JsonGeminiMixed => apply_gemini(&target.path, title, draft),
        Dialect::ClaudeJsonUser => apply_claude_json(&target.path, title, draft),
    }
}

fn remove_one(agent: &str, title: &str) -> Result<PathBuf, OpError> {
    if matches!(
        crate::agents::find(agent).map(|spec| spec.mcp.path),
        Some(McpPath::ClaudeDesktopScan)
    ) {
        let h = home()?;
        let paths = claude_desktop_config_paths(&h);
        let paths = if paths.is_empty() {
            vec![resolve_claude_desktop_config(&h)?]
        } else {
            paths
        };
        return apply_to_paths(&paths, |path| {
            remove_json_object_key(path, "mcpServers", title, false)
        });
    }

    let target = resolve_target(agent)?;
    match target.dialect {
        Dialect::TomlMcpServers => remove_toml_mcp_servers(&target.path, title),
        Dialect::JsonMcpServers => {
            remove_json_object_key(&target.path, "mcpServers", title, target.jsonc)
        }
        Dialect::JsonMcp => remove_json_object_key(&target.path, "mcp", title, target.jsonc),
        Dialect::JsonGeminiMixed => remove_json_object_key(&target.path, "mcpServers", title, false),
        Dialect::ClaudeJsonUser => remove_claude_json(&target.path, title),
    }
}

fn apply_to_paths<F>(paths: &[PathBuf], operation: F) -> Result<PathBuf, OpError>
where
    F: Fn(&Path) -> Result<PathBuf, OpError>,
{
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        match operation(path) {
            Ok(path) => succeeded.push(path),
            Err(error) => failed.push(error),
        }
    }

    let succeeded = succeeded
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if failed.is_empty() {
        return Ok(PathBuf::from(succeeded.join("; ")));
    }

    let details = failed
        .iter()
        .map(|error| {
            let path = error.path.as_deref().unwrap_or("未知路径");
            format!("{path}: {}", error.message)
        })
        .collect::<Vec<_>>();
    Err(err(
        Some(PathBuf::from(succeeded.join("; "))),
        format!("{} 个配置写入失败：{}", failed.len(), details.join("；")),
    ))
}

/* ===== JSON helpers ===== */

fn read_json_value(path: &Path, jsonc: bool) -> Result<Value, OpError> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| err(Some(path.to_path_buf()), format!("读取失败: {e}")))?;
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    if jsonc {
        json5::from_str(&raw)
            .map_err(|e| err(Some(path.to_path_buf()), format!("JSONC 解析失败: {e}")))
    } else {
        serde_json::from_str(&raw)
            .map_err(|e| err(Some(path.to_path_buf()), format!("JSON 解析失败: {e}")))
    }
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), OpError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| err(Some(path.to_path_buf()), format!("创建目录失败: {e}")))?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| err(Some(path.to_path_buf()), format!("序列化失败: {e}")))?;
    atomic_write(path, text.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), OpError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| err(Some(path.to_path_buf()), format!("创建目录失败: {e}")))?;
    }
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config");
    // Same directory, unique temp name (handles leading-dot files like .claude.json).
    // A per-call atomic sequence keeps the temp path unique even for concurrent writers
    // in the same process (e.g. parallel tests touching the same file), so they never
    // share a temp file and corrupt each other's write.
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_file_name(format!(
        ".{}.agentbuddy-{}-{}.tmp",
        file_name,
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, bytes)
        .map_err(|e| err(Some(path.to_path_buf()), format!("写入临时文件失败: {e}")))?;
    // On some platforms rename over existing can fail if locked; try remove+rename as fallback
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Fallback: write directly
            let direct = fs::write(path, bytes);
            let _ = fs::remove_file(&tmp);
            direct.map_err(|e2| {
                err(
                    Some(path.to_path_buf()),
                    format!("替换配置文件失败: {e}; 直接写入也失败: {e2}"),
                )
            })
        }
    }
}

fn map_mut<'a>(doc: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>, OpError> {
    if !doc.is_object() {
        *doc = json!({});
    }
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| err(None, "配置根节点不是对象"))?;
    if !obj.contains_key(key) || !obj.get(key).map(|v| v.is_object()).unwrap_or(false) {
        obj.insert(key.to_string(), json!({}));
    }
    obj.get_mut(key)
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| err(None, format!("无法取得 `{key}` 对象")))
}

fn ui_to_mcp_servers_entry(draft: &McpDraft) -> Value {
    let mut entry = Map::new();
    match draft.transport.as_str() {
        "stdio" => {
            entry.insert("type".into(), json!("stdio"));
            entry.insert("command".into(), json!(draft.command.trim()));
            if !draft.args.is_empty() {
                entry.insert("args".into(), json!(draft.args));
            }
            if !draft.env.is_empty() {
                entry.insert("env".into(), json!(draft.env));
            }
        }
        "http" | "sse" => {
            entry.insert("type".into(), json!(draft.transport));
            entry.insert("url".into(), json!(draft.url.trim()));
            if !draft.headers.is_empty() {
                entry.insert("headers".into(), json!(draft.headers));
            }
        }
        other => {
            entry.insert("type".into(), json!(other));
            if !draft.command.trim().is_empty() {
                entry.insert("command".into(), json!(draft.command.trim()));
            }
            if !draft.url.trim().is_empty() {
                entry.insert("url".into(), json!(draft.url.trim()));
            }
        }
    }
    Value::Object(entry)
}

fn ui_to_opencode_entry(draft: &McpDraft) -> Value {
    let mut entry = Map::new();
    match draft.transport.as_str() {
        "stdio" => {
            entry.insert("type".into(), json!("local"));
            entry.insert("enabled".into(), json!(true));
            let mut cmd = vec![draft.command.trim().to_string()];
            cmd.extend(draft.args.iter().cloned());
            entry.insert("command".into(), json!(cmd));
            if !draft.env.is_empty() {
                entry.insert("environment".into(), json!(draft.env));
            }
        }
        "http" | "sse" => {
            entry.insert("type".into(), json!("remote"));
            entry.insert("enabled".into(), json!(true));
            entry.insert("url".into(), json!(draft.url.trim()));
            if !draft.headers.is_empty() {
                entry.insert("headers".into(), json!(draft.headers));
            }
        }
        other => {
            entry.insert("type".into(), json!(other));
            entry.insert("enabled".into(), json!(true));
        }
    }
    Value::Object(entry)
}

fn ui_to_gemini_entry(draft: &McpDraft) -> Value {
    let mut entry = Map::new();
    match draft.transport.as_str() {
        "stdio" => {
            entry.insert("command".into(), json!(draft.command.trim()));
            if !draft.args.is_empty() {
                entry.insert("args".into(), json!(draft.args));
            }
            if !draft.env.is_empty() {
                entry.insert("env".into(), json!(draft.env));
            }
            entry.insert("timeout".into(), json!(60000));
        }
        "http" | "sse" => {
            // Gemini/Antigravity remote uses httpUrl
            entry.insert("httpUrl".into(), json!(draft.url.trim()));
            entry.insert("timeout".into(), json!(60000));
        }
        other => {
            entry.insert("type".into(), json!(other));
        }
    }
    Value::Object(entry)
}

/* ===== JSONC comment-preserving insert (best-effort) ===== */

/// 粗略判断文本是否含 `//` 或 `/*` 注释（跳过字符串内）。
fn jsonc_has_comments(raw: &str) -> bool {
    let b = raw.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut esc = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'/' && i + 1 < b.len() && (b[i + 1] == b'/' || b[i + 1] == b'*') {
            return true;
        }
        i += 1;
    }
    false
}

/// 定位顶层 `root_key` 对象的开括号 `{` 下标（字符串/注释/嵌套感知）。找不到返回 None。
fn jsonc_locate_root_object_open(raw: &str, root_key: &str) -> Option<usize> {
    let b = raw.as_bytes();
    let mut i = 0;
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut last_key: Option<String> = None;
    let mut str_start = 0usize;
    while i < b.len() {
        let c = b[i];
        if line_comment {
            if c == b'\n' {
                line_comment = false;
            }
            i += 1;
            continue;
        }
        if block_comment {
            if c == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
                if depth == 1 {
                    last_key = std::str::from_utf8(&b[str_start + 1..i])
                        .ok()
                        .map(|s| s.to_string());
                }
            }
            i += 1;
            continue;
        }
        match c {
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                line_comment = true;
                i += 2;
                continue;
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                block_comment = true;
                i += 2;
                continue;
            }
            b'"' => {
                in_str = true;
                str_start = i;
            }
            b'{' => {
                if depth == 1 && last_key.as_deref() == Some(root_key) {
                    return Some(i);
                }
                depth += 1;
            }
            b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b',' if depth == 1 => last_key = None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// 保留注释地在 JSONC 文件的 `root_key` 对象内插入一个**新** key（`title`）。
/// 插入后用 json5 重新校验；校验不过返回 None 交由调用方回退整体重写。
fn jsonc_insert_new_key(
    raw: &str,
    root_key: &str,
    title: &str,
    entry: &Value,
) -> Option<String> {
    let open = jsonc_locate_root_object_open(raw, root_key)?;
    let entry_text = serde_json::to_string_pretty(entry).ok()?;
    // 对象内属性再缩进一层（首行紧跟 "title": 不加前缀，其余行 +4 空格）。
    let indented: String = entry_text
        .lines()
        .enumerate()
        .map(|(idx, l)| {
            if idx == 0 {
                l.to_string()
            } else {
                format!("    {}", l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let insert = format!("\n    \"{}\": {},", title, indented);

    let mut out = String::with_capacity(raw.len() + insert.len());
    out.push_str(&raw[..=open]); // 含 `{`
    out.push_str(&insert);
    out.push_str(&raw[open + 1..]);

    // 校验：结果须能被 json5 解析，且 root_key.title 等于目标 entry。
    let doc: Value = json5::from_str(&out).ok()?;
    let ok = doc
        .get(root_key)
        .and_then(|v| v.get(title))
        .map(|v| v == entry)
        .unwrap_or(false);
    if ok {
        Some(out)
    } else {
        None
    }
}

fn apply_json_object_key(
    path: &Path,
    root_key: &str,
    title: &str,
    draft: &McpDraft,
    opencode_style: bool,
    jsonc: bool,
) -> Result<PathBuf, OpError> {
    let entry = if opencode_style {
        ui_to_opencode_entry(draft)
    } else {
        ui_to_mcp_servers_entry(draft)
    };

    // JSONC 且原文件含注释、且为「新增 key」时，尝试保留注释的最小插入。
    if jsonc && path.exists() {
        if let Ok(raw) = fs::read_to_string(path) {
            if jsonc_has_comments(&raw) {
                let existing = json5::from_str::<Value>(&raw).ok();
                let root_is_obj = existing
                    .as_ref()
                    .and_then(|v| v.get(root_key))
                    .map(|v| v.is_object())
                    .unwrap_or(false);
                let title_exists = existing
                    .as_ref()
                    .and_then(|v| v.get(root_key))
                    .and_then(|v| v.get(title))
                    .is_some();
                if root_is_obj && !title_exists {
                    if let Some(new_text) = jsonc_insert_new_key(&raw, root_key, title, &entry) {
                        atomic_write(path, new_text.as_bytes())?;
                        return Ok(path.to_path_buf());
                    }
                }
            }
        }
    }

    // 默认：整体重写（无注释 / 非 JSONC / 已存在同名 / 保留失败时回退）。
    let mut doc = read_json_value(path, jsonc)?;
    let map = map_mut(&mut doc, root_key)?;
    map.insert(title.to_string(), entry);
    write_json_value(path, &doc)?;
    Ok(path.to_path_buf())
}

fn remove_json_object_key(
    path: &Path,
    root_key: &str,
    title: &str,
    jsonc: bool,
) -> Result<PathBuf, OpError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let mut doc = read_json_value(path, jsonc)?;
    if let Some(obj) = doc.as_object_mut() {
        if let Some(servers) = obj.get_mut(root_key).and_then(|v| v.as_object_mut()) {
            servers.remove(title);
        }
    }
    write_json_value(path, &doc)?;
    Ok(path.to_path_buf())
}

fn apply_gemini(path: &Path, title: &str, draft: &McpDraft) -> Result<PathBuf, OpError> {
    let mut doc = read_json_value(path, false)?;
    let map = map_mut(&mut doc, "mcpServers")?;
    map.insert(title.to_string(), ui_to_gemini_entry(draft));
    write_json_value(path, &doc)?;
    Ok(path.to_path_buf())
}

fn apply_claude_json(path: &Path, title: &str, draft: &McpDraft) -> Result<PathBuf, OpError> {
    // Preserve all other keys in ~/.claude.json
    let mut doc = read_json_value(path, false)?;
    let map = map_mut(&mut doc, "mcpServers")?;
    map.insert(title.to_string(), ui_to_mcp_servers_entry(draft));
    write_json_value(path, &doc)?;
    Ok(path.to_path_buf())
}

fn remove_claude_json(path: &Path, title: &str) -> Result<PathBuf, OpError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let mut doc = read_json_value(path, false)?;
    if let Some(obj) = doc.as_object_mut() {
        if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            servers.remove(title);
        }
    }
    write_json_value(path, &doc)?;
    Ok(path.to_path_buf())
}

/* ===== TOML (Codex) ===== */

fn apply_toml_mcp_servers(path: &Path, title: &str, draft: &McpDraft) -> Result<PathBuf, OpError> {
    let mut doc = if path.exists() {
        let raw = fs::read_to_string(path)
            .map_err(|e| err(Some(path.to_path_buf()), format!("读取失败: {e}")))?;
        raw.parse::<toml_edit::DocumentMut>()
            .map_err(|e| err(Some(path.to_path_buf()), format!("TOML 解析失败: {e}")))?
    } else {
        toml_edit::DocumentMut::new()
    };

    // Ensure [mcp_servers] exists as a table container for dotted keys
    if doc.get("mcp_servers").is_none() {
        doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    let table_path = format!("mcp_servers.{title}");
    // Remove old server table + env subtable if present
    remove_toml_server_tables(&mut doc, title);

    let mut table = toml_edit::Table::new();
    match draft.transport.as_str() {
        "stdio" => {
            table["type"] = toml_edit::value("stdio");
            table["command"] = toml_edit::value(draft.command.trim());
            if !draft.args.is_empty() {
                let mut arr = toml_edit::Array::new();
                for a in &draft.args {
                    arr.push(a.as_str());
                }
                table["args"] = toml_edit::Item::Value(toml_edit::Value::Array(arr));
            }
            table["enabled"] = toml_edit::value(true);
        }
        "http" | "sse" => {
            // Codex uses type = "http" for streamable HTTP
            table["type"] = toml_edit::value("http");
            table["url"] = toml_edit::value(draft.url.trim());
            table["enabled"] = toml_edit::value(true);
            if !draft.headers.is_empty() {
                let mut headers = toml_edit::InlineTable::new();
                for (k, v) in &draft.headers {
                    headers.insert(k, v.as_str().into());
                }
                table["http_headers"] =
                    toml_edit::Item::Value(toml_edit::Value::InlineTable(headers));
            }
        }
        other => {
            table["type"] = toml_edit::value(other);
        }
    }

    // Insert as dotted table: mcp_servers.<title>
    // Using document item path
    ensure_mcp_servers_table(&mut doc);
    if let Some(mcp_servers) = doc["mcp_servers"].as_table_mut() {
        mcp_servers.set_implicit(true);
        mcp_servers.insert(title, toml_edit::Item::Table(table));
    } else {
        return Err(err(
            Some(path.to_path_buf()),
            format!("无法写入 {table_path}"),
        ));
    }

    // env subtable for stdio
    if draft.transport == "stdio" && !draft.env.is_empty() {
        if let Some(mcp_servers) = doc["mcp_servers"].as_table_mut() {
            if let Some(server) = mcp_servers.get_mut(title).and_then(|i| i.as_table_mut()) {
                let mut env_table = toml_edit::Table::new();
                for (k, v) in &draft.env {
                    env_table.insert(k, toml_edit::value(v.as_str()));
                }
                server.insert("env", toml_edit::Item::Table(env_table));
            }
        }
    }

    let text = doc.to_string();
    atomic_write(path, text.as_bytes())?;
    Ok(path.to_path_buf())
}

fn ensure_mcp_servers_table(doc: &mut toml_edit::DocumentMut) {
    if doc.get("mcp_servers").is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        doc["mcp_servers"] = toml_edit::Item::Table(t);
    } else if let Some(t) = doc["mcp_servers"].as_table_mut() {
        t.set_implicit(true);
    }
}

fn remove_toml_server_tables(doc: &mut toml_edit::DocumentMut, title: &str) {
    if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|i| i.as_table_mut()) {
        mcp_servers.remove(title);
    }
}

fn remove_toml_mcp_servers(path: &Path, title: &str) -> Result<PathBuf, OpError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| err(Some(path.to_path_buf()), format!("读取失败: {e}")))?;
    let mut doc = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| err(Some(path.to_path_buf()), format!("TOML 解析失败: {e}")))?;
    remove_toml_server_tables(&mut doc, title);
    let text = doc.to_string();
    atomic_write(path, text.as_bytes())?;
    Ok(path.to_path_buf())
}

/* ===== Connectivity test ===== */

/// Probe whether an MCP draft is reachable, without touching any config file.
/// - stdio: spawn the process, send one MCP `initialize` JSON-RPC line, and
///   treat any response within the timeout as "started and responsive".
/// - http/sse: POST one `initialize` to the URL; 2xx means reachable.
pub fn test_mcp_connection(draft: McpDraft) -> McpTestResult {
    match draft.transport.as_str() {
        "stdio" => test_stdio(&draft),
        "http" | "sse" => test_http(&draft),
        other => McpTestResult {
            ok: false,
            message: format!("未知类型: {}", other),
            detail: String::new(),
        },
    }
}

fn test_stdio(draft: &McpDraft) -> McpTestResult {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    let command = draft.command.trim();
    if command.is_empty() {
        return McpTestResult {
            ok: false,
            message: "stdio 类型需要填写命令".into(),
            detail: String::new(),
        };
    }

    let mut cmd = Command::new(command);
    cmd.args(&draft.args);
    for (k, v) in &draft.env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return McpTestResult {
                ok: false,
                message: format!("无法启动命令「{}」：{}", command, e),
                detail: String::new(),
            }
        }
    };

    // Send one initialize request (newline-delimited, as most stdio servers read).
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"AgentBuddy","version":"0.1"}}}"#;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(init.as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }

    // std has no read-with-timeout; read one line on a worker and bound it with recv_timeout.
    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut reader = BufReader::new(out);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = tx.send(line);
        });
    }

    let outcome = match rx.recv_timeout(Duration::from_secs(8)) {
        Ok(line) if line.contains("\"jsonrpc\"") || line.contains("\"result\"") => McpTestResult {
            ok: true,
            message: "连接成功：server 已响应 initialize".into(),
            detail: line.trim().chars().take(240).collect(),
        },
        Ok(line) if !line.trim().is_empty() => McpTestResult {
            ok: true,
            message: "server 已启动并有输出（非标准 initialize 响应）".into(),
            detail: line.trim().chars().take(240).collect(),
        },
        Ok(_) => McpTestResult {
            ok: false,
            message: "server 启动但未返回任何响应".into(),
            detail: String::new(),
        },
        Err(_) => McpTestResult {
            ok: false,
            message: "连接超时：8 秒内未收到 initialize 响应".into(),
            detail: String::new(),
        },
    };

    let _ = child.kill();
    let _ = child.wait();
    outcome
}

fn test_http(draft: &McpDraft) -> McpTestResult {
    use std::time::Duration;

    let url = draft.url.trim();
    if url.is_empty() {
        return McpTestResult {
            ok: false,
            message: format!("{} 类型需要填写 URL", draft.transport.to_uppercase()),
            detail: String::new(),
        };
    }

    let client = {
        let builder = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("AgentBuddy/0.1 (MCP probe)");
        match crate::http_client::apply_proxy(builder).and_then(|b| {
            b.build()
                .map_err(|e| format!("HTTP 客户端创建失败: {}", e))
        }) {
            Ok(c) => c,
            Err(e) => {
                return McpTestResult {
                    ok: false,
                    message: e,
                    detail: String::new(),
                }
            }
        }
    };

    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "AgentBuddy", "version": "0.1"}
        }
    });

    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");
    for (k, v) in &draft.headers {
        req = req.header(k.as_str(), v.as_str());
    }

    match req.json(&init_body).send() {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if (200..300).contains(&code) {
                McpTestResult {
                    ok: true,
                    message: format!("连接成功（HTTP {}）", code),
                    detail: resp
                        .text()
                        .unwrap_or_default()
                        .trim()
                        .chars()
                        .take(240)
                        .collect(),
                }
            } else if code == 401 || code == 403 {
                McpTestResult {
                    ok: false,
                    message: format!("认证失败（HTTP {}），请检查 headers", code),
                    detail: String::new(),
                }
            } else if code == 404 {
                McpTestResult {
                    ok: false,
                    message: "URL 不存在（HTTP 404）".into(),
                    detail: String::new(),
                }
            } else {
                McpTestResult {
                    ok: false,
                    message: format!("服务器返回 HTTP {}", code),
                    detail: String::new(),
                }
            }
        }
        Err(e) => {
            let msg = if e.is_timeout() {
                "连接超时（10 秒）".to_string()
            } else if e.is_connect() {
                "无法连接服务器".to_string()
            } else {
                e.to_string().chars().take(120).collect::<String>()
            };
            McpTestResult {
                ok: false,
                message: format!("请求失败: {}", msg),
                detail: String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("agent-buddy-{name}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn draft_stdio(title: &str) -> McpDraft {
        McpDraft {
            title: title.into(),
            transport: "stdio".into(),
            command: "echo".into(),
            args: vec!["hello".into()],
            env: Default::default(),
            url: String::new(),
            headers: Default::default(),
        }
    }

    fn draft_http(title: &str) -> McpDraft {
        McpDraft {
            title: title.into(),
            transport: "http".into(),
            command: String::new(),
            args: vec![],
            env: Default::default(),
            url: "https://example.com/mcp".into(),
            headers: Default::default(),
        }
    }

    #[test]
    fn merge_sniffed_servers_reconciles_applied_agents() {
        let record = |title: &str, agents: &[&str]| McpServerRecord {
            id: format!("id-{title}"),
            title: title.into(),
            transport: "stdio".into(),
            command: "echo".into(),
            args: vec!["old".into()],
            env: HashMap::new(),
            url: String::new(),
            headers: HashMap::new(),
            applied_agents: agents.iter().map(|agent| (*agent).into()).collect(),
            created_at: 1,
        };

        let existing = vec![
            record("Still-present", &["codex", "claude-code"]),
            record("Removed-from-disk", &["codex"]),
        ];
        let mut still_present = record("still-PRESENT", &["claude-code"]);
        still_present.command = "updated".into();
        let new_on_disk = record("New-on-disk", &["codex"]);

        let merged = merge_sniffed_servers(&existing, &[still_present, new_on_disk]);
        let by_title = merged
            .iter()
            .map(|record| (record.title.to_lowercase(), record))
            .collect::<HashMap<_, _>>();

        let still_present = by_title["still-present"];
        assert_eq!(still_present.applied_agents, vec!["claude-code"]);
        assert_eq!(still_present.command, "updated");
        assert_eq!(still_present.id, "id-Still-present");

        assert!(by_title["removed-from-disk"].applied_agents.is_empty());
        assert_eq!(by_title["new-on-disk"].applied_agents, vec!["codex"]);
    }

    #[test]
    fn claude_desktop_configs_apply_remove_and_read_all_paths() {
        let home = temp_dir("claude-desktop-configs");
        let root = home.join("Library/Application Support");
        let configs = [
            root.join("Claude/claude_desktop_config.json"),
            root.join("Claude-personal/claude_desktop_config.json"),
            root.join("Claude-work/claude_desktop_config.json"),
        ];
        for path in &configs {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "{\n  \"mcpServers\": {}\n}\n").unwrap();
        }

        let paths = claude_desktop_config_paths(&home);
        assert_eq!(paths, configs);

        let empty_home = temp_dir("claude-desktop-empty");
        assert_eq!(
            resolve_claude_desktop_config(&empty_home).unwrap(),
            empty_home.join("Library/Application Support/Claude/claude_desktop_config.json")
        );
        fs::remove_dir_all(empty_home).unwrap();

        let title = "all-configs";
        apply_to_paths(&paths, |path| {
            apply_json_object_key(path, "mcpServers", title, &draft_stdio(title), false, false)
        })
        .unwrap();
        for path in &configs {
            let doc: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            assert!(doc["mcpServers"][title].is_object(), "{}", path.display());
        }

        let entries = read_claude_desktop_mcp_entries(&paths).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, title);

        apply_json_object_key(
            &configs[1],
            "mcpServers",
            "Personal-only",
            &draft_stdio("Personal-only"),
            false,
            false,
        )
        .unwrap();
        apply_json_object_key(
            &configs[2],
            "mcpServers",
            "ALL-CONFIGS",
            &draft_http("ALL-CONFIGS"),
            false,
            false,
        )
        .unwrap();

        let entries = read_claude_desktop_mcp_entries(&paths).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, title);
        assert_eq!(entries[1].0, "Personal-only");

        apply_to_paths(&paths, |path| {
            remove_json_object_key(path, "mcpServers", title, false)
        })
        .unwrap();
        for path in &configs {
            let doc: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            assert!(doc["mcpServers"].get(title).is_none(), "{}", path.display());
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn apply_remove_all_sniffed_agents() {
        let title = "__agentbuddy_all_smoke__";
        let agents = vec![
            "codex".into(),
            "claude-code".into(),
            "claude-desktop".into(),
            "opencode".into(),
            "deveco-code".into(),
            "antigravity".into(),
            "codebuddy-cn".into(),
            "workbuddy".into(),
        ];
        let r = apply_mcp_to_agents(&draft_stdio(title), &agents);
        assert_eq!(r.results.len(), 8, "{:?}", r.results);
        assert!(r.all_ok, "{:?}", r.results);

        // dialect-specific checks
        let home = dirs::home_dir().unwrap();

        let oc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(oc["mcp"][title]["type"], "local");

        let gm: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".gemini/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(gm["mcpServers"][title]["command"].is_string());

        let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.contains(title));

        // http mapping for gemini
        let title2 = "__agentbuddy_http_smoke__";
        let r2 = apply_mcp_to_agents(&draft_http(title2), &["antigravity".into()]);
        assert!(r2.all_ok, "{:?}", r2.results);
        let gm2: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".gemini/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(gm2["mcpServers"][title2]["httpUrl"], "https://example.com/mcp");

        // cleanup
        let r3 = remove_mcp_from_agents(title, &agents);
        assert!(r3.all_ok, "{:?}", r3.results);
        let r4 = remove_mcp_from_agents(title2, &["antigravity".into()]);
        assert!(r4.all_ok, "{:?}", r4.results);

        let codex2 = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(!codex2.contains(title));
        let oc2: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.json")).unwrap(),
        )
        .unwrap();
        assert!(oc2["mcp"].get(title).is_none());
    }

    #[test]
    fn sniff_mcp_servers_runs() {
        let r = sniff_mcp_servers();
        // should not panic; servers list is vec
        assert!(r.scanned_agents >= 1);
        println!("{}", r.message);
    }

    #[test]
    fn claude_json_preserves_other_keys() {
        let path = dirs::home_dir().unwrap().join(".claude.json");
        let before: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let title = "__agentbuddy_claude_preserve__";
        let r = apply_mcp_to_agents(&draft_stdio(title), &["claude-code".into()]);
        assert!(r.all_ok, "{:?}", r.results);
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // preserve a known top-level key if present
        if before.get("numStartups").is_some() {
            assert_eq!(before["numStartups"], after["numStartups"]);
        }
        assert!(after["mcpServers"][title].is_object());
        let r2 = remove_mcp_from_agents(title, &["claude-code".into()]);
        assert!(r2.all_ok, "cleanup failed: {:?}", r2.results);
        let after_rm: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            after_rm
                .get("mcpServers")
                .and_then(|m| m.get(title))
                .is_none(),
            "test MCP leftover still present"
        );
    }
}
