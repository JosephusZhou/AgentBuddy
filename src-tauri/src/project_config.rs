//! Project-level AI agent config skeleton init (Full / Symlink modes).
//!
//! Spec tables live here as the backend source of truth. Keep
//! `src/components/pages/project-config/types.ts` (`AGENT_PROJECT_INFOS`) in sync
//! when adding/removing agents or changing root/config paths.
//!
//! Intentionally excluded (not typical repo-level skeletons):
//! - `claude-desktop` — desktop app config, not project tree
//! - `codebuddy` — 国际版已移除支持，仅保留 CodeBuddy CN
//!
//! See also `PROJECT_AI_CONFIG_IMPROVEMENTS.md`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agents::McpDialect;
use crate::mcp_config::McpDraft;
use crate::platform;
use crate::skills::SkillInstallMode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum InitMode {
    Full,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExistingItem {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub existing: Vec<ExistingItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InitResult {
    pub created: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

/// Project-level MCP config file spec for one agent (path relative to the repo root).
struct ProjectMcpSpec {
    file: &'static str,
    dialect: McpDialect,
    jsonc: bool,
}

struct AgentProjectSpec {
    name: &'static str,
    root_file: Option<&'static str>,
    config_dir: &'static str,
    /// sub-dirs for Full mode (officially supported / common skeleton)
    full_sub_dirs: &'static [&'static str],
    /// extra files to create inside config_dir in Full mode
    config_files: &'static [(&'static str, &'static str)], // (name, content)
    /// project-level MCP config file (written as a merge, keyed by server title)
    mcp: Option<ProjectMcpSpec>,
}

/// Shared sub-dirs under `.agents/` and symlinked into each agent config dir (Symlink mode).
/// Full mode uses per-agent `full_sub_dirs` instead — sets intentionally differ.
const SHARED_SUB_DIRS: &[&str] = &["commands", "rules", "skills", "agents"];

static AGENT_SPECS: &[AgentProjectSpec] = &[
    AgentProjectSpec {
        name: "claude-code",
        root_file: Some("CLAUDE.md"),
        config_dir: ".claude",
        full_sub_dirs: &["commands", "agents"],
        config_files: &[],
        mcp: Some(ProjectMcpSpec {
            file: ".mcp.json",
            dialect: McpDialect::JsonMcpServers,
            jsonc: false,
        }),
    },
    AgentProjectSpec {
        name: "codex",
        root_file: Some("AGENTS.md"),
        config_dir: ".codex",
        full_sub_dirs: &[],
        config_files: &[("instructions.md", "")],
        mcp: Some(ProjectMcpSpec {
            file: ".codex/config.toml",
            dialect: McpDialect::TomlMcpServers,
            jsonc: false,
        }),
    },
    AgentProjectSpec {
        name: "opencode",
        root_file: Some("AGENTS.md"),
        config_dir: ".opencode",
        full_sub_dirs: &["agent", "command", "plugin", "tool"],
        config_files: &[],
        mcp: Some(ProjectMcpSpec {
            file: "opencode.json",
            dialect: McpDialect::JsonMcp,
            jsonc: false,
        }),
    },
    AgentProjectSpec {
        name: "antigravity",
        root_file: Some("GEMINI.md"),
        config_dir: ".gemini",
        full_sub_dirs: &["commands"],
        config_files: &[],
        mcp: Some(ProjectMcpSpec {
            file: ".gemini/settings.json",
            dialect: McpDialect::JsonGeminiMixed,
            jsonc: false,
        }),
    },
    AgentProjectSpec {
        name: "codebuddy-cn",
        root_file: Some("AGENTS.md"),
        config_dir: ".codebuddy",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
        mcp: Some(ProjectMcpSpec {
            file: ".mcp.json",
            dialect: McpDialect::JsonMcpServers,
            jsonc: false,
        }),
    },
    AgentProjectSpec {
        name: "workbuddy",
        root_file: Some("AGENTS.md"),
        config_dir: ".workbuddy",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
        mcp: Some(ProjectMcpSpec {
            file: ".mcp.json",
            dialect: McpDialect::JsonMcpServers,
            jsonc: false,
        }),
    },
    AgentProjectSpec {
        name: "deveco-code",
        root_file: Some("AGENTS.md"),
        config_dir: ".deveco",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
        mcp: Some(ProjectMcpSpec {
            file: ".deveco/deveco.jsonc",
            dialect: McpDialect::JsonMcp,
            jsonc: true,
        }),
    },
];

fn find_spec(name: &str) -> Option<&'static AgentProjectSpec> {
    AGENT_SPECS.iter().find(|s| s.name == name)
}

fn resolve_target_dir(target_dir: &str) -> Result<PathBuf, String> {
    let trimmed = target_dir.trim();
    if trimmed.is_empty() {
        return Err("目标目录不能为空".into());
    }
    let base = PathBuf::from(trimmed);
    if !base.is_dir() {
        return Err(format!(
            "目标目录不存在或不是目录: {}",
            base.display()
        ));
    }
    Ok(base)
}

fn root_file_template(root_file: &str) -> &'static str {
    match root_file {
        "CLAUDE.md" => "# CLAUDE.md\n\nRead `AGENTS.md` for full project guidance before any work.\n",
        "GEMINI.md" => "# gemini.md\n\nRead `AGENTS.md` for full project guidance before any work.\n",
        _ => "# Project AI Configuration\n",
    }
}

