//! WebDAV connection management: encrypted password storage + connectivity probe + upload.

use crate::config;
use crate::crypto;
use crate::db;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// Max AgentBuddy backup archives retained per remote directory after a successful upload.
pub const BACKUP_REMOTE_KEEP: usize = 3;
const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop><d:resourcetype/></d:prop>
</d:propfind>"#;
const PROPFIND_LIST_BODY: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:resourcetype/>
    <d:getcontentlength/>
    <d:getlastmodified/>
    <d:displayname/>
  </d:prop>
</d:propfind>"#;

/// Public list item — never includes password material.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavConnection {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    pub status: String,
    pub last_error: String,
    pub last_checked_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Internal DB row including encrypted password fields.
#[derive(Debug, Clone)]
pub struct WebDavConnectionRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password_salt: String,
    pub password_nonce: String,
    pub password_cipher: String,
    pub status: String,
    pub last_error: String,
    pub last_checked_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<WebDavConnectionRow> for WebDavConnection {
    fn from(row: WebDavConnectionRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            url: row.url,
            username: row.username,
            status: row.status,
            last_error: row.last_error,
            last_checked_at: row.last_checked_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavUpsertPayload {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    /// Required on create; empty/omitted on edit keeps existing ciphertext.
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavTestResult {
    pub ok: bool,
    pub status: String,
    pub message: String,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavDraftProbe {
    pub url: String,
    pub username: String,
    pub password: String,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn validate_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("服务器地址必须以 http:// 或 https:// 开头".to_string());
    }
    Ok(())
}

fn normalize_status(status: &str) -> String {
    match status {
        "connected" => "connected".to_string(),
        _ => "disconnected".to_string(),
    }
}

pub fn list_connections() -> Result<Vec<WebDavConnection>, String> {
    db::load_webdav_connections()
}

pub fn upsert_connection(payload: WebDavUpsertPayload) -> Result<WebDavConnection, String> {
    let id = payload.id.trim().to_string();
    let name = payload.name.trim().to_string();
    let url = payload.url.trim().to_string();
    let username = payload.username.trim().to_string();
    let password = payload
        .password
        .as_ref()
        .map(|p| p.as_str().trim())
        .filter(|p| !p.is_empty());

    if id.is_empty() {
        return Err("连接 ID 不能为空".to_string());
    }
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if username.is_empty() {
        return Err("用户名不能为空".to_string());
    }
    validate_url(&url)?;

    let existing = db::get_webdav_connection_row(&id)?;
    let now = now_secs();
    let master = config::load_secrets_key()?;

    let (password_salt, password_nonce, password_cipher) = match password {
        Some(plain) => {
            let enc = crypto::encrypt_secret(&master, plain)?;
            (enc.salt, enc.nonce, enc.cipher)
        }
        None => {
            let row = existing
                .as_ref()
                .ok_or_else(|| "新建连接时密码不能为空".to_string())?;
            (
                row.password_salt.clone(),
                row.password_nonce.clone(),
                row.password_cipher.clone(),
            )
        }
    };

    let (status, last_error, last_checked_at, created_at) = match &existing {
        Some(row) => (
            normalize_status(&row.status),
            row.last_error.clone(),
            row.last_checked_at,
            row.created_at,
        ),
        None => ("disconnected".to_string(), String::new(), None, now),
    };

    let row = WebDavConnectionRow {
        id: id.clone(),
        name,
        url,
        username,
        password_salt,
        password_nonce,
        password_cipher,
        status,
        last_error,
        last_checked_at,
        created_at,
        updated_at: now,
    };

    db::upsert_webdav_connection_row(&row)?;
    Ok(WebDavConnection::from(row))
}

pub fn delete_connection(id: String) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("连接 ID 不能为空".to_string());
    }
    db::delete_webdav_connection(id)
}

pub fn test_connection(id: String) -> Result<WebDavTestResult, String> {
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err("连接 ID 不能为空".to_string());
    }

    let row = db::get_webdav_connection_row(&id)?
        .ok_or_else(|| format!("WebDAV 连接不存在: {}", id))?;

    let master = config::load_secrets_key()?;
    let password = crypto::decrypt_secret(
        &master,
        &row.password_salt,
        &row.password_nonce,
        &row.password_cipher,
    )?;

    let result = probe_webdav(&row.url, &row.username, &password);
    let checked_at = now_secs();
    let last_error = if result.ok {
        String::new()
    } else {
        result.message.clone()
    };

    db::update_webdav_status(&id, &result.status, &last_error, checked_at)?;
    Ok(result)
}

