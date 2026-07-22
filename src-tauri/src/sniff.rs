use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize)]
pub struct SniffResult {
    pub name: String,
    pub display_name: String,
    pub icon: String,
    pub found: bool,
    pub install_paths: Vec<String>,
    pub config_dirs: Vec<String>,
}

// ===== Path resolution =====

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

fn resolve_path(path: &str) -> Option<PathBuf> {
    let expanded = expand_tilde(path);
    let p = PathBuf::from(&expanded);
    if p.exists() { Some(p) } else { None }
}

/// A path that lives in a CLI shim/interceptor location rather than a real
/// install location. Tools like cmux prepend `$TMPDIR/cmux-cli-shims/<uuid>/`
/// to PATH and drop same-named wrapper scripts there to re-route
/// `claude`/`codex`; resolving those would report an ephemeral temp shim as the
/// agent's real path. Accepts both a PATH directory entry (used while scanning)
/// and a full stored binary path (used to sanitize cached results). The
/// `cmux-cli-shims` marker mirrors cmux's own PATH-stripping logic, and no
/// legitimately installed CLI lives under the OS temp dir, so both are skipped.
pub(crate) fn is_shim_path(path: &str) -> bool {
    if path.is_empty() {
        // Empty PATH entry means CWD on Unix — never a real install location.
        return true;
    }
    if path.contains("/cmux-cli-shims/") || path.ends_with("/cmux-cli-shims") {
        return true;
    }
    if let Some(tmp) = std::env::temp_dir().to_str() {
        let tmp = tmp.trim_end_matches('/');
        if !tmp.is_empty() && (path == tmp || path.starts_with(&format!("{tmp}/"))) {
            return true;
        }
    }
    false
}

fn find_in_path(binary_name: &str) -> Option<PathBuf> {
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in paths.split(':') {
        if is_shim_path(dir) {
            continue;
        }
        let full_path = Path::new(dir).join(binary_name);
        if full_path.exists() {
            return Some(full_path);
        }
    }
    None
}

