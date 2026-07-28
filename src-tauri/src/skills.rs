//! Skills library under `~/.agentbuddy/skills` + sniff/apply status across agents.
//! Spec: AGENT_MCP_SKILLS_MAP.md (dir.SKILL.md dialect).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db;

/* ===== Public types ===== */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    Local,
    Github,
    Gitcode,
}

impl SkillSource {
    /// Whether this source tracks a remote git repository (github/gitcode).
    fn is_remote(&self) -> bool {
        matches!(self, SkillSource::Github | SkillSource::Gitcode)
    }
}

impl Default for SkillSource {
    fn default() -> Self {
        Self::Local
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    /// Directory name under ~/.agentbuddy/skills (stable id)
    pub id: String,
    /// Display title (frontmatter `name` or dir name)
    pub title: String,
    /// Short description from SKILL.md frontmatter / first paragraph
    pub description: String,
    pub source: SkillSource,
    /// e.g. https://github.com/owner/repo
    #[serde(default)]
    pub repo_url: String,
    #[serde(default)]
    pub github_owner: String,
    #[serde(default)]
    pub github_repo: String,
    /// Relative path inside the github repo when skill is a subdir
    #[serde(default)]
    pub github_path: String,
    /// Absolute path of the skill directory in the library
    pub local_path: String,
    /// User-assigned tag/category (empty = untagged)
    #[serde(default)]
    pub tag: String,
    /// Agent sniff `name`s that currently have this skill installed
    #[serde(default)]
    pub applied_agents: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// For github skills: whether a remote update is available
    #[serde(default)]
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListResult {
    pub skills: Vec<SkillRecord>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSniffResult {
    pub skills: Vec<SkillRecord>,
    pub scanned_agents: usize,
    pub found_entries: usize,
    pub imported: usize,
    pub message: String,
}

/// One skill discovered in an Agent's skills dir, for the sniff preview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffPreviewItem {
    /// Stable selection key = absolute source path of the skill dir.
    pub key: String,
    /// Directory name (used as preferred library id on import).
    pub directory: String,
    pub title: String,
    pub description: String,
    /// Absolute path of the discovered skill dir.
    pub source_path: String,
    /// Agent sniff names where this exact source dir was found.
    pub found_agents: Vec<String>,
    /// "import" | "skip_exists"
    pub status: String,
    pub status_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffPreviewResult {
    pub ok: bool,
    pub items: Vec<SniffPreviewItem>,
    pub scanned_agents: usize,
    pub total: usize,
    pub importable: usize,
    pub skip_exists: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffImportResult {
    pub ok: bool,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub skills: Vec<SkillRecord>,
    pub message: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateCheckResult {
    pub skills: Vec<SkillRecord>,
    pub checked: usize,
    pub updates: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillActionResult {
    pub ok: bool,
    pub skill: Option<SkillRecord>,
    pub message: String,
}

/// Result of applying a skill to a set of agents.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillApplyResult {
    pub ok: bool,
    /// Refreshed record with up-to-date `applied_agents` / `tag`.
    pub skill: Option<SkillRecord>,
    /// Agents newly installed (or already present) this run.
    pub linked: usize,
    /// Agents whose stale entry was removed this run.
    pub unlinked: usize,
    pub message: String,
    /// Per-agent non-fatal failures (unsupported agent, IO error, …).
    pub errors: Vec<String>,
}

/// One skill discovered from ~/.cc-switch for migration preview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchPreviewItem {
    /// Stable id from cc-switch.db `skills.id`
    pub cc_id: String,
    /// Target directory / library id (cc-switch `directory`)
    pub directory: String,
    pub title: String,
    pub description: String,
    pub source: SkillSource,
    #[serde(default)]
    pub repo_url: String,
    #[serde(default)]
    pub github_owner: String,
    #[serde(default)]
    pub github_repo: String,
    #[serde(default)]
    pub github_path: String,
    /// Absolute path under ~/.cc-switch/skills
    pub source_path: String,
    /// Agents enabled in cc-switch (mapped to AgentBuddy sniff names)
    #[serde(default)]
    pub enabled_agents: Vec<String>,
    /// "import" | "skip_exists" | "missing"
    pub status: String,
    pub status_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchPreviewResult {
    pub ok: bool,
    pub items: Vec<CcSwitchPreviewItem>,
    pub total: usize,
    pub importable: usize,
    pub skip_exists: usize,
    pub missing: usize,
    pub message: String,
    pub cc_switch_root: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchMigrateResult {
    pub ok: bool,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub skills: Vec<SkillRecord>,
    pub message: String,
    pub errors: Vec<String>,
}

/// 批量操作（删除 / 软链接到目录 / 应用到 Agent）的统一聚合返回。
/// `skills` 为操作后重新加载的整表，前端可直接 setSkills 刷新，保持与
/// sniff / migrate 一致的数据流。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSkillResult {
    /// 全部选中项均成功时为真；有任一失败 / 跳过则为假。
    pub ok: bool,
    /// 成功处理的技能个数。
    pub succeeded: usize,
    /// 失败的技能个数（软链接场景下「目标已存在同名」计入 skipped，不计此项）。
    pub failed: usize,
    /// 软链接到目录时因目标已存在而跳过的个数；其它操作恒为 0。
    pub skipped: usize,
    /// 操作后刷新的技能整表。
    pub skills: Vec<SkillRecord>,
    pub message: String,
    /// 逐项失败原因（含技能标题前缀）。
    pub errors: Vec<String>,
}

/* ===== Meta registry (source info, persisted in SQLite) ===== */

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetaEntry {
    #[serde(default)]
    pub source: SkillSource,
    #[serde(default)]
    pub repo_url: String,
    #[serde(default)]
    pub github_owner: String,
    #[serde(default)]
    pub github_repo: String,
    #[serde(default)]
    pub github_path: String,
    /// User-assigned tag/category (empty = untagged)
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    /// last known remote commit / etag hint for update checks
    #[serde(default)]
    pub remote_ref: String,
    #[serde(default)]
    pub local_ref: String,
}

/* ===== Paths ===== */

fn app_dir() -> Result<PathBuf, String> {
    crate::config::app_dir()
}

pub fn skills_library_dir() -> Result<PathBuf, String> {
    let dir = app_dir()?.join("skills");
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("创建 skills 目录失败: {}", e))?;
    }
    Ok(dir)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn expand_tilde(path: &str) -> PathBuf {
    crate::platform::expand_home(path).unwrap_or_else(|_| PathBuf::from(path))
}

fn default_local_meta() -> SkillMetaEntry {
    let ts = now_secs();
    SkillMetaEntry {
        source: SkillSource::Local,
        created_at: ts,
        updated_at: ts,
        ..Default::default()
    }
}

/* ===== SKILL.md parsing ===== */

#[derive(Debug, Default)]
struct SkillDoc {
    name: String,
    description: String,
}

fn parse_skill_md(content: &str) -> SkillDoc {
    let mut doc = SkillDoc::default();
    let trimmed = content.trim_start_matches('\u{feff}');

    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            let fm_lines: Vec<&str> = fm.lines().collect();
            let mut i = 0;
            while i < fm_lines.len() {
                let line = fm_lines[i].trim();
                if line.is_empty() || line.starts_with('#') {
                    i += 1;
                    continue;
                }
                if let Some((k, v)) = line.split_once(':') {
                    let key = k.trim().to_ascii_lowercase();
                    let val_raw = v.trim();
                    // YAML block scalars: `description: |` / `>` collect the indented body.
                    if val_raw == "|"
                        || val_raw == ">"
                        || val_raw.starts_with("|-")
                        || val_raw.starts_with(">-")
                        || val_raw.starts_with("|+")
                        || val_raw.starts_with(">+")
                    {
                        let folded = val_raw.starts_with('>');
                        let mut block: Vec<String> = Vec::new();
                        i += 1;
                        while i < fm_lines.len() {
                            let raw = fm_lines[i];
                            if raw.trim().is_empty() {
                                block.push(String::new());
                                i += 1;
                                continue;
                            }
                            // Block ends when indentation returns to column 0 (next key).
                            if !raw.starts_with(' ') && !raw.starts_with('\t') {
                                break;
                            }
                            block.push(raw.trim_start().to_string());
                            i += 1;
                        }
                        let joined = if folded {
                            block.join(" ")
                        } else {
                            block.join("\n")
                        };
                        let value = joined.trim().to_string();
                        match key.as_str() {
                            "name" => doc.name = value,
                            "description" => {
                                // Block scalars are often a lead sentence followed by a
                                // structured trigger list (no blank-line separator). Keep the
                                // card blurb to the author's first line for literal `|` blocks;
                                // fold-and-summarize `>` blocks that flow as one paragraph.
                                doc.description = if folded {
                                    first_paragraph_summary(&value)
                                } else {
                                    let lead = block
                                        .iter()
                                        .map(|l| l.trim())
                                        .find(|l| !l.is_empty())
                                        .unwrap_or("");
                                    first_paragraph_summary(lead)
                                };
                            }
                            _ => {}
                        }
                        continue;
                    }
                    let mut val = val_raw.to_string();
                    // strip surrounding quotes
                    if (val.starts_with('"') && val.ends_with('"'))
                        || (val.starts_with('\'') && val.ends_with('\''))
                    {
                        val = val[1..val.len() - 1].to_string();
                    }
                    match key.as_str() {
                        "name" => doc.name = val,
                        "description" => doc.description = val,
                        _ => {}
                    }
                }
                i += 1;
            }
            // body after frontmatter — use first non-empty paragraph if no description
            if doc.description.is_empty() {
                let body = rest[end + 4..].trim();
                doc.description = first_paragraph_summary(body);
            }
            return doc;
        }
    }

    // No frontmatter: first # heading as name, first paragraph as description
    for line in trimmed.lines() {
        let t = line.trim();
        if t.starts_with("# ") && doc.name.is_empty() {
            doc.name = t.trim_start_matches('#').trim().to_string();
            continue;
        }
        if !t.is_empty() && !t.starts_with('#') && doc.description.is_empty() {
            doc.description = t.to_string();
            break;
        }
    }
    doc
}

fn first_paragraph_summary(body: &str) -> String {
    let mut lines = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with('#') {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        lines.push(t);
        if lines.join(" ").len() > 240 {
            break;
        }
    }
    let mut s = lines.join(" ");
    if s.chars().count() > 200 {
        s = s.chars().take(200).collect::<String>() + "…";
    }
    s
}

fn read_skill_doc(skill_dir: &Path) -> SkillDoc {
    let skill_md = skill_dir.join("SKILL.md");
    let alt = skill_dir.join("skill.md");
    let path = if skill_md.exists() {
        skill_md
    } else if alt.exists() {
        alt
    } else {
        return SkillDoc::default();
    };
    match fs::read_to_string(&path) {
        Ok(content) => parse_skill_md(&content),
        Err(_) => SkillDoc::default(),
    }
}

fn is_skill_dir(path: &Path) -> bool {
    path.is_dir() && (path.join("SKILL.md").exists() || path.join("skill.md").exists())
}

/// Returns true when an entry is a visible skill directory (including a directory symlink).
pub(crate) fn is_visible_skill_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !name.starts_with('.') && !name.starts_with("__agentbuddy") && is_skill_dir(path)
}

/// Count installed skills in a concrete skills root using the same rules as skills management.
pub(crate) fn count_skills_in_root(root: &Path) -> usize {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| is_visible_skill_dir(&entry.path()))
        .count()
}

/* ===== Agent skills roots ===== */

struct AgentSkillsTarget {
    agent: &'static str,
    /// Prefer first existing path when scanning; write to first entry
    roots: Vec<PathBuf>,
    supported: bool,
}

fn agent_skills_targets() -> Vec<AgentSkillsTarget> {
    // 单一数据源：从 `agents` 注册表派生（roots 相对 ~，用 expand_tilde 展开）。
    crate::agents::agents()
        .iter()
        .map(|spec| AgentSkillsTarget {
            agent: spec.name,
            roots: spec.skills_roots.iter().map(|&r| expand_tilde(r)).collect(),
            supported: spec.skills_supported,
        })
        .collect()
}

/// Map skill_id → set of agent names that have it installed.
fn scan_applied_agents() -> HashMap<String, BTreeSet<String>> {
    let mut map: HashMap<String, BTreeSet<String>> = HashMap::new();
    // Shared root（shared_root 标识相同的 agent）— 任一路径有 skill 则标记同根所有 agent
    for target in agent_skills_targets() {
        if !target.supported {
            continue;
        }
        for root in &target.roots {
            if !root.is_dir() {
                continue;
            }
            let entries = match fs::read_dir(root) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_skill_dir(&path) {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().to_string();
                if id.starts_with('.') || id.starts_with("__agentbuddy") {
                    continue;
                }
                map.entry(id).or_default().insert(target.agent.to_string());
            }
        }
    }
    map
}

/// Count skills installed in one agent's skills roots (unique dir names across all roots;
/// 0 for unknown/unsupported agents). Mirrors `scan_applied_agents`'s entry filtering.
pub(crate) fn count_agent_skills(agent: &str) -> usize {
    let Some(spec) = crate::agents::find(agent) else {
        return 0;
    };
    if !spec.skills_supported {
        return 0;
    }
    let mut names: BTreeSet<String> = BTreeSet::new();
    for root in spec.skills_roots.iter().map(|&r| expand_tilde(r)) {
        if !root.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_skill_dir(&path) {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if id.starts_with('.') || id.starts_with("__agentbuddy") {
                continue;
            }
            names.insert(id);
        }
    }
    names.len()
}

