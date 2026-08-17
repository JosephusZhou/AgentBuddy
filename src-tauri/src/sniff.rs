use crate::platform;
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
    platform::expand_tilde_lossy(path)
}

fn resolve_path(path: &str) -> Option<PathBuf> {
    let expanded = expand_tilde(path);
    let p = PathBuf::from(&expanded);
    if p.exists() {
        Some(p)
    } else {
        None
    }
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

    // Normalize separators + case for cross-platform comparisons.
    let normalized = path.replace('\\', "/").to_ascii_lowercase();

    if normalized.contains("/cmux-cli-shims/") || normalized.ends_with("/cmux-cli-shims") {
        return true;
    }

    if let Ok(tmp) = std::env::temp_dir().canonicalize() {
        let candidate = Path::new(path);
        // starts_with works with mixed separators after canonicalize on the tmp side;
        // also try a string prefix fallback when candidate cannot be canonicalized.
        if candidate.starts_with(&tmp) {
            return true;
        }
        let tmp_s = tmp.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        if !tmp_s.is_empty()
            && (normalized == tmp_s || normalized.starts_with(&format!("{tmp_s}/")))
        {
            return true;
        }
    } else if let Some(tmp) = std::env::temp_dir().to_str() {
        let tmp = tmp
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if !tmp.is_empty() && (normalized == tmp || normalized.starts_with(&format!("{tmp}/"))) {
            return true;
        }
    }
    false
}

fn find_in_path(binary_name: &str) -> Option<PathBuf> {
    platform::find_in_path(binary_name, is_shim_path)
}

/// Scan platform config roots for Claude Desktop config dirs:
/// - macOS: `~/Library/Application Support/Claude` (+ `Claude-*` with config file)
/// - Windows: `%APPDATA%\Claude` (+ `Claude-*`)
/// - Linux: `dirs::config_dir()` under the same name rules
fn find_claude_desktop_configs() -> Vec<String> {
    let mut results = Vec::new();
    let roots = claude_desktop_scan_roots();
    for app_support in roots {
        if let Ok(entries) = std::fs::read_dir(&app_support) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "Claude" || name.starts_with("Claude-") {
                    let config_file = entry.path().join("claude_desktop_config.json");
                    if config_file.exists() {
                        let s = entry.path().to_string_lossy().to_string();
                        if !results.contains(&s) {
                            results.push(s);
                        }
                    }
                }
            }
        }
    }
    results
}

fn claude_desktop_scan_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join("Library/Application Support"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::config_dir() {
            // dirs::config_dir() == %APPDATA% on Windows
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
    // Fallback: config_dir when platform-specific root empty
    if roots.is_empty() {
        if let Some(cfg) = dirs::config_dir() {
            roots.push(cfg);
        }
    }
    roots
}

// ===== Main sniff =====

/// Collect install paths for one agent (static `bin_paths` + PATH `search_names`
/// + optional Windows-only candidates). Shim / temp-dir wrappers are filtered out.
fn collect_install_paths(spec: &crate::agents::AgentSpec) -> Vec<String> {
    let mut install_paths = Vec::new();

    // 1. Check static bin_paths (unix/macOS style kept for all platforms)
    for p in spec.bin_paths {
        if let Some(resolved) = resolve_path(p) {
            let path_str = resolved.to_string_lossy().to_string();
            if !is_shim_path(&path_str) && !install_paths.contains(&path_str) {
                install_paths.push(path_str);
            }
        }
    }

    // 1b. Windows-only static candidates from registry
    #[cfg(windows)]
    {
        for p in crate::agents::windows_bin_candidates(spec) {
            if p.exists() {
                let path_str = p.to_string_lossy().to_string();
                if !is_shim_path(&path_str) && !install_paths.contains(&path_str) {
                    install_paths.push(path_str);
                }
            }
        }
    }

    // 2. Always search PATH for CLI binary (may find CLI in addition to App)
    for bin_name in spec.search_names {
        if let Some(found) = find_in_path(bin_name) {
            let path_str = found.to_string_lossy().to_string();
            if !is_shim_path(&path_str) && !install_paths.contains(&path_str) {
                install_paths.push(path_str);
            }
            break;
        }
    }

    // Desktop-launched apps commonly lack shell-managed PATH entries (for
    // example ~/.nvm/.../bin). Check known user-level manager directories for
    // every CLI agent, so detection is consistent with terminal usage.
    if install_paths.is_empty() {
        for bin_name in spec.search_names {
            if let Some(found) = crate::platform::find_in_managed_bin_dirs(bin_name, is_shim_path) {
                let path_str = found.to_string_lossy().to_string();
                if !is_shim_path(&path_str) && !install_paths.contains(&path_str) {
                    install_paths.push(path_str);
                }
                break;
            }
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

            // 4. Special: scan Application Support / AppData for Claude Desktop
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

/// Prefer app bundles / installers over bare CLI paths.
fn order_install_paths(paths: &mut Vec<String>) {
    paths.sort_by_key(|path| {
        let n = path.replace('\\', "/").to_ascii_lowercase();
        let is_app = n.ends_with(".app")
            || n.contains(".app/")
            || n.ends_with(".exe")
            || n.contains("/programs/")
            || n.contains("/program files/")
            || n.contains("/program files (x86)/");
        if is_app {
            0
        } else {
            1
        }
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
        // Windows-style separators
        assert!(is_shim_path(
            r"C:\Users\x\AppData\Local\Temp\cmux-cli-shims\uuid"
        ));
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
        assert!(!is_shim_path(r"C:\Users\x\AppData\Local\Programs\Claude\Claude.exe"));
    }
}