fn agents_md_content() -> &'static str {
    "# AGENTS.md\n\nProject-specific guidance for AI coding agents working on this repository.\n"
}

/// True when path exists as any entry (including a dangling symlink).
fn entry_present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Remove an existing entry only when safe for skeleton overwrite:
/// - symlink → remove the link itself (never follow into target)
/// - regular file → remove file
/// - empty directory → remove dir
/// - non-empty directory → refuse (returns Err)
fn remove_entry_for_overwrite(path: &Path) -> Result<(), String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()), // already gone
    };
    let ft = meta.file_type();

    // Symlink first: on some platforms dir-symlinks also report is_dir().
    if ft.is_symlink() {
        // Unix: remove_file drops any symlink. Windows dir symlinks may need remove_dir.
        if let Err(e) = fs::remove_file(path) {
            fs::remove_dir(path).map_err(|e2| {
                format!(
                    "删除软链接 {} 失败: {} / {}",
                    path.display(),
                    e,
                    e2
                )
            })?;
        }
        return Ok(());
    }

    if ft.is_file() {
        return fs::remove_file(path)
            .map_err(|e| format!("删除文件 {} 失败: {}", path.display(), e));
    }

    if ft.is_dir() {
        let mut rd = fs::read_dir(path)
            .map_err(|e| format!("读取目录 {} 失败: {}", path.display(), e))?;
        if rd.next().is_some() {
            return Err(format!(
                "拒绝删除非空目录 {}（可能含用户数据）；请手动处理后重试或选择「跳过已存在」",
                path.display()
            ));
        }
        return fs::remove_dir(path)
            .map_err(|e| format!("删除空目录 {} 失败: {}", path.display(), e));
    }

    Err(format!("无法处理的路径类型: {}", path.display()))
}

fn write_file(
    path: &Path,
    content: &str,
    overwrite: bool,
    created: &mut Vec<String>,
    skipped: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    if entry_present(path) {
        if !overwrite {
            skipped.push(path.to_string_lossy().to_string());
            return;
        }
        // Refuse to clobber a directory (empty or not) with a file write.
        if let Ok(meta) = fs::symlink_metadata(path) {
            let ft = meta.file_type();
            if ft.is_dir() && !ft.is_symlink() {
                errors.push(format!(
                    "路径 {} 是目录，无法覆盖为文件",
                    path.display()
                ));
                return;
            }
            if let Err(e) = remove_entry_for_overwrite(path) {
                errors.push(e);
                return;
            }
        }
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            errors.push(format!("创建目录 {} 失败: {}", parent.display(), e));
            return;
        }
    }
    match fs::write(path, content) {
        Ok(_) => created.push(path.to_string_lossy().to_string()),
        Err(e) => errors.push(format!("写入 {} 失败: {}", path.display(), e)),
    }
}

fn create_dir(
    path: &Path,
    created: &mut Vec<String>,
    skipped: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    if entry_present(path) {
        skipped.push(path.to_string_lossy().to_string());
        return;
    }
    match fs::create_dir_all(path) {
        Ok(_) => created.push(path.to_string_lossy().to_string()),
        Err(e) => errors.push(format!("创建目录 {} 失败: {}", path.display(), e)),
    }
}

fn create_dir_symlink(
    source: &Path,
    dest: &Path,
    overwrite: bool,
    created: &mut Vec<String>,
    skipped: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    if entry_present(dest) {
        if !overwrite {
            skipped.push(dest.to_string_lossy().to_string());
            return;
        }
        if let Err(e) = remove_entry_for_overwrite(dest) {
            errors.push(e);
            return;
        }
    }
    match platform::symlink_dir(source, dest) {
        Ok(_) => created.push(format!("{} -> {}", dest.display(), source.display())),
        Err(e) => errors.push(format!(
            "创建软链接 {} 失败（Windows 需在「设置→开发者选项」中开启开发者模式）: {}",
            dest.display(),
            e
        )),
    }
}