pub fn test_connection_draft(draft: WebDavDraftProbe) -> Result<WebDavTestResult, String> {
    let url = draft.url.trim();
    let username = draft.username.trim();
    let password = draft.password.as_str();
    if username.is_empty() {
        return Err("用户名不能为空".to_string());
    }
    if password.is_empty() {
        return Err("密码不能为空".to_string());
    }
    validate_url(url)?;
    Ok(probe_webdav(url, username, password))
}

fn probe_webdav(url: &str, username: &str, password: &str) -> WebDavTestResult {
    let client = match build_client() {
        Ok(c) => c,
        Err(message) => {
            return WebDavTestResult {
                ok: false,
                status: "disconnected".to_string(),
                message,
                http_status: None,
            };
        }
    };

    match send_propfind(&client, url, username, password) {
        Ok(result) => {
            if matches!(result.http_status, Some(405) | Some(501)) {
                return send_options(&client, url, username, password);
            }
            result
        }
        Err(_) => send_options(&client, url, username, password),
    }
}

fn build_client() -> Result<Client, String> {
    let builder = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("AgentBuddy/0.1 (WebDAV probe)");
    crate::http_client::apply_proxy(builder)?
        .build()
        .map_err(|e| format!("无法创建 HTTP 客户端: {}", e))
}

fn auth_header(username: &str, password: &str) -> Result<HeaderValue, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let token = B64.encode(format!("{}:{}", username, password));
    HeaderValue::from_str(&format!("Basic {}", token))
        .map_err(|e| format!("无效的认证头: {}", e))
}

fn send_propfind(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<WebDavTestResult, String> {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth_header(username, password)?);
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("AgentBuddy/0.1 (WebDAV probe)"),
    );
    headers.insert("Depth", HeaderValue::from_static("0"));
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );

    let method = Method::from_bytes(b"PROPFIND").unwrap_or(Method::GET);
    let response = client
        .request(method, url)
        .headers(headers)
        .body(PROPFIND_BODY)
        .send()
        .map_err(map_network_error)?;

    Ok(map_http_status(response.status().as_u16(), false))
}

fn send_options(client: &Client, url: &str, username: &str, password: &str) -> WebDavTestResult {
    let mut headers = HeaderMap::new();
    match auth_header(username, password) {
        Ok(value) => {
            headers.insert(AUTHORIZATION, value);
        }
        Err(message) => {
            return WebDavTestResult {
                ok: false,
                status: "disconnected".to_string(),
                message,
                http_status: None,
            };
        }
    }

    let response = match client.request(Method::OPTIONS, url).headers(headers).send() {
        Ok(r) => r,
        Err(err) => {
            return WebDavTestResult {
                ok: false,
                status: "disconnected".to_string(),
                message: map_network_error(err),
                http_status: None,
            };
        }
    };

    let code = response.status().as_u16();
    let has_dav = response.headers().contains_key("dav")
        || response
            .headers()
            .get("allow")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_uppercase().contains("PROPFIND") || !s.is_empty())
            .unwrap_or(false);

    if (200..300).contains(&code) || has_dav {
        WebDavTestResult {
            ok: true,
            status: "connected".to_string(),
            message: "连接成功".to_string(),
            http_status: Some(code),
        }
    } else {
        map_http_status(code, true)
    }
}

fn map_http_status(code: u16, from_options: bool) -> WebDavTestResult {
    match code {
        200 | 207 => WebDavTestResult {
            ok: true,
            status: "connected".to_string(),
            message: "连接成功".to_string(),
            http_status: Some(code),
        },
        401 | 403 => WebDavTestResult {
            ok: false,
            status: "disconnected".to_string(),
            message: "认证失败，请检查用户名或密码".to_string(),
            http_status: Some(code),
        },
        404 => WebDavTestResult {
            ok: false,
            status: "disconnected".to_string(),
            message: "路径不存在".to_string(),
            http_status: Some(code),
        },
        405 | 501 if !from_options => WebDavTestResult {
            ok: false,
            status: "disconnected".to_string(),
            message: format!("服务器返回 HTTP {}", code),
            http_status: Some(code),
        },
        _ => WebDavTestResult {
            ok: false,
            status: "disconnected".to_string(),
            message: format!("服务器返回 HTTP {}", code),
            http_status: Some(code),
        },
    }
}