/// One skill installed in an agent's skills roots (for the agent detail view).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillInfo {
    /// Directory name (stable id within the agent's skills root).
    pub id: String,
    /// Display title from SKILL.md frontmatter (falls back to `id`).
    pub title: String,
    /// Description from SKILL.md frontmatter (may be empty).
    pub description: String,
    /// Absolute path of the skill directory.
    pub path: String,
}

/// List skills installed in one agent's skills roots (unique dir names across all roots;
/// empty for unknown/unsupported agents). Mirrors `count_agent_skills`'s entry filtering.
pub(crate) fn list_agent_skills(agent: &str) -> Vec<AgentSkillInfo> {
    let Some(spec) = crate::agents::find(agent) else {
        return vec![];
    };
    if !spec.skills_supported {
        return vec![];
    }
    let mut by_id: std::collections::BTreeMap<String, AgentSkillInfo> = Default::default();
    for root in spec.skills_roots.iter().map(|&r| expand_tilde(r)) {
        if !root.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_skill_dir(&path) {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if id.starts_with('.') || id.starts_with("__agentbuddy") {
                continue;
            }
            by_id.entry(id.clone()).or_insert_with(|| {
                let doc = read_skill_doc(&path);
                AgentSkillInfo {
                    title: if doc.name.is_empty() { id.clone() } else { doc.name },
                    description: doc.description,
                    id,
                    path: path.to_string_lossy().to_string(),
                }
            });
        }
    }
    by_id.into_values().collect()
}

/* ===== List library ===== */

fn sanitize_skill_id(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else if c.is_whitespace() {
                '-'
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        format!("skill-{}", now_secs())
    } else {
        s
    }
}

fn build_record(
    id: &str,
    skill_dir: &Path,
    meta: &SkillMetaEntry,
    applied: &[String],
    update_available: bool,
) -> SkillRecord {
    let doc = read_skill_doc(skill_dir);
    let title = if !doc.name.is_empty() {
        doc.name
    } else {
        id.to_string()
    };
    SkillRecord {
        id: id.to_string(),
        title,
        description: doc.description,
        source: meta.source.clone(),
        repo_url: meta.repo_url.clone(),
        github_owner: meta.github_owner.clone(),
        github_repo: meta.github_repo.clone(),
        github_path: meta.github_path.clone(),
        local_path: skill_dir.to_string_lossy().to_string(),
        tag: meta.tag.clone(),
        applied_agents: applied.to_vec(),
        created_at: if meta.created_at > 0 {
            meta.created_at
        } else {
            now_secs()
        },
        updated_at: if meta.updated_at > 0 {
            meta.updated_at
        } else {
            now_secs()
        },
        update_available,
    }
}

pub fn list_skills() -> Result<SkillsListResult, String> {
    // One-time: fold legacy skills-meta.json into SQLite if present.
    let _ = db::migrate_skills_meta_json_if_present();

    let lib = skills_library_dir()?;
    let mut meta_map = db::load_skill_meta_map().unwrap_or_default();
    let applied_map = scan_applied_agents();
    let mut skills = Vec::new();

    let entries = fs::read_dir(&lib).map_err(|e| format!("读取 skills 目录失败: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_skill_dir(&path) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if id.starts_with('.') {
            continue;
        }

        let entry_meta = if let Some(existing) = meta_map.get(&id).cloned() {
            existing
        } else {
            let created = default_local_meta();
            if let Err(e) = db::upsert_skill_meta(&id, &created) {
                eprintln!("[skills] ensure meta for {} failed: {}", id, e);
            }
            meta_map.insert(id.clone(), created.clone());
            created
        };

        let mut applied: Vec<String> = applied_map
            .get(&id)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        applied.sort();

        // Also try matching by frontmatter name if dir differs
        let doc = read_skill_doc(&path);
        if !doc.name.is_empty() && doc.name != id {
            if let Some(extra) = applied_map.get(&doc.name) {
                for a in extra {
                    if !applied.contains(a) {
                        applied.push(a.clone());
                    }
                }
                applied.sort();
            }
        }

        skills.push(build_record(
            &id,
            &path,
            &entry_meta,
            &applied,
            false, // refresh on explicit check
        ));
    }

    skills.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let count = skills.len();
    Ok(SkillsListResult {
        skills,
        message: if count == 0 {
            "技能库为空，可通过「添加」或「嗅探」导入".into()
        } else {
            format!("已加载 {} 个技能", count)
        },
    })
}

/* ===== Copy helpers ===== */

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源路径不是目录: {}", src.display()));
    }
    fs::create_dir_all(dst).map_err(|e| format!("创建目标目录失败: {}", e))?;

    let mut stack = vec![src.to_path_buf()];
    while let Some(current) = stack.pop() {
        let rel = current
            .strip_prefix(src)
            .map_err(|e| format!("路径计算失败: {}", e))?;
        let target = dst.join(rel);
        if current != src {
            fs::create_dir_all(&target).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        for entry in fs::read_dir(&current).map_err(|e| format!("读取目录失败: {}", e))? {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();
            let name = entry.file_name();
            // skip VCS noise when importing
            if name == ".git" || name == ".DS_Store" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let dest_file = target.join(&name);
                fs::copy(&path, &dest_file).map_err(|e| {
                    format!(
                        "复制文件失败 {} → {}: {}",
                        path.display(),
                        dest_file.display(),
                        e
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn unique_skill_id(preferred: &str, lib: &Path) -> String {
    let base = sanitize_skill_id(preferred);
    if !lib.join(&base).exists() {
        return base;
    }
    for i in 2..1000 {
        let candidate = format!("{}-{}", base, i);
        if !lib.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{}-{}", base, now_secs())
}

fn import_skill_dir(
    src: &Path,
    preferred_id: Option<&str>,
    source: SkillSource,
    git: Option<GitRepoRef>,
    tag: &str,
) -> Result<SkillRecord, String> {
    if !is_skill_dir(src) {
        return Err(format!("目录缺少 SKILL.md: {}", src.display()));
    }
    let lib = skills_library_dir()?;
    let doc = read_skill_doc(src);
    let preferred = preferred_id
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if !doc.name.is_empty() {
                Some(doc.name.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            src.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("skill-{}", now_secs()))
        });

    let id = unique_skill_id(&preferred, &lib);
    let dest = lib.join(&id);
    copy_dir_recursive(src, &dest)?;

    let ts = now_secs();
    let entry = SkillMetaEntry {
        source: source.clone(),
        repo_url: git.as_ref().map(|g| g.repo_url.clone()).unwrap_or_default(),
        github_owner: git.as_ref().map(|g| g.owner.clone()).unwrap_or_default(),
        github_repo: git.as_ref().map(|g| g.repo.clone()).unwrap_or_default(),
        github_path: git.as_ref().map(|g| g.path.clone()).unwrap_or_default(),
        tag: tag.trim().to_string(),
        created_at: ts,
        updated_at: ts,
        remote_ref: String::new(),
        local_ref: String::new(),
    };
    db::upsert_skill_meta(&id, &entry)?;

    let applied_map = scan_applied_agents();
    let mut applied: Vec<String> = applied_map
        .get(&id)
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default();
    if !doc.name.is_empty() {
        if let Some(extra) = applied_map.get(&doc.name) {
            for a in extra {
                if !applied.contains(a) {
                    applied.push(a.clone());
                }
            }
        }
    }
    applied.sort();

    Ok(build_record(&id, &dest, &entry, &applied, false))
}

/* ===== Add: local ===== */

pub fn add_skill_local(path: String, tag: String) -> Result<SkillActionResult, String> {
    let src = expand_tilde(path.trim());
    if !src.exists() {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: format!("路径不存在: {}", src.display()),
        });
    }

    // If path itself is a skill dir
    if is_skill_dir(&src) {
        let skill = import_skill_dir(&src, None, SkillSource::Local, None, &tag)?;
        return Ok(SkillActionResult {
            ok: true,
            message: format!("已导入本地技能「{}」", skill.title),
            skill: Some(skill),
        });
    }

    // If path contains nested skill dirs (one level)
    if src.is_dir() {
        let mut imported = Vec::new();
        for entry in fs::read_dir(&src).map_err(|e| format!("读取目录失败: {}", e))? {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let p = entry.path();
            if is_skill_dir(&p) {
                imported.push(import_skill_dir(&p, None, SkillSource::Local, None, &tag)?);
            }
        }
        if imported.is_empty() {
            return Ok(SkillActionResult {
                ok: false,
                skill: None,
                message: "所选目录中未找到包含 SKILL.md 的技能".into(),
            });
        }
        let n = imported.len();
        return Ok(SkillActionResult {
            ok: true,
            skill: imported.into_iter().next(),
            message: format!("已从本地导入 {} 个技能", n),
        });
    }

    Ok(SkillActionResult {
        ok: false,
        skill: None,
        message: "请选择包含 SKILL.md 的技能目录".into(),
    })
}

/// Native folder picker. Returns selected path or cancel.
pub fn pick_local_skill_folder(tag: String) -> Result<SkillActionResult, String> {
    match crate::platform::pick_folder("选择 Skill 目录（需包含 SKILL.md）") {
        Ok(Some(path)) => add_skill_local(path.to_string_lossy().to_string(), tag),
        Ok(None) => Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "已取消选择".into(),
        }),
        Err(e) => Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: e,
        }),
    }
}

/* ===== Delete: remove a library skill (dir + metadata) ===== */

/// 计算某技能在各 Agent skills 目录下需要清理的条目名集合：
/// 库 id 本身，加上 frontmatter `name`（部分 Agent 以展示名安装）。
/// 二者都要求是「纯目录名」，防止路径穿越。
fn managed_entry_names(id: &str, skill_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = vec![id.to_string()];
    let doc = read_skill_doc(skill_dir);
    if !doc.name.is_empty() && doc.name != id && is_plain_entry_name(&doc.name) {
        names.push(doc.name);
    }
    names
}

/// 删除该技能在所有 Agent skills 目录下的副本（软链接 / 真实目录 / 悬空链接）。
/// 共享物理根（shared_root 标识相同者）只处理一次；错误不致命，逐条收集。
/// 返回清理成功的「物理根」数量，供上层提示。
///
/// `managed_names` 通常由 `managed_entry_names` 生成，已含库 id 与 frontmatter 名。
fn remove_skill_agent_copies(managed_names: &[String], errors: &mut Vec<String>) -> usize {
    let targets = agent_skills_targets();
    let mut handled_roots: BTreeSet<PathBuf> = BTreeSet::new();
    let mut removed_roots = 0usize;
    for target in &targets {
        if !target.supported {
            continue;
        }
        for root in &target.roots {
            if handled_roots.contains(root) {
                continue;
            }
            handled_roots.insert(root.clone());
            if !root.is_dir() {
                continue;
            }
            let mut removed_here = false;
            for name in managed_names {
                match remove_agent_skill_entry(root, name) {
                    Ok(true) => removed_here = true,
                    Ok(false) => {}
                    Err(e) => errors.push(format!("{}：{}", target.agent, e)),
                }
            }
            if removed_here {
                removed_roots += 1;
            }
        }
    }
    removed_roots
}

/// 删除单个技能的核心逻辑，供单卡与批量复用。
///
/// - `delete_agent_copies`：为真时，一并清理各 Agent skills 目录下的副本，
///   避免删除库目录后残留悬空软链接。
/// - 先在库目录仍存在时算出 `managed_names`（含 frontmatter 名），再删库目录，
///   最后按名清理各 Agent 副本，保证展示名安装也能被移除。
fn delete_skill_core(
    id: &str,
    delete_agent_copies: bool,
    errors: &mut Vec<String>,
) -> Result<(), String> {
    let lib = skills_library_dir()?;
    let dir = lib.join(id);
    // Ensure the resolved path stays within the library root.
    if !dir.starts_with(&lib) {
        return Err("非法的技能路径".into());
    }

    // 库目录尚在时先解析受管条目名（含 frontmatter 名），删除后便无从读取。
    let managed = if delete_agent_copies {
        Some(managed_entry_names(id, &dir))
    } else {
        None
    };

    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("删除技能目录失败: {}", e))?;
    }
    // Best-effort metadata cleanup; disk removal already succeeded.
    let _ = db::delete_skill_meta(id);

    if let Some(names) = managed {
        remove_skill_agent_copies(&names, errors);
    }
    Ok(())
}

/// 删除技能（可选一并清理各 Agent 副本）。副本清理为尽力而为：
/// 库目录删除成功即视为 ok，副本清理失败仅并入消息提示。
pub fn delete_skill(
    skill_id: String,
    delete_agent_copies: bool,
) -> Result<SkillActionResult, String> {
    let id = skill_id.trim().to_string();
    if id.is_empty() {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "缺少技能标识".into(),
        });
    }
    // Guard against path traversal: id must be a plain directory name.
    if !is_plain_entry_name(&id) {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "非法的技能标识".into(),
        });
    }

    let mut errors: Vec<String> = Vec::new();
    delete_skill_core(&id, delete_agent_copies, &mut errors)?;

    let message = if errors.is_empty() {
        "已删除技能".into()
    } else {
        format!("已删除技能，但 {} 项副本清理未成功", errors.len())
    };
    Ok(SkillActionResult {
        ok: true,
        skill: None,
        message,
    })
}