/// Scan ~/Library/Application Support for Claude Desktop config dirs:
/// - ~/Library/Application Support/Claude (always included if exists)
/// - ~/Library/Application Support/Claude-* that contains claude_desktop_config.json
fn find_claude_desktop_configs() -> Vec<String> {
    let mut results = Vec::new();
    let app_support = if let Some(dir) = dirs::config_dir() {
        dir
    } else {
        return results;
    };

    if let Ok(entries) = std::fs::read_dir(&app_support) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "Claude" || name.starts_with("Claude-") {
                let config_file = entry.path().join("claude_desktop_config.json");
                if config_file.exists() {
                    results.push(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }
    results
}

// ===== Main sniff =====

/// Collect install paths for one agent (static `bin_paths` + PATH `search_names`).
/// Shim / temp-dir wrappers are filtered out the same way as full agent sniff.
fn collect_install_paths(spec: &crate::agents::AgentSpec) -> Vec<String> {
    let mut install_paths = Vec::new();

    // 1. Check static bin_paths
    for p in spec.bin_paths {
        if let Some(resolved) = resolve_path(p) {
            let path_str = resolved.to_string_lossy().to_string();
            if !install_paths.contains(&path_str) {
                install_paths.push(path_str);
            }
        }
    }

    // 2. Always search PATH for CLI binary (may find CLI in addition to App)
    for bin_name in spec.search_names {
        if let Some(found) = find_in_path(bin_name) {
            let path_str = found.to_string_lossy().to_string();
            if !install_paths.contains(&path_str) {
                install_paths.push(path_str);
            }
            break;
        }
    }

    // Prefer App paths over CLI paths in list items
    order_install_paths(&mut install_paths);
    install_paths
}

/// Whether an agent has a real App/CLI install path.
/// Config dir alone is **not** enough (same rule as `sniff_agents`).
pub fn is_agent_installed(name: &str) -> bool {
    crate::agents::find(name)
        .map(|spec| !collect_install_paths(spec).is_empty())
        .unwrap_or(false)
}

pub fn sniff_agents() -> Vec<SniffResult> {
    crate::agents::agents()
        .iter()
        .map(|spec| {
            let install_paths = collect_install_paths(spec);

            // 3. Check config_paths
            let mut config_dirs: Vec<String> = spec
                .config_paths
                .iter()
                .filter_map(|p| resolve_path(p))
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            // 4. Special: scan Application Support for Claude Desktop
            if spec.scan_app_support {
                let claude_configs = find_claude_desktop_configs();
                for c in claude_configs {
                    if !config_dirs.contains(&c) {
                        config_dirs.push(c);
                    }
                }
            }

            // Installed only when App/CLI path is present; config alone is not enough
            let found = !install_paths.is_empty();

            SniffResult {
                name: spec.name.to_string(),
                display_name: spec.display_name.to_string(),
                icon: spec.icon.to_string(),
                found,
                install_paths,
                config_dirs,
            }
        })
        .collect()
}

/// App paths first (…/*.app), then CLI/other install paths.
fn order_install_paths(paths: &mut Vec<String>) {
    paths.sort_by_key(|path| {
        let is_app = path.ends_with(".app") || path.contains(".app/");
        if is_app { 0 } else { 1 }
    });
}

#[cfg(test)]
mod tests {
    use super::{is_agent_installed, is_shim_path};

    #[test]
    fn unknown_agent_is_not_installed() {
        assert!(!is_agent_installed("__no_such_agent__"));
    }

    #[test]
    fn cmux_shim_dirs_are_skipped() {
        // Real cmux layout: $TMPDIR/cmux-cli-shims/<uuid> (note the double slash
        // cmux emits from `$TMPDIR` already ending in `/`).
        assert!(is_shim_path(
            "/var/folders/jg/xxx/T//cmux-cli-shims/F2FD4F12-045A-46BC-8015-3B41BA0AFE8D"
        ));
        assert!(is_shim_path(
            "/var/folders/jg/xxx/T/cmux-cli-shims/some-uuid"
        ));
        // Marker match even outside the temp dir, matching cmux's own filter.
        assert!(is_shim_path("/home/user/cmux-cli-shims"));
    }

    #[test]
    fn cmux_shim_full_binary_paths_are_skipped() {
        // Cached install_paths store the full binary path, not just the dir.
        assert!(is_shim_path(
            "/var/folders/jg/xxx/T//cmux-cli-shims/EE14E304-43DD-4820-BF7C-B9C485418D07/claude"
        ));
        assert!(is_shim_path(
            "/var/folders/jg/xxx/T//cmux-cli-shims/EE14E304-43DD-4820-BF7C-B9C485418D07/codex"
        ));
    }

    #[test]
    fn empty_path_entry_is_skipped() {
        // Empty PATH segment resolves to CWD on Unix — never a real install.
        assert!(is_shim_path(""));
    }

    #[test]
    fn real_bin_dirs_are_kept() {
        assert!(!is_shim_path("/usr/local/bin"));
        assert!(!is_shim_path("/opt/homebrew/bin"));
        assert!(!is_shim_path("/usr/bin"));
        // Real installed binaries must survive sanitization.
        assert!(!is_shim_path("/Users/x/.local/bin/claude"));
        assert!(!is_shim_path("/opt/homebrew/bin/codex"));
        // A path that merely contains "cmux" but is not a shim dir must survive.
        assert!(!is_shim_path("/Applications/cmux.app/Contents/Resources/bin"));
    }

    #[test]
    fn os_temp_dir_entries_are_skipped() {
        // Any PATH entry under the OS temp dir is treated as ephemeral.
        let tmp = std::env::temp_dir();
        let child = tmp.join("some-tool/bin");
        assert!(is_shim_path(&child.to_string_lossy()));
    }
}