fn map_network_error(err: reqwest::Error) -> String {
    if err.is_timeout() {
        return "连接超时".to_string();
    }
    if err.is_connect() {
        return "无法连接服务器".to_string();
    }
    let msg = err.to_string();
    if msg.to_ascii_lowercase().contains("certificate")
        || msg.to_ascii_lowercase().contains("tls")
        || msg.to_ascii_lowercase().contains("ssl")
    {
        return "TLS 证书错误".to_string();
    }
    if msg.chars().count() > 80 {
        format!("{}…", msg.chars().take(80).collect::<String>())
    } else {
        msg
    }
}

// ===== Upload primitives (MKCOL / PUT) =====

fn build_upload_client() -> Result<Client, String> {
    let builder = Client::builder()
        .timeout(UPLOAD_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("AgentBuddy/0.1 (WebDAV upload)");
    crate::http_client::apply_proxy(builder)?
        .build()
        .map_err(|e| format!("无法创建 HTTP 客户端: {}", e))
}

/// Join base WebDAV URL with path segments (each segment is percent-encoded for path).
pub fn join_webdav_url(base: &str, segments: &[&str]) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("WebDAV 地址为空".to_string());
    }
    let mut url = base.to_string();
    for seg in segments {
        let seg = seg.trim().trim_matches('/');
        if seg.is_empty() {
            continue;
        }
        for part in seg.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err("远程路径不允许包含 ..".to_string());
            }
            url.push('/');
            url.push_str(&encode_path_segment(part));
        }
    }
    Ok(url)
}

fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// Ensure remote directory exists by recursive MKCOL. Existing dirs (405/409/301) are ok.
pub fn ensure_remote_dir(
    base_url: &str,
    username: &str,
    password: &str,
    rel_dir: &str,
) -> Result<String, String> {
    let client = build_upload_client()?;
    let parts: Vec<&str> = rel_dir
        .split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.iter().any(|p| *p == "..") {
        return Err("远程路径不允许包含 ..".to_string());
    }

    let mut accumulated: Vec<&str> = Vec::new();
    for part in parts {
        accumulated.push(part);
        let url = join_webdav_url(base_url, &accumulated)?;
        mkcol_one(&client, &url, username, password)?;
    }
    join_webdav_url(base_url, &accumulated)
}

fn mkcol_one(client: &Client, url: &str, username: &str, password: &str) -> Result<(), String> {
    // RFC 4918 §9.3 MKCOL：在父集合下创建新集合（目录）。
    // 已存在时各服务器返回不一：405/409/301/200/204 均视为可继续。
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth_header(username, password)?);
    let method = Method::from_bytes(b"MKCOL").unwrap_or(Method::PUT);
    let response = client
        .request(method, url)
        .headers(headers)
        .send()
        .map_err(map_network_error)?;
    let code = response.status().as_u16();
    if (200..300).contains(&code) || matches!(code, 301 | 302 | 405 | 409 | 423) {
        return Ok(());
    }
    if code == 401 || code == 403 {
        return Err("认证失败，请检查用户名或密码".to_string());
    }
    if code == 507 {
        return Err("远程存储空间不足，无法创建目录".to_string());
    }
    Err(format!("创建远程目录失败 HTTP {}", code))
}

/// Upload a local file to `rel_path` under the WebDAV base (creates parent dirs).
pub fn upload_file(
    base_url: &str,
    username: &str,
    password: &str,
    rel_dir: &str,
    file_name: &str,
    local_path: &Path,
) -> Result<String, String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name == ".." || file_name.is_empty()
    {
        return Err("非法的远程文件名".to_string());
    }
    let client = build_upload_client()?;
    if !rel_dir.trim().is_empty() {
        ensure_remote_dir(base_url, username, password, rel_dir)?;
    }
    let mut segs: Vec<&str> = rel_dir
        .split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    segs.push(file_name);
    let remote_url = join_webdav_url(base_url, &segs)?;

    let mut file = File::open(local_path).map_err(|e| format!("读取本地备份失败: {}", e))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("读取本地备份失败: {}", e))?;

    let content_type = if file_name.ends_with(".zip") {
        "application/zip"
    } else if file_name.ends_with(".abenc") {
        "application/octet-stream"
    } else {
        "application/octet-stream"
    };

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth_header(username, password)?);
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );

    let response = client
        .put(&remote_url)
        .headers(headers)
        .body(buf)
        .send()
        .map_err(map_network_error)?;
    let code = response.status().as_u16();
    if (200..300).contains(&code) {
        Ok(remote_url)
    } else if code == 401 || code == 403 {
        Err("认证失败，请检查用户名或密码".to_string())
    } else if code == 507 {
        Err("远程存储空间不足".to_string())
    } else {
        Err(format!("上传失败 HTTP {}", code))
    }
}