/* ===== Apply: sync a library skill into agents' skills dirs ===== */

/// How a library skill is installed into an Agent's skills directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillInstallMode {
    Link,
    Copy,
}

impl SkillInstallMode {
    /// Missing or unrecognised wire values retain the historical link behavior.
    pub fn from_wire(value: Option<&str>) -> Self {
        match value {
            Some("copy") => Self::Copy,
            _ => Self::Link,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Link => "软链接",
            Self::Copy => "完整复制",
        }
    }
}

/// A directory-entry name must be a single, plain path component — never a
/// traversal token — before we join it onto an agent's skills root.
fn is_plain_entry_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

/// Remove a filesystem entry without following a symlink.
fn remove_path_entry(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("读取目标项失败: {}", e))?;
    if meta.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("删除技能目录失败: {}", e))
    } else {
        fs::remove_file(path).map_err(|e| format!("删除技能项失败: {}", e))
    }
}

/// Install `target` under `<root>/<entry_name>` using the requested mode.
///
/// The new entry is prepared before an existing target is moved aside. If the
/// final replacement fails, the original entry is restored so an Agent's
/// existing skill is not lost to an IO or permissions error.
fn ensure_skill_install(
    root: &Path,
    entry_name: &str,
    target: &Path,
    mode: SkillInstallMode,
) -> Result<(), String> {
    if !is_plain_entry_name(entry_name) {
        return Err("非法的技能标识".into());
    }
    fs::create_dir_all(root).map_err(|e| format!("创建 skills 目录失败: {}", e))?;
    let destination = root.join(entry_name);
    // Defense in depth: resolved path must stay directly under root.
    if destination.parent() != Some(root) {
        return Err("非法的技能路径".into());
    }
    let nonce = format!("{}-{}", std::process::id(), now_secs());
    let staging = root.join(format!(".agentbuddy-{}-staging-{}", entry_name, nonce));
    let backup = root.join(format!(".agentbuddy-{}-backup-{}", entry_name, nonce));

    if fs::symlink_metadata(&staging).is_ok() || fs::symlink_metadata(&backup).is_ok() {
        return Err("技能安装临时路径已存在，请稍后重试".into());
    }

    let prepare = match mode {
        SkillInstallMode::Link => crate::platform::symlink_any(target, &staging),
        SkillInstallMode::Copy => crate::platform::copy_dir_recursive(target, &staging),
    };
    if let Err(e) = prepare {
        let _ = remove_path_entry(&staging);
        return Err(format!("创建{}失败: {}", mode.label(), e));
    }

    let had_destination = fs::symlink_metadata(&destination).is_ok();
    if had_destination {
        if let Err(e) = fs::rename(&destination, &backup) {
            let _ = remove_path_entry(&staging);
            return Err(format!("备份已有技能项失败: {}", e));
        }
    }

    if let Err(e) = fs::rename(&staging, &destination) {
        let _ = remove_path_entry(&staging);
        if had_destination {
            if let Err(restore_err) = fs::rename(&backup, &destination) {
                return Err(format!(
                    "安装失败: {}；恢复原技能项失败: {}",
                    e, restore_err
                ));
            }
        }
        return Err(format!("安装{}失败: {}", mode.label(), e));
    }

    if had_destination {
        remove_path_entry(&backup).map_err(|e| format!("清理旧技能项失败: {}", e))?;
    }
    Ok(())
}

/// Remove `<root>/<entry_name>` whether it is a symlink, real dir, or file.
/// Returns whether anything was removed.
fn remove_agent_skill_entry(root: &Path, entry_name: &str) -> Result<bool, String> {
    if !is_plain_entry_name(entry_name) {
        return Err("非法的技能标识".into());
    }
    let path = root.join(entry_name);
    if path.parent() != Some(root) {
        return Err("非法的技能路径".into());
    }
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_dir() {
                fs::remove_dir_all(&path).map_err(|e| format!("删除技能目录失败: {}", e))?;
            } else {
                // symlink or file
                fs::remove_file(&path).map_err(|e| format!("删除技能项失败: {}", e))?;
            }
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

/// Update a skill's tag and sync its installation across agents.
///
/// - `agents`: the desired set of agent sniff-names that should have this skill.
/// - Newly desired agents get the requested installation under their first skills root (shared
///   roots（shared_root 标识相同者）只写一次）。掉了右括号修正
/// - Agents that were previously applied but are now unchecked have their entry
///   removed (symlink or real copy), unless the same root is still needed by a
///   desired agent.
/// - Agents never applied and not desired are left completely untouched.
/// 安装同步结果（不含 record 重建，交由调用方）。
struct InstallationSyncOutcome {
    linked: usize,
    unlinked: usize,
    errors: Vec<String>,
}

/// 将库技能的安装同步到「恰好等于 `desired`」的最终态：
/// - `desired` 是最终应用集合（共享物理根的删除保护仍按此集合计算）；
/// - 之前已应用但不在 `desired` 的 Agent 移除其条目（保护仍被占用的根）。
/// - `install_targets` 是本次需要重装的 Agent；批量追加时仅包含新增目标，
///   避免改动其它已应用 Agent 的安装方式。
/// 不触碰 tag / meta。`lib_skill_dir` 须为库内、已校验的技能目录。
/// 记录重建（applied_after 扫描）由调用方用 `applied_after_for` 完成。
fn sync_skill_installations(
    id: &str,
    lib_skill_dir: &Path,
    desired: &BTreeSet<String>,
    install_targets: &BTreeSet<String>,
    install_mode: SkillInstallMode,
) -> InstallationSyncOutcome {
    let targets = agent_skills_targets();

    // Candidate entry names we manage: the library id, plus the frontmatter
    // name when it differs (some agents install under the display name).
    let managed_names = managed_entry_names(id, lib_skill_dir);

    let applied_before = scan_applied_agents();
    let doc_name = read_skill_doc(lib_skill_dir).name;
    let mut current: BTreeSet<String> = applied_before.get(id).cloned().unwrap_or_default();
    if !doc_name.is_empty() && doc_name != id {
        if let Some(extra) = applied_before.get(&doc_name) {
            current.extend(extra.iter().cloned());
        }
    }
    let to_unlink: BTreeSet<String> = current.difference(desired).cloned().collect();

    let mut errors: Vec<String> = Vec::new();

    // Install desired agents (dedupe by physical root for shared configs).
    let mut linked = 0usize;
    let mut ensured_roots: BTreeSet<PathBuf> = BTreeSet::new();
    let mut failed_roots: BTreeSet<PathBuf> = BTreeSet::new();
    for agent in install_targets {
        let target = match targets.iter().find(|t| t.agent == agent.as_str()) {
            Some(t) => t,
            None => {
                errors.push(format!("未知 Agent：{}", agent));
                continue;
            }
        };
        if !target.supported {
            errors.push(format!("{} 暂不支持 Skills 目录，已跳过", agent));
            continue;
        }
        let root = match target.roots.first() {
            Some(r) => r.clone(),
            None => {
                errors.push(format!("{} 无可用 skills 目录", agent));
                continue;
            }
        };
        if ensured_roots.contains(&root) {
            linked += 1;
            continue;
        }
        if failed_roots.contains(&root) {
            continue; // error already recorded for this shared root
        }
        match ensure_skill_install(&root, id, lib_skill_dir, install_mode) {
            Ok(()) => {
                ensured_roots.insert(root);
                linked += 1;
            }
            Err(e) => {
                failed_roots.insert(root);
                errors.push(format!("{}：{}", agent, e));
            }
        }
    }

    // Remove entries for de-selected agents, protecting roots still in use.
    let mut protected: BTreeSet<PathBuf> = BTreeSet::new();
    for agent in desired {
        if let Some(t) = targets.iter().find(|t| t.agent == agent.as_str()) {
            for r in &t.roots {
                protected.insert(r.clone());
            }
        }
    }

    let mut unlinked = 0usize;
    let mut removed_roots: BTreeSet<PathBuf> = BTreeSet::new();
    for agent in &to_unlink {
        let target = match targets.iter().find(|t| t.agent == agent.as_str()) {
            Some(t) => t,
            None => continue,
        };
        let mut removed_any = false;
        for root in &target.roots {
            if protected.contains(root) {
                continue;
            }
            if removed_roots.contains(root) {
                removed_any = true;
                continue;
            }
            let mut removed_here = false;
            for name in &managed_names {
                match remove_agent_skill_entry(root, name) {
                    Ok(true) => removed_here = true,
                    Ok(false) => {}
                    Err(e) => errors.push(format!("{}：{}", agent, e)),
                }
            }
            if removed_here {
                removed_roots.insert(root.clone());
                removed_any = true;
            }
        }
        if removed_any {
            unlinked += 1;
        }
    }

    InstallationSyncOutcome {
        linked,
        unlinked,
        errors,
    }
}

/// 以一次全新扫描重建某技能的 applied Agent 列表（含 frontmatter 名匹配、排序）。
fn applied_after_for(id: &str, lib_skill_dir: &Path) -> Vec<String> {
    let doc_name = read_skill_doc(lib_skill_dir).name;
    let applied_after = scan_applied_agents();
    let mut applied: Vec<String> = applied_after
        .get(id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    if !doc_name.is_empty() && doc_name != id {
        if let Some(extra) = applied_after.get(&doc_name) {
            for a in extra {
                if !applied.contains(a) {
                    applied.push(a.clone());
                }
            }
        }
    }
    applied.sort();
    applied
}

pub fn apply_skill_to_agents(
    skill_id: String,
    agents: Vec<String>,
    tag: String,
    install_mode: SkillInstallMode,
) -> Result<SkillApplyResult, String> {
    let id = skill_id.trim().to_string();
    if !is_plain_entry_name(&id) {
        return Ok(SkillApplyResult {
            ok: false,
            skill: None,
            linked: 0,
            unlinked: 0,
            message: "非法的技能标识".into(),
            errors: vec![],
        });
    }

    let lib = skills_library_dir()?;
    let lib_skill_dir = lib.join(&id);
    if !lib_skill_dir.starts_with(&lib) || !is_skill_dir(&lib_skill_dir) {
        return Ok(SkillApplyResult {
            ok: false,
            skill: None,
            linked: 0,
            unlinked: 0,
            message: "技能不存在或缺少 SKILL.md".into(),
            errors: vec![],
        });
    }

    // 1) Persist the (possibly changed) tag first — independent of installation sync.
    let mut meta = db::get_skill_meta(&id)
        .unwrap_or_default()
        .unwrap_or_else(default_local_meta);
    meta.tag = tag.trim().to_string();
    meta.updated_at = now_secs();
    db::upsert_skill_meta(&id, &meta)?;

    // 2) Sync installations to the desired final state (single-card semantics: replace).
    let desired: BTreeSet<String> = agents
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    let outcome = sync_skill_installations(&id, &lib_skill_dir, &desired, &desired, install_mode);

    // 3) Rebuild the record with a fresh applied scan.
    let applied = applied_after_for(&id, &lib_skill_dir);
    let record = build_record(&id, &lib_skill_dir, &meta, &applied, false);

    let message = if outcome.errors.is_empty() {
        format!(
            "已更新「{}」：以{}应用 {} 个、解除 {} 个 Agent",
            record.title,
            install_mode.label(),
            outcome.linked,
            outcome.unlinked
        )
    } else {
        format!(
            "「{}」部分完成：以{}应用 {} 个、解除 {} 个，{} 项未成功",
            record.title,
            install_mode.label(),
            outcome.linked,
            outcome.unlinked,
            outcome.errors.len()
        )
    };

    Ok(SkillApplyResult {
        ok: outcome.errors.is_empty(),
        skill: Some(record),
        linked: outcome.linked,
        unlinked: outcome.unlinked,
        message,
        errors: outcome.errors,
    })
}

/* ===== Batch operations (delete / export / apply) ===== */

/// 去重并保留原始顺序地规整一批技能标识；过滤掉非法（含穿越）项。
fn sanitize_id_batch(ids: &[String]) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in ids {
        let id = raw.trim().to_string();
        if !is_plain_entry_name(&id) {
            continue;
        }
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    out
}

/// 批量删除技能（可选一并清理各 Agent 副本）。逐项容错：任一失败不中断，
/// 记入 `errors`；最后统一刷新整表返回。
pub fn batch_delete_skills(
    skill_ids: Vec<String>,
    delete_agent_copies: bool,
) -> Result<BatchSkillResult, String> {
    let ids = sanitize_id_batch(&skill_ids);
    if ids.is_empty() {
        let list = list_skills()?;
        return Ok(BatchSkillResult {
            ok: false,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            skills: list.skills,
            message: "未选择有效技能".into(),
            errors: vec![],
        });
    }

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for id in &ids {
        let mut copy_errors: Vec<String> = Vec::new();
        match delete_skill_core(id, delete_agent_copies, &mut copy_errors) {
            Ok(()) => {
                succeeded += 1;
                // 副本清理失败不影响删除结果，但需提示。
                for e in copy_errors {
                    errors.push(format!("「{}」副本清理：{}", id, e));
                }
            }
            Err(e) => {
                failed += 1;
                errors.push(format!("「{}」：{}", id, e));
            }
        }
    }

    let list = list_skills()?;
    let message = if failed == 0 {
        format!("已删除 {} 个技能", succeeded)
    } else {
        format!("已删除 {} 个，{} 个失败", succeeded, failed)
    };
    Ok(BatchSkillResult {
        ok: failed == 0,
        succeeded,
        failed,
        skipped: 0,
        skills: list.skills,
        message,
        errors,
    })
}

/// 批量应用技能到用户选定的单个目录：只弹一次文件夹选择器。
/// 目标已存在同名条目的项计入 `skipped`（非致命）。
pub fn batch_export_skills_to_dir(
    skill_ids: Vec<String>,
    install_mode: SkillInstallMode,
) -> Result<BatchSkillResult, String> {
    let ids = sanitize_id_batch(&skill_ids);
    if ids.is_empty() {
        let list = list_skills()?;
        return Ok(BatchSkillResult {
            ok: false,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            skills: list.skills,
            message: "未选择有效技能".into(),
            errors: vec![],
        });
    }

    let target_root = match pick_target_folder("选择应用目标目录（批量）") {
        Ok(Some(root)) => root,
        Ok(None) => {
            let list = list_skills()?;
            return Ok(BatchSkillResult {
                ok: false,
                succeeded: 0,
                failed: 0,
                skipped: 0,
                skills: list.skills,
                message: "已取消选择".into(),
                errors: vec![],
            });
        }
        Err(msg) => return Err(msg),
    };

    let mut succeeded = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for id in &ids {
        match install_skill_into_dir(id, &target_root, install_mode) {
            Ok(_) => succeeded += 1,
            Err(msg) => {
                // 目标已存在视为跳过而非失败，避免用户误以为出错。
                if msg.starts_with("目标已存在同名") {
                    skipped += 1;
                } else {
                    failed += 1;
                }
                errors.push(format!("「{}」：{}", id, msg));
            }
        }
    }

    // 应用到目录不改动技能库，但为保持返回结构一致仍带上整表。
    let list = list_skills()?;
    let mut parts: Vec<String> = vec![format!("已{} {} 个", install_mode.label(), succeeded)];
    if skipped > 0 {
        parts.push(format!("跳过 {} 个（同名已存在）", skipped));
    }
    if failed > 0 {
        parts.push(format!("失败 {} 个", failed));
    }
    Ok(BatchSkillResult {
        ok: failed == 0 && skipped == 0,
        succeeded,
        failed,
        skipped,
        skills: list.skills,
        message: parts.join("，"),
        errors,
    })
}

/// 批量应用模式：追加（并入现有应用）或覆盖（以选中 Agent 为最终态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchApplyMode {
    Add,
    Replace,
}

/// 批量把选中技能应用到一组 Agent。
/// - `Add`：把 `agents` 并入每个技能已应用的 Agent（不解除任何现有应用）。
/// - `Replace`：把每个技能的应用最终态设为恰好 `agents`（会解除多余项）。
/// tag 一律不改动；逐项按 `install_mode` 同步。逐项容错。
pub fn batch_apply_skills_to_agents(
    skill_ids: Vec<String>,
    agents: Vec<String>,
    mode: BatchApplyMode,
    install_mode: SkillInstallMode,
) -> Result<BatchSkillResult, String> {
    let ids = sanitize_id_batch(&skill_ids);
    if ids.is_empty() {
        let list = list_skills()?;
        return Ok(BatchSkillResult {
            ok: false,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            skills: list.skills,
            message: "未选择有效技能".into(),
            errors: vec![],
        });
    }

    let selected_agents: BTreeSet<String> = agents
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    // 覆盖模式允许清空（= 从所有 Agent 解除）；追加模式空选无意义。
    if selected_agents.is_empty() && mode == BatchApplyMode::Add {
        let list = list_skills()?;
        return Ok(BatchSkillResult {
            ok: false,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            skills: list.skills,
            message: "未选择要应用的 Agent".into(),
            errors: vec![],
        });
    }

    let lib = skills_library_dir()?;
    // 追加模式需要每个技能的当前应用集，先做一次全量扫描避免重复 IO。
    let applied_map = scan_applied_agents();

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for id in &ids {
        let lib_skill_dir = lib.join(id);
        if !lib_skill_dir.starts_with(&lib) || !is_skill_dir(&lib_skill_dir) {
            failed += 1;
            errors.push(format!("「{}」：技能不存在或缺少 SKILL.md", id));
            continue;
        }

        // 计算该技能的目标最终态。
        let desired: BTreeSet<String> = if mode == BatchApplyMode::Replace {
            selected_agents.clone()
        } else {
            // 并入现有应用：库 id 与 frontmatter 名两处都算作已应用。
            let mut cur: BTreeSet<String> = applied_map.get(id).cloned().unwrap_or_default();
            let doc_name = read_skill_doc(&lib_skill_dir).name;
            if !doc_name.is_empty() && doc_name != *id {
                if let Some(extra) = applied_map.get(&doc_name) {
                    cur.extend(extra.iter().cloned());
                }
            }
            cur.extend(selected_agents.iter().cloned());
            cur
        };

        let install_targets = if mode == BatchApplyMode::Replace {
            &desired
        } else {
            &selected_agents
        };
        let outcome =
            sync_skill_installations(id, &lib_skill_dir, &desired, install_targets, install_mode);
        if outcome.errors.is_empty() {
            succeeded += 1;
        } else {
            failed += 1;
            for e in outcome.errors {
                errors.push(format!("「{}」：{}", id, e));
            }
        }
    }

    let list = list_skills()?;
    let verb = if mode == BatchApplyMode::Replace {
        "覆盖应用"
    } else {
        "追加应用"
    };
    let message = if failed == 0 {
        format!("已以{}{} {} 个技能", install_mode.label(), verb, succeeded)
    } else {
        format!(
            "以{}{} {} 个成功，{} 个失败",
            install_mode.label(),
            verb,
            succeeded,
            failed
        )
    };
    Ok(BatchSkillResult {
        ok: failed == 0,
        succeeded,
        failed,
        skipped: 0,
        skills: list.skills,
        message,
        errors,
    })
}

/// 批量设置选中技能的标签（tag）。只更新 SQLite meta 的 `tag` / `updated_at`，
/// 不触碰软链接、不改动库目录，也不影响各 Agent 的应用状态。逐项容错。
///
/// - `tag` 会做 trim；允许传空串表示「清除标签」。
/// - 技能不在库中（缺失或非技能目录）计入 `failed` 并记录原因。
pub fn batch_set_skill_tag(
    skill_ids: Vec<String>,
    tag: String,
) -> Result<BatchSkillResult, String> {
    let ids = sanitize_id_batch(&skill_ids);
    if ids.is_empty() {
        let list = list_skills()?;
        return Ok(BatchSkillResult {
            ok: false,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            skills: list.skills,
            message: "未选择有效技能".into(),
            errors: vec![],
        });
    }

    let normalized_tag = tag.trim().to_string();
    let lib = skills_library_dir()?;
    let ts = now_secs();

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for id in &ids {
        let lib_skill_dir = lib.join(id);
        if !lib_skill_dir.starts_with(&lib) || !is_skill_dir(&lib_skill_dir) {
            failed += 1;
            errors.push(format!("「{}」：技能不存在或缺少 SKILL.md", id));
            continue;
        }
        let mut meta = db::get_skill_meta(id)
            .unwrap_or_default()
            .unwrap_or_else(default_local_meta);
        meta.tag = normalized_tag.clone();
        meta.updated_at = ts;
        match db::upsert_skill_meta(id, &meta) {
            Ok(()) => succeeded += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("「{}」：{}", id, e));
            }
        }
    }

    let list = list_skills()?;
    let action = if normalized_tag.is_empty() {
        "清除标签"
    } else {
        "设置标签"
    };
    let message = if failed == 0 {
        format!("已为 {} 个技能{}", succeeded, action)
    } else {
        format!("{} {} 个成功，{} 个失败", action, succeeded, failed)
    };
    Ok(BatchSkillResult {
        ok: failed == 0,
        succeeded,
        failed,
        skipped: 0,
        skills: list.skills,
        message,
        errors,
    })
}

