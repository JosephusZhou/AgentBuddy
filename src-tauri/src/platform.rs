//! Cross-platform helpers for paths, process launch, permissions, and links.
//!
//! Keep `cfg(windows)` / `cfg(unix)` branches here so business modules stay
//! free of scattered platform checks.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/* ===== Home / app data ===== */

/// User home directory (`dirs::home_dir`).
pub fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "无法解析用户主目录".to_string())
}

/// Legacy app data root: `~/.agentbuddy` (works on all platforms, including Windows).
pub fn legacy_app_data_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".agentbuddy"))
}

/// Preferred Windows app data root: `%LOCALAPPDATA%\AgentBuddy`.
#[cfg(windows)]
fn windows_preferred_app_data_dir() -> Result<PathBuf, String> {
    if let Some(base) = dirs::data_local_dir() {
        return Ok(base.join("AgentBuddy"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            return Ok(PathBuf::from(local).join("AgentBuddy"));
        }
    }
    if let Some(base) = dirs::data_dir() {
        return Ok(base.join("AgentBuddy"));
    }
    legacy_app_data_dir()
}

/// AgentBuddy application data directory.
///
/// - Non-Windows: always `~/.agentbuddy`
/// - Windows: prefer existing `~/.agentbuddy` (legacy continuity); otherwise
///   `%LOCALAPPDATA%\AgentBuddy`. If the preferred dir does not exist yet but
///   legacy already has data, keep using legacy without migrating.
pub fn app_data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let legacy = legacy_app_data_dir()?;
        let preferred = windows_preferred_app_data_dir()?;
        if preferred.exists() {
            return Ok(preferred);
        }
        if legacy.exists() {
            return Ok(legacy);
        }
        // Fresh install: use Windows-native location.
        Ok(preferred)
    }
    #[cfg(not(windows))]
    {
        legacy_app_data_dir()
    }
}

/// Expand `~`, `~/…`, and on Windows `~\…` / bare `%VAR%` prefixes into a path.
/// Does **not** resolve environment variables embedded mid-string beyond the
/// common home / profile prefixes used in UI display.
pub fn expand_home(input: &str) -> Result<PathBuf, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("路径为空".to_string());
    }

    if s == "~" {
        return home_dir();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }
    if let Some(rest) = s.strip_prefix("~\\") {
        return Ok(home_dir()?.join(rest));
    }

    #[cfg(windows)]
    {
        // Expand a few well-known env-style prefixes used in docs/UI.
        for (var, prefix) in [
            ("USERPROFILE", "%USERPROFILE%"),
            ("APPDATA", "%APPDATA%"),
            ("LOCALAPPDATA", "%LOCALAPPDATA%"),
            ("PROGRAMFILES", "%PROGRAMFILES%"),
            ("PROGRAMFILES(X86)", "%PROGRAMFILES(X86)%"),
        ] {
            if s.eq_ignore_ascii_case(prefix) {
                if let Ok(v) = std::env::var(var) {
                    return Ok(PathBuf::from(v));
                }
            }
            let with_slash_fwd = format!("{prefix}/");
            let with_slash_bwd = format!("{prefix}\\");
            if let Some(rest) = s
                .strip_prefix(&with_slash_fwd)
                .or_else(|| s.strip_prefix(&with_slash_bwd))
            {
                if let Ok(v) = std::env::var(var) {
                    return Ok(PathBuf::from(v).join(rest));
                }
            }
            // Case-insensitive prefix match for Windows.
            let lower = s.to_ascii_lowercase();
            let pref_l = prefix.to_ascii_lowercase();
            if lower.starts_with(&pref_l) {
                let rest = &s[prefix.len()..];
                let rest = rest.trim_start_matches(['/', '\\']);
                if let Ok(v) = std::env::var(var) {
                    return Ok(if rest.is_empty() {
                        PathBuf::from(v)
                    } else {
                        PathBuf::from(v).join(rest)
                    });
                }
            }
        }
    }

    Ok(PathBuf::from(s))
}