/// Resolve password for a stored connection and upload.
pub fn upload_file_for_connection(
    connection_id: &str,
    rel_dir: &str,
    file_name: &str,
    local_path: &Path,
) -> Result<(String, String), String> {
    let row = db::get_webdav_connection_row(connection_id)?
        .ok_or_else(|| format!("WebDAV 连接不存在: {}", connection_id))?;
    let master = config::load_secrets_key()?;
    let password = crypto::decrypt_secret(
        &master,
        &row.password_salt,
        &row.password_nonce,
        &row.password_cipher,
    )?;
    let remote = upload_file(
        &row.url,
        &row.username,
        &password,
        rel_dir,
        file_name,
        local_path,
    )?;
    Ok((row.name, remote))
}

// ===== Remote directory list / delete / download / prune =====

/// A non-collection entry under a WebDAV directory (Depth: 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavRemoteEntry {
    pub name: String,
    pub href: String,
    pub bytes: u64,
    /// Raw getlastmodified header value when present.
    pub last_modified: String,
    pub is_collection: bool,
}

fn build_transfer_client(timeout: Duration) -> Result<Client, String> {
    let builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("AgentBuddy/0.1 (WebDAV transfer)");
    crate::http_client::apply_proxy(builder)?
        .build()
        .map_err(|e| format!("无法创建 HTTP 客户端: {}", e))
}

fn resolve_connection_auth(connection_id: &str) -> Result<(String, String, String, String), String> {
    // (name, base_url, username, password)
    let row = db::get_webdav_connection_row(connection_id)?
        .ok_or_else(|| format!("WebDAV 连接不存在: {}", connection_id))?;
    let master = config::load_secrets_key()?;
    let password = crypto::decrypt_secret(
        &master,
        &row.password_salt,
        &row.password_nonce,
        &row.password_cipher,
    )?;
    Ok((row.name, row.url, row.username, password))
}

fn rel_dir_segments(rel_dir: &str) -> Result<Vec<&str>, String> {
    let parts: Vec<&str> = rel_dir
        .split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.iter().any(|p| *p == "..") {
        return Err("远程路径不允许包含 ..".to_string());
    }
    Ok(parts)
}

/// List children of `rel_dir` (Depth: 1). Directory itself is excluded when possible.
pub fn list_remote_dir(
    base_url: &str,
    username: &str,
    password: &str,
    rel_dir: &str,
) -> Result<Vec<WebDavRemoteEntry>, String> {
    let client = build_transfer_client(PROBE_TIMEOUT)?;
    let segs = rel_dir_segments(rel_dir)?;
    let dir_url = if segs.is_empty() {
        base_url.trim().trim_end_matches('/').to_string() + "/"
    } else {
        let mut u = join_webdav_url(base_url, &segs)?;
        if !u.ends_with('/') {
            u.push('/');
        }
        u
    };

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth_header(username, password)?);
    headers.insert("Depth", HeaderValue::from_static("1"));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );

    let method = Method::from_bytes(b"PROPFIND").unwrap_or(Method::GET);
    let response = client
        .request(method, &dir_url)
        .headers(headers)
        .body(PROPFIND_LIST_BODY)
        .send()
        .map_err(map_network_error)?;
    let code = response.status().as_u16();
    if code == 401 || code == 403 {
        return Err("认证失败，请检查用户名或密码".to_string());
    }
    if code == 404 {
        return Ok(Vec::new());
    }
    if code != 207 && !(200..300).contains(&code) {
        return Err(format!("列举远程目录失败 HTTP {}", code));
    }
    let body = response
        .text()
        .map_err(|e| format!("读取 PROPFIND 响应失败: {}", e))?;
    Ok(parse_propfind_list(&body, &dir_url))
}