/* ===== Apply an existing library skill into a chosen directory ===== */

/// 弹出系统文件夹选择器，返回用户选定并校验通过的目标根目录。
/// - `Ok(Some(root))`：选定的有效目录；
/// - `Ok(None)`：用户取消（不视为错误）；
/// - `Err(msg)`：选择器调用失败或目录无效。
fn pick_target_folder(prompt: &str) -> Result<Option<PathBuf>, String> {
    match crate::platform::pick_folder(prompt)? {
        None => Ok(None),
        Some(path) => {
            let target_root = expand_tilde(&path.to_string_lossy());
            if !target_root.is_dir() {
                return Err("目标目录无效".into());
            }
            Ok(Some(target_root))
        }
    }
}

/// 在 `target_root` 下把库技能 `id` 安装为 `<target_root>/<id>`。
/// 目标已存在同名条目一律不覆盖（避免误伤用户数据）。
fn install_skill_into_dir(
    id: &str,
    target_root: &Path,
    install_mode: SkillInstallMode,
) -> Result<String, String> {
    let lib = skills_library_dir()?;
    let src = lib.join(id);
    if !src.starts_with(&lib) || !is_skill_dir(&src) {
        return Err("技能目录不存在或缺少 SKILL.md".into());
    }
    let dest = target_root.join(id);
    // Defense in depth: resolved dest must stay directly under target_root.
    if dest.parent() != Some(target_root) {
        return Err("非法的技能路径".into());
    }
    // 已存在同名条目一律不覆盖，交由用户先行清理。
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(format!("目标已存在同名条目：{}", dest.display()));
    }
    let source = fs::canonicalize(&src).unwrap_or(src);
    match install_mode {
        SkillInstallMode::Link => {
            crate::platform::symlink_any(&source, &dest)?;
            Ok(format!(
                "已创建软链接 {} → {}",
                dest.display(),
                source.display()
            ))
        }
        SkillInstallMode::Copy => {
            crate::platform::copy_dir_recursive(&source, &dest)?;
            Ok(format!("已完整复制到 {}", dest.display()))
        }
    }
}

/// “应用到目录”：在用户选定目录下生成 `<target>/<id>`，可选择软链接或完整复制。
/// 目标若已存在同名条目（软链接 / 文件 / 目录）一律不覆盖，避免破坏用户数据。
pub fn export_skill_to_dir(
    skill_id: String,
    install_mode: SkillInstallMode,
) -> Result<SkillActionResult, String> {
    let id = skill_id.trim().to_string();
    if id.is_empty() {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "缺少技能标识".into(),
        });
    }
    // Guard against path traversal: id must be a plain directory name.
    if !is_plain_entry_name(&id) {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "非法的技能标识".into(),
        });
    }

    let lib = skills_library_dir()?;
    let src = lib.join(&id);
    if !is_skill_dir(&src) {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "技能目录不存在或缺少 SKILL.md".into(),
        });
    }

    let target_root = match pick_target_folder("选择应用目标目录") {
        Ok(Some(root)) => root,
        Ok(None) => {
            return Ok(SkillActionResult {
                ok: false,
                skill: None,
                message: "已取消选择".into(),
            });
        }
        Err(msg) => {
            return Ok(SkillActionResult {
                ok: false,
                skill: None,
                message: msg,
            });
        }
    };

    match install_skill_into_dir(&id, &target_root, install_mode) {
        Ok(message) => Ok(SkillActionResult {
            ok: true,
            skill: None,
            message,
        }),
        Err(message) => Ok(SkillActionResult {
            ok: false,
            skill: None,
            message,
        }),
    }
}

/* ===== Add: git repo (GitHub / GitCode) ===== */

/// Supported git hosting platforms for remote skill import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitHost {
    Github,
    Gitcode,
}

impl GitHost {
    fn domain(&self) -> &'static str {
        match self {
            GitHost::Github => "github.com",
            GitHost::Gitcode => "gitcode.com",
        }
    }

    fn source(&self) -> SkillSource {
        match self {
            GitHost::Github => SkillSource::Github,
            GitHost::Gitcode => SkillSource::Gitcode,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            GitHost::Github => "GitHub",
            GitHost::Gitcode => "GitCode",
        }
    }
}

#[derive(Debug, Clone)]
struct GitRepoRef {
    host: GitHost,
    owner: String,
    repo: String,
    /// optional subpath inside repo
    path: String,
    repo_url: String,
    /// branch or empty (default)
    branch: String,
}

