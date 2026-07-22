//! OpenCode 全局配置：提供商 / 模型可视化读写。
//!
//! - 配置：`~/.config/opencode/opencode.json(c)`（与 MCP 路径解析一致）
//! - 密钥：`~/.local/share/opencode/auth.json`（`{ providerId: { type, key } }`）
//! - 目录：Models.dev `https://models.dev/api.json`（精简后返回，进程内缓存）
//!
//! 列表 DTO **永不**回传明文 API Key；编辑时用 `get_opencode_provider_secret` 按需拉取。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SCHEMA_URL: &str = "https://opencode.ai/config.json";
const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const CATALOG_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/* ===== DTOs ===== */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeConfigView {
    pub config_path: String,
    pub config_exists: bool,
    pub is_jsonc: bool,
    /// Whether OpenCode App/CLI is installed (same rule as agent sniff).
    /// Config dir alone is **not** enough. UI treats this as the highest-priority gate.
    pub opencode_installed: bool,
    pub model: Option<String>,
    pub small_model: Option<String>,
    pub enabled_providers: Option<Vec<String>>,
    pub disabled_providers: Option<Vec<String>>,
    pub providers: Vec<OpencodeProviderView>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeProviderView {
    pub id: String,
    pub name: Option<String>,
    pub npm: Option<String>,
    pub api: Option<String>,
    pub has_api_key: bool,
    /// "auth" | "config" | "both" | "none"
    pub api_key_source: String,
    pub base_url: Option<String>,
    pub set_cache_key: Option<bool>,
    pub timeout: Option<i64>,
    pub chunk_timeout: Option<i64>,
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
    pub models: Vec<OpencodeModelView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeModelView {
    pub id: String,
    pub name: Option<String>,
    pub limit_context: Option<f64>,
    pub limit_input: Option<f64>,
    pub limit_output: Option<f64>,
    pub modalities_input: Vec<String>,
    pub modalities_output: Vec<String>,
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub attachment: Option<bool>,
    pub status: Option<String>,
    pub thinking_type: Option<String>,
    pub thinking_budget_tokens: Option<u64>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub variants: Vec<OpencodeVariantView>,
    /// 未建模的 options 字段（原样透传，序列化为 JSON 对象）。
    pub extra_options: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeVariantView {
    pub id: String,
    pub disabled: Option<bool>,
    pub reasoning_effort: Option<String>,
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogReasoningOption {
    #[serde(rename = "type")]
    pub r#type: String,
    pub values: Option<Vec<String>>,
    pub min: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevCatalog {
    pub fetched_at: u64,
    pub from_cache: bool,
    pub providers: Vec<CatalogProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub env: Vec<String>,
    pub npm: Option<String>,
    pub models: Vec<CatalogModelSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModelSummary {
    pub id: String,
    pub name: String,
    pub limit_context: Option<f64>,
    pub limit_input: Option<f64>,
    pub limit_output: Option<f64>,
    pub modalities_input: Vec<String>,
    pub modalities_output: Vec<String>,
    pub reasoning: bool,
    pub reasoning_options: Vec<CatalogReasoningOption>,
    pub tool_call: bool,
    pub attachment: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeActionResult {
    pub ok: bool,
    pub message: String,
    pub view: Option<OpencodeConfigView>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDefaultsPayload {
    /// None = 不改动；Some("") = 删除键；Some("provider/model") = 写入
    pub model: Option<String>,
    pub small_model: Option<String>,
    pub enabled_providers: Option<Option<Vec<String>>>,
    pub disabled_providers: Option<Option<Vec<String>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertProviderPayload {
    /// 新建时的 id；编辑时也传当前 id
    pub id: String,
    /// 若重命名 provider key，传旧 id（仅当与 id 不同）
    pub previous_id: Option<String>,
    pub name: Option<String>,
    pub npm: Option<String>,
    pub api: Option<String>,
    pub base_url: Option<String>,
    pub set_cache_key: Option<bool>,
    pub timeout: Option<i64>,
    pub chunk_timeout: Option<i64>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
    /// 三态：None=不改动，Some("")=清除，Some(v)=写入 auth.json 并尽量去掉 config 明文
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertModelPayload {
    pub provider_id: String,
    pub id: String,
    pub previous_id: Option<String>,
    pub name: Option<String>,
    pub limit_context: Option<f64>,
    pub limit_input: Option<f64>,
    pub limit_output: Option<f64>,
    pub modalities_input: Option<Vec<String>>,
    pub modalities_output: Option<Vec<String>>,
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub attachment: Option<bool>,
    pub status: Option<String>,
    pub thinking_type: Option<String>,
    pub thinking_budget_tokens: Option<u64>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub variants: Option<Vec<OpencodeVariantView>>,
    /// 合并进 options 的额外字段；传空对象表示不改 extra
    pub extra_options: Option<Map<String, Value>>,
    /// true 时用 extra_options 整体替换 options 中未知字段（删除未列出的）
    pub replace_extra_options: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeModelsResult {
    pub ok: bool,
    pub message: String,
    pub model_ids: Vec<String>,
}

/// OpenCode 同源 fork（JsonMcp 方言）的同步目标状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeForkTarget {
    pub agent: String,
    pub display_name: String,
    pub found: bool,
    pub config_path: String,
    pub config_exists: bool,
    /// in_sync | out_of_sync | missing | no_source | not_installed | error
    pub status: String,
    pub provider_count: u32,
    pub mcp_count: u32,
    pub source_provider_count: u32,
    pub source_mcp_count: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeForkSyncStatus {
    pub source_config_path: String,
    pub source_exists: bool,
    pub source_provider_count: u32,
    pub source_mcp_count: u32,
    pub targets: Vec<OpencodeForkTarget>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeForkSyncItem {
    pub agent: String,
    pub display_name: String,
    pub ok: bool,
    pub status: String,
    pub provider_count: u32,
    pub mcp_count: u32,
    pub auth_keys_synced: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeForkSyncResult {
    pub ok: bool,
    pub message: String,
    pub results: Vec<OpencodeForkSyncItem>,
}

/* ===== Paths ===== */

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "无法解析用户主目录".to_string())
}

fn config_path() -> Result<(PathBuf, bool), String> {
    // Prefer existing resolution used by MCP (jsonc over json).
    let path = crate::mcp_config::resolve_mcp_path("opencode")?;
    let is_jsonc = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("jsonc"))
        .unwrap_or(false);
    Ok((path, is_jsonc))
}

fn default_config_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".config/opencode/opencode.json"))
}

fn auth_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".local/share/opencode/auth.json"))
}

fn display_path(path: &Path) -> String {
    if let Ok(home) = home_dir() {
        let home_s = home.to_string_lossy();
        let abs = path.to_string_lossy();
        if let Some(rest) = abs.strip_prefix(home_s.as_ref()) {
            if rest.is_empty() {
                return "~".into();
            }
            if let Some(s) = rest.strip_prefix('/') {
                return format!("~/{s}");
            }
        }
    }
    path.to_string_lossy().into_owned()
}

/* ===== IO helpers ===== */

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无效路径: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = parent.join(format!(
        ".{}.agentbuddy-{}-{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("tmp"),
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut f =
            fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
        f.write_all(content.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.sync_all().ok();
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let direct = fs::write(path, content);
            let _ = fs::remove_file(&tmp);
            direct.map_err(|e2| format!("写入失败: {e} / {e2}"))
        }
    }
}

fn atomic_write_secret(path: &Path, content: &str) -> Result<(), String> {
    atomic_write(path, content)?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    Ok(())
}

fn read_json_value(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    json5::from_str(&raw).map_err(|e| format!("解析 {} 失败: {e}", path.display()))
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败: {e}"))?;
    let text = format!("{text}\n");
    atomic_write(path, &text)
}

fn ensure_schema(root: &mut Value) {
    let obj = root.as_object_mut().expect("root must be object");
    if !obj.contains_key("$schema") {
        obj.insert("$schema".into(), Value::String(SCHEMA_URL.into()));
    }
}

fn load_or_empty_config() -> Result<(PathBuf, bool, Value, bool), String> {
    let (resolved, is_jsonc) = config_path()?;
    if resolved.exists() {
        let v = read_json_value(&resolved)?;
        let root = match v {
            Value::Object(_) => v,
            _ => return Err("OpenCode 配置根节点必须是对象".into()),
        };
        return Ok((resolved, is_jsonc, root, true));
    }
    // Prefer creating plain json when nothing exists.
    let path = default_config_path()?;
    Ok((path, false, json!({}), false))
}

fn load_auth() -> Result<Map<String, Value>, String> {
    let path = auth_path()?;
    if !path.exists() {
        return Ok(Map::new());
    }
    let v = read_json_value(&path)?;
    Ok(v.as_object().cloned().unwrap_or_default())
}

fn save_auth(map: &Map<String, Value>) -> Result<(), String> {
    let path = auth_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 auth 目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(map.clone()))
        .map_err(|e| format!("序列化 auth 失败: {e}"))?;
    atomic_write_secret(&path, &format!("{text}\n"))
}

fn auth_has_key(auth: &Map<String, Value>, provider_id: &str) -> bool {
    auth.get(provider_id)
        .and_then(|v| v.get("key"))
        .and_then(|k| k.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

fn auth_get_key(auth: &Map<String, Value>, provider_id: &str) -> Option<String> {
    auth.get(provider_id)
        .and_then(|v| v.get("key"))
        .and_then(|k| k.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn config_has_key(provider: &Value) -> bool {
    provider
        .pointer("/options/apiKey")
        .and_then(|k| k.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

fn config_get_key(provider: &Value) -> Option<String> {
    provider
        .pointer("/options/apiKey")
        .and_then(|k| k.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn api_key_source(in_auth: bool, in_config: bool) -> String {
    match (in_auth, in_config) {
        (true, true) => "both".into(),
        (true, false) => "auth".into(),
        (false, true) => "config".into(),
        (false, false) => "none".into(),
    }
}

/* ===== Parse view ===== */

fn str_opt(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn string_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn f64_opt(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64()).or_else(|| {
        v.get(key)
            .and_then(|x| x.as_i64())
            .map(|n| n as f64)
    })
}

fn bool_opt(v: &Value, key: &str) -> Option<bool> {
    v.get(key).and_then(|x| x.as_bool())
}

fn i64_timeout_opt(options: &Value, key: &str) -> Option<i64> {
    match options.get(key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::Bool(false)) => Some(-1), // sentinel: disabled
        _ => None,
    }
}

fn parse_model(id: &str, raw: &Value) -> OpencodeModelView {
    let limit = raw.get("limit").cloned().unwrap_or(Value::Null);
    let modalities = raw.get("modalities").cloned().unwrap_or(Value::Null);
    let options = raw
        .get("options")
        .and_then(|o| o.as_object())
        .cloned()
        .unwrap_or_default();

    let thinking = options.get("thinking").cloned().unwrap_or(Value::Null);
    let thinking_type = thinking
        .get("type")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    let thinking_budget_tokens = thinking
        .get("budgetTokens")
        .and_then(|b| b.as_u64())
        .or_else(|| {
            thinking
                .get("budget_tokens")
                .and_then(|b| b.as_u64())
        });

    let reasoning_effort = options
        .get("reasoningEffort")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let text_verbosity = options
        .get("textVerbosity")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());

    // Typed option keys; everything else (incl. reasoningSummary) stays in extra_options.
    let known = ["thinking", "reasoningEffort", "textVerbosity"];
    let mut extra_options = Map::new();
    for (k, v) in &options {
        if !known.contains(&k.as_str()) {
            extra_options.insert(k.clone(), v.clone());
        }
    }

    let variants = raw
        .get("variants")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(vid, vv)| {
                    let mut extra = Map::new();
                    let disabled = vv.get("disabled").and_then(|d| d.as_bool());
                    let reasoning_effort = vv
                        .get("reasoningEffort")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                    if let Some(map) = vv.as_object() {
                        for (k, val) in map {
                            if k != "disabled" && k != "reasoningEffort" {
                                extra.insert(k.clone(), val.clone());
                            }
                        }
                    }
                    OpencodeVariantView {
                        id: vid.clone(),
                        disabled,
                        reasoning_effort,
                        extra,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    OpencodeModelView {
        id: id.to_string(),
        name: str_opt(raw, "name"),
        limit_context: f64_opt(&limit, "context"),
        limit_input: f64_opt(&limit, "input"),
        limit_output: f64_opt(&limit, "output"),
        modalities_input: string_list(&modalities, "input"),
        modalities_output: string_list(&modalities, "output"),
        reasoning: bool_opt(raw, "reasoning"),
        tool_call: bool_opt(raw, "tool_call").or_else(|| bool_opt(raw, "toolCall")),
        attachment: bool_opt(raw, "attachment"),
        status: str_opt(raw, "status"),
        thinking_type,
        thinking_budget_tokens,
        reasoning_effort,
        text_verbosity,
        variants,
        extra_options,
    }
}

fn parse_provider(id: &str, raw: &Value, auth: &Map<String, Value>) -> OpencodeProviderView {
    let options = raw.get("options").cloned().unwrap_or(Value::Null);
    let in_auth = auth_has_key(auth, id);
    let in_config = config_has_key(raw);

    let models = raw
        .get("models")
        .and_then(|m| m.as_object())
        .map(|obj| {
            let mut list: Vec<_> = obj
                .iter()
                .map(|(mid, mv)| parse_model(mid, mv))
                .collect();
            list.sort_by(|a, b| a.id.cmp(&b.id));
            list
        })
        .unwrap_or_default();

    OpencodeProviderView {
        id: id.to_string(),
        name: str_opt(raw, "name"),
        npm: str_opt(raw, "npm"),
        api: str_opt(raw, "api"),
        has_api_key: in_auth || in_config,
        api_key_source: api_key_source(in_auth, in_config),
        base_url: options
            .get("baseURL")
            .or_else(|| options.get("baseUrl"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        set_cache_key: options.get("setCacheKey").and_then(|x| x.as_bool()),
        timeout: i64_timeout_opt(&options, "timeout"),
        chunk_timeout: options
            .get("chunkTimeout")
            .and_then(|x| x.as_i64())
            .or_else(|| {
                options
                    .get("chunkTimeout")
                    .and_then(|x| x.as_f64())
                    .map(|f| f as i64)
            }),
        whitelist: string_list(raw, "whitelist"),
        blacklist: string_list(raw, "blacklist"),
        models,
    }
}

fn top_string_list(root: &Value, key: &str) -> Option<Vec<String>> {
    root.get(key).and_then(|v| {
        if v.is_null() {
            return None;
        }
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect()
        })
    })
}

/* ===== Public API ===== */

pub fn get_config() -> Result<OpencodeConfigView, String> {
    // Highest priority for the UI: is OpenCode App/CLI actually installed?
    // Config dir alone (e.g. leftover ~/.config/opencode) is not enough —
    // same rule as agent sniff (`found` requires install paths).
    let opencode_installed = crate::sniff::is_agent_installed("opencode");

    let (path, is_jsonc, root, exists) = load_or_empty_config()?;
    let auth = load_auth().unwrap_or_default();
    let mut warnings = Vec::new();
    if exists && is_jsonc {
        warnings.push(
            "当前配置为 JSONC；保存后注释与部分格式会被规范化为标准 JSON。".into(),
        );
    }

    // Not installed → still return path metadata so empty-state copy can mention it,
    // but never surface provider inventory (UI shows not-installed empty state).
    if !opencode_installed {
        return Ok(OpencodeConfigView {
            config_path: display_path(&path),
            config_exists: exists,
            is_jsonc,
            opencode_installed: false,
            model: None,
            small_model: None,
            enabled_providers: None,
            disabled_providers: None,
            providers: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let providers = root
        .get("provider")
        .and_then(|p| p.as_object())
        .map(|obj| {
            let mut list: Vec<_> = obj
                .iter()
                .map(|(id, raw)| parse_provider(id, raw, &auth))
                .collect();
            list.sort_by(|a, b| a.id.cmp(&b.id));
            list
        })
        .unwrap_or_default();

    Ok(OpencodeConfigView {
        config_path: display_path(&path),
        config_exists: exists,
        is_jsonc,
        opencode_installed: true,
        model: root
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        small_model: root
            .get("small_model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        enabled_providers: top_string_list(&root, "enabled_providers"),
        disabled_providers: top_string_list(&root, "disabled_providers"),
        providers,
        warnings,
    })
}

fn apply_optional_string_key(root: &mut Value, key: &str, value: &Option<String>) {
    if let Some(v) = value {
        let obj = root.as_object_mut().unwrap();
        if v.is_empty() {
            obj.remove(key);
        } else {
            obj.insert(key.to_string(), Value::String(v.clone()));
        }
    }
}

fn apply_optional_list_key(root: &mut Value, key: &str, value: &Option<Option<Vec<String>>>) {
    // Some(None) = clear; Some(Some(list)) = set; None = no change
    if let Some(inner) = value {
        let obj = root.as_object_mut().unwrap();
        match inner {
            None => {
                obj.remove(key);
            }
            Some(list) => {
                obj.insert(
                    key.to_string(),
                    Value::Array(list.iter().map(|s| Value::String(s.clone())).collect()),
                );
            }
        }
    }
}

pub fn set_defaults(payload: SetDefaultsPayload) -> Result<OpencodeActionResult, String> {
    let (path, _, mut root, _) = load_or_empty_config()?;
    if !root.is_object() {
        root = json!({});
    }
    ensure_schema(&mut root);
    apply_optional_string_key(&mut root, "model", &payload.model);
    apply_optional_string_key(&mut root, "small_model", &payload.small_model);
    apply_optional_list_key(&mut root, "enabled_providers", &payload.enabled_providers);
    apply_optional_list_key(&mut root, "disabled_providers", &payload.disabled_providers);
    write_json_value(&path, &root)?;
    Ok(OpencodeActionResult {
        ok: true,
        message: "默认模型已更新".into(),
        view: Some(get_config()?),
    })
}

fn provider_object_mut<'a>(root: &'a mut Value, id: &str) -> Result<&'a mut Map<String, Value>, String> {
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| "配置根节点无效".to_string())?;
    let provider_root = root_obj
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let map = provider_root
        .as_object_mut()
        .ok_or_else(|| "provider 节点必须是对象".to_string())?;
    let entry = map
        .entry(id.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    entry
        .as_object_mut()
        .ok_or_else(|| format!("provider.{id} 必须是对象"))
}

fn set_or_remove_str(map: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(v) = value {
        if v.is_empty() {
            map.remove(key);
        } else {
            map.insert(key.to_string(), Value::String(v.clone()));
        }
    }
}

fn ensure_options_mut(provider: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let opts = provider
        .entry("options".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !opts.is_object() {
        *opts = Value::Object(Map::new());
    }
    opts.as_object_mut().unwrap()
}

pub fn upsert_provider(payload: UpsertProviderPayload) -> Result<OpencodeActionResult, String> {
    let id = payload.id.trim().to_string();
    if id.is_empty() {
        return Err("提供商 ID 不能为空".into());
    }
    if id.contains('/') || id.contains(' ') {
        return Err("提供商 ID 不能包含空格或 /".into());
    }

    let (path, _, mut root, _) = load_or_empty_config()?;
    if !root.is_object() {
        root = json!({});
    }
    ensure_schema(&mut root);

    // Rename: move previous_id → id
    if let Some(prev) = payload
        .previous_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && *s != id)
    {
        let root_obj = root.as_object_mut().unwrap();
        let provider_root = root_obj
            .entry("provider".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let map = provider_root.as_object_mut().unwrap();
        if let Some(old) = map.remove(&prev) {
            if map.contains_key(&id) {
                return Err(format!("提供商 `{id}` 已存在，无法重命名"));
            }
            map.insert(id.clone(), old);
        }
        // Move auth entry too
        let mut auth = load_auth()?;
        if let Some(a) = auth.remove(&prev) {
            auth.insert(id.clone(), a);
            save_auth(&auth)?;
        }
    }

    {
        let p = provider_object_mut(&mut root, &id)?;
        set_or_remove_str(p, "name", &payload.name);
        set_or_remove_str(p, "npm", &payload.npm);
        set_or_remove_str(p, "api", &payload.api);

        if payload.base_url.is_some()
            || payload.set_cache_key.is_some()
            || payload.timeout.is_some()
            || payload.chunk_timeout.is_some()
        {
            let opts = ensure_options_mut(p);
            if let Some(ref bu) = payload.base_url {
                if bu.is_empty() {
                    opts.remove("baseURL");
                } else {
                    opts.insert("baseURL".into(), Value::String(bu.clone()));
                }
            }
            if let Some(sck) = payload.set_cache_key {
                opts.insert("setCacheKey".into(), Value::Bool(sck));
            }
            if let Some(t) = payload.timeout {
                if t < 0 {
                    opts.insert("timeout".into(), Value::Bool(false));
                } else if t == 0 {
                    opts.remove("timeout");
                } else {
                    opts.insert("timeout".into(), json!(t));
                }
            }
            if let Some(ct) = payload.chunk_timeout {
                if ct <= 0 {
                    opts.remove("chunkTimeout");
                } else {
                    opts.insert("chunkTimeout".into(), json!(ct));
                }
            }
            if opts.is_empty() {
                p.remove("options");
            }
        }

        if let Some(ref wl) = payload.whitelist {
            if wl.is_empty() {
                p.remove("whitelist");
            } else {
                p.insert(
                    "whitelist".into(),
                    Value::Array(wl.iter().map(|s| Value::String(s.clone())).collect()),
                );
            }
        }
        if let Some(ref bl) = payload.blacklist {
            if bl.is_empty() {
                p.remove("blacklist");
            } else {
                p.insert(
                    "blacklist".into(),
                    Value::Array(bl.iter().map(|s| Value::String(s.clone())).collect()),
                );
            }
        }

        // Clear config-side apiKey when managing via auth (avoid dual storage on write).
        if payload.api_key.is_some() {
            if let Some(opts) = p.get_mut("options").and_then(|o| o.as_object_mut()) {
                opts.remove("apiKey");
                if opts.is_empty() {
                    p.remove("options");
                }
            }
        }
    }

    if let Some(ref key) = payload.api_key {
        let mut auth = load_auth()?;
        if key.is_empty() {
            auth.remove(&id);
        } else {
            auth.insert(
                id.clone(),
                json!({
                    "type": "api",
                    "key": key,
                }),
            );
        }
        save_auth(&auth)?;
    }

    write_json_value(&path, &root)?;
    Ok(OpencodeActionResult {
        ok: true,
        message: format!("提供商 `{id}` 已保存"),
        view: Some(get_config()?),
    })
}

pub fn delete_provider(provider_id: String, delete_auth: bool) -> Result<OpencodeActionResult, String> {
    let id = provider_id.trim().to_string();
    if id.is_empty() {
        return Err("提供商 ID 不能为空".into());
    }
    let (path, _, mut root, exists) = load_or_empty_config()?;
    if !exists {
        return Err("配置文件不存在".into());
    }
    let removed = root
        .as_object_mut()
        .and_then(|o| o.get_mut("provider"))
        .and_then(|p| p.as_object_mut())
        .and_then(|m| m.remove(&id))
        .is_some();
    if !removed {
        return Err(format!("未找到提供商 `{id}`"));
    }
    // Clean empty provider object? keep {}
    write_json_value(&path, &root)?;
    if delete_auth {
        let mut auth = load_auth()?;
        auth.remove(&id);
        save_auth(&auth)?;
    }
    Ok(OpencodeActionResult {
        ok: true,
        message: format!("已删除提供商 `{id}`"),
        view: Some(get_config()?),
    })
}

fn set_limit_field(limit: &mut Map<String, Value>, key: &str, value: Option<f64>) {
    match value {
        None => {}
        Some(v) if v <= 0.0 => {
            limit.remove(key);
        }
        Some(v) => {
            limit.insert(key.to_string(), json!(v));
        }
    }
}

pub fn upsert_model(payload: UpsertModelPayload) -> Result<OpencodeActionResult, String> {
    let pid = payload.provider_id.trim().to_string();
    let mid = payload.id.trim().to_string();
    if pid.is_empty() || mid.is_empty() {
        return Err("提供商 ID 与模型 ID 均不能为空".into());
    }

    let (path, _, mut root, _) = load_or_empty_config()?;
    if !root.is_object() {
        root = json!({});
    }
    ensure_schema(&mut root);

    // Ensure provider exists
    {
        let _ = provider_object_mut(&mut root, &pid)?;
    }

    // Rename model key
    if let Some(prev) = payload
        .previous_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && *s != mid)
    {
        let p = provider_object_mut(&mut root, &pid)?;
        let models = p
            .entry("models".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let map = models
            .as_object_mut()
            .ok_or_else(|| "models 必须是对象".to_string())?;
        if let Some(old) = map.remove(&prev) {
            if map.contains_key(&mid) {
                return Err(format!("模型 `{mid}` 已存在"));
            }
            map.insert(mid.clone(), old);
        }
    }

    {
        let p = provider_object_mut(&mut root, &pid)?;
        let models = p
            .entry("models".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let map = models
            .as_object_mut()
            .ok_or_else(|| "models 必须是对象".to_string())?;
        let model_val = map
            .entry(mid.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        let m = model_val
            .as_object_mut()
            .ok_or_else(|| format!("model `{mid}` 必须是对象"))?;

        if let Some(ref n) = payload.name {
            if n.is_empty() {
                m.remove("name");
            } else {
                m.insert("name".into(), Value::String(n.clone()));
            }
        }

        // limit
        if payload.limit_context.is_some()
            || payload.limit_input.is_some()
            || payload.limit_output.is_some()
        {
            let limit_val = m
                .entry("limit".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !limit_val.is_object() {
                *limit_val = Value::Object(Map::new());
            }
            let limit = limit_val.as_object_mut().unwrap();
            set_limit_field(limit, "context", payload.limit_context);
            set_limit_field(limit, "input", payload.limit_input);
            set_limit_field(limit, "output", payload.limit_output);
            if limit.is_empty() {
                m.remove("limit");
            }
        }

        // modalities
        if payload.modalities_input.is_some() || payload.modalities_output.is_some() {
            let mod_val = m
                .entry("modalities".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !mod_val.is_object() {
                *mod_val = Value::Object(Map::new());
            }
            let mods = mod_val.as_object_mut().unwrap();
            if let Some(ref inputs) = payload.modalities_input {
                if inputs.is_empty() {
                    mods.remove("input");
                } else {
                    mods.insert(
                        "input".into(),
                        Value::Array(inputs.iter().map(|s| Value::String(s.clone())).collect()),
                    );
                }
            }
            if let Some(ref outputs) = payload.modalities_output {
                if outputs.is_empty() {
                    mods.remove("output");
                } else {
                    mods.insert(
                        "output".into(),
                        Value::Array(outputs.iter().map(|s| Value::String(s.clone())).collect()),
                    );
                }
            }
            if mods.is_empty() {
                m.remove("modalities");
            }
        }

        if let Some(r) = payload.reasoning {
            m.insert("reasoning".into(), Value::Bool(r));
        }
        if let Some(t) = payload.tool_call {
            m.insert("tool_call".into(), Value::Bool(t));
        }
        if let Some(a) = payload.attachment {
            m.insert("attachment".into(), Value::Bool(a));
        }
        if let Some(ref st) = payload.status {
            if st.is_empty() {
                m.remove("status");
            } else {
                m.insert("status".into(), Value::String(st.clone()));
            }
        }

        // options: thinking / reasoningEffort / textVerbosity / extra
        let need_options = payload.thinking_type.is_some()
            || payload.thinking_budget_tokens.is_some()
            || payload.reasoning_effort.is_some()
            || payload.text_verbosity.is_some()
            || payload.extra_options.is_some();

        if need_options {
            let opts_val = m
                .entry("options".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !opts_val.is_object() {
                *opts_val = Value::Object(Map::new());
            }
            let opts = opts_val.as_object_mut().unwrap();

            if payload.thinking_type.is_some() || payload.thinking_budget_tokens.is_some() {
                let thinking_type = payload.thinking_type.clone().unwrap_or_default();
                if thinking_type.is_empty() || thinking_type == "disabled" {
                    // remove thinking if disabled and no budget intent
                    if thinking_type == "disabled" || thinking_type.is_empty() {
                        if payload.thinking_type.as_deref() == Some("")
                            || payload.thinking_type.as_deref() == Some("disabled")
                        {
                            opts.remove("thinking");
                        }
                    }
                }
                if let Some(ref tt) = payload.thinking_type {
                    if tt.is_empty() {
                        opts.remove("thinking");
                    } else if tt != "disabled" {
                        let mut thinking = opts
                            .get("thinking")
                            .and_then(|t| t.as_object())
                            .cloned()
                            .unwrap_or_default();
                        thinking.insert("type".into(), Value::String(tt.clone()));
                        if let Some(b) = payload.thinking_budget_tokens {
                            if b == 0 {
                                thinking.remove("budgetTokens");
                            } else {
                                thinking.insert("budgetTokens".into(), json!(b));
                            }
                        }
                        opts.insert("thinking".into(), Value::Object(thinking));
                    } else {
                        opts.remove("thinking");
                    }
                } else if let Some(b) = payload.thinking_budget_tokens {
                    let mut thinking = opts
                        .get("thinking")
                        .and_then(|t| t.as_object())
                        .cloned()
                        .unwrap_or_default();
                    if !thinking.contains_key("type") {
                        thinking.insert("type".into(), Value::String("enabled".into()));
                    }
                    if b == 0 {
                        thinking.remove("budgetTokens");
                    } else {
                        thinking.insert("budgetTokens".into(), json!(b));
                    }
                    opts.insert("thinking".into(), Value::Object(thinking));
                }
            }

            if let Some(ref re) = payload.reasoning_effort {
                if re.is_empty() {
                    opts.remove("reasoningEffort");
                } else {
                    opts.insert("reasoningEffort".into(), Value::String(re.clone()));
                }
            }
            if let Some(ref tv) = payload.text_verbosity {
                if tv.is_empty() {
                    opts.remove("textVerbosity");
                } else {
                    opts.insert("textVerbosity".into(), Value::String(tv.clone()));
                }
            }

            if let Some(ref extra) = payload.extra_options {
                let replace = payload.replace_extra_options.unwrap_or(false);
                if replace {
                    // Keep only known typed option keys, then merge extra
                    let keep_keys = ["thinking", "reasoningEffort", "textVerbosity"];
                    let retained: Map<String, Value> = opts
                        .iter()
                        .filter(|(k, _)| keep_keys.contains(&k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    *opts = retained;
                }
                for (k, v) in extra {
                    opts.insert(k.clone(), v.clone());
                }
            }

            if opts.is_empty() {
                m.remove("options");
            }
        }

        // variants
        if let Some(ref variants) = payload.variants {
            if variants.is_empty() {
                m.remove("variants");
            } else {
                let mut vmap = Map::new();
                for v in variants {
                    let mut obj = Map::new();
                    if let Some(d) = v.disabled {
                        obj.insert("disabled".into(), Value::Bool(d));
                    }
                    if let Some(ref re) = v.reasoning_effort {
                        if !re.is_empty() {
                            obj.insert("reasoningEffort".into(), Value::String(re.clone()));
                        }
                    }
                    for (k, val) in &v.extra {
                        obj.insert(k.clone(), val.clone());
                    }
                    vmap.insert(v.id.clone(), Value::Object(obj));
                }
                m.insert("variants".into(), Value::Object(vmap));
            }
        }
    }

    write_json_value(&path, &root)?;
    Ok(OpencodeActionResult {
        ok: true,
        message: format!("模型 `{pid}/{mid}` 已保存"),
        view: Some(get_config()?),
    })
}

pub fn delete_model(provider_id: String, model_id: String) -> Result<OpencodeActionResult, String> {
    let pid = provider_id.trim().to_string();
    let mid = model_id.trim().to_string();
    let (path, _, mut root, exists) = load_or_empty_config()?;
    if !exists {
        return Err("配置文件不存在".into());
    }
    let removed = root
        .as_object_mut()
        .and_then(|o| o.get_mut("provider"))
        .and_then(|p| p.as_object_mut())
        .and_then(|pm| pm.get_mut(&pid))
        .and_then(|p| p.as_object_mut())
        .and_then(|p| p.get_mut("models"))
        .and_then(|m| m.as_object_mut())
        .and_then(|m| m.remove(&mid))
        .is_some();
    if !removed {
        return Err(format!("未找到模型 `{pid}/{mid}`"));
    }
    write_json_value(&path, &root)?;
    Ok(OpencodeActionResult {
        ok: true,
        message: format!("已删除模型 `{pid}/{mid}`"),
        view: Some(get_config()?),
    })
}

pub fn get_provider_secret(provider_id: String) -> Result<String, String> {
    let id = provider_id.trim().to_string();
    let auth = load_auth()?;
    if let Some(k) = auth_get_key(&auth, &id) {
        return Ok(k);
    }
    // fallback config options.apiKey
    let (_, _, root, _) = load_or_empty_config()?;
    if let Some(p) = root
        .get("provider")
        .and_then(|p| p.get(&id))
    {
        if let Some(k) = config_get_key(p) {
            return Ok(k);
        }
    }
    Ok(String::new())
}

pub fn set_provider_secret(provider_id: String, api_key: String) -> Result<OpencodeActionResult, String> {
    upsert_provider(UpsertProviderPayload {
        id: provider_id,
        previous_id: None,
        name: None,
        npm: None,
        api: None,
        base_url: None,
        set_cache_key: None,
        timeout: None,
        chunk_timeout: None,
        whitelist: None,
        blacklist: None,
        api_key: Some(api_key),
    })
}

pub fn reveal_config() -> Result<OpencodeActionResult, String> {
    let (path, _, _, exists) = load_or_empty_config()?;
    if !exists {
        // Reveal parent dir if possible
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
            let status = Command::new("open").arg(parent).status();
            match status {
                Ok(s) if s.success() => {
                    return Ok(OpencodeActionResult {
                        ok: true,
                        message: format!("已打开目录 {}", display_path(parent)),
                        view: None,
                    });
                }
                _ => return Err("打开目录失败".into()),
            }
        }
        return Err("配置文件尚不存在".into());
    }
    let status = Command::new("open").args(["-R", &path.to_string_lossy()]).status();
    match status {
        Ok(s) if s.success() => Ok(OpencodeActionResult {
            ok: true,
            message: format!("已在 Finder 中显示 {}", display_path(&path)),
            view: None,
        }),
        Ok(_) | Err(_) => Err("打开 Finder 失败".into()),
    }
}

/* ===== Sync provider + mcp to OpenCode forks (e.g. DevEco Code) ===== */

/// 与 OpenCode 共享 `json.mcp` 配置形态的二开 agent（不含 opencode 自身）。
fn opencode_fork_agent_names() -> Vec<&'static str> {
    crate::agents::agents()
        .iter()
        .filter(|s| {
            s.name != "opencode" && matches!(s.mcp.dialect, crate::agents::McpDialect::JsonMcp)
        })
        .map(|s| s.name)
        .collect()
}

fn object_key_count(v: Option<&Value>) -> u32 {
    v.and_then(|x| x.as_object())
        .map(|m| m.len() as u32)
        .unwrap_or(0)
}

fn clone_object_value(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(|x| x.as_object())
        .cloned()
        .unwrap_or_default()
}

/// 稳定序列化辅助：对 Object 键排序后再序列化，避免 Map 顺序差异。
fn sort_value(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                if let Some(child) = map.get(&k) {
                    out.insert(k, sort_value(child));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_value).collect()),
        other => other.clone(),
    }
}

/// 归一化比较：去掉 $schema 与空白差异，仅比 provider 内容。
fn provider_fingerprint(root: &Value) -> String {
    let provider = root
        .get("provider")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let payload = json!({ "provider": sort_value(&provider) });
    serde_json::to_string(&payload).unwrap_or_default()
}

/// 归一化比较：去掉 $schema 与空白差异，仅比 mcp 内容。
fn mcp_fingerprint(root: &Value) -> String {
    let mcp = root
        .get("mcp")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let payload = json!({ "mcp": sort_value(&mcp) });
    serde_json::to_string(&payload).unwrap_or_default()
}

fn agent_found(agent: &str) -> bool {
    // Prefer live sniff so UI reflects current install; fall back to cache.
    // 若配置文件或配置目录已存在，也视为可同步（兼容「有配置但 sniff 未命中」）。
    if let Ok(path) = crate::mcp_config::resolve_mcp_path(agent) {
        if path.is_file() {
            return true;
        }
        if let Some(parent) = path.parent() {
            if parent.is_dir() {
                return true;
            }
        }
    }
    if let Ok(cached) = crate::db::load_agents() {
        if let Some(row) = cached.iter().find(|a| a.name == agent) {
            return row.found;
        }
    }
    crate::sniff::sniff_agents()
        .into_iter()
        .find(|a| a.name == agent)
        .map(|a| a.found)
        .unwrap_or(false)
}

fn fork_auth_path(agent: &str) -> Result<PathBuf, String> {
    // DevEco / OpenCode 同源：auth 在 ~/.local/share/<name>/auth.json
    // name 取 config 根目录最后一段（deveco / opencode）。
    let cfg = crate::mcp_config::resolve_mcp_path(agent)?;
    let share_name = cfg
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("无法推断 {agent} 的 data 目录"))?;
    Ok(home_dir()?.join(".local/share").join(share_name).join("auth.json"))
}

fn load_auth_at(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let v = read_json_value(path)?;
    match v {
        Value::Object(m) => Ok(m),
        _ => Err(format!("auth 文件根节点必须是对象: {}", path.display())),
    }
}

fn save_auth_at(path: &Path, map: &Map<String, Value>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 auth 目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(map).map_err(|e| format!("序列化 auth 失败: {e}"))?;
    atomic_write_secret(path, &format!("{text}\n"))
}

/// 将 OpenCode 源里出现的 provider id 对应的 auth 条目 merge 到目标（不删除目标其它密钥）。
fn merge_auth_for_providers(
    src_auth: &Map<String, Value>,
    dst_path: &Path,
    provider_ids: &[String],
) -> Result<u32, String> {
    if provider_ids.is_empty() {
        return Ok(0);
    }
    let mut dst = load_auth_at(dst_path)?;
    let mut n = 0u32;
    for id in provider_ids {
        if let Some(entry) = src_auth.get(id) {
            dst.insert(id.clone(), entry.clone());
            n += 1;
        }
    }
    if n > 0 {
        save_auth_at(dst_path, &dst)?;
    }
    Ok(n)
}

fn build_fork_target(
    agent: &str,
    source_root: &Value,
    source_exists: bool,
    source_provider_count: u32,
    source_mcp_count: u32,
) -> OpencodeForkTarget {
    let display_name = crate::agents::find(agent)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| agent.to_string());
    let found = agent_found(agent);
    let config_path = crate::mcp_config::resolve_mcp_path(agent)
        .map(|p| display_path(&p))
        .unwrap_or_else(|_| "—".into());
    let path = crate::mcp_config::resolve_mcp_path(agent).ok();
    let config_exists = path.as_ref().map(|p| p.is_file()).unwrap_or(false);

    if !found {
        return OpencodeForkTarget {
            agent: agent.into(),
            display_name,
            found: false,
            config_path,
            config_exists: false,
            status: "not_installed".into(),
            provider_count: 0,
            mcp_count: 0,
            source_provider_count,
            source_mcp_count,
            message: "本机未检测到安装".into(),
        };
    }
    if !source_exists {
        return OpencodeForkTarget {
            agent: agent.into(),
            display_name,
            found: true,
            config_path,
            config_exists,
            status: "no_source".into(),
            provider_count: 0,
            mcp_count: 0,
            source_provider_count,
            source_mcp_count,
            message: "OpenCode 源配置不存在".into(),
        };
    }

    let (provider_count, mcp_count, status, message) = match path.as_ref() {
        Some(p) if p.is_file() => match read_json_value(p) {
            Ok(root) => {
                let pc = object_key_count(root.get("provider"));
                let mc = object_key_count(root.get("mcp"));
                let provider_same =
                    provider_fingerprint(&root) == provider_fingerprint(source_root);
                let mcp_same = mcp_fingerprint(&root) == mcp_fingerprint(source_root);
                if provider_same && mcp_same {
                    (
                        pc,
                        mc,
                        "in_sync".into(),
                        "provider / mcp 已与 OpenCode 对齐".into(),
                    )
                } else if provider_same && !mcp_same {
                    (
                        pc,
                        mc,
                        "out_of_sync".into(),
                        "provider 已对齐，mcp 与 OpenCode 不一致（可勾选同步 MCP）".into(),
                    )
                } else if !provider_same && mcp_same {
                    (
                        pc,
                        mc,
                        "out_of_sync".into(),
                        "mcp 已对齐，provider 与 OpenCode 不一致".into(),
                    )
                } else {
                    (
                        pc,
                        mc,
                        "out_of_sync".into(),
                        "provider 与 mcp 均与 OpenCode 不一致".into(),
                    )
                }
            }
            Err(e) => (0, 0, "error".into(), e),
        },
        Some(_) => (
            0,
            0,
            "missing".into(),
            "配置文件尚未创建".into(),
        ),
        None => (0, 0, "error".into(), "无法解析配置路径".into()),
    };

    OpencodeForkTarget {
        agent: agent.into(),
        display_name,
        found: true,
        config_path,
        config_exists,
        status,
        provider_count,
        mcp_count,
        source_provider_count,
        source_mcp_count,
        message,
    }
}

/// 状态查询：OpenCode 源 + 各 JsonMcp fork（当前主要是 DevEco Code）。
pub fn get_fork_sync_status() -> Result<OpencodeForkSyncStatus, String> {
    let (src_path, _, src_root, source_exists) = load_or_empty_config()?;
    let source_provider_count = object_key_count(src_root.get("provider"));
    let source_mcp_count = object_key_count(src_root.get("mcp"));
    let targets = opencode_fork_agent_names()
        .into_iter()
        .map(|name| {
            build_fork_target(
                name,
                &src_root,
                source_exists,
                source_provider_count,
                source_mcp_count,
            )
        })
        .collect();
    Ok(OpencodeForkSyncStatus {
        source_config_path: display_path(&src_path),
        source_exists,
        source_provider_count,
        source_mcp_count,
        targets,
    })
}

/// 将 OpenCode 的 provider（以及可选的 mcp）同步到指定 fork agent。
///
/// - `sync_mcp == true`：覆盖写入目标的 `mcp` 字段
/// - `sync_mcp == false`：仅同步 `provider`（及对应 auth），保留目标现有 `mcp`
fn sync_provider_mcp_to_agent(
    agent: &str,
    sync_mcp: bool,
) -> Result<OpencodeForkSyncItem, String> {
    let display_name = crate::agents::find(agent)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| agent.to_string());

    if !agent_found(agent) {
        return Ok(OpencodeForkSyncItem {
            agent: agent.into(),
            display_name,
            ok: false,
            status: "not_installed".into(),
            provider_count: 0,
            mcp_count: 0,
            auth_keys_synced: 0,
            message: "本机未检测到安装，已跳过".into(),
        });
    }

    let (_, _, src_root, source_exists) = load_or_empty_config()?;
    if !source_exists {
        return Ok(OpencodeForkSyncItem {
            agent: agent.into(),
            display_name,
            ok: false,
            status: "no_source".into(),
            provider_count: 0,
            mcp_count: 0,
            auth_keys_synced: 0,
            message: "OpenCode 源配置不存在".into(),
        });
    }

    let dst_path = crate::mcp_config::resolve_mcp_path(agent)?;
    // 保护：绝不写回 OpenCode 自身路径
    let (src_path, _) = config_path()?;
    if let (Ok(a), Ok(b)) = (src_path.canonicalize(), dst_path.canonicalize()) {
        if a == b {
            return Ok(OpencodeForkSyncItem {
                agent: agent.into(),
                display_name,
                ok: true,
                status: "in_sync".into(),
                provider_count: object_key_count(src_root.get("provider")),
                mcp_count: object_key_count(src_root.get("mcp")),
                auth_keys_synced: 0,
                message: "目标与源为同一文件，跳过".into(),
            });
        }
    } else if src_path == dst_path {
        return Ok(OpencodeForkSyncItem {
            agent: agent.into(),
            display_name,
            ok: true,
            status: "in_sync".into(),
            provider_count: object_key_count(src_root.get("provider")),
            mcp_count: object_key_count(src_root.get("mcp")),
            auth_keys_synced: 0,
            message: "目标与源为同一文件，跳过".into(),
        });
    }

    let provider = clone_object_value(src_root.get("provider"));
    let provider_count = provider.len() as u32;
    let provider_ids: Vec<String> = provider.keys().cloned().collect();

    // 目标文件：保留其它顶层键；始终替换 provider；仅在 sync_mcp 时替换 mcp。
    // 缺省补 $schema。
    let mut dst_root = if dst_path.is_file() {
        let v = read_json_value(&dst_path)?;
        match v {
            Value::Object(_) => v,
            _ => return Err(format!("{} 根节点必须是对象", display_path(&dst_path))),
        }
    } else {
        json!({})
    };

    let mcp_count = {
        let obj = dst_root
            .as_object_mut()
            .ok_or_else(|| "目标配置根节点必须是对象".to_string())?;
        if !obj.contains_key("$schema") {
            obj.insert("$schema".into(), Value::String(SCHEMA_URL.into()));
        }
        obj.insert("provider".into(), Value::Object(provider));
        if sync_mcp {
            let mcp = clone_object_value(src_root.get("mcp"));
            let n = mcp.len() as u32;
            obj.insert("mcp".into(), Value::Object(mcp));
            n
        } else {
            // 保留目标现有 mcp，计数用目标侧
            object_key_count(obj.get("mcp"))
        }
    };
    write_json_value(&dst_path, &dst_root)?;

    // auth：按源 provider id 合并密钥条目到 fork 的 auth.json（不删除其它条目）
    let src_auth = load_auth().unwrap_or_default();
    let auth_keys_synced = match fork_auth_path(agent) {
        Ok(auth_path) => merge_auth_for_providers(&src_auth, &auth_path, &provider_ids)
            .unwrap_or(0),
        Err(_) => 0,
    };

    let mcp_part = if sync_mcp {
        format!("、mcp {} 个", mcp_count)
    } else {
        format!("（未同步 mcp，目标现有 {} 个）", mcp_count)
    };
    Ok(OpencodeForkSyncItem {
        agent: agent.into(),
        display_name,
        ok: true,
        status: "in_sync".into(),
        provider_count,
        mcp_count,
        auth_keys_synced,
        message: format!(
            "已同步 provider {} 个{}{}",
            provider_count,
            mcp_part,
            if auth_keys_synced > 0 {
                format!("，密钥条目 {} 个", auth_keys_synced)
            } else {
                String::new()
            }
        ),
    })
}

/// 同步到指定 fork agent（例如 `deveco-code`）。
///
/// `sync_mcp`：是否同时覆盖目标 `mcp`；为 false 时仅同步 provider + auth。
pub fn sync_to_fork_agent(
    agent: String,
    sync_mcp: bool,
) -> Result<OpencodeForkSyncResult, String> {
    let name = agent.trim().to_string();
    if name.is_empty() {
        return Err("agent 不能为空".into());
    }
    if name == "opencode" {
        return Err("不能同步到 OpenCode 自身".into());
    }
    let allowed = opencode_fork_agent_names();
    if !allowed.contains(&name.as_str()) {
        return Err(format!(
            "「{name}」不是 OpenCode 同源 fork（需 JsonMcp 方言）"
        ));
    }
    let item = sync_provider_mcp_to_agent(&name, sync_mcp)?;
    let ok = item.ok;
    let message = item.message.clone();
    Ok(OpencodeForkSyncResult {
        ok,
        message,
        results: vec![item],
    })
}

/// 同步到全部已登记的 OpenCode fork（当前主要是 DevEco Code）。
///
/// `sync_mcp`：是否同时覆盖各目标的 `mcp`；为 false 时仅同步 provider + auth。
pub fn sync_to_all_forks(sync_mcp: bool) -> Result<OpencodeForkSyncResult, String> {
    let names = opencode_fork_agent_names();
    if names.is_empty() {
        return Ok(OpencodeForkSyncResult {
            ok: true,
            message: "没有可同步的 OpenCode 二开 agent".into(),
            results: vec![],
        });
    }
    let mut results = Vec::new();
    let mut ok_n = 0u32;
    let mut fail_n = 0u32;
    let mut skip_n = 0u32;
    for name in names {
        match sync_provider_mcp_to_agent(name, sync_mcp) {
            Ok(item) => {
                if item.ok {
                    if item.status == "not_installed" || item.status == "no_source" {
                        skip_n += 1;
                    } else {
                        ok_n += 1;
                    }
                } else if item.status == "not_installed" || item.status == "no_source" {
                    skip_n += 1;
                } else {
                    fail_n += 1;
                }
                results.push(item);
            }
            Err(e) => {
                fail_n += 1;
                results.push(OpencodeForkSyncItem {
                    agent: name.into(),
                    display_name: crate::agents::find(name)
                        .map(|s| s.display_name.to_string())
                        .unwrap_or_else(|| name.to_string()),
                    ok: false,
                    status: "error".into(),
                    provider_count: 0,
                    mcp_count: 0,
                    auth_keys_synced: 0,
                    message: e,
                });
            }
        }
    }
    let ok = fail_n == 0;
    let mcp_hint = if sync_mcp { "（含 mcp）" } else { "（仅 provider）" };
    let message = if ok {
        format!(
            "已同步 {} 个目标{}{}",
            ok_n,
            mcp_hint,
            if skip_n > 0 {
                format!("，跳过 {} 个", skip_n)
            } else {
                String::new()
            }
        )
    } else {
        format!(
            "部分失败：成功 {}，失败 {}，跳过 {}{}",
            ok_n, fail_n, skip_n, mcp_hint
        )
    };
    Ok(OpencodeForkSyncResult {
        ok,
        message,
        results,
    })
}

/* ===== Models.dev catalog ===== */

struct CatalogCache {
    at: Instant,
    catalog: ModelsDevCatalog,
}

static CATALOG_CACHE: Mutex<Option<CatalogCache>> = Mutex::new(None);

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_catalog_json(raw: &Value) -> Result<ModelsDevCatalog, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| "models.dev 响应必须是对象".to_string())?;
    let mut providers = Vec::new();
    for (pid, pv) in obj {
        let name = pv
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(pid)
            .to_string();
        let env = pv
            .get("env")
            .and_then(|e| e.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let npm = pv
            .get("npm")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        let mut models = Vec::new();
        if let Some(mobj) = pv.get("models").and_then(|m| m.as_object()) {
            for (mid, mv) in mobj {
                let limit = mv.get("limit").cloned().unwrap_or(Value::Null);
                let modalities = mv.get("modalities").cloned().unwrap_or(Value::Null);
                let reasoning_options = mv
                    .get("reasoning_options")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let t = item.get("type")?.as_str()?.to_string();
                                let values = item.get("values").and_then(|v| {
                                    v.as_array().map(|a| {
                                        a.iter()
                                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                            .collect()
                                    })
                                });
                                let min = item.get("min").and_then(|m| m.as_u64());
                                Some(CatalogReasoningOption {
                                    r#type: t,
                                    values,
                                    min,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                models.push(CatalogModelSummary {
                    id: mid.clone(),
                    name: mv
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or(mid)
                        .to_string(),
                    limit_context: f64_opt(&limit, "context"),
                    limit_input: f64_opt(&limit, "input"),
                    limit_output: f64_opt(&limit, "output"),
                    modalities_input: string_list(&modalities, "input"),
                    modalities_output: string_list(&modalities, "output"),
                    reasoning: bool_opt(mv, "reasoning").unwrap_or(false),
                    reasoning_options,
                    tool_call: bool_opt(mv, "tool_call").unwrap_or(false),
                    attachment: bool_opt(mv, "attachment").unwrap_or(false),
                    status: str_opt(mv, "status"),
                });
            }
            models.sort_by(|a, b| a.id.cmp(&b.id));
        }
        providers.push(CatalogProvider {
            id: pid.clone(),
            name,
            env,
            npm,
            models,
        });
    }
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ModelsDevCatalog {
        fetched_at: now_unix(),
        from_cache: false,
        providers,
    })
}

fn build_http_client() -> Result<reqwest::blocking::Client, String> {
    let builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("AgentBuddy/0.1 (opencode-config)");
    crate::http_client::apply_proxy(builder)?
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))
}

fn catalog_cache_file() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".agentbuddy/cache/models-dev.json"))
}

fn load_file_cache() -> Option<ModelsDevCatalog> {
    let path = catalog_cache_file().ok()?;
    let meta = fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    if modified.elapsed().ok()? > CATALOG_TTL {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    // Stored as already-slim catalog
    let mut cat: ModelsDevCatalog = serde_json::from_value(v).ok()?;
    cat.from_cache = true;
    Some(cat)
}

fn save_file_cache(cat: &ModelsDevCatalog) {
    if let Ok(path) = catalog_cache_file() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(cat) {
            let _ = atomic_write(&path, &text);
        }
    }
}

pub fn fetch_models_dev_catalog(force: bool) -> Result<ModelsDevCatalog, String> {
    if !force {
        if let Ok(guard) = CATALOG_CACHE.lock() {
            if let Some(c) = guard.as_ref() {
                if c.at.elapsed() < CATALOG_TTL {
                    let mut cat = c.catalog.clone();
                    cat.from_cache = true;
                    return Ok(cat);
                }
            }
        }
        if let Some(cat) = load_file_cache() {
            if let Ok(mut guard) = CATALOG_CACHE.lock() {
                *guard = Some(CatalogCache {
                    at: Instant::now(),
                    catalog: cat.clone(),
                });
            }
            return Ok(cat);
        }
    }

    let client = build_http_client()?;
    let resp = client
        .get(MODELS_DEV_URL)
        .send()
        .map_err(|e| format!("请求 models.dev 失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("models.dev 返回 HTTP {}", resp.status()));
    }
    let raw: Value = resp
        .json()
        .map_err(|e| format!("解析 models.dev JSON 失败: {e}"))?;
    let mut catalog = parse_catalog_json(&raw)?;
    catalog.from_cache = false;
    save_file_cache(&catalog);
    if let Ok(mut guard) = CATALOG_CACHE.lock() {
        *guard = Some(CatalogCache {
            at: Instant::now(),
            catalog: catalog.clone(),
        });
    }
    Ok(catalog)
}

pub fn probe_models_endpoint(base_url: String) -> Result<ProbeModelsResult, String> {
    let base = base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("baseURL 不能为空".into());
    }
    let url = if base.ends_with("/models") {
        base
    } else if base.ends_with("/v1") {
        format!("{base}/models")
    } else {
        format!("{base}/models")
    };
    let client = build_http_client()?;
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("探测失败: {e}"))?;
    if !resp.status().is_success() {
        return Ok(ProbeModelsResult {
            ok: false,
            message: format!("HTTP {}", resp.status()),
            model_ids: vec![],
        });
    }
    let v: Value = resp
        .json()
        .map_err(|e| format!("解析响应失败: {e}"))?;
    let mut ids = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                ids.push(id.to_string());
            }
        }
    } else if let Some(arr) = v.as_array() {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                ids.push(id.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ProbeModelsResult {
        ok: true,
        message: format!("发现 {} 个模型", ids.len()),
        model_ids: ids,
    })
}

/* ===== Tests ===== */

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that touch HOME via env — avoid races.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TempHome {
        path: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempHome {
        fn new() -> Self {
            let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let path = std::env::temp_dir().join(format!(
                "agentbuddy-oc-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(path.join(".config/opencode")).unwrap();
            fs::create_dir_all(path.join(".local/share/opencode")).unwrap();
            // dirs::home_dir does not honor HOME on all platforms the same way —
            // we set HOME for path helpers that use dirs::home_dir.
            std::env::set_var("HOME", &path);
            Self {
                path,
                _guard: guard,
            }
        }

        fn config_file(&self) -> PathBuf {
            self.path.join(".config/opencode/opencode.json")
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn empty_config_view() {
        let _h = TempHome::new();
        let view = get_config().unwrap();
        assert!(!view.config_exists);
        assert!(view.providers.is_empty());
        assert!(view.model.is_none());
    }

    #[test]
    fn merge_preserves_mcp_and_model_fields() {
        let h = TempHome::new();
        let initial = json!({
            "$schema": SCHEMA_URL,
            "mcp": { "demo": { "type": "local", "command": ["echo"] } },
            "provider": {
                "local": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "Local",
                    "options": { "baseURL": "http://127.0.0.1:8080/v1" },
                    "models": {
                        "qwen": {
                            "name": "Qwen",
                            "limit": { "context": 128000, "output": 65536 },
                            "modalities": { "input": ["text"], "output": ["text"] }
                        }
                    }
                }
            }
        });
        fs::write(h.config_file(), serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        upsert_model(UpsertModelPayload {
            provider_id: "local".into(),
            id: "qwen".into(),
            previous_id: None,
            name: Some("Qwen Coder".into()),
            limit_context: Some(200000.0),
            limit_input: None,
            limit_output: Some(80000.0),
            modalities_input: Some(vec!["text".into(), "image".into()]),
            modalities_output: Some(vec!["text".into()]),
            reasoning: Some(true),
            tool_call: None,
            attachment: None,
            status: None,
            thinking_type: None,
            thinking_budget_tokens: None,
            reasoning_effort: Some("high".into()),
            text_verbosity: None,
            variants: None,
            extra_options: None,
            replace_extra_options: None,
        })
        .unwrap();

        let raw = fs::read_to_string(h.config_file()).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("mcp").is_some(), "mcp must be preserved");
        assert_eq!(
            v.pointer("/provider/local/models/qwen/limit/context")
                .and_then(|x| x.as_f64()),
            Some(200000.0)
        );
        assert_eq!(
            v.pointer("/provider/local/models/qwen/options/reasoningEffort")
                .and_then(|x| x.as_str()),
            Some("high")
        );
        let mods = v
            .pointer("/provider/local/models/qwen/modalities/input")
            .and_then(|x| x.as_array())
            .unwrap();
        assert_eq!(mods.len(), 2);

        let view = get_config().unwrap();
        assert_eq!(view.providers.len(), 1);
        let m = &view.providers[0].models[0];
        assert_eq!(m.limit_context, Some(200000.0));
        assert!(m.modalities_input.contains(&"image".into()));
        assert_eq!(m.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn auth_secret_roundtrip_not_in_view() {
        let h = TempHome::new();
        fs::write(
            h.config_file(),
            r#"{"provider":{"demo":{"name":"Demo"}}}"#,
        )
        .unwrap();

        set_provider_secret("demo".into(), "sk-test-secret".into()).unwrap();
        let view = get_config().unwrap();
        let p = view.providers.iter().find(|p| p.id == "demo").unwrap();
        assert!(p.has_api_key);
        assert_eq!(p.api_key_source, "auth");
        // Ensure file content of view path doesn't embed key — auth is separate
        let cfg = fs::read_to_string(h.config_file()).unwrap();
        assert!(!cfg.contains("sk-test-secret"));

        let secret = get_provider_secret("demo".into()).unwrap();
        assert_eq!(secret, "sk-test-secret");

        set_provider_secret("demo".into(), "".into()).unwrap();
        let view = get_config().unwrap();
        let p = view.providers.iter().find(|p| p.id == "demo").unwrap();
        assert!(!p.has_api_key);
    }

    #[test]
    fn delete_provider_and_model() {
        let h = TempHome::new();
        fs::write(
            h.config_file(),
            r#"{
              "mcp": {"x":1},
              "provider": {
                "a": { "models": { "m1": {"name":"M1"}, "m2": {"name":"M2"} } },
                "b": { "models": { "n": {} } }
              }
            }"#,
        )
        .unwrap();

        delete_model("a".into(), "m1".into()).unwrap();
        let view = get_config().unwrap();
        let a = view.providers.iter().find(|p| p.id == "a").unwrap();
        assert_eq!(a.models.len(), 1);
        assert_eq!(a.models[0].id, "m2");

        delete_provider("b".into(), true).unwrap();
        let view = get_config().unwrap();
        assert_eq!(view.providers.len(), 1);
        let raw: Value = serde_json::from_str(&fs::read_to_string(h.config_file()).unwrap()).unwrap();
        assert!(raw.get("mcp").is_some());
    }

    #[test]
    fn set_defaults_and_jsonc_parse() {
        let h = TempHome::new();
        let jsonc = h.path.join(".config/opencode/opencode.jsonc");
        // Prefer jsonc when present (mcp resolve prefers jsonc)
        fs::write(
            &jsonc,
            r#"{
              // comment
              "model": "a/b",
              "provider": {}
            }"#,
        )
        .unwrap();
        let view = get_config().unwrap();
        assert!(view.config_exists);
        assert!(view.is_jsonc);
        assert_eq!(view.model.as_deref(), Some("a/b"));
        assert!(!view.warnings.is_empty());

        set_defaults(SetDefaultsPayload {
            model: Some("local/qwen".into()),
            small_model: Some("local/tiny".into()),
            enabled_providers: None,
            disabled_providers: None,
        })
        .unwrap();
        let view = get_config().unwrap();
        assert_eq!(view.model.as_deref(), Some("local/qwen"));
        assert_eq!(view.small_model.as_deref(), Some("local/tiny"));
    }

    #[test]
    fn parse_catalog_fixture() {
        let raw = json!({
            "anthropic": {
                "id": "anthropic",
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "npm": "@ai-sdk/anthropic",
                "models": {
                    "claude-x": {
                        "id": "claude-x",
                        "name": "Claude X",
                        "reasoning": true,
                        "reasoning_options": [
                            { "type": "effort", "values": ["low", "high"] },
                            { "type": "budget_tokens", "min": 1024 }
                        ],
                        "tool_call": true,
                        "attachment": true,
                        "modalities": { "input": ["text", "image"], "output": ["text"] },
                        "limit": { "context": 200000, "output": 64000 }
                    }
                }
            }
        });
        let cat = parse_catalog_json(&raw).unwrap();
        assert_eq!(cat.providers.len(), 1);
        let m = &cat.providers[0].models[0];
        assert_eq!(m.limit_context, Some(200000.0));
        assert_eq!(m.modalities_input, vec!["text", "image"]);
        assert_eq!(m.reasoning_options.len(), 2);
    }

    #[test]
    fn upsert_provider_custom() {
        let _h = TempHome::new();
        upsert_provider(UpsertProviderPayload {
            id: "ollama".into(),
            previous_id: None,
            name: Some("Ollama".into()),
            npm: Some("@ai-sdk/openai-compatible".into()),
            api: None,
            base_url: Some("http://localhost:11434/v1".into()),
            set_cache_key: Some(true),
            timeout: Some(600000),
            chunk_timeout: None,
            whitelist: Some(vec!["llama3".into()]),
            blacklist: None,
            api_key: None,
        })
        .unwrap();
        let view = get_config().unwrap();
        assert_eq!(view.providers.len(), 1);
        let p = &view.providers[0];
        assert_eq!(p.base_url.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(p.whitelist, vec!["llama3".to_string()]);
        assert_eq!(p.set_cache_key, Some(true));
    }

    #[test]
    fn sync_provider_mcp_to_deveco_fork() {
        let h = TempHome::new();
        // source OpenCode config
        let src = json!({
            "$schema": SCHEMA_URL,
            "model": "cpa/foo",
            "provider": {
                "cpa": {
                    "npm": "@ai-sdk/openai-compatible",
                    "options": { "baseURL": "http://127.0.0.1:9/v1" },
                    "models": { "m1": { "name": "M1" } }
                }
            },
            "mcp": {
                "demo": { "type": "local", "command": ["echo"], "enabled": true }
            }
        });
        fs::write(h.config_file(), serde_json::to_string_pretty(&src).unwrap()).unwrap();
        // source auth for provider cpa
        let auth_src = h.path.join(".local/share/opencode/auth.json");
        fs::write(
            &auth_src,
            r#"{"cpa":{"type":"api","key":"sk-sync-test"}}"#,
        )
        .unwrap();

        // target DevEco: existing file with other top-level keys that must survive
        let deveco_dir = h.path.join(".config/deveco");
        fs::create_dir_all(&deveco_dir).unwrap();
        let deveco_cfg = deveco_dir.join("deveco.jsonc");
        fs::write(
            &deveco_cfg,
            r#"{
              "theme": "dark",
              "provider": { "old": {} },
              "mcp": { "stale": { "type": "local", "command": ["true"] } }
            }"#,
        )
        .unwrap();

        // 默认含 mcp 同步
        let res = sync_to_fork_agent("deveco-code".into(), true).unwrap();
        assert!(res.ok, "{}", res.message);
        assert_eq!(res.results.len(), 1);
        assert!(res.results[0].ok);
        assert_eq!(res.results[0].provider_count, 1);
        assert_eq!(res.results[0].mcp_count, 1);
        assert_eq!(res.results[0].auth_keys_synced, 1);

        let raw = fs::read_to_string(&deveco_cfg).unwrap();
        let v: Value = json5::from_str(&raw).unwrap();
        // preserved unrelated key
        assert_eq!(v.get("theme").and_then(|t| t.as_str()), Some("dark"));
        // replaced provider/mcp
        assert!(v.pointer("/provider/cpa/models/m1").is_some());
        assert!(v.pointer("/provider/old").is_none());
        assert!(v.pointer("/mcp/demo").is_some());
        assert!(v.pointer("/mcp/stale").is_none());

        let auth_dst = h.path.join(".local/share/deveco/auth.json");
        assert!(auth_dst.is_file());
        let auth_raw = fs::read_to_string(&auth_dst).unwrap();
        assert!(auth_raw.contains("sk-sync-test"));

        // status should report in_sync after write
        let status = get_fork_sync_status().unwrap();
        let t = status
            .targets
            .iter()
            .find(|t| t.agent == "deveco-code")
            .expect("deveco target");
        assert_eq!(t.status, "in_sync");
    }

    #[test]
    fn sync_provider_only_preserves_target_mcp() {
        let h = TempHome::new();
        let src = json!({
            "$schema": SCHEMA_URL,
            "provider": {
                "cpa": {
                    "npm": "@ai-sdk/openai-compatible",
                    "options": { "baseURL": "http://127.0.0.1:9/v1" },
                    "models": { "m1": { "name": "M1" } }
                }
            },
            "mcp": {
                "demo": { "type": "local", "command": ["echo"], "enabled": true }
            }
        });
        fs::write(h.config_file(), serde_json::to_string_pretty(&src).unwrap()).unwrap();

        let deveco_dir = h.path.join(".config/deveco");
        fs::create_dir_all(&deveco_dir).unwrap();
        let deveco_cfg = deveco_dir.join("deveco.jsonc");
        fs::write(
            &deveco_cfg,
            r#"{
              "theme": "dark",
              "provider": { "old": {} },
              "mcp": { "keep-me": { "type": "local", "command": ["true"] } }
            }"#,
        )
        .unwrap();

        let res = sync_to_fork_agent("deveco-code".into(), false).unwrap();
        assert!(res.ok, "{}", res.message);
        assert_eq!(res.results[0].provider_count, 1);
        // 未同步 mcp 时计数为目标侧保留的 mcp
        assert_eq!(res.results[0].mcp_count, 1);

        let raw = fs::read_to_string(&deveco_cfg).unwrap();
        let v: Value = json5::from_str(&raw).unwrap();
        assert_eq!(v.get("theme").and_then(|t| t.as_str()), Some("dark"));
        assert!(v.pointer("/provider/cpa/models/m1").is_some());
        assert!(v.pointer("/provider/old").is_none());
        // 目标 mcp 保留，源 mcp 未写入
        assert!(v.pointer("/mcp/keep-me").is_some());
        assert!(v.pointer("/mcp/demo").is_none());
    }
}