fn parse_propfind_list(xml: &str, dir_url: &str) -> Vec<WebDavRemoteEntry> {
    #[derive(Default)]
    struct ResponseEntry {
        href: String,
        bytes: u64,
        last_modified: String,
        display_name: String,
        is_collection: bool,
    }

    fn assign_text(entry: &mut ResponseEntry, field: &[u8], value: &str) {
        let value = value.trim();
        match field {
            b"href" if entry.href.is_empty() => entry.href = value.to_string(),
            b"getcontentlength" => {
                if let Ok(bytes) = value.parse::<u64>() {
                    entry.bytes = bytes;
                }
            }
            b"getlastmodified" if entry.last_modified.is_empty() => {
                entry.last_modified = value.to_string();
            }
            b"displayname" if entry.display_name.is_empty() => {
                entry.display_name = value.to_string();
            }
            _ => {}
        }
    }

    // WebDAV servers freely choose namespace prefixes (`d:`, `D:`, `lp1:`, `ns1:`...).
    // Parse by XML local name so those wire-format differences do not erase metadata.
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();
    let mut current: Option<ResponseEntry> = None;
    let mut text_field: Option<Vec<u8>> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                let local = tag.local_name();
                match local.as_ref() {
                    b"response" => current = Some(ResponseEntry::default()),
                    b"collection" => {
                        if let Some(entry) = current.as_mut() {
                            entry.is_collection = true;
                        }
                    }
                    b"href" | b"getcontentlength" | b"getlastmodified" | b"displayname"
                        if current.is_some() =>
                    {
                        text_field = Some(local.as_ref().to_vec());
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(tag)) => {
                if tag.local_name().as_ref() == b"collection" {
                    if let Some(entry) = current.as_mut() {
                        entry.is_collection = true;
                    }
                }
            }
            Ok(Event::Text(text)) => {
                if let (Some(entry), Some(field)) = (current.as_mut(), text_field.as_deref()) {
                    if let Ok(decoded) = text.decode() {
                        if let Ok(unescaped) = quick_xml::escape::unescape(&decoded) {
                            assign_text(entry, field, &unescaped);
                        }
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if let (Some(entry), Some(field)) = (current.as_mut(), text_field.as_deref()) {
                    if let Ok(decoded) = text.decode() {
                        assign_text(entry, field, &decoded);
                    }
                }
            }
            Ok(Event::End(tag)) => {
                let local = tag.local_name();
                if local.as_ref() == b"response" {
                    text_field = None;
                    if let Some(entry) = current.take() {
                        let href = entry.href.trim().to_string();
                        let display = entry.display_name.trim();
                        let name = if !display.is_empty() && !display.contains('/') {
                            display.to_string()
                        } else {
                            href_file_name(&href)
                        };
                        if !href.is_empty()
                            && !name.is_empty()
                            && name != "."
                            && name != ".."
                            && !is_self_href(&href, dir_url)
                        {
                            entries.push(WebDavRemoteEntry {
                                name,
                                href,
                                bytes: entry.bytes,
                                last_modified: entry.last_modified,
                                is_collection: entry.is_collection,
                            });
                        }
                    }
                } else if text_field.as_deref() == Some(local.as_ref()) {
                    text_field = None;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    entries
}

fn href_file_name(href: &str) -> String {
    let trimmed = href.trim().trim_end_matches('/');
    // strip query
    let path = trimmed.split('?').next().unwrap_or(trimmed);
    let name = path.rsplit('/').next().unwrap_or(path);
    percent_decode_basic(name)
}

fn percent_decode_basic(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = || {
                let a = (bytes[i + 1] as char).to_digit(16)?;
                let b = (bytes[i + 2] as char).to_digit(16)?;
                Some((a * 16 + b) as u8)
            };
            if let Some(b) = h() {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_self_href(href: &str, dir_url: &str) -> bool {
    let a = href.trim().trim_end_matches('/');
    let b = dir_url.trim().trim_end_matches('/');
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    // Relative href like /remote.php/dav/files/u/AgentBuddy
    if let Some(path) = b.split("://").nth(1).and_then(|s| s.find('/').map(|i| &s[i..])) {
        let path = path.trim_end_matches('/');
        if a.trim_end_matches('/').eq_ignore_ascii_case(path) {
            return true;
        }
    }
    false
}

/// DELETE a remote resource URL (absolute).
pub fn delete_remote_url(
    base_url: &str,
    username: &str,
    password: &str,
    remote_url: &str,
) -> Result<(), String> {
    let _ = base_url; // auth scope only
    let client = build_transfer_client(PROBE_TIMEOUT)?;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth_header(username, password)?);
    let response = client
        .delete(remote_url)
        .headers(headers)
        .send()
        .map_err(map_network_error)?;
    let code = response.status().as_u16();
    if (200..300).contains(&code) || code == 404 {
        // 404 = already gone
        return Ok(());
    }
    if code == 401 || code == 403 {
        return Err("认证失败，请检查用户名或密码".to_string());
    }
    Err(format!("删除远程文件失败 HTTP {}", code))
}

/// Download remote file at `rel_dir/file_name` into `local_path` (overwrite).
pub fn download_file(
    base_url: &str,
    username: &str,
    password: &str,
    rel_dir: &str,
    file_name: &str,
    local_path: &Path,
) -> Result<u64, String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name == ".." || file_name.is_empty()
    {
        return Err("非法的远程文件名".to_string());
    }
    let segs_owned: Vec<String> = {
        let mut v: Vec<String> = rel_dir_segments(rel_dir)?
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        v.push(file_name.to_string());
        v
    };
    let segs_ref: Vec<&str> = segs_owned.iter().map(|s| s.as_str()).collect();
    let remote_url = join_webdav_url(base_url, &segs_ref)?;

    let client = build_transfer_client(DOWNLOAD_TIMEOUT)?;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth_header(username, password)?);
    let mut response = client
        .get(&remote_url)
        .headers(headers)
        .send()
        .map_err(map_network_error)?;
    let code = response.status().as_u16();
    if code == 401 || code == 403 {
        return Err("认证失败，请检查用户名或密码".to_string());
    }
    if code == 404 {
        return Err("远程备份不存在".to_string());
    }
    if !(200..300).contains(&code) {
        return Err(format!("下载失败 HTTP {}", code));
    }

    if let Some(parent) = local_path.parent() {
        fs_create_dir_all(parent)?;
    }
    let mut file = File::create(local_path).map_err(|e| format!("创建本地临时文件失败: {}", e))?;
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| format!("下载读取失败: {}", e))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入本地临时文件失败: {}", e))?;
        total += n as u64;
    }
    Ok(total)
}

fn fs_create_dir_all(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("创建目录失败: {}", e))
}

/// Whether `name` looks like an AgentBuddy backup archive.
pub fn is_agentbuddy_backup_name(name: &str) -> bool {
    let n = name.trim();
    if !n.starts_with("agentbuddy-backup-") {
        return false;
    }
    n.ends_with(".zip") || n.ends_with(".abenc")
}

/// After a successful upload: keep only the newest [`BACKUP_REMOTE_KEEP`] backup archives
/// in `rel_dir`. Deletion failures are collected as warning strings (do not fail the upload).
pub fn prune_old_backups(
    base_url: &str,
    username: &str,
    password: &str,
    rel_dir: &str,
    keep: usize,
) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    let entries = match list_remote_dir(base_url, username, password, rel_dir) {
        Ok(e) => e,
        Err(e) => {
            warnings.push(format!("清理旧备份时列举目录失败: {}", e));
            return Ok(warnings);
        }
    };

    let mut backups: Vec<WebDavRemoteEntry> = entries
        .into_iter()
        .filter(|e| !e.is_collection && is_agentbuddy_backup_name(&e.name))
        .collect();

    // Filename embeds yyyyMMddHHmmss — sort descending (newest first).
    backups.sort_by(|a, b| b.name.cmp(&a.name));

    if backups.len() <= keep {
        return Ok(warnings);
    }

    for old in backups.into_iter().skip(keep) {
        let segs_owned: Vec<String> = {
            let mut v: Vec<String> = rel_dir_segments(rel_dir)?
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            v.push(old.name.clone());
            v
        };
        let segs_ref: Vec<&str> = segs_owned.iter().map(|s| s.as_str()).collect();
        let url = match join_webdav_url(base_url, &segs_ref) {
            Ok(u) => u,
            Err(e) => {
                warnings.push(format!("跳过删除 {}: {}", old.name, e));
                continue;
            }
        };
        // Prefer absolute href from PROPFIND when it looks like a full URL.
        let delete_url = if old.href.starts_with("http://") || old.href.starts_with("https://") {
            old.href.clone()
        } else {
            url
        };
        if let Err(e) = delete_remote_url(base_url, username, password, &delete_url) {
            warnings.push(format!("删除旧备份 {} 失败: {}", old.name, e));
        }
    }
    Ok(warnings)
}