/// Parse a git repo reference for the given host. Accepts:
/// - https://{domain}/owner/repo(.git)
/// - https://{domain}/owner/repo/tree|blob/branch/path
/// - {domain}/owner/repo
/// - owner/repo (shorthand — assumes `host`)
/// - git@{domain}:owner/repo.git
fn parse_git_url(input: &str, host: GitHost) -> Result<GitRepoRef, String> {
    let domain = host.domain();
    let raw = input.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(format!("{} 仓库地址不能为空", host.label()));
    }

    let mut s = raw.to_string();
    let ssh_prefix = format!("git@{}:", domain);
    if let Some(rest) = s.strip_prefix(&ssh_prefix) {
        s = format!("https://{}/{}", domain, rest.trim_end_matches(".git"));
    }
    let domain_prefix = format!("{}/", domain);
    if s.starts_with(&domain_prefix) {
        s = format!("https://{}", s);
    }

    if !s.contains("://") && s.matches('/').count() == 1 {
        // owner/repo shorthand
        let parts: Vec<&str> = s.splitn(2, '/').collect();
        let repo = parts[1].trim_end_matches(".git").to_string();
        return Ok(GitRepoRef {
            host,
            owner: parts[0].to_string(),
            repo: repo.clone(),
            path: String::new(),
            repo_url: format!("https://{}/{}/{}", domain, parts[0], repo),
            branch: String::new(),
        });
    }

    let url = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(&s);
    let url = url.trim_start_matches("www.");
    if !url.starts_with(&domain_prefix) {
        return Err(format!("仅支持 {} 仓库地址", host.label()));
    }
    let rest = &url[domain_prefix.len()..];
    let segs: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    if segs.len() < 2 {
        return Err(format!(
            "无法解析 {} 仓库，请使用 owner/repo 或完整 URL",
            host.label()
        ));
    }
    let owner = segs[0].to_string();
    let repo = segs[1].trim_end_matches(".git").to_string();
    let mut branch = String::new();
    let mut path = String::new();
    if segs.len() >= 4 && (segs[2] == "tree" || segs[2] == "blob") {
        branch = segs[3].to_string();
        if segs.len() > 4 {
            path = segs[4..].join("/");
            if segs[2] == "blob" {
                // drop file name — take parent as skill path later
                if let Some(parent) = Path::new(&path).parent() {
                    path = parent.to_string_lossy().to_string();
                    if path == "." {
                        path.clear();
                    }
                }
            }
        }
    }
    Ok(GitRepoRef {
        host,
        owner: owner.clone(),
        repo: repo.clone(),
        path,
        repo_url: format!("https://{}/{}/{}", domain, owner, repo),
        branch,
    })
}

fn find_skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if is_skill_dir(root) {
        out.push(root.to_path_buf());
        return out;
    }
    // shallow BFS (depth <= 3) to find skill packages
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = queue.pop() {
        if depth > 3 {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == ".git" || name == "node_modules" || name == ".github" {
                continue;
            }
            if path.is_dir() {
                if is_skill_dir(&path) {
                    out.push(path);
                } else if depth < 3 {
                    queue.push((path, depth + 1));
                }
            }
        }
    }
    out
}

fn git_clone(url: &str, branch: &str, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        let _ = fs::remove_dir_all(dest);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建临时目录失败: {}", e))?;
    }

    let mut args = vec!["clone".to_string(), "--depth".into(), "1".into()];
    if !branch.is_empty() {
        args.push("--branch".into());
        args.push(branch.to_string());
    }
    args.push(url.to_string());
    args.push(dest.to_string_lossy().to_string());

    let output = Command::new("git")
        .args(&args)
        .output()
        .map_err(|e| format!("执行 git 失败（请确认已安装 Git）: {}", e))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone 失败: {}", err.trim()));
    }
    Ok(())
}

fn temp_clone_dir(owner: &str, repo: &str) -> Result<PathBuf, String> {
    let base = std::env::temp_dir().join("agentbuddy-skills");
    fs::create_dir_all(&base).map_err(|e| format!("创建临时目录失败: {}", e))?;
    Ok(base.join(format!("{}-{}-{}", owner, repo, now_secs())))
}

pub fn add_skill_github(url: String, tag: String) -> Result<SkillActionResult, String> {
    let gh = parse_git_url(&url, GitHost::Github)?;
    add_skill_from_git(gh, &tag)
}

pub fn add_skill_gitcode(url: String, tag: String) -> Result<SkillActionResult, String> {
    let gh = parse_git_url(&url, GitHost::Gitcode)?;
    add_skill_from_git(gh, &tag)
}

fn add_skill_from_git(gh: GitRepoRef, tag: &str) -> Result<SkillActionResult, String> {
    let source = gh.host.source();
    let host_label = gh.host.label();
    let clone_url = format!("{}.git", gh.repo_url);
    let tmp = temp_clone_dir(&gh.owner, &gh.repo)?;
    git_clone(&clone_url, &gh.branch, &tmp)?;

    let search_root = if gh.path.is_empty() {
        tmp.clone()
    } else {
        tmp.join(&gh.path)
    };
    if !search_root.exists() {
        let _ = fs::remove_dir_all(&tmp);
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: format!("仓库中不存在路径: {}", gh.path),
        });
    }

    let skill_dirs = find_skill_dirs(&search_root);
    if skill_dirs.is_empty() {
        let _ = fs::remove_dir_all(&tmp);
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "仓库中未找到包含 SKILL.md 的技能目录".into(),
        });
    }

    // Dedupe by skill identity (frontmatter `name`, else dir name) so a repo that
    // vendors the same SKILL.md under multiple paths imports it only once, and so a
    // re-import does not create `name-2` copies of skills already in the library.
    let mut seen_keys: BTreeSet<String> = list_skills()
        .map(|r| {
            r.skills
                .iter()
                .map(|s| s.title.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let dedupe_key = |dir: &Path| -> String {
        let name = read_skill_doc(dir).name.trim().to_lowercase();
        if !name.is_empty() {
            name
        } else {
            dir.file_name()
                .map(|s| s.to_string_lossy().trim().to_lowercase())
                .unwrap_or_default()
        }
    };

    let mut last: Option<SkillRecord> = None;
    let mut count = 0usize;
    let mut skipped_dupe = 0usize;
    for dir in &skill_dirs {
        let key = dedupe_key(dir);
        if !key.is_empty() && !seen_keys.insert(key) {
            skipped_dupe += 1;
            continue;
        }
        let sub_path = dir
            .strip_prefix(&tmp)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut gh_one = gh.clone();
        gh_one.path = sub_path;
        match import_skill_dir(dir, None, source.clone(), Some(gh_one), tag) {
            Ok(rec) => {
                // capture local git HEAD for update checks
                if let Ok(head) = git_rev_parse(dir.parent().unwrap_or(dir)) {
                    let _ = db::update_skill_refs(&rec.id, Some(&head), Some(&head), now_secs());
                }
                count += 1;
                last = Some(rec);
            }
            Err(e) => {
                eprintln!("[skills] import failed {}: {}", dir.display(), e);
            }
        }
    }

    // Prefer capturing commit from clone root
    if let Ok(head) = git_rev_parse(&tmp) {
        if let Some(ref rec) = last {
            let _ = db::update_skill_refs(&rec.id, Some(&head), Some(&head), now_secs());
        }
        // stamp all skills from this owner/repo that still lack local_ref
        if let Ok(map) = db::load_skill_meta_map() {
            for (id, entry) in map {
                if entry.source == source
                    && entry.github_owner == gh.owner
                    && entry.github_repo == gh.repo
                    && entry.local_ref.is_empty()
                {
                    let _ = db::update_skill_refs(&id, Some(&head), Some(&head), now_secs());
                }
            }
        }
    }

    let _ = fs::remove_dir_all(&tmp);

    if count == 0 {
        let message = if skipped_dupe > 0 {
            format!(
                "未导入新技能：{} 个技能已存在于技能库或在仓库中重复",
                skipped_dupe
            )
        } else {
            "导入失败，请检查仓库内容".into()
        };
        return Ok(SkillActionResult {
            ok: skipped_dupe > 0,
            skill: None,
            message,
        });
    }

    let message = if skipped_dupe > 0 {
        format!(
            "已从 {} {}/{} 导入 {} 个技能，跳过 {} 个重复项",
            host_label, gh.owner, gh.repo, count, skipped_dupe
        )
    } else {
        format!(
            "已从 {} {}/{} 导入 {} 个技能",
            host_label, gh.owner, gh.repo, count
        )
    };

    Ok(SkillActionResult {
        ok: true,
        skill: last,
        message,
    })
}

fn git_rev_parse(repo: &Path) -> Result<String, String> {
    // walk up for .git
    let mut cur = repo.to_path_buf();
    let root = loop {
        if cur.join(".git").exists() {
            break cur;
        }
        if !cur.pop() {
            return Err("不是 git 仓库".into());
        }
    };
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("git rev-parse 失败: {}", e))?;
    if !output.status.success() {
        return Err("git rev-parse 失败".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/* ===== Sniff skills from agents ===== */

/// One aggregated skill discovered across agents' skills dirs.
struct SniffScanEntry {
    /// Lowercased identity: frontmatter `name` if present, else dir name.
    identity: String,
    /// Directory name (preferred library id on import).
    directory: String,
    source_path: PathBuf,
    title: String,
    description: String,
    /// Agents where this skill identity was found.
    found_agents: BTreeSet<String>,
}

/// Compute the lowercased identity of a skill dir (frontmatter `name`, else dir name).
fn skill_identity(dir_name: &str, doc: &SkillDoc) -> String {
    if !doc.name.trim().is_empty() {
        doc.name.trim().to_lowercase()
    } else {
        dir_name.trim().to_lowercase()
    }
}

/// Scan every supported agent's skills dir, aggregating by skill identity so a
/// skill installed in multiple agents becomes one entry with merged `found_agents`.
/// Returns (scanned_agents, entries). The first physical dir seen wins as the
/// import source. Mirrors `scan_applied_agents` for shared roots,
/// so both agents are credited.
fn collect_sniff_scan() -> (usize, Vec<SniffScanEntry>) {
    let mut scanned_agents = 0usize;
    let mut map: std::collections::BTreeMap<String, SniffScanEntry> =
        std::collections::BTreeMap::new();

    for target in agent_skills_targets() {
        if !target.supported {
            continue;
        }
        scanned_agents += 1;
        for root in &target.roots {
            if !root.is_dir() {
                continue;
            }
            let entries = match fs::read_dir(root) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_skill_dir(&path) {
                    continue;
                }
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name.starts_with('.') || dir_name.starts_with("__agentbuddy") {
                    continue;
                }
                let doc = read_skill_doc(&path);
                let identity = skill_identity(&dir_name, &doc);
                let slot = map
                    .entry(identity.clone())
                    .or_insert_with(|| SniffScanEntry {
                        identity,
                        directory: dir_name.clone(),
                        source_path: path.clone(),
                        title: if !doc.name.trim().is_empty() {
                            doc.name.trim().to_string()
                        } else {
                            dir_name.clone()
                        },
                        description: doc.description.clone(),
                        found_agents: BTreeSet::new(),
                    });
                slot.found_agents.insert(target.agent.to_string());
            }
        }
    }

    (scanned_agents, map.into_values().collect())
}

/// Identities already present in the library: lowercased dir id + frontmatter name.
fn existing_library_identities(lib: &Path) -> BTreeSet<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    let entries = match fs::read_dir(lib) {
        Ok(e) => e,
        Err(_) => return set,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_skill_dir(&path) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if id.starts_with('.') {
            continue;
        }
        set.insert(id.trim().to_lowercase());
        let doc = read_skill_doc(&path);
        if !doc.name.trim().is_empty() {
            set.insert(doc.name.trim().to_lowercase());
        }
    }
    set
}

/// True when a scanned skill already exists in the library (by identity or dir name).
fn sniff_entry_exists(entry: &SniffScanEntry, existing: &BTreeSet<String>) -> bool {
    existing.contains(&entry.identity) || existing.contains(&entry.directory.trim().to_lowercase())
}

/// Scan agents and build an import preview (no files copied). Each item is
/// tagged "import" or "skip_exists" and lists the agents it was found in.
pub fn preview_sniff_skills() -> Result<SniffPreviewResult, String> {
    let _ = db::migrate_skills_meta_json_if_present();
    let lib = skills_library_dir()?;
    let (scanned_agents, scan) = collect_sniff_scan();
    let existing = existing_library_identities(&lib);

    let mut items: Vec<SniffPreviewItem> = Vec::new();
    for entry in &scan {
        let exists = sniff_entry_exists(entry, &existing);
        let (status, status_label) = if exists {
            ("skip_exists", "技能库已存在，将跳过")
        } else {
            ("import", "将导入")
        };
        items.push(SniffPreviewItem {
            key: entry.source_path.to_string_lossy().to_string(),
            directory: entry.directory.clone(),
            title: entry.title.clone(),
            description: entry.description.clone(),
            source_path: entry.source_path.to_string_lossy().to_string(),
            found_agents: entry.found_agents.iter().cloned().collect(),
            status: status.to_string(),
            status_label: status_label.to_string(),
        });
    }

    items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let importable = items.iter().filter(|i| i.status == "import").count();
    let skip_exists = items.iter().filter(|i| i.status == "skip_exists").count();
    let total = items.len();

    let message = if total == 0 {
        format!("已扫描 {} 个 Agent，未发现任何 Skill", scanned_agents)
    } else {
        format!(
            "已扫描 {} 个 Agent，发现 {} 个技能：可导入 {}，已存在跳过 {}",
            scanned_agents, total, importable, skip_exists
        )
    };

    Ok(SniffPreviewResult {
        ok: true,
        items,
        scanned_agents,
        total,
        importable,
        skip_exists,
        message,
    })
}

/// Import the selected sniffed skills (by `key` = source path). Re-scans and
/// re-checks existence so a stale selection can't overwrite or duplicate.
pub fn import_sniffed_skills(keys: Vec<String>) -> Result<SniffImportResult, String> {
    if keys.is_empty() {
        return Ok(SniffImportResult {
            ok: true,
            imported: 0,
            skipped: 0,
            failed: 0,
            skills: list_skills()?.skills,
            message: "未选择任何技能".into(),
            errors: vec![],
        });
    }

    let lib = skills_library_dir()?;
    let (_scanned, scan) = collect_sniff_scan();
    let existing = existing_library_identities(&lib);
    let wanted: BTreeSet<&str> = keys.iter().map(|s| s.as_str()).collect();

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for entry in &scan {
        let key = entry.source_path.to_string_lossy().to_string();
        if !wanted.contains(key.as_str()) {
            continue;
        }
        // Re-check: another import may have added it since preview.
        if sniff_entry_exists(entry, &existing) {
            skipped += 1;
            continue;
        }
        match import_skill_dir(
            &entry.source_path,
            Some(&entry.directory),
            SkillSource::Local,
            None,
            "",
        ) {
            Ok(_rec) => imported += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("{}: {}", entry.title, e));
            }
        }
    }

    let listed = list_skills()?;
    let message = if failed == 0 {
        format!("导入完成：新增 {} 个，跳过 {} 个", imported, skipped)
    } else {
        format!(
            "导入结束：新增 {}，跳过 {}，失败 {}",
            imported, skipped, failed
        )
    };

    Ok(SniffImportResult {
        ok: failed == 0,
        imported,
        skipped,
        failed,
        skills: listed.skills,
        message,
        errors,
    })
}