/// Expand `~` for convenience when failure should fall back to the original string.
pub fn expand_tilde_lossy(path: &str) -> String {
    expand_home(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// Display path with home replaced by `~` (forward-slash style for UI).
pub fn display_path(abs: &str) -> String {
    if let Ok(home) = home_dir() {
        let home_s = home.to_string_lossy();
        if abs == home_s.as_ref() {
            return "~".to_string();
        }
        // Accept both separators when stripping home prefix.
        let candidates = [format!("{}/", home_s), format!("{}\\", home_s)];
        for prefix in &candidates {
            if let Some(rest) = abs.strip_prefix(prefix.as_str()) {
                let rest = rest.replace('\\', "/");
                return format!("~/{rest}");
            }
        }
        // Case-insensitive on Windows.
        #[cfg(windows)]
        {
            let abs_l = abs.to_ascii_lowercase();
            let home_l = home_s.to_ascii_lowercase();
            if abs_l == home_l {
                return "~".to_string();
            }
            for sep in ['/', '\\'] {
                let prefix = format!("{home_l}{sep}");
                if let Some(rest) = abs_l.strip_prefix(&prefix) {
                    // Preserve original casing from abs after the home prefix length.
                    let rest_orig = &abs[prefix.len()..];
                    let rest_norm = rest_orig.replace('\\', "/");
                    let _ = rest;
                    return format!("~/{rest_norm}");
                }
            }
        }
    }
    abs.to_string()
}

/* ===== PATH / executables ===== */

/// Candidate executable names for PATH search on the current OS.
pub fn candidate_executable_names(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if name.is_empty() {
        return out;
    }
    #[cfg(windows)]
    {
        // PATHEXT order (default if unset).
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
        let mut exts: Vec<String> = pathext
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if exts.is_empty() {
            exts = vec![".EXE".into(), ".CMD".into(), ".BAT".into(), ".COM".into()];
        }
        // If the name already has an extension matching PATHEXT, keep as-is first.
        let has_ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let dotted = format!(".{}", e);
                exts.iter().any(|x| x.eq_ignore_ascii_case(&dotted))
            })
            .unwrap_or(false);
        if has_ext {
            out.push(name.to_string());
        } else {
            for ext in &exts {
                out.push(format!("{name}{ext}"));
            }
            // Bare name last (may still resolve via associations).
            out.push(name.to_string());
        }
    }
    #[cfg(not(windows))]
    {
        out.push(name.to_string());
    }
    out
}