pub fn check_project_config_exists(
    target_dir: &str,
    selected_agents: &[AgentConfigRequest],
    mode: &InitMode,
    skill_ids: &[String],
) -> Result<CheckResult, String> {
    let base = resolve_target_dir(target_dir)?;
    let mut existing = Vec::new();
    let mut seen_config_dirs: HashSet<String> = HashSet::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    let mut push_existing = |path: PathBuf, is_dir: bool| {
        let key = path.to_string_lossy().to_string();
        if seen_paths.insert(key.clone()) && entry_present(&path) {
            existing.push(ExistingItem {
                path: key,
                is_dir,
            });
        }
    };

    push_existing(base.join("AGENTS.md"), false);

    let skill_ids = sanitize_skill_ids(skill_ids);
    let with_skills = !skill_ids.is_empty();

    if *mode == InitMode::Symlink || with_skills {
        push_existing(base.join(".agents"), true);
    }
    if with_skills {
        push_existing(base.join(".agents").join("skills"), true);
        for id in &skill_ids {
            push_existing(base.join(".agents").join("skills").join(id), true);
        }
    }

    for req in selected_agents {
        let spec = find_spec(&req.name).ok_or_else(|| format!("未知 agent: {}", req.name))?;

        if let Some(rf) = spec.root_file {
            push_existing(base.join(rf), false);
        }

        let config_dir_key = spec.config_dir.to_string();
        if !seen_config_dirs.insert(config_dir_key) {
            continue;
        }

        push_existing(base.join(spec.config_dir), true);

        // Full mode + selected skills: each agent's skills dir becomes a link.
        if with_skills && *mode == InitMode::Full {
            push_existing(base.join(spec.config_dir).join("skills"), true);
        }
    }

    Ok(CheckResult { existing })
}

/// Trim / dedupe skill ids, keeping order; silently drops invalid entries
/// (empty or containing path separators).
fn sanitize_skill_ids(skill_ids: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in skill_ids {
        let id = raw.trim();
        if id.is_empty() || id.contains('/') || id.contains('\\') || id == "." || id == ".." {
            continue;
        }
        if seen.insert(id.to_string()) {
            out.push(id.to_string());
        }
    }
    out
}

pub fn init_project_config(
    target_dir: &str,
    selected_agents: &[AgentConfigRequest],
    mode: &InitMode,
    overwrite: bool,
    mcp_servers: &[McpDraft],
    skill_ids: &[String],
    skill_mode: SkillInstallMode,
) -> Result<InitResult, String> {
    let base = resolve_target_dir(target_dir)?;

    // Resolve library skill ids to their on-disk source dirs up front.
    let mut skill_sources: Vec<(String, PathBuf)> = Vec::new();
    let mut resolve_errors: Vec<String> = Vec::new();
    for id in sanitize_skill_ids(skill_ids) {
        match crate::skills::library_skill_dir(&id) {
            Ok(src) => skill_sources.push((id, src)),
            Err(e) => resolve_errors.push(e),
        }
    }

    let mut result = run_init(
        &base,
        selected_agents,
        mode,
        overwrite,
        mcp_servers,
        &skill_sources,
        skill_mode,
    );
    result.errors.extend(resolve_errors);
    Ok(result)
}