pub fn sniff_skills() -> Result<SkillSniffResult, String> {
    let _ = db::migrate_skills_meta_json_if_present();
    let lib = skills_library_dir()?;
    let mut meta_map = db::load_skill_meta_map().unwrap_or_default();
    let mut imported = 0usize;
    let mut found_entries = 0usize;
    let mut scanned_agents = 0usize;
    // Avoid double-importing shared roots (shared_root 标识相同者)
    let mut seen_paths: BTreeSet<String> = BTreeSet::new();

    for target in agent_skills_targets() {
        if !target.supported {
            continue;
        }
        scanned_agents += 1;
        for root in &target.roots {
            let key = root.to_string_lossy().to_string();
            if !seen_paths.insert(key) {
                continue;
            }
            if !root.is_dir() {
                continue;
            }
            for entry in fs::read_dir(root).into_iter().flatten().flatten() {
                let path = entry.path();
                if !is_skill_dir(&path) {
                    continue;
                }
                let id_hint = entry.file_name().to_string_lossy().to_string();
                if id_hint.starts_with('.') || id_hint.starts_with("__agentbuddy") {
                    continue;
                }
                found_entries += 1;

                // already in library by same dir name?
                if lib.join(&id_hint).exists() && is_skill_dir(&lib.join(&id_hint)) {
                    // ensure meta exists
                    if !meta_map.contains_key(&id_hint) {
                        let created = default_local_meta();
                        if let Ok(()) = db::upsert_skill_meta(&id_hint, &created) {
                            meta_map.insert(id_hint.clone(), created);
                        }
                    }
                    continue;
                }

                // match by frontmatter name against existing library
                let doc = read_skill_doc(&path);
                let already = meta_map.keys().any(|k| {
                    k == &id_hint || (!doc.name.is_empty() && k == &doc.name) || {
                        let p = lib.join(k);
                        let d = read_skill_doc(&p);
                        !doc.name.is_empty() && d.name == doc.name
                    }
                });
                if already {
                    continue;
                }

                match import_skill_dir(&path, Some(&id_hint), SkillSource::Local, None, "") {
                    Ok(rec) => {
                        imported += 1;
                        meta_map.insert(
                            rec.id.clone(),
                            SkillMetaEntry {
                                source: SkillSource::Local,
                                created_at: rec.created_at,
                                updated_at: rec.updated_at,
                                ..Default::default()
                            },
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[skills] sniff import {} from {} failed: {}",
                            id_hint, target.agent, e
                        );
                    }
                }
            }
        }
    }

    let listed = list_skills()?;
    let message = if imported > 0 {
        format!(
            "嗅探完成：扫描 {} 个 Agent，发现 {} 个技能，新导入 {} 个",
            scanned_agents, found_entries, imported
        )
    } else {
        format!(
            "嗅探完成：扫描 {} 个 Agent，发现 {} 个技能，无新导入",
            scanned_agents, found_entries
        )
    };

    Ok(SkillSniffResult {
        skills: listed.skills,
        scanned_agents,
        found_entries,
        imported,
        message,
    })
}

/* ===== Check updates (github) ===== */

fn github_default_branch_sha(owner: &str, repo: &str) -> Result<String, String> {
    // Use GitHub API: GET /repos/{owner}/{repo}/commits/HEAD
    // Fallback: /repos/{owner}/{repo}
    let url = format!(
        "https://api.github.com/repos/{}/{}/commits/HEAD",
        owner, repo
    );
    let client = {
        let builder = reqwest::blocking::Client::builder()
            .user_agent("AgentBuddy/0.1 (skills-update-check)")
            .timeout(std::time::Duration::from_secs(15));
        crate::http_client::apply_proxy(builder)?
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?
    };

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("请求 GitHub 失败: {}", e))?;

    if !resp.status().is_success() {
        // fallback repo info
        let repo_url = format!("https://api.github.com/repos/{}/{}", owner, repo);
        let resp2 = client
            .get(&repo_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| format!("请求 GitHub 失败: {}", e))?;
        if !resp2.status().is_success() {
            return Err(format!("GitHub API 返回 {}", resp2.status()));
        }
        let v: serde_json::Value = resp2
            .json()
            .map_err(|e| format!("解析 GitHub 响应失败: {}", e))?;
        if let Some(sha) = v
            .pointer("/default_branch")
            .and_then(|b| b.as_str())
            .map(|branch| {
                let commits = format!(
                    "https://api.github.com/repos/{}/{}/commits/{}",
                    owner, repo, branch
                );
                client
                    .get(&commits)
                    .header("Accept", "application/vnd.github+json")
                    .send()
                    .ok()
                    .and_then(|r| r.json::<serde_json::Value>().ok())
                    .and_then(|j| j.get("sha").and_then(|s| s.as_str()).map(|s| s.to_string()))
            })
            .flatten()
        {
            return Ok(sha);
        }
        return Err("无法获取远程提交信息".into());
    }

    let v: serde_json::Value = resp
        .json()
        .map_err(|e| format!("解析 GitHub 响应失败: {}", e))?;
    v.get("sha")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "GitHub 响应缺少 sha".into())
}

/// Resolve the HEAD commit of a repo's default branch via `git ls-remote`.
/// Host-agnostic and token-free — used for GitCode (and as GitHub fallback).
fn git_ls_remote_head(repo_url: &str) -> Result<String, String> {
    let clone_url = if repo_url.ends_with(".git") {
        repo_url.to_string()
    } else {
        format!("{}.git", repo_url)
    };
    let output = Command::new("git")
        .args(["ls-remote", &clone_url, "HEAD"])
        .output()
        .map_err(|e| format!("执行 git ls-remote 失败（请确认已安装 Git）: {}", e))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git ls-remote 失败: {}", err.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(|sha| sha.to_string())
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| "git ls-remote 未返回 HEAD".into())
}

/// Fetch the latest remote commit for a skill's repo, dispatching by source.
fn remote_head_for_source(
    source: &SkillSource,
    owner: &str,
    repo: &str,
    repo_url: &str,
) -> Result<String, String> {
    match source {
        SkillSource::Github => github_default_branch_sha(owner, repo),
        SkillSource::Gitcode => {
            let url = if repo_url.is_empty() {
                format!("https://gitcode.com/{}/{}", owner, repo)
            } else {
                repo_url.to_string()
            };
            git_ls_remote_head(&url)
        }
        SkillSource::Local => Err("本地技能无远程源".into()),
    }
}

pub fn check_skill_updates() -> Result<SkillUpdateCheckResult, String> {
    let listed = list_skills()?;
    let mut checked = 0usize;
    let mut updates = 0usize;
    let mut update_ids: BTreeSet<String> = BTreeSet::new();

    // group by source+owner/repo to avoid redundant remote calls
    let mut repo_sha: HashMap<(String, String, String), Result<String, String>> = HashMap::new();

    for skill in &listed.skills {
        if !skill.source.is_remote() {
            continue;
        }
        if skill.github_owner.is_empty() || skill.github_repo.is_empty() {
            continue;
        }
        checked += 1;
        let source_key = match skill.source {
            SkillSource::Github => "github",
            SkillSource::Gitcode => "gitcode",
            SkillSource::Local => continue,
        };
        let key = (
            source_key.to_string(),
            skill.github_owner.clone(),
            skill.github_repo.clone(),
        );
        if !repo_sha.contains_key(&key) {
            let sha = remote_head_for_source(
                &skill.source,
                &skill.github_owner,
                &skill.github_repo,
                &skill.repo_url,
            );
            repo_sha.insert(key.clone(), sha);
        }
        let remote = match repo_sha.get(&key) {
            Some(Ok(sha)) => sha.clone(),
            Some(Err(_)) | None => continue,
        };

        let mut entry = match db::get_skill_meta(&skill.id)? {
            Some(e) => e,
            None => continue,
        };
        let local = entry.local_ref.clone();
        entry.remote_ref = remote.clone();
        entry.updated_at = now_secs();
        if !local.is_empty() && local != remote {
            updates += 1;
            update_ids.insert(skill.id.clone());
        } else if local.is_empty() {
            // no baseline — store remote as baseline without flagging
            entry.local_ref = remote;
        }
        db::upsert_skill_meta(&skill.id, &entry)?;
    }

    let mut skills = list_skills()?.skills;
    for s in &mut skills {
        if update_ids.contains(&s.id) {
            s.update_available = true;
        }
    }

    let message = if checked == 0 {
        "没有来自 GitHub / GitCode 的技能可检查".into()
    } else if updates > 0 {
        format!("已检查 {} 个远程技能，其中 {} 个有更新", checked, updates)
    } else {
        format!("已检查 {} 个远程技能，全部为最新", checked)
    };

    Ok(SkillUpdateCheckResult {
        skills,
        checked,
        updates,
        message,
    })
}

/* ===== Tests ===== */

/* ===== Migrate from CC Switch (~/.cc-switch) ===== */

fn cc_switch_root() -> PathBuf {
    expand_tilde("~/.cc-switch")
}

fn cc_switch_db_path() -> PathBuf {
    cc_switch_root().join("cc-switch.db")
}

fn cc_switch_skills_dir() -> PathBuf {
    cc_switch_root().join("skills")
}

/// Map cc-switch enabled_* flags to AgentBuddy sniff agent names.
fn map_cc_enabled_agents(
    enabled_claude: bool,
    enabled_codex: bool,
    enabled_gemini: bool,
    enabled_opencode: bool,
    enabled_hermes: bool,
) -> Vec<String> {
    let mut agents = Vec::new();
    if enabled_claude {
        agents.push("claude-code".into());
    }
    if enabled_codex {
        agents.push("codex".into());
    }
    if enabled_gemini {
        // Gemini-family config root used by Antigravity
        agents.push("antigravity".into());
    }
    if enabled_opencode {
        agents.push("opencode".into());
    }
    // hermes has no AgentBuddy mapping yet
    let _ = enabled_hermes;
    agents
}

fn github_path_from_readme_url(readme_url: &str, owner: &str, repo: &str) -> String {
    // e.g. https://github.com/flutter/skills/blob/main/skills/flutter-building-forms/SKILL.md
    let marker = format!("github.com/{}/{}/", owner, repo);
    let Some(idx) = readme_url.find(&marker) else {
        return String::new();
    };
    let rest = &readme_url[idx + marker.len()..];
    // rest: blob/main/skills/foo/SKILL.md  or tree/main/skills/foo
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        return String::new();
    }
    // skip blob|tree + branch
    let path_parts = &parts[2..];
    let mut path = path_parts.join("/");
    if path.ends_with("/SKILL.md") {
        path = path.trim_end_matches("/SKILL.md").to_string();
    } else if path.ends_with("SKILL.md") {
        path = path
            .trim_end_matches("SKILL.md")
            .trim_end_matches('/')
            .to_string();
    }
    path
}

struct CcDbRow {
    id: String,
    name: String,
    description: String,
    directory: String,
    repo_owner: String,
    repo_name: String,
    #[allow(dead_code)]
    repo_branch: String,
    readme_url: String,
    enabled_claude: bool,
    enabled_codex: bool,
    enabled_gemini: bool,
    enabled_opencode: bool,
    enabled_hermes: bool,
}