fn find_in_dirs<I, F>(dirs: I, binary_name: &str, is_shim: F) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
    F: Fn(&str) -> bool + Copy,
{
    for dir in dirs {
        let dir_s = dir.to_string_lossy();
        if is_shim(dir_s.as_ref()) {
            continue;
        }
        for name in candidate_executable_names(binary_name) {
            let full = dir.join(&name);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// Search `PATH` for `binary_name`, applying PATHEXT on Windows and skipping shim dirs.
pub fn find_in_path<F>(binary_name: &str, is_shim: F) -> Option<PathBuf>
where
    F: Fn(&str) -> bool + Copy,
{
    let path_var = std::env::var_os("PATH")?;
    find_in_dirs(std::env::split_paths(&path_var), binary_name, is_shim)
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn append_version_bin_dirs(paths: &mut Vec<PathBuf>, root: &Path, suffix: &[&str]) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let mut bin = entry.path();
        for part in suffix {
            bin.push(part);
        }
        push_unique_path(paths, bin);
    }
}

fn managed_cli_bin_dirs_for_home(home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Common user-level package-manager and version-manager shims.
    for relative in [
        ".local/bin",
        ".bun/bin",
        ".volta/bin",
        ".asdf/shims",
        ".nodenv/shims",
        ".nodebrew/current/bin",
        ".npm-global/bin",
        ".yarn/bin",
        ".config/yarn/global/node_modules/.bin",
        ".local/share/pnpm",
        ".local/share/mise/shims",
        ".mise/shims",
        "Library/pnpm",
    ] {
        push_unique_path(&mut dirs, home.join(relative));
    }

    // nvm keeps each active Node version's global binaries in its own bin dir.
    append_version_bin_dirs(&mut dirs, &home.join(".nvm/versions/node"), &["bin"]);

    // fnm uses a different layout on Unix/macOS installations.
    append_version_bin_dirs(
        &mut dirs,
        &home.join(".local/share/fnm/node-versions"),
        &["installation", "bin"],
    );
    append_version_bin_dirs(
        &mut dirs,
        &home.join("Library/Application Support/fnm/node-versions"),
        &["installation", "bin"],
    );

    #[cfg(windows)]
    {
        for relative in [
            "AppData/Roaming/npm",
            "AppData/Local/pnpm",
            "AppData/Local/Volta/bin",
        ] {
            push_unique_path(&mut dirs, home.join(relative));
        }
        if let Some(nvm_home) = std::env::var_os("NVM_HOME") {
            push_unique_path(&mut dirs, PathBuf::from(nvm_home));
        }
    }

    dirs
}

/// Find a CLI in known user-level package/version-manager directories.
///
/// Desktop apps often do not inherit shell startup files, so relying only on
/// `PATH` misses CLIs installed by nvm/fnm and similar tools. The directory
/// list is deliberately explicit and shallow to avoid treating arbitrary home
/// directory files as installed agents.
pub fn find_in_managed_bin_dirs<F>(binary_name: &str, is_shim: F) -> Option<PathBuf>
where
    F: Fn(&str) -> bool + Copy,
{
    let home = home_dir().ok()?;
    find_in_dirs(managed_cli_bin_dirs_for_home(&home), binary_name, is_shim)
}

/* ===== Open / reveal ===== */

/// Open a file or directory with the system default handler.
/// Path is always passed as a separate argv element (never shell-concatenated).
pub fn open_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("路径不存在: {}", path.display()));
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg("--")
            .arg(path.as_os_str())
            .status()
            .map_err(|e| format!("打开失败: {e}"))?;
        if !status.success() {
            return Err(format!("打开失败（退出码: {:?}）", status.code()));
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // `cmd /c start "" <path>` can mishandle paths; use explorer / ShellExecute via `start`.
        // Prefer `explorer.exe` for directories; for files use `cmd /C start "" path`.
        if path.is_dir() {
            let status = Command::new("explorer.exe")
                .arg(path.as_os_str())
                .status()
                .map_err(|e| format!("打开失败: {e}"))?;
            // explorer often returns non-zero even on success; only fail on spawn error.
            let _ = status;
            return Ok(());
        }
        // For files: use `cmd /C start "" <path>` with CREATE_NO_WINDOW-friendly args.
        let status = Command::new("cmd")
            .args(["/C", "start", "", "/B"])
            .arg(path.as_os_str())
            .status()
            .map_err(|e| format!("打开失败: {e}"))?;
        if !status.success() {
            // Fallback: explorer with the file path.
            let _ = Command::new("explorer.exe").arg(path.as_os_str()).status();
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let status = Command::new("xdg-open")
            .arg(path.as_os_str())
            .status()
            .map_err(|e| format!("打开失败: {e}"))?;
        if !status.success() {
            return Err(format!("打开失败（退出码: {:?}）", status.code()));
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        Err("当前平台不支持打开路径".into())
    }
}

/// Reveal a path in the file manager (select file when possible).
pub fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if path.exists() {
            let status = Command::new("open")
                .arg("-R")
                .arg(path.as_os_str())
                .status()
                .map_err(|e| format!("在 Finder 中显示失败: {e}"))?;
            if !status.success() {
                return Err(format!(
                    "在 Finder 中显示失败（退出码: {:?}）",
                    status.code()
                ));
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            return open_path(parent);
        }
        return Err(format!("路径不存在: {}", path.display()));
    }

    #[cfg(target_os = "windows")]
    {
        if path.exists() {
            if path.is_file() {
                // explorer /select,<path> — comma must be glued to the path.
                let arg = format!("/select,{}", path.display());
                let _ = Command::new("explorer.exe")
                    .arg(arg)
                    .status()
                    .map_err(|e| format!("在资源管理器中显示失败: {e}"))?;
                return Ok(());
            }
            return open_path(path);
        }
        if let Some(parent) = path.parent().filter(|p| p.exists()) {
            return open_path(parent);
        }
        return Err(format!("路径不存在: {}", path.display()));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if path.is_dir() && path.exists() {
            return open_path(path);
        }
        if let Some(parent) = path.parent().filter(|p| p.exists()) {
            return open_path(parent);
        }
        if path.exists() {
            return open_path(path);
        }
        return Err(format!("路径不存在: {}", path.display()));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        Err("当前平台不支持显示路径".into())
    }
}

/// Open an http(s) URL in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err(format!("不支持的链接协议: {trimmed}"));
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg("--")
            .arg(trimmed)
            .status()
            .map_err(|e| format!("打开链接失败: {e}"))?;
        if !status.success() {
            return Err(format!("打开链接失败（退出码 {:?}）", status.code()));
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd")
            .args(["/C", "start", "", "/B"])
            .arg(trimmed)
            .status()
            .map_err(|e| format!("打开链接失败: {e}"))?;
        if !status.success() {
            return Err(format!("打开链接失败（退出码 {:?}）", status.code()));
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let status = Command::new("xdg-open")
            .arg(trimmed)
            .status()
            .map_err(|e| format!("打开链接失败: {e}"))?;
        if !status.success() {
            return Err(format!("打开链接失败（退出码 {:?}）", status.code()));
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        Err("当前平台不支持打开链接".into())
    }
}

/* ===== Permissions ===== */

/// Restrict a file to owner-only access (`0o600` on Unix). No-op on Windows.
pub fn set_owner_only_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Restrict a directory to owner-only access (`0o700` on Unix). No-op on Windows.
#[allow(dead_code)] // used by backup / future ACL paths; kept for API symmetry
pub fn set_owner_only_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Set unix mode bits when available; no-op on Windows.
pub fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

/// Read current unix mode bits; `None` on Windows or if metadata fails.
pub fn current_mode(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).ok().map(|m| m.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// After an atomic rewrite: restore previous mode if known, else owner-only file.
pub fn restore_or_private_file(path: &Path, previous_mode: Option<u32>) {
    if let Some(mode) = previous_mode {
        set_mode(path, mode);
    } else {
        set_owner_only_file(path);
    }
}

/* ===== Symlink / copy ===== */

/// Create a directory symlink (or file symlink when `source` is a file).
/// On Windows this requires privilege / Developer Mode for `symlink_dir`.
///
/// **Caveat:** on Windows the dir/file choice uses `source.is_dir()`, which for a
/// *relative* `source` is resolved against the process CWD — not `dest`'s parent.
/// Prefer [`symlink_dir`] when the link target is known to be a directory
/// (especially with relative sources like `../.agents/commands`).
pub fn symlink_any(source: &Path, dest: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, dest).map_err(|e| format!("创建软链接失败: {e}"))
    }
    #[cfg(windows)]
    {
        if source.is_dir() {
            std::os::windows::fs::symlink_dir(source, dest)
                .map_err(|e| format!("创建目录软链接失败: {e}"))
        } else {
            std::os::windows::fs::symlink_file(source, dest)
                .map_err(|e| format!("创建文件软链接失败: {e}"))
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (source, dest);
        Err("当前平台不支持软链接".into())
    }
}

/// Create a **directory** symlink. `source` is stored as-is (absolute or relative);
/// this never probes `source.is_dir()` against the process CWD.
/// On Windows requires privilege / Developer Mode.
pub fn symlink_dir(source: &Path, dest: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, dest).map_err(|e| format!("创建软链接失败: {e}"))
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, dest)
            .map_err(|e| format!("创建目录软链接失败: {e}"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (source, dest);
        Err("当前平台不支持软链接".into())
    }
}

/// Recursive directory copy (files + dirs only; skips symlink entries).
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源不是目录: {}", src.display()));
    }
    fs::create_dir_all(dst).map_err(|e| format!("创建目标目录失败: {e}"))?;
    for entry in fs::read_dir(src).map_err(|e| format!("读取源目录失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取文件类型失败: {e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {e}"))?;
            }
            fs::copy(&from, &to).map_err(|e| format!("复制文件失败: {e}"))?;
        }
    }
    Ok(())
}