fn run_init(
    base: &Path,
    selected_agents: &[AgentConfigRequest],
    mode: &InitMode,
    overwrite: bool,
    mcp_servers: &[McpDraft],
    skill_sources: &[(String, PathBuf)],
    skill_mode: SkillInstallMode,
) -> InitResult {
    let mut created = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();

    let with_skills = !skill_sources.is_empty();

    write_file(
        &base.join("AGENTS.md"),
        agents_md_content(),
        overwrite,
        &mut created,
        &mut skipped,
        &mut errors,
    );

    match mode {
        InitMode::Full => {
            let mut seen_config_dirs: HashSet<String> = HashSet::new();
            // AGENTS.md is the shared guide written above; dedupe other agent root files.
            let mut written_root_files: HashSet<String> = HashSet::new();

            for req in selected_agents {
                let spec = match find_spec(&req.name) {
                    Some(s) => s,
                    None => {
                        errors.push(format!("未知 agent: {}", req.name));
                        continue;
                    }
                };

                if let Some(rf) = spec.root_file.filter(|rf| *rf != "AGENTS.md") {
                    if written_root_files.insert(rf.to_string()) {
                        let p = base.join(rf);
                        write_file(
                            &p,
                            root_file_template(rf),
                            overwrite,
                            &mut created,
                            &mut skipped,
                            &mut errors,
                        );
                    }
                }

                let config_dir_key = spec.config_dir.to_string();
                if !seen_config_dirs.insert(config_dir_key) {
                    continue;
                }

                let config_dir = base.join(spec.config_dir);
                create_dir(&config_dir, &mut created, &mut skipped, &mut errors);

                for sub in spec.full_sub_dirs {
                    // When skills were selected, `.agents/skills` is the shared
                    // store and each agent's skills dir becomes a link to it
                    // (created below) — do not create a real dir here.
                    if with_skills && *sub == "skills" {
                        continue;
                    }
                    create_dir(
                        &config_dir.join(sub),
                        &mut created,
                        &mut skipped,
                        &mut errors,
                    );
                }

                for (fname, content) in spec.config_files {
                    let p = config_dir.join(fname);
                    write_file(
                        &p,
                        content,
                        overwrite,
                        &mut created,
                        &mut skipped,
                        &mut errors,
                    );
                }
            }
        }

        InitMode::Symlink => {
            // 1. Create .agents/ with all shared sub-dirs
            let agents_dir = base.join(".agents");
            create_dir(&agents_dir, &mut created, &mut skipped, &mut errors);
            for sub in SHARED_SUB_DIRS {
                create_dir(
                    &agents_dir.join(sub),
                    &mut created,
                    &mut skipped,
                    &mut errors,
                );
            }

            let mut seen_config_dirs: HashSet<String> = HashSet::new();
            let mut written_root_files: HashSet<String> = HashSet::new();

            for req in selected_agents {
                let spec = match find_spec(&req.name) {
                    Some(s) => s,
                    None => {
                        errors.push(format!("未知 agent: {}", req.name));
                        continue;
                    }
                };

                // AGENTS.md is the shared guide written above; other root files point to it.
                if let Some(rf) = spec.root_file.filter(|rf| *rf != "AGENTS.md") {
                    if written_root_files.insert(rf.to_string()) {
                        let p = base.join(rf);
                        write_file(
                            &p,
                            root_file_template(rf),
                            overwrite,
                            &mut created,
                            &mut skipped,
                            &mut errors,
                        );
                    }
                }

                let config_dir_key = spec.config_dir.to_string();
                if !seen_config_dirs.insert(config_dir_key) {
                    continue;
                }

                let config_dir = base.join(spec.config_dir);
                create_dir(&config_dir, &mut created, &mut skipped, &mut errors);

                // Symlink all shared sub-dirs into config_dir (relative targets for portability).
                // Always directory links via platform::symlink_dir — never CWD-based is_dir().
                for sub in SHARED_SUB_DIRS {
                    let dest = config_dir.join(sub);
                    let source = PathBuf::from("..").join(".agents").join(sub);
                    create_dir_symlink(
                        &source,
                        &dest,
                        overwrite,
                        &mut created,
                        &mut skipped,
                        &mut errors,
                    );
                }
            }
        }
    }

    // --- Project-level MCP files (merge by server title; never gated on overwrite) ---
    if !mcp_servers.is_empty() {
        // Dedupe target files across selected agents (claude-code / codebuddy-cn /
        // workbuddy all share the repo-root `.mcp.json`).
        let mut mcp_files: Vec<(PathBuf, McpDialect, bool)> = Vec::new();
        for req in selected_agents {
            let spec = match find_spec(&req.name) {
                Some(s) => s,
                None => continue, // unknown agent already reported above
            };
            if let Some(mcp) = &spec.mcp {
                let path = base.join(mcp.file);
                if !mcp_files.iter().any(|(p, _, _)| p == &path) {
                    mcp_files.push((path, mcp.dialect, mcp.jsonc));
                }
            }
        }
        for (path, dialect, jsonc) in mcp_files {
            let mut ok_count = 0usize;
            for draft in mcp_servers {
                match crate::mcp_config::apply_draft_to_file(&path, dialect, jsonc, draft) {
                    Ok(_) => ok_count += 1,
                    Err(e) => errors.push(format!("写入 MCP「{}」失败: {}", draft.title, e)),
                }
            }
            if ok_count > 0 {
                created.push(format!("{} (MCP {} 项)", path.display(), ok_count));
            }
        }
    }

    // --- Shared skills store under .agents/skills (copy or symlink from library) ---
    if with_skills {
        let agents_dir = base.join(".agents");
        create_dir(&agents_dir, &mut created, &mut skipped, &mut errors);
        let skills_dir = agents_dir.join("skills");
        create_dir(&skills_dir, &mut created, &mut skipped, &mut errors);

        for (id, src) in skill_sources {
            install_project_skill(
                src,
                &skills_dir.join(id),
                skill_mode,
                overwrite,
                &mut created,
                &mut skipped,
                &mut errors,
            );
        }

        // Full mode: link each agent's skills dir to the shared store so every
        // selected agent sees the same skills. Symlink mode already links
        // `<config_dir>/skills` above via SHARED_SUB_DIRS.
        if *mode == InitMode::Full {
            let mut seen_config_dirs: HashSet<String> = HashSet::new();
            for req in selected_agents {
                let spec = match find_spec(&req.name) {
                    Some(s) => s,
                    None => continue,
                };
                if !seen_config_dirs.insert(spec.config_dir.to_string()) {
                    continue;
                }
                let config_dir = base.join(spec.config_dir);
                create_dir(&config_dir, &mut created, &mut skipped, &mut errors);
                create_dir_symlink(
                    &PathBuf::from("..").join(".agents").join("skills"),
                    &config_dir.join("skills"),
                    overwrite,
                    &mut created,
                    &mut skipped,
                    &mut errors,
                );
            }
        }
    }

    InitResult {
        created,
        skipped,
        errors,
    }
}