fn read_cc_switch_skill_rows() -> Result<Vec<CcDbRow>, String> {
    let db_path = cc_switch_db_path();
    if !db_path.exists() {
        return Err(format!("未找到 CC Switch 数据库: {}", db_path.display()));
    }

    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = rusqlite::Connection::open_with_flags(&db_path, flags)
        .map_err(|e| format!("打开 cc-switch.db 失败: {}", e))?;

    // Column set evolved across cc-switch versions; select with COALESCE for safety.
    let mut stmt = conn
        .prepare(
            "SELECT id, name,
                    COALESCE(description, '') AS description,
                    directory,
                    COALESCE(repo_owner, '') AS repo_owner,
                    COALESCE(repo_name, '') AS repo_name,
                    COALESCE(repo_branch, 'main') AS repo_branch,
                    COALESCE(readme_url, '') AS readme_url,
                    COALESCE(enabled_claude, 0) AS enabled_claude,
                    COALESCE(enabled_codex, 0) AS enabled_codex,
                    COALESCE(enabled_gemini, 0) AS enabled_gemini,
                    COALESCE(enabled_opencode, 0) AS enabled_opencode,
                    COALESCE(enabled_hermes, 0) AS enabled_hermes
             FROM skills
             ORDER BY name COLLATE NOCASE",
        )
        .map_err(|e| format!("查询 cc-switch skills 失败（表结构可能不兼容）: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(CcDbRow {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                directory: row.get(3)?,
                repo_owner: row.get(4)?,
                repo_name: row.get(5)?,
                repo_branch: row.get(6)?,
                readme_url: row.get(7)?,
                enabled_claude: row.get::<_, i32>(8)? != 0,
                enabled_codex: row.get::<_, i32>(9)? != 0,
                enabled_gemini: row.get::<_, i32>(10)? != 0,
                enabled_opencode: row.get::<_, i32>(11)? != 0,
                enabled_hermes: row.get::<_, i32>(12)? != 0,
            })
        })
        .map_err(|e| format!("读取 cc-switch skills 失败: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("解析 cc-switch skills 行失败: {}", e))?;

    Ok(rows)
}

fn build_cc_preview_item(row: &CcDbRow, lib: &Path) -> CcSwitchPreviewItem {
    let dir_name = if row.directory.trim().is_empty() {
        // fallback: strip local: prefix from id
        row.id
            .strip_prefix("local:")
            .unwrap_or(&row.id)
            .rsplit(['/', ':'])
            .next()
            .unwrap_or(&row.id)
            .to_string()
    } else {
        row.directory.trim().to_string()
    };

    let source_path = cc_switch_skills_dir().join(&dir_name);
    let has_github = !row.repo_owner.trim().is_empty() && !row.repo_name.trim().is_empty();
    let source = if has_github {
        SkillSource::Github
    } else {
        SkillSource::Local
    };
    let github_owner = row.repo_owner.trim().to_string();
    let github_repo = row.repo_name.trim().to_string();
    let repo_url = if has_github {
        format!("https://github.com/{}/{}", github_owner, github_repo)
    } else {
        String::new()
    };
    let github_path = if has_github {
        github_path_from_readme_url(&row.readme_url, &github_owner, &github_repo)
    } else {
        String::new()
    };

    let mut title = row.name.trim().to_string();
    let mut description = row.description.trim().to_string();
    if source_path.exists() && is_skill_dir(&source_path) {
        let doc = read_skill_doc(&source_path);
        if title.is_empty() && !doc.name.is_empty() {
            title = doc.name;
        }
        if description.is_empty() && !doc.description.is_empty() {
            description = doc.description;
        }
    }
    if title.is_empty() {
        title = dir_name.clone();
    }

    let enabled_agents = map_cc_enabled_agents(
        row.enabled_claude,
        row.enabled_codex,
        row.enabled_gemini,
        row.enabled_opencode,
        row.enabled_hermes,
    );

    let (status, status_label) = if !source_path.exists() || !is_skill_dir(&source_path) {
        ("missing".to_string(), "源目录缺失或无 SKILL.md".to_string())
    } else if lib.join(&dir_name).exists() && is_skill_dir(&lib.join(&dir_name)) {
        (
            "skip_exists".to_string(),
            "技能库已存在，将跳过".to_string(),
        )
    } else {
        ("import".to_string(), "将导入".to_string())
    };

    CcSwitchPreviewItem {
        cc_id: row.id.clone(),
        directory: dir_name,
        title,
        description,
        source,
        repo_url,
        github_owner,
        github_repo,
        github_path,
        source_path: source_path.to_string_lossy().to_string(),
        enabled_agents,
        status,
        status_label,
    }
}

/// Scan ~/.cc-switch skills dir + cc-switch.db and build a migration preview.
pub fn preview_cc_switch_skills() -> Result<CcSwitchPreviewResult, String> {
    let root = cc_switch_root();
    if !root.exists() {
        return Ok(CcSwitchPreviewResult {
            ok: false,
            items: vec![],
            total: 0,
            importable: 0,
            skip_exists: 0,
            missing: 0,
            message: "未找到 ~/.cc-switch 目录，请确认已安装并使用过 CC Switch".into(),
            cc_switch_root: root.to_string_lossy().to_string(),
        });
    }

    let lib = skills_library_dir()?;
    let mut items: Vec<CcSwitchPreviewItem> = Vec::new();
    let mut seen_dirs: BTreeSet<String> = BTreeSet::new();

    // Prefer DB rows (richer metadata); fall back to disk-only if DB unreadable.
    match read_cc_switch_skill_rows() {
        Ok(rows) => {
            for row in &rows {
                let item = build_cc_preview_item(row, &lib);
                seen_dirs.insert(item.directory.clone());
                items.push(item);
            }
        }
        Err(e) => {
            eprintln!(
                "[skills] cc-switch.db read failed, disk-only fallback: {}",
                e
            );
        }
    }

    // Disk skills not present in DB (or DB missing entirely)
    let skills_dir = cc_switch_skills_dir();
    if skills_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_skill_dir(&path) {
                    continue;
                }
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name.starts_with('.') || seen_dirs.contains(&dir_name) {
                    continue;
                }
                let doc = read_skill_doc(&path);
                let title = if !doc.name.is_empty() {
                    doc.name
                } else {
                    dir_name.clone()
                };
                let status = if lib.join(&dir_name).exists() && is_skill_dir(&lib.join(&dir_name)) {
                    ("skip_exists", "技能库已存在，将跳过")
                } else {
                    ("import", "将导入")
                };
                items.push(CcSwitchPreviewItem {
                    cc_id: format!("disk:{}", dir_name),
                    directory: dir_name.clone(),
                    title,
                    description: doc.description,
                    source: SkillSource::Local,
                    repo_url: String::new(),
                    github_owner: String::new(),
                    github_repo: String::new(),
                    github_path: String::new(),
                    source_path: path.to_string_lossy().to_string(),
                    enabled_agents: vec![],
                    status: status.0.into(),
                    status_label: status.1.into(),
                });
            }
        }
    }

    items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let importable = items.iter().filter(|i| i.status == "import").count();
    let skip_exists = items.iter().filter(|i| i.status == "skip_exists").count();
    let missing = items.iter().filter(|i| i.status == "missing").count();
    let total = items.len();

    let message = if total == 0 {
        "CC Switch 中未发现可迁移的 Skills".into()
    } else {
        format!(
            "发现 {} 个技能：可导入 {}，已存在跳过 {}，源缺失 {}",
            total, importable, skip_exists, missing
        )
    };

    Ok(CcSwitchPreviewResult {
        ok: true,
        items,
        total,
        importable,
        skip_exists,
        missing,
        message,
        cc_switch_root: root.to_string_lossy().to_string(),
    })
}

/// Import selected cc-switch skills (by `cc_id` from preview). Empty list → no-op.
pub fn migrate_cc_switch_skills(cc_ids: Vec<String>) -> Result<CcSwitchMigrateResult, String> {
    if cc_ids.is_empty() {
        return Ok(CcSwitchMigrateResult {
            ok: true,
            imported: 0,
            skipped: 0,
            failed: 0,
            skills: list_skills()?.skills,
            message: "未选择任何技能".into(),
            errors: vec![],
        });
    }

    let preview = preview_cc_switch_skills()?;
    if !preview.ok && preview.items.is_empty() {
        return Ok(CcSwitchMigrateResult {
            ok: false,
            imported: 0,
            skipped: 0,
            failed: 0,
            skills: list_skills()?.skills,
            message: preview.message,
            errors: vec![],
        });
    }

    let wanted: BTreeSet<&str> = cc_ids.iter().map(|s| s.as_str()).collect();
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    for item in &preview.items {
        if !wanted.contains(item.cc_id.as_str()) {
            continue;
        }
        if item.status == "skip_exists" {
            skipped += 1;
            continue;
        }
        if item.status == "missing" {
            failed += 1;
            errors.push(format!("{}: {}", item.title, item.status_label));
            continue;
        }

        let src = PathBuf::from(&item.source_path);
        let preferred = item.directory.as_str();
        let source = item.source.clone();
        let git = if source == SkillSource::Github {
            Some(GitRepoRef {
                host: GitHost::Github,
                owner: item.github_owner.clone(),
                repo: item.github_repo.clone(),
                path: item.github_path.clone(),
                repo_url: item.repo_url.clone(),
                branch: String::new(),
            })
        } else {
            None
        };

        match import_skill_dir(&src, Some(preferred), source, git, "") {
            Ok(_rec) => {
                imported += 1;
            }
            Err(e) => {
                failed += 1;
                errors.push(format!("{}: {}", item.title, e));
            }
        }
    }

    let listed = list_skills()?;
    let message = if failed == 0 {
        format!("迁移完成：成功导入 {} 个，跳过 {} 个", imported, skipped)
    } else {
        format!(
            "迁移结束：成功 {}，跳过 {}，失败 {}",
            imported, skipped, failed
        )
    };

    Ok(CcSwitchMigrateResult {
        ok: failed == 0,
        imported,
        skipped,
        failed,
        skills: listed.skills,
        message,
        errors,
    })
}

/// Open an external URL in the system default browser.
/// Only http/https URLs are allowed to avoid handing arbitrary schemes/args to the shell.
pub fn open_external_url(url: String) -> Result<(), String> {
    crate::platform::open_url(&url)
}

/* ===== Update: pull a remote skill to the repo's latest ===== */

