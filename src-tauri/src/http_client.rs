//! Shared HTTP client helpers that honor app network/proxy settings.

use crate::config::{load_network_settings, ProxyMode, ProxyProtocol, ProxySettings};
use reqwest::blocking::ClientBuilder;
use reqwest::Proxy;
use std::process::Command;

/// Apply `~/.agentbuddy` network proxy settings onto a reqwest builder.
/// Call this on every outbound HTTP client so WebDAV / Skills / MCP / OpenCode share one policy.
pub fn apply_proxy(builder: ClientBuilder) -> Result<ClientBuilder, String> {
    let settings = load_network_settings().unwrap_or_default();
    apply_proxy_settings(builder, &settings.proxy)
}

pub fn apply_proxy_settings(
    builder: ClientBuilder,
    settings: &ProxySettings,
) -> Result<ClientBuilder, String> {
    match settings.mode {
        ProxyMode::None => Ok(builder.no_proxy()),
        ProxyMode::System => apply_system_proxy(builder),
        ProxyMode::Custom => apply_custom_proxy(builder, settings),
    }
}

fn apply_custom_proxy(
    builder: ClientBuilder,
    settings: &ProxySettings,
) -> Result<ClientBuilder, String> {
    let url = build_custom_proxy_url(settings)?;
    let proxy =
        Proxy::all(&url).map_err(|e| format!("无效的自定义代理地址「{}」: {}", url, e))?;
    Ok(builder.proxy(proxy))
}

fn build_custom_proxy_url(settings: &ProxySettings) -> Result<String, String> {
    let host = settings.host.trim();
    if host.is_empty() {
        return Err("自定义代理需要填写主机地址".to_string());
    }
    if host.contains("://") || host.contains('/') || host.contains('@') {
        return Err("主机地址只需填写域名或 IP，不要包含协议或路径".to_string());
    }

    let port = settings.port;
    if port == 0 {
        return Err("自定义代理端口无效".to_string());
    }

    let scheme = match settings.protocol {
        ProxyProtocol::Http => "http",
        ProxyProtocol::Socks5 => "socks5",
    };

    let user = settings.username.trim();
    let pass = settings.password.as_str();
    if user.is_empty() {
        Ok(format!("{scheme}://{host}:{port}"))
    } else {
        let user_enc = encode_userinfo(user);
        let pass_enc = encode_userinfo(pass);
        Ok(format!("{scheme}://{user_enc}:{pass_enc}@{host}:{port}"))
    }
}

fn encode_userinfo(value: &str) -> String {
    // Minimal URL userinfo encoding (RFC 3986 unreserved + sub-delims commonly allowed).
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'=' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(nibble(b >> 4));
                out.push(nibble(b & 0x0f));
            }
        }
    }
    out
}

fn nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => '0',
    }
}

fn apply_system_proxy(builder: ClientBuilder) -> Result<ClientBuilder, String> {
    // 1) Environment variables (standard for CLI/desktop tools).
    if let Some(url) = env_proxy_url() {
        match Proxy::all(&url) {
            Ok(proxy) => return Ok(builder.proxy(proxy)),
            Err(e) => {
                return Err(format!(
                    "系统代理环境变量无效「{}」: {}",
                    redact_proxy_url(&url),
                    e
                ));
            }
        }
    }

    // 2) macOS system proxy via `scutil --proxy`.
    #[cfg(target_os = "macos")]
    if let Some(url) = macos_system_proxy_url() {
        match Proxy::all(&url) {
            Ok(proxy) => return Ok(builder.proxy(proxy)),
            Err(e) => {
                return Err(format!(
                    "macOS 系统代理无效「{}」: {}",
                    redact_proxy_url(&url),
                    e
                ));
            }
        }
    }

    // No system proxy configured — direct connection (not no_proxy; leave default).
    Ok(builder)
}