/// Install one library skill into `<repo>/.agents/skills/<id>` as a copy or a
/// symlink. Existing entries follow the same safety rules as the skeleton:
/// skip without `overwrite`; overwrite replaces symlinks / empty dirs but
/// never deletes a non-empty real directory.
fn install_project_skill(
    src: &Path,
    dest: &Path,
    mode: SkillInstallMode,
    overwrite: bool,
    created: &mut Vec<String>,
    skipped: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    if entry_present(dest) {
        if !overwrite {
            skipped.push(dest.to_string_lossy().to_string());
            return;
        }
        if let Err(e) = remove_entry_for_overwrite(dest) {
            errors.push(e);
            return;
        }
    }
    if let Some(parent) = dest.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            errors.push(format!("创建目录 {} 失败: {}", parent.display(), e));
            return;
        }
    }
    let result = match mode {
        SkillInstallMode::Link => platform::symlink_any(src, dest),
        SkillInstallMode::Copy => platform::copy_dir_recursive(src, dest),
    };
    match result {
        Ok(_) => match mode {
            SkillInstallMode::Link => {
                created.push(format!("{} -> {}", dest.display(), src.display()))
            }
            SkillInstallMode::Copy => created.push(dest.to_string_lossy().to_string()),
        },
        Err(e) => errors.push(format!(
            "安装技能到 {} 失败（Windows 软链接需开启开发者模式）: {}",
            dest.display(),
            e
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(tag: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("agentbuddy-projcfg-{tag}-{n}"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn cleanup(base: &Path) {
        let _ = fs::remove_dir_all(base);
    }

    fn req(name: &str) -> AgentConfigRequest {
        AgentConfigRequest {
            name: name.to_string(),
        }
    }

    #[test]
    fn rejects_empty_or_missing_target_dir() {
        let err = init_project_config("", &[req("claude-code")], &InitMode::Full, false, &[], &[], SkillInstallMode::Link)
            .unwrap_err();
        assert!(err.contains("空"), "{err}");

        let missing = std::env::temp_dir().join("agentbuddy-projcfg-missing-nope");
        let _ = fs::remove_dir_all(&missing);
        let err = init_project_config(
            missing.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            false,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap_err();
        assert!(err.contains("不存在") || err.contains("不是目录"), "{err}");
    }

    #[test]
    fn full_mode_creates_shared_guide_and_codebuddy_cn_dir() {
        let base = scratch("full-dedupe");
        let agents = vec![req("codebuddy-cn"), req("codex"), req("opencode")];
        let result =
            init_project_config(base.to_str().unwrap(), &agents, &InitMode::Full, false, &[], &[], SkillInstallMode::Link).unwrap();
        assert!(
            result.errors.is_empty(),
            "errors: {:?}",
            result.errors
        );

        assert!(base.join("AGENTS.md").is_file());
        let agents_md = fs::read_to_string(base.join("AGENTS.md")).unwrap();
        assert_eq!(
            agents_md,
            "# AGENTS.md\n\nProject-specific guidance for AI coding agents working on this repository.\n"
        );

        assert!(base.join(".codebuddy").is_dir());
        assert!(base.join(".codebuddy/rules").is_dir());
        assert!(base.join(".codebuddy/skills").is_dir());
        assert!(base.join(".codex/instructions.md").is_file());
        assert!(base.join(".opencode/agent").is_dir());

        // Only one AGENTS.md create entry
        let agents_creates = result
            .created
            .iter()
            .filter(|p| p.ends_with("AGENTS.md"))
            .count();
        assert_eq!(agents_creates, 1, "created: {:?}", result.created);

        cleanup(&base);
    }

    #[test]
    fn full_mode_creates_shared_guide_without_agents_md_agent() {
        let base = scratch("full-shared-guide");
        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            false,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(base.join("AGENTS.md").is_file());
        assert_eq!(
            fs::read_to_string(base.join("CLAUDE.md")).unwrap(),
            "# CLAUDE.md\n\nRead `AGENTS.md` for full project guidance before any work.\n"
        );
        cleanup(&base);
    }

    #[test]
    fn full_mode_creates_gemini_pointer() {
        let base = scratch("full-gemini-pointer");
        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("antigravity")],
            &InitMode::Full,
            false,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            fs::read_to_string(base.join("GEMINI.md")).unwrap(),
            "# gemini.md\n\nRead `AGENTS.md` for full project guidance before any work.\n"
        );
        cleanup(&base);
    }

    #[test]
    fn skip_existing_without_overwrite() {
        let base = scratch("skip");
        fs::write(base.join("CLAUDE.md"), "user content\n").unwrap();
        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            false,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.skipped.iter().any(|p| p.ends_with("CLAUDE.md")));
        assert_eq!(
            fs::read_to_string(base.join("CLAUDE.md")).unwrap(),
            "user content\n"
        );
        cleanup(&base);
    }

    #[test]
    fn overwrite_file_but_not_nonempty_dir() {
        let base = scratch("overwrite-safe");
        fs::write(base.join("CLAUDE.md"), "old\n").unwrap();
        fs::create_dir_all(base.join(".claude/skills")).unwrap();
        fs::write(base.join(".claude/skills/keep.md"), "keep\n").unwrap();

        // Full mode does not touch .claude/skills (not in full_sub_dirs), but we
        // exercise remove_entry_for_overwrite via Symlink mode on skills link dest.
        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            true,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let claude = fs::read_to_string(base.join("CLAUDE.md")).unwrap();
        assert_eq!(
            claude,
            "# CLAUDE.md\n\nRead `AGENTS.md` for full project guidance before any work.\n"
        );
        assert!(base.join(".claude/skills/keep.md").is_file());

        // Symlink mode with overwrite: nonempty .claude/skills must not be wiped
        let result2 = init_project_config(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Symlink,
            true,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(
            base.join(".claude/skills/keep.md").is_file(),
            "user skill must survive"
        );
        assert!(
            result2.errors.iter().any(|e| e.contains("非空目录")),
            "expected nonempty-dir error, got: {:?}",
            result2.errors
        );
        cleanup(&base);
    }

    #[test]
    fn check_reports_shared_guide_for_claude_code() {
        let base = scratch("check");
        fs::write(base.join("AGENTS.md"), "x\n").unwrap();
        fs::write(base.join("CLAUDE.md"), "x\n").unwrap();
        let result = check_project_config_exists(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            &[],
        )
        .unwrap();
        let paths: Vec<_> = result.existing.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths
                .iter()
                .filter(|p| p.ends_with("AGENTS.md"))
                .count(),
            1
        );
        assert!(paths.iter().any(|p| p.ends_with("CLAUDE.md")), "{paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("AGENTS.md")), "{paths:?}");
        cleanup(&base);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_mode_creates_relative_dir_links() {
        let base = scratch("symlink");
        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("claude-code"), req("codex")],
            &InitMode::Symlink,
            false,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);

        let agents_md = fs::read_to_string(base.join("AGENTS.md")).unwrap();
        assert_eq!(
            agents_md,
            "# AGENTS.md\n\nProject-specific guidance for AI coding agents working on this repository.\n"
        );
        assert!(!base.join(".agents/AGENTS.md").exists());

        let link = base.join(".claude/commands");
        let meta = fs::symlink_metadata(&link).unwrap();
        assert!(meta.file_type().is_symlink());
        let target = fs::read_link(&link).unwrap();
        assert_eq!(target, PathBuf::from("../.agents/commands"));

        // Relative link resolves from dest parent
        assert!(base.join(".claude").join(&target).is_dir());

        // Shared AGENTS.md root once; CLAUDE.md points to it.
        assert!(base.join("AGENTS.md").is_file());
        assert!(base.join("CLAUDE.md").is_file());
        let pointer = fs::read_to_string(base.join("CLAUDE.md")).unwrap();
        assert!(pointer.contains("`AGENTS.md`"), "{pointer}");
        assert!(!pointer.contains(".agents/AGENTS.md"), "{pointer}");

        cleanup(&base);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_overwrite_replaces_old_link_not_target() {
        let base = scratch("symlink-ow");
        fs::create_dir_all(base.join(".agents/commands")).unwrap();
        fs::create_dir_all(base.join(".claude")).unwrap();
        // Pre-seed a wrong link and ensure target content is safe
        fs::write(base.join(".agents/commands/a.md"), "shared\n").unwrap();
        std::os::unix::fs::symlink("../.agents/commands", base.join(".claude/commands")).unwrap();

        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Symlink,
            true,
            &[],
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            fs::read_to_string(base.join(".agents/commands/a.md")).unwrap(),
            "shared\n"
        );
        assert!(fs::symlink_metadata(base.join(".claude/commands"))
            .unwrap()
            .file_type()
            .is_symlink());
        cleanup(&base);
    }

    fn mcp_draft(title: &str) -> McpDraft {
        McpDraft {
            title: title.to_string(),
            transport: "stdio".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "some-mcp-package".into()],
            env: std::collections::HashMap::new(),
            url: String::new(),
            headers: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn mcp_written_to_project_files_and_deduped() {
        let base = scratch("mcp");
        let agents = vec![req("claude-code"), req("codebuddy-cn"), req("codex")];
        let mcps = vec![mcp_draft("shared-server")];
        let result = init_project_config(
            base.to_str().unwrap(),
            &agents,
            &InitMode::Full,
            false,
            &mcps,
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);

        // claude-code + codebuddy-cn share the repo-root .mcp.json, written once
        let mcp_json = base.join(".mcp.json");
        assert!(mcp_json.is_file());
        let doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&mcp_json).unwrap()).unwrap();
        let server = &doc["mcpServers"]["shared-server"];
        assert_eq!(server["command"], "npx");
        assert_eq!(server["args"][0], "-y");
        let mcp_lines = result
            .created
            .iter()
            .filter(|p| p.contains(".mcp.json"))
            .count();
        assert_eq!(mcp_lines, 1, "created: {:?}", result.created);

        // codex project MCP lands in TOML dialect
        let toml_text = fs::read_to_string(base.join(".codex/config.toml")).unwrap();
        assert!(toml_text.contains("mcp_servers"), "{toml_text}");
        assert!(toml_text.contains("shared-server"), "{toml_text}");
        assert!(toml_text.contains("command = \"npx\""), "{toml_text}");
        cleanup(&base);
    }

    #[test]
    fn mcp_merge_preserves_existing_file_content() {
        let base = scratch("mcp-merge");
        fs::create_dir_all(base.join(".gemini")).unwrap();
        fs::write(
            base.join(".gemini/settings.json"),
            "{\n  \"theme\": \"dark\",\n  \"mcpServers\": {\n    \"keep\": { \"command\": \"x\" }\n  }\n}\n",
        )
        .unwrap();
        let mcps = vec![mcp_draft("new-server")];
        let result = init_project_config(
            base.to_str().unwrap(),
            &[req("antigravity")],
            &InitMode::Full,
            false,
            &mcps,
            &[],
            SkillInstallMode::Link,
        )
        .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let doc: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(base.join(".gemini/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(doc["theme"], "dark");
        assert_eq!(doc["mcpServers"]["keep"]["command"], "x");
        assert_eq!(doc["mcpServers"]["new-server"]["command"], "npx");
        cleanup(&base);
    }

    #[cfg(unix)]
    #[test]
    fn skills_copy_installs_into_shared_store() {
        let base = scratch("skills-copy");
        let lib_skill = scratch("skills-copy-src");
        fs::write(lib_skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        fs::write(lib_skill.join("extra.txt"), "x\n").unwrap();
        let sources = vec![("demo".to_string(), lib_skill.clone())];
        let result = run_init(
            &base,
            &[req("claude-code"), req("codebuddy-cn")],
            &InitMode::Full,
            false,
            &[],
            &sources,
            SkillInstallMode::Copy,
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(base.join(".agents/skills/demo/SKILL.md").is_file());
        assert!(base.join(".agents/skills/demo/extra.txt").is_file());
        assert!(!fs::symlink_metadata(base.join(".agents/skills/demo"))
            .unwrap()
            .file_type()
            .is_symlink());
        // source untouched
        assert!(lib_skill.join("SKILL.md").is_file());
        cleanup(&base);
        cleanup(&lib_skill);
    }

    #[cfg(unix)]
    #[test]
    fn skills_link_mode_links_shared_store_and_agents() {
        let base = scratch("skills-link");
        let lib_skill = scratch("skills-link-src");
        fs::write(lib_skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        let sources = vec![("demo".to_string(), lib_skill.clone())];
        let result = run_init(
            &base,
            &[req("claude-code"), req("codebuddy-cn")],
            &InitMode::Full,
            false,
            &[],
            &sources,
            SkillInstallMode::Link,
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);

        let store_entry = base.join(".agents/skills/demo");
        assert!(fs::symlink_metadata(&store_entry)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&store_entry).unwrap(), lib_skill);

        // Full mode: each agent's skills dir links to the shared store
        for dir in [".claude", ".codebuddy"] {
            let link = base.join(dir).join("skills");
            assert!(
                fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
                "{dir}"
            );
            assert_eq!(
                fs::read_link(&link).unwrap(),
                PathBuf::from("../.agents/skills")
            );
        }
        // skills resolvable through the agent link
        assert!(base.join(".claude/skills/demo/SKILL.md").is_file());
        cleanup(&base);
        cleanup(&lib_skill);
    }

    #[cfg(unix)]
    #[test]
    fn skills_symlink_mode_uses_existing_shared_links() {
        let base = scratch("skills-symlink-mode");
        let lib_skill = scratch("skills-symlink-src");
        fs::write(lib_skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        let sources = vec![("demo".to_string(), lib_skill.clone())];
        let result = run_init(
            &base,
            &[req("claude-code"), req("codex")],
            &InitMode::Symlink,
            false,
            &[],
            &sources,
            SkillInstallMode::Copy,
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(base.join(".agents/skills/demo/SKILL.md").is_file());
        // agent skills link from the skeleton points at the same store
        assert_eq!(
            fs::read_link(base.join(".claude/skills")).unwrap(),
            PathBuf::from("../.agents/skills")
        );
        assert!(base.join(".claude/skills/demo/SKILL.md").is_file());
        cleanup(&base);
        cleanup(&lib_skill);
    }

    #[cfg(unix)]
    #[test]
    fn skills_existing_entry_skipped_without_overwrite() {
        let base = scratch("skills-skip");
        let lib_skill = scratch("skills-skip-src");
        fs::write(lib_skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        fs::create_dir_all(base.join(".agents/skills/demo")).unwrap();
        fs::write(base.join(".agents/skills/demo/user.txt"), "keep\n").unwrap();
        let sources = vec![("demo".to_string(), lib_skill.clone())];
        let result = run_init(
            &base,
            &[req("claude-code")],
            &InitMode::Full,
            false,
            &[],
            &sources,
            SkillInstallMode::Copy,
        );
        assert!(result
            .skipped
            .iter()
            .any(|p| p.ends_with(".agents/skills/demo")));
        assert_eq!(
            fs::read_to_string(base.join(".agents/skills/demo/user.txt")).unwrap(),
            "keep\n"
        );

        // Even with overwrite, a non-empty real directory is never wiped
        let result2 = run_init(
            &base,
            &[req("claude-code")],
            &InitMode::Full,
            true,
            &[],
            &sources,
            SkillInstallMode::Copy,
        );
        assert!(
            result2.errors.iter().any(|e| e.contains("非空目录")),
            "expected nonempty-dir error, got: {:?}",
            result2.errors
        );
        assert_eq!(
            fs::read_to_string(base.join(".agents/skills/demo/user.txt")).unwrap(),
            "keep\n"
        );
        cleanup(&base);
        cleanup(&lib_skill);
    }

    #[test]
    fn check_lists_skill_entries() {
        let base = scratch("check-skills");
        fs::create_dir_all(base.join(".agents/skills/demo")).unwrap();
        let result = check_project_config_exists(
            base.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            &["demo".to_string()],
        )
        .unwrap();
        let paths: Vec<_> = result.existing.iter().map(|e| e.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with(".agents")), "{paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("demo")), "{paths:?}");
        cleanup(&base);
    }
}