/// 把一个来自 GitHub / GitCode 的技能更新到远端仓库最新内容：
/// 重新浅克隆 → 定位子目录 → 在暂存目录构建新内容 → 原子替换库目录 →
/// 对齐 `local_ref` 到远端 HEAD。本地技能不支持更新。
///
/// 采用「先复制到暂存目录、成功后再替换」而非「先删后拷」，避免克隆或复制
/// 中途失败导致既有技能被清空的数据丢失。
pub fn update_skill(skill_id: String) -> Result<SkillActionResult, String> {
    let id = skill_id.trim().to_string();
    // Guard against path traversal: id must be a plain directory name.
    if !is_plain_entry_name(&id) {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "非法的技能标识".into(),
        });
    }
    let lib = skills_library_dir()?;
    let dest = lib.join(&id);
    if !dest.starts_with(&lib) || !is_skill_dir(&dest) {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "技能不存在或缺少 SKILL.md".into(),
        });
    }
    let meta = db::get_skill_meta(&id)?.ok_or_else(|| format!("技能元数据缺失: {}", id))?;
    if !meta.source.is_remote() {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "仅支持更新来自 GitHub / GitCode 的技能".into(),
        });
    }
    if meta.github_owner.is_empty() || meta.github_repo.is_empty() {
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "缺少仓库信息，无法更新".into(),
        });
    }

    // 借用 source（unit 变体无字段），避免 move 掉 meta 以便后续复用。
    let host = match &meta.source {
        SkillSource::Github => GitHost::Github,
        SkillSource::Gitcode => GitHost::Gitcode,
        SkillSource::Local => {
            return Ok(SkillActionResult {
                ok: false,
                skill: None,
                message: "本地技能无远程源".into(),
            })
        }
    };
    let repo_url = if meta.repo_url.is_empty() {
        format!(
            "https://{}/{}/{}",
            host.domain(),
            meta.github_owner,
            meta.github_repo
        )
    } else {
        meta.repo_url.clone()
    };
    let clone_url = format!("{}.git", repo_url.trim_end_matches(".git"));

    let tmp = temp_clone_dir(&meta.github_owner, &meta.github_repo)?;
    if let Err(e) = git_clone(&clone_url, "", &tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }

    // 定位仓库内该技能目录：优先 github_path，其次浅搜第一个 skill 目录。
    let search_root = if meta.github_path.is_empty() {
        tmp.clone()
    } else {
        tmp.join(&meta.github_path)
    };
    let src_dir = if is_skill_dir(&search_root) {
        Some(search_root.clone())
    } else {
        find_skill_dirs(&search_root).into_iter().next()
    };
    let Some(src_dir) = src_dir else {
        let _ = fs::remove_dir_all(&tmp);
        return Ok(SkillActionResult {
            ok: false,
            skill: None,
            message: "远端仓库中未找到该技能目录（可能已被移动或删除）".into(),
        });
    };

    // 先在暂存目录构建新内容，成功后再原子替换，避免中途失败清空既有技能。
    let staging = lib.join(format!(".{}.update-{}", id, std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    if let Err(e) = copy_dir_recursive(&src_dir, &staging) {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    if let Err(e) = fs::remove_dir_all(&dest) {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("清理旧技能目录失败: {}", e));
    }
    if let Err(e) = fs::rename(&staging, &dest) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!(
            "替换技能目录失败: {}（新内容暂存于 {}，可手动恢复）",
            e,
            staging.display()
        ));
    }

    // 对齐 ref 到远端 HEAD（拿不到也不致命，仅刷新 updated_at）。
    let ts = now_secs();
    match git_rev_parse(&tmp) {
        Ok(head) if !head.is_empty() => {
            let _ = db::update_skill_refs(&id, Some(&head), Some(&head), ts);
        }
        _ => {
            let _ = db::update_skill_refs(&id, None, None, ts);
        }
    }
    let _ = fs::remove_dir_all(&tmp);

    let meta2 = db::get_skill_meta(&id)?.unwrap_or(meta);
    let applied = applied_after_for(&id, &dest);
    let record = build_record(&id, &dest, &meta2, &applied, false);
    let title = record.title.clone();
    Ok(SkillActionResult {
        ok: true,
        skill: Some(record),
        message: format!("已将「{}」更新到远端最新", title),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter() {
        let md = r#"---
name: demo-skill
description: A short demo skill for tests.
---

# Demo

body
"#;
        let doc = parse_skill_md(md);
        assert_eq!(doc.name, "demo-skill");
        assert_eq!(doc.description, "A short demo skill for tests.");
    }

    #[test]
    fn parse_github_simple() {
        let g = parse_git_url("https://github.com/foo/bar", GitHost::Github).unwrap();
        assert_eq!(g.owner, "foo");
        assert_eq!(g.repo, "bar");
        assert!(g.path.is_empty());
        assert_eq!(g.repo_url, "https://github.com/foo/bar");
    }

    #[test]
    fn parse_github_tree() {
        let g = parse_git_url(
            "https://github.com/foo/bar/tree/main/skills/baz",
            GitHost::Github,
        )
        .unwrap();
        assert_eq!(g.owner, "foo");
        assert_eq!(g.repo, "bar");
        assert_eq!(g.branch, "main");
        assert_eq!(g.path, "skills/baz");
    }

    #[test]
    fn parse_owner_repo() {
        let g = parse_git_url("anthropics/skills", GitHost::Github).unwrap();
        assert_eq!(g.owner, "anthropics");
        assert_eq!(g.repo, "skills");
    }

    #[test]
    fn parse_gitcode_full_url() {
        let g = parse_git_url(
            "https://gitcode.com/HarmonyOS_Skills/harmonyos-agent-skills",
            GitHost::Gitcode,
        )
        .unwrap();
        assert_eq!(g.host, GitHost::Gitcode);
        assert_eq!(g.owner, "HarmonyOS_Skills");
        assert_eq!(g.repo, "harmonyos-agent-skills");
        assert_eq!(
            g.repo_url,
            "https://gitcode.com/HarmonyOS_Skills/harmonyos-agent-skills"
        );
        assert_eq!(g.host.source(), SkillSource::Gitcode);
    }

    #[test]
    fn parse_gitcode_tree_and_shorthand() {
        let g = parse_git_url(
            "gitcode.com/owner/repo/tree/master/07-tools/foo",
            GitHost::Gitcode,
        )
        .unwrap();
        assert_eq!(g.owner, "owner");
        assert_eq!(g.repo, "repo");
        assert_eq!(g.branch, "master");
        assert_eq!(g.path, "07-tools/foo");

        let g2 = parse_git_url("owner/repo", GitHost::Gitcode).unwrap();
        assert_eq!(g2.owner, "owner");
        assert_eq!(g2.repo, "repo");
        assert_eq!(g2.repo_url, "https://gitcode.com/owner/repo");
    }

    #[test]
    fn parse_git_url_rejects_wrong_host() {
        // A github URL must not parse under the GitCode host.
        assert!(parse_git_url("https://github.com/foo/bar", GitHost::Gitcode).is_err());
    }

    #[test]
    fn parse_block_scalar_description() {
        let md = "---\nname: hmos-arkui\ndescription: |\n  第一段说明文本。\n\n  触发场景：xyz\n---\n\n# Body\n";
        let doc = parse_skill_md(md);
        assert_eq!(doc.name, "hmos-arkui");
        assert_eq!(doc.description, "第一段说明文本。");
    }

    #[test]
    fn github_path_from_readme() {
        let p = github_path_from_readme_url(
            "https://github.com/flutter/skills/blob/main/skills/flutter-building-forms/SKILL.md",
            "flutter",
            "skills",
        );
        assert_eq!(p, "skills/flutter-building-forms");
    }

    #[test]
    fn map_enabled_agents() {
        let a = map_cc_enabled_agents(true, true, false, true, true);
        assert_eq!(a, vec!["claude-code", "codex", "opencode"]);
    }

    #[test]
    fn preview_cc_switch_if_present() {
        let root = cc_switch_root();
        if !root.exists() {
            return;
        }
        let res = preview_cc_switch_skills().expect("preview");
        assert!(res.ok, "preview message: {}", res.message);
        assert!(res.total > 0, "expected skills in ~/.cc-switch");
        // statuses only use known tokens
        for item in &res.items {
            assert!(
                matches!(item.status.as_str(), "import" | "skip_exists" | "missing"),
                "unexpected status {} for {}",
                item.status,
                item.cc_id
            );
        }
    }

    #[test]
    fn migrate_one_cc_switch_skill_if_present() {
        let root = cc_switch_root();
        if !root.exists() {
            return;
        }
        let preview = preview_cc_switch_skills().expect("preview");
        let candidate = preview.items.iter().find(|i| i.status == "import").cloned();
        let Some(item) = candidate else {
            eprintln!("no importable cc-switch skill; skip migrate smoke");
            return;
        };

        // Prefer a small-looking skill (name starts with a common short one) if present
        let pick = preview
            .items
            .iter()
            .find(|i| i.status == "import" && i.directory == "find-skills")
            .cloned()
            .unwrap_or(item);

        let res = migrate_cc_switch_skills(vec![pick.cc_id.clone()]).expect("migrate");
        assert!(
            res.imported >= 1,
            "expected import of {}, got {:?}",
            pick.directory,
            res
        );
        let lib = skills_library_dir().expect("lib");
        assert!(
            is_skill_dir(&lib.join(&pick.directory)),
            "skill dir missing after migrate: {}",
            pick.directory
        );
        let meta = crate::db::get_skill_meta(&pick.directory)
            .expect("meta q")
            .expect("meta row");
        if pick.source == SkillSource::Github {
            assert_eq!(meta.github_owner, pick.github_owner);
            assert_eq!(meta.github_repo, pick.github_repo);
        } else {
            assert!(matches!(meta.source, SkillSource::Local));
        }
        // cleanup so re-runs stay importable
        let _ = fs::remove_dir_all(lib.join(&pick.directory));
        let _ = crate::db::delete_skill_meta(&pick.directory);
    }

    /// Offline structural check against a local clone of the HarmonyOS GitCode
    /// repo (if present at /tmp/gitcode-probe). Validates that the shallow BFS
    /// finds deeply-nested standalone skills and does NOT split out the
    /// `references/*/SKILL.md` sub-skills bundled inside a parent skill.
    #[test]
    fn gitcode_find_skill_dirs_layout_if_present() {
        let root = PathBuf::from("/tmp/gitcode-probe");
        if !root.exists() {
            eprintln!("no local gitcode clone; skip layout smoke");
            return;
        }
        let found = find_skill_dirs(&root);
        // A deeply-nested standalone skill must be discovered.
        assert!(
            found.iter().any(|p| p.ends_with(
                "04-development/01-application-framework/ArkUI/hmos-arkui-develop-skill"
            )),
            "expected to find nested ArkUI skill; got {} dirs",
            found.len()
        );
        // Sub-skills under a parent skill's references/ must NOT be surfaced
        // as separate top-level skills.
        assert!(
            !found.iter().any(|p| p
                .to_string_lossy()
                .contains("deveco-native-flow/references")),
            "references/* sub-skills should stay bundled in parent skill"
        );

        // Block-scalar `description: |` must summarize to its first paragraph.
        let arkui =
            root.join("04-development/01-application-framework/ArkUI/hmos-arkui-develop-skill");
        let doc = read_skill_doc(&arkui);
        assert_eq!(doc.name, "hmos-arkui-develop-skill");
        assert!(
            !doc.description.is_empty() && !doc.description.contains("触发场景"),
            "block-scalar description should be summarized to first paragraph, got: {}",
            doc.description
        );
    }

    #[test]
    fn skill_identity_prefers_frontmatter_name() {
        let doc = SkillDoc {
            name: "My-Skill".into(),
            description: String::new(),
        };
        // frontmatter name wins, lowercased
        assert_eq!(skill_identity("some-dir", &doc), "my-skill");
        // no name → dir name, lowercased
        let empty = SkillDoc::default();
        assert_eq!(skill_identity("Some-Dir", &empty), "some-dir");
    }

    #[test]
    fn sniff_entry_exists_matches_identity_or_dir() {
        let entry = SniffScanEntry {
            identity: "demo-skill".into(),
            directory: "demo-dir".into(),
            source_path: PathBuf::from("/x/demo-dir"),
            title: "Demo".into(),
            description: String::new(),
            found_agents: BTreeSet::new(),
        };
        // matches by identity
        let mut existing: BTreeSet<String> = BTreeSet::new();
        existing.insert("demo-skill".into());
        assert!(sniff_entry_exists(&entry, &existing));
        // matches by dir name
        let mut by_dir: BTreeSet<String> = BTreeSet::new();
        by_dir.insert("demo-dir".into());
        assert!(sniff_entry_exists(&entry, &by_dir));
        // no match
        let empty: BTreeSet<String> = BTreeSet::new();
        assert!(!sniff_entry_exists(&entry, &empty));
    }

    #[test]
    fn is_plain_entry_name_rejects_traversal() {
        assert!(is_plain_entry_name("my-skill"));
        assert!(is_plain_entry_name("skill_1.2"));
        assert!(!is_plain_entry_name(""));
        assert!(!is_plain_entry_name("."));
        assert!(!is_plain_entry_name(".."));
        assert!(!is_plain_entry_name("a/b"));
        assert!(!is_plain_entry_name("a\\b"));
    }

    /// A unique scratch dir under the system temp dir for symlink tests.
    fn scratch_dir(tag: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("agentbuddy-symlink-test-{}-{}", tag, now_secs()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("mk scratch");
        base
    }

    #[test]
    fn count_skills_in_root_includes_directory_symlinks() {
        let base = scratch_dir("count-links");
        let library_skill = base.join("library-skill");
        fs::create_dir_all(&library_skill).unwrap();
        fs::write(library_skill.join("SKILL.md"), "name: linked").unwrap();
        let root = base.join("agent-root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&library_skill, root.join("linked")).unwrap();
        fs::create_dir_all(root.join("plain")).unwrap();
        fs::write(root.join("plain/SKILL.md"), "name: plain").unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/SKILL.md"), "name: hidden").unwrap();

        assert_eq!(count_skills_in_root(&root), 2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_link_install_creates_and_replaces_idempotently() {
        let base = scratch_dir("ensure");
        let target = base.join("lib-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "---\nname: t\n---\n").unwrap();
        let root = base.join("agent-root");

        ensure_skill_install(&root, "lib-skill", &target, SkillInstallMode::Link)
            .expect("first link");
        let link = root.join("lib-skill");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);

        // Idempotent: correct link stays a link, no error.
        ensure_skill_install(&root, "lib-skill", &target, SkillInstallMode::Link)
            .expect("second link");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_link_install_replaces_real_directory() {
        let base = scratch_dir("replace");
        let target = base.join("lib-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "x").unwrap();
        let root = base.join("agent-root");
        fs::create_dir_all(&root).unwrap();
        // Pre-existing REAL directory occupying the name.
        let occupied = root.join("lib-skill");
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join("old.txt"), "old").unwrap();

        ensure_skill_install(&root, "lib-skill", &target, SkillInstallMode::Link).expect("replace");
        assert!(fs::symlink_metadata(&occupied)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&occupied).unwrap(), target);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_copy_install_creates_independent_directory() {
        let base = scratch_dir("copy");
        let target = base.join("lib-skill");
        fs::create_dir_all(target.join("nested")).unwrap();
        fs::write(target.join("SKILL.md"), "original").unwrap();
        fs::write(target.join("nested/file.txt"), "content").unwrap();
        let root = base.join("agent-root");

        ensure_skill_install(&root, "lib-skill", &target, SkillInstallMode::Copy)
            .expect("copy install");
        let installed = root.join("lib-skill");
        assert!(fs::symlink_metadata(&installed)
            .unwrap()
            .file_type()
            .is_dir());
        assert!(!fs::symlink_metadata(&installed)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(installed.join("nested/file.txt")).unwrap(),
            "content"
        );

        fs::write(target.join("nested/file.txt"), "updated").unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("nested/file.txt")).unwrap(),
            "content"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn remove_agent_skill_entry_handles_symlink_dir_and_missing() {
        let base = scratch_dir("remove");
        let target = base.join("lib-skill");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "x").unwrap();
        let root = base.join("agent-root");
        fs::create_dir_all(&root).unwrap();

        // Missing → false, no error.
        assert!(!remove_agent_skill_entry(&root, "lib-skill").unwrap());

        // Symlink → removed, target survives.
        ensure_skill_install(&root, "lib-skill", &target, SkillInstallMode::Link).unwrap();
        assert!(remove_agent_skill_entry(&root, "lib-skill").unwrap());
        assert!(!root.join("lib-skill").exists());
        assert!(
            target.join("SKILL.md").exists(),
            "real target must survive unlink"
        );

        // Real directory → removed.
        let real = root.join("lib-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("f.txt"), "y").unwrap();
        assert!(remove_agent_skill_entry(&root, "lib-skill").unwrap());
        assert!(!real.exists());

        let _ = fs::remove_dir_all(&base);
    }
}