fn env_proxy_url() -> Option<String> {
    const KEYS: &[&str] = &[
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ];
    for key in KEYS {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

/// Parse `scutil --proxy` output into a single proxy URL preferred order:
/// SOCKS → HTTPS → HTTP.
#[cfg(target_os = "macos")]
fn macos_system_proxy_url() -> Option<String> {
    let output = Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_scutil_proxy(&text)
}

#[cfg(target_os = "macos")]
fn parse_scutil_proxy(text: &str) -> Option<String> {
    let mut map = std::collections::HashMap::<String, String>::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Formats: "HTTPEnable : 1" or "  HTTPProxy : 127.0.0.1"
        // Skip non key-value lines (e.g. "<dictionary> {", "}") — never `?` here
        // or a single header line would abort the whole parse.
        let Some((key_raw, val_raw)) = line.split_once(':') else {
            continue;
        };
        let key = key_raw.trim();
        let val = val_raw.trim();
        if !key.is_empty() {
            map.insert(key.to_string(), val.to_string());
        }
    }

    let enabled = |key: &str| {
        map.get(key)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false)
    };

    if enabled("SOCKSEnable") {
        if let (Some(host), Some(port)) = (map.get("SOCKSProxy"), map.get("SOCKSPort")) {
            let host = host.trim();
            let port = port.trim();
            if !host.is_empty() && !port.is_empty() {
                return Some(format!("socks5://{host}:{port}"));
            }
        }
    }
    if enabled("HTTPSEnable") {
        if let (Some(host), Some(port)) = (map.get("HTTPSProxy"), map.get("HTTPSPort")) {
            let host = host.trim();
            let port = port.trim();
            if !host.is_empty() && !port.is_empty() {
                return Some(format!("http://{host}:{port}"));
            }
        }
    }
    if enabled("HTTPEnable") {
        if let (Some(host), Some(port)) = (map.get("HTTPProxy"), map.get("HTTPPort")) {
            let host = host.trim();
            let port = port.trim();
            if !host.is_empty() && !port.is_empty() {
                return Some(format!("http://{host}:{port}"));
            }
        }
    }
    None
}

fn redact_proxy_url(url: &str) -> String {
    // Hide credentials if present: scheme://user:pass@host → scheme://***@host
    if let Some(at) = url.rfind('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..scheme_end + 3];
            let host_part = &url[at + 1..];
            return format!("{scheme}***@{host_part}");
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProxyMode, ProxyProtocol, ProxySettings};

    #[test]
    fn custom_http_url_without_auth() {
        let s = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Http,
            host: "127.0.0.1".into(),
            port: 7890,
            username: String::new(),
            password: String::new(),
        };
        assert_eq!(
            build_custom_proxy_url(&s).unwrap(),
            "http://127.0.0.1:7890"
        );
    }

    #[test]
    fn custom_socks5_url_with_auth_encodes() {
        let s = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Socks5,
            host: "proxy.example.com".into(),
            port: 1080,
            username: "user@x".into(),
            password: "p@ss:w".into(),
        };
        let url = build_custom_proxy_url(&s).unwrap();
        assert_eq!(url, "socks5://user%40x:p%40ss%3Aw@proxy.example.com:1080");
    }

    #[test]
    fn rejects_empty_host() {
        let s = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Http,
            host: "  ".into(),
            port: 8080,
            username: String::new(),
            password: String::new(),
        };
        assert!(build_custom_proxy_url(&s).is_err());
    }

    #[test]
    fn rejects_zero_port() {
        let s = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Http,
            host: "127.0.0.1".into(),
            port: 0,
            username: String::new(),
            password: String::new(),
        };
        assert!(build_custom_proxy_url(&s).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_scutil_prefers_socks() {
        let sample = r#"
<dictionary> {
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
  SOCKSEnable : 1
  SOCKSPort : 7891
  SOCKSProxy : 127.0.0.1
}
"#;
        assert_eq!(
            parse_scutil_proxy(sample).as_deref(),
            Some("socks5://127.0.0.1:7891")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_scutil_https_when_no_socks() {
        let sample = r#"
  HTTPEnable : 0
  HTTPSEnable : 1
  HTTPSPort : 8080
  HTTPSProxy : 10.0.0.2
  SOCKSEnable : 0
"#;
        assert_eq!(
            parse_scutil_proxy(sample).as_deref(),
            Some("http://10.0.0.2:8080")
        );
    }

    #[test]
    fn redact_hides_userinfo() {
        assert_eq!(
            redact_proxy_url("http://alice:secret@127.0.0.1:7890"),
            "http://***@127.0.0.1:7890"
        );
        assert_eq!(
            redact_proxy_url("socks5://127.0.0.1:1080"),
            "socks5://127.0.0.1:1080"
        );
    }
}