/* ===== Folder picker (native dialogs) ===== */

/// Open a native folder picker. Returns `Ok(None)` when the user cancels.
pub fn pick_folder(prompt: &str) -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    {
        // Escape double quotes in AppleScript string literal.
        let safe = prompt.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "try\nPOSIX path of (choose folder with prompt \"{safe}\")\non error number -128\nreturn \"\"\nend try"
        );
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| format!("无法打开文件夹选择器: {e}"))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "文件夹选择失败".into()
            } else {
                err
            });
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            return Ok(None);
        }
        return Ok(Some(PathBuf::from(path)));
    }

    #[cfg(target_os = "windows")]
    {
        // PowerShell FolderBrowserDialog — works without extra crates.
        let safe = prompt.replace('\'', "''");
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = '{safe}'; \
             $d.ShowNewFolderButton = $true; \
             if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ \
               [Console]::Out.Write($d.SelectedPath) \
             }}"
        );
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &ps,
            ])
            .output()
            .map_err(|e| format!("无法打开文件夹选择器: {e}"))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "文件夹选择失败".into()
            } else {
                err
            });
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            return Ok(None);
        }
        return Ok(Some(PathBuf::from(path)));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Prefer zenity, then kdialog.
        let try_zenity = Command::new("zenity")
            .args(["--file-selection", "--directory", "--title", prompt])
            .output();
        if let Ok(output) = try_zenity {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(PathBuf::from(path)));
            }
            // Cancel often exits non-zero with empty stdout.
            if output.stdout.is_empty() {
                return Ok(None);
            }
        }
        let try_kdialog = Command::new("kdialog")
            .args(["--getexistingdirectory", ".", "--title", prompt])
            .output();
        if let Ok(output) = try_kdialog {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(PathBuf::from(path)));
            }
            if output.stdout.is_empty() {
                return Ok(None);
            }
        }
        return Err("未找到文件夹选择器（需要 zenity 或 kdialog）".into());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = prompt;
        Err("当前平台不支持文件夹选择器".into())
    }
}

