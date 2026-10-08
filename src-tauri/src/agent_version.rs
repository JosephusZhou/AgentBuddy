//! Agent 版本探测：对注册表内的 CLI agent 执行 `--version` 并解析出版本号。
//!
//! 安全边界：只接受注册表 `name`，可执行文件路径由本模块按 `AgentSpec` 自行解析
//! （`sniff::cli_executable`），**绝不接受前端传入的任意路径** —— 与
//! `open_agent_config_file` 的约束保持一致。
//!
//! 设计取向：探测是"锦上添花"的展示信息，任何异常（未安装 / 无 CLI 入口 / 命令不存在
//! / 输出无版本号 / 超时）都必须安静降级为 `None`，绝不阻塞或影响发现页本身。

use crate::agents::AgentSpec;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// 单个 agent 的探测超时。CLI 的 `--version` 都是即返回的，3s 足够覆盖
/// node/deno 类 CLI 的冷启动；超时后立即 kill，避免留下僵尸进程。
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// 单个 agent 最多读取的行数（stdout / stderr 各取前几行）。版本号总在头部，
/// 限制读取量可防止个别 CLI 把 banner/帮助全文灌进 stdout。
const MAX_LINES: usize = 4;

/// 单个 agent 读取的字节上限，防御异常大的单行输出。
const MAX_LINE_BYTES: usize = 512;

/// 一个 agent 的版本探测结果。
pub struct VersionProbe {
    pub name: String,
    /// `None` = 无 CLI 入口 / 未安装 / 探测失败或超时 / 输出中无版本号。
    pub version: Option<String>,
}

/// 批量探测。每个 agent 一个线程并行执行，单个失败不影响其它 agent；
/// 结果顺序与传入的 `names` 一致。
pub fn probe_versions(names: &[String]) -> Vec<VersionProbe> {
    // 只探测注册表内、且确实有 CLI 入口的 agent（`cli_executable` 已过滤 .app 桌面端）。
    let specs: Vec<&AgentSpec> = names
        .iter()
        .filter_map(|name| crate::agents::find(name))
        .filter(|spec| crate::sniff::cli_executable(spec.name).is_some())
        .collect();

    let results: Vec<Option<String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = specs
            .iter()
            .map(|spec| scope.spawn(|| probe_one(spec)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(None))
            .collect()
    });

    let probed: std::collections::HashMap<&str, Option<String>> = specs
        .iter()
        .zip(results)
        .map(|(spec, version)| (spec.name, version))
        .collect();

    // 保持调用方给定的顺序，未探测到的（未知 name / 无 CLI）也要占位，
    // 前端按 name 建 map，不需要自己补齐。
    names
        .iter()
        .map(|name| VersionProbe {
            version: probed.get(name.as_str()).cloned().flatten(),
            name: name.clone(),
        })
        .collect()
}

/// 执行单个 agent 的 `--version` 并解析版本号。
fn probe_one(spec: &AgentSpec) -> Option<String> {
    let exe = crate::sniff::cli_executable(spec.name)?;

    let mut child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let stdout = child.stdout.take().map(BufReader::new);
    let stderr = child.stderr.take().map(BufReader::new);

    // std 没有"带超时的读"，沿用 `mcp_config::test_stdio` 的做法：读交给工作线程，
    // 主线程用 recv_timeout 兜住上限。
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // 部分 CLI 把版本打到 stderr（如 npm 全局包），故两个流都看，stdout 优先。
        let version = scan_reader(stdout).or_else(|| scan_reader(stderr));
        let _ = tx.send(version);
    });

    let version = rx.recv_timeout(PROBE_TIMEOUT).unwrap_or(None);
    if version.is_none() {
        // 超时（或读线程异常退出）：回收子进程，避免留下挂起的 CLI。
        let _ = child.kill();
        let _ = child.wait();
    }
    version
}

/// 扫描一个输出流的前几行，返回首个能解析出版本号的行。
fn scan_reader<R: std::io::Read>(reader: Option<BufReader<R>>) -> Option<String> {
    reader?
        .lines()
        .take(MAX_LINES)
        .map_while(Result::ok)
        .find_map(|line| parse_version_line(&truncate(line.as_str())))
}

fn truncate(line: &str) -> String {
    if line.len() <= MAX_LINE_BYTES {
        return line.to_string();
    }
    let mut end = MAX_LINE_BYTES;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_string()
}

/// 从一行 CLI 输出里提取版本号，统一归一化为 `v<major>.<minor>[.<patch>]` 形式。
///
/// 纯函数，便于单测。判定规则：按空白与常见分隔符切词，取第一个"以数字开头、
/// 含至少一个点、只由数字与常见版本字符组成"的词；命中后补 `v` 前缀。
/// 形如 `claude-code 2.1.258 (Claude Code)`、`omp 0.6.11`、`pi 0.30.2-rc1+build`
/// 都能解析；`Usage: …`、`error: unknown flag` 等解析为 `None`。
pub fn parse_version_line(line: &str) -> Option<String> {
    for token in line.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')') {
        let token = token.trim_start_matches(['v', 'V', '=']);
        let token = token.trim_end_matches(['.', ',', ';']);
        if !looks_like_version(token) {
            continue;
        }
        return Some(format!("v{token}"));
    }
    None
}

/// 是否是形如 `1.2.3` / `0.6` / `1.2.3-rc.1+build` 的版本号。
fn looks_like_version(token: &str) -> bool {
    if token.is_empty() || !token.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    if !token.contains('.') {
        // 裸整数太容易误命中（模型名、参数值等），要求至少一个点。
        return false;
    }
    token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

#[cfg(test)]
mod tests {
    use super::{looks_like_version, parse_version_line};

    #[test]
    fn parses_common_cli_version_outputs() {
        assert_eq!(parse_version_line("1.2.3").as_deref(), Some("v1.2.3"));
        assert_eq!(
            parse_version_line("claude-code 2.1.258 (Claude Code)").as_deref(),
            Some("v2.1.258")
        );
        assert_eq!(parse_version_line("omp 0.6.11").as_deref(), Some("v0.6.11"));
        assert_eq!(
            parse_version_line("codex-cli 0.20.0").as_deref(),
            Some("v0.20.0")
        );
        assert_eq!(
            parse_version_line("pi version v0.30.2").as_deref(),
            Some("v0.30.2")
        );
    }

    #[test]
    fn keeps_prerelease_and_build_metadata() {
        assert_eq!(
            parse_version_line("oh-my-pi 0.6.11-rc.1+build").as_deref(),
            Some("v0.6.11-rc.1+build")
        );
    }

    #[test]
    fn rejects_non_version_output() {
        assert_eq!(parse_version_line("Usage: claude [options]"), None);
        assert_eq!(parse_version_line("error: unknown flag `--version`"), None);
        assert_eq!(parse_version_line(""), None);
        // 裸整数不应被当成版本号（避免把模型名/参数值误报成版本）。
        assert_eq!(parse_version_line("gpt 4"), None);
        assert_eq!(parse_version_line("2"), None);
    }

    #[test]
    fn version_token_shape_is_strict() {
        assert!(looks_like_version("1.2.3"));
        assert!(looks_like_version("0.6"));
        assert!(looks_like_version("1.2.3-rc.1"));
        assert!(!looks_like_version("1"));
        assert!(!looks_like_version("v1.2.3"));
        assert!(!looks_like_version("1.2.3/4"));
        assert!(!looks_like_version("latest"));
    }
}