pub fn prune_old_backups_for_connection(
    connection_id: &str,
    rel_dir: &str,
    keep: usize,
) -> Result<Vec<String>, String> {
    let (_name, url, user, pass) = resolve_connection_auth(connection_id)?;
    prune_old_backups(&url, &user, &pass, rel_dir, keep)
}

pub fn list_remote_dir_for_connection(
    connection_id: &str,
    rel_dir: &str,
) -> Result<Vec<WebDavRemoteEntry>, String> {
    let (_name, url, user, pass) = resolve_connection_auth(connection_id)?;
    list_remote_dir(&url, &user, &pass, rel_dir)
}

pub fn download_file_for_connection(
    connection_id: &str,
    rel_dir: &str,
    file_name: &str,
    local_path: &Path,
) -> Result<u64, String> {
    let (_name, url, user, pass) = resolve_connection_auth(connection_id)?;
    download_file(&url, &user, &pass, rel_dir, file_name, local_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_encodes_and_trims() {
        let u = join_webdav_url("https://dav.example.com/remote.php/dav/files/u/", &["AgentBuddy/backups", "2026", "07", "a b.zip"])
            .unwrap();
        assert!(u.starts_with("https://dav.example.com/remote.php/dav/files/u/AgentBuddy/backups/2026/07/"));
        assert!(u.contains("a%20b.zip"));
    }

    #[test]
    fn join_url_rejects_dotdot() {
        assert!(join_webdav_url("https://x/", &["a/../b"]).is_err());
    }

    #[test]
    fn backup_name_filter() {
        assert!(is_agentbuddy_backup_name(
            "agentbuddy-backup-20260721120000.zip"
        ));
        assert!(is_agentbuddy_backup_name(
            "agentbuddy-backup-20260721120000.abenc"
        ));
        assert!(!is_agentbuddy_backup_name("other.zip"));
        assert!(!is_agentbuddy_backup_name("agentbuddy-backup-foo.txt"));
    }

    #[test]
    fn propfind_list_parses_files() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/u/AgentBuddy/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/files/u/AgentBuddy/agentbuddy-backup-20260721120000.zip</d:href>
    <d:propstat><d:prop>
      <d:resourcetype/>
      <d:getcontentlength>1234</d:getcontentlength>
      <d:getlastmodified>Mon, 21 Jul 2026 12:00:00 GMT</d:getlastmodified>
      <d:displayname>agentbuddy-backup-20260721120000.zip</d:displayname>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = parse_propfind_list(
            xml,
            "https://dav.example.com/remote.php/dav/files/u/AgentBuddy/",
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "agentbuddy-backup-20260721120000.zip");
        assert_eq!(entries[0].bytes, 1234);
        assert!(!entries[0].is_collection);
    }

    #[test]
    fn propfind_list_accepts_mixed_namespace_prefixes_and_attributes() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:lp1="DAV:">
  <D:response id="directory">
    <D:href>/dav/AgentBuddy/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection /></D:resourcetype></D:prop></D:propstat>
  </D:response>
  <D:response id="backup">
    <D:href>/dav/AgentBuddy/agentbuddy-backup-20260819102722.abenc</D:href>
    <D:propstat><D:prop>
      <lp1:getcontentlength unit="bytes">25794969</lp1:getcontentlength>
      <lp1:getlastmodified>Wed, 19 Aug 2026 02:27:22 GMT</lp1:getlastmodified>
      <lp1:displayname>agentbuddy-backup-20260819102722.abenc</lp1:displayname>
    </D:prop></D:propstat>
  </D:response>
</D:multistatus>"#;

        let entries = parse_propfind_list(xml, "https://dav.example.com/dav/AgentBuddy/");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "agentbuddy-backup-20260819102722.abenc");
        assert_eq!(entries[0].bytes, 25_794_969);
        assert_eq!(
            entries[0].last_modified,
            "Wed, 19 Aug 2026 02:27:22 GMT"
        );
    }
}