/* ===== Env helpers for Windows path candidates ===== */

/// Resolve a Windows env-based path fragment, e.g. `LOCALAPPDATA` + `Programs/Claude`.
/// Returns `None` when the env var is missing.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_env_path(var: &str, relative: &str) -> Option<PathBuf> {
    let base = std::env::var_os(var)?;
    if base.is_empty() {
        return None;
    }
    let mut p = PathBuf::from(base);
    if !relative.is_empty() {
        for part in relative.split(['/', '\\']) {
            if !part.is_empty() {
                p.push(part);
            }
        }
    }
    Some(p)
}

/* ===== Tests ===== */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_home_tilde() {
        let home = home_dir().expect("home");
        assert_eq!(expand_home("~").unwrap(), home);
        assert_eq!(expand_home("~/foo/bar").unwrap(), home.join("foo/bar"));
    }

    #[test]
    fn expand_home_plain() {
        assert_eq!(expand_home("/tmp/x").unwrap(), PathBuf::from("/tmp/x"));
    }

    #[test]
    fn display_path_strips_home() {
        let home = home_dir().expect("home");
        let abs = home.join("a/b").to_string_lossy().to_string();
        let d = display_path(&abs);
        assert!(d.starts_with("~/"), "got {d}");
        assert!(d.contains("a/b") || d.contains("a\\b"));
    }

    #[test]
    fn candidate_names_include_original() {
        let names = candidate_executable_names("codex");
        assert!(!names.is_empty());
        assert!(names
            .iter()
            .any(|n| n == "codex" || n.starts_with("codex.")));
    }

    #[test]
    fn managed_dirs_include_nvm_and_fnm_version_bins() {
        let base = std::env::temp_dir().join(format!(
            "agentbuddy-managed-bin-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let nvm_bin = base.join(".nvm/versions/node/v22.1.0/bin");
        let fnm_bin = base.join(".local/share/fnm/node-versions/v22.1.0/installation/bin");
        fs::create_dir_all(&nvm_bin).unwrap();
        fs::create_dir_all(&fnm_bin).unwrap();

        let dirs = managed_cli_bin_dirs_for_home(&base);
        assert!(dirs.contains(&nvm_bin));
        assert!(dirs.contains(&fnm_bin));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn find_in_dirs_finds_cli_without_path_environment() {
        let base = std::env::temp_dir().join(format!(
            "agentbuddy-managed-bin-find-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let bin = base.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let pi = bin.join("pi");
        fs::write(&pi, "#!/usr/bin/env node\n").unwrap();

        let found = find_in_dirs(vec![bin], "pi", |_| false);
        assert_eq!(found, Some(pi));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn copy_dir_recursive_roundtrip() {
        let base =
            std::env::temp_dir().join(format!("agentbuddy-platform-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let src = base.join("src");
        let dst = base.join("dst");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("nested/file.txt"), b"hi").unwrap();
        copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(
            fs::read_to_string(dst.join("nested/file.txt")).unwrap(),
            "hi"
        );
        let _ = fs::remove_dir_all(&base);
    }
}
