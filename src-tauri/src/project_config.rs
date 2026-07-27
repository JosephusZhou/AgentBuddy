//! Project-level AI agent config skeleton init (Full / Symlink modes).
//!
//! Spec tables live here as the backend source of truth. Keep
//! `src/components/pages/project-config/types.ts` (`AGENT_PROJECT_INFOS`) in sync
//! when adding/removing agents or changing root/config paths.
//!
//! Intentionally excluded (not typical repo-level skeletons):
//! - `claude-desktop` — desktop app config, not project tree
//! - `kiro` — no settled project-level layout yet
//!
//! See also `PROJECT_AI_CONFIG_IMPROVEMENTS.md`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::platform;

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

struct AgentProjectSpec {
    name: &'static str,
    root_file: Option<&'static str>,
    config_dir: &'static str,
    /// sub-dirs for Full mode (officially supported / common skeleton)
    full_sub_dirs: &'static [&'static str],
    /// extra files to create inside config_dir in Full mode
    config_files: &'static [(&'static str, &'static str)], // (name, content)
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
    },
    AgentProjectSpec {
        name: "codex",
        root_file: Some("AGENTS.md"),
        config_dir: ".codex",
        full_sub_dirs: &[],
        config_files: &[("instructions.md", "")],
    },
    AgentProjectSpec {
        name: "opencode",
        root_file: Some("AGENTS.md"),
        config_dir: ".opencode",
        full_sub_dirs: &["agent", "command", "plugin", "tool"],
        config_files: &[],
    },
    AgentProjectSpec {
        name: "antigravity",
        root_file: Some("GEMINI.md"),
        config_dir: ".gemini",
        full_sub_dirs: &["commands"],
        config_files: &[],
    },
    AgentProjectSpec {
        name: "codebuddy",
        root_file: Some("AGENTS.md"),
        config_dir: ".codebuddy",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
    },
    AgentProjectSpec {
        name: "codebuddy-cn",
        root_file: Some("AGENTS.md"),
        config_dir: ".codebuddy",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
    },
    AgentProjectSpec {
        name: "workbuddy",
        root_file: Some("AGENTS.md"),
        config_dir: ".workbuddy",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
    },
    AgentProjectSpec {
        name: "deveco-code",
        root_file: Some("AGENTS.md"),
        config_dir: ".deveco",
        full_sub_dirs: &["rules", "skills"],
        config_files: &[],
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

    if *mode == InitMode::Symlink {
        push_existing(base.join(".agents"), true);
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
    }

    Ok(CheckResult { existing })
}

pub fn init_project_config(
    target_dir: &str,
    selected_agents: &[AgentConfigRequest],
    mode: &InitMode,
    overwrite: bool,
) -> Result<InitResult, String> {
    let base = resolve_target_dir(target_dir)?;
    let mut created = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();

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

    Ok(InitResult {
        created,
        skipped,
        errors,
    })
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
        let err = init_project_config("", &[req("claude-code")], &InitMode::Full, false)
            .unwrap_err();
        assert!(err.contains("空"), "{err}");

        let missing = std::env::temp_dir().join("agentbuddy-projcfg-missing-nope");
        let _ = fs::remove_dir_all(&missing);
        let err = init_project_config(
            missing.to_str().unwrap(),
            &[req("claude-code")],
            &InitMode::Full,
            false,
        )
        .unwrap_err();
        assert!(err.contains("不存在") || err.contains("不是目录"), "{err}");
    }

    #[test]
    fn full_mode_creates_shared_guide_and_dedupes_codebuddy_dir() {
        let base = scratch("full-dedupe");
        let agents = vec![
            req("codebuddy"),
            req("codebuddy-cn"),
            req("codex"),
            req("opencode"),
        ];
        let result =
            init_project_config(base.to_str().unwrap(), &agents, &InitMode::Full, false).unwrap();
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
}
