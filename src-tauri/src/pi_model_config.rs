//! Pi / Oh-My-Pi（omp）供应商与模型配置读写（通用模型配置框架的 Pi 家族后端）。
//!
//! - Pi：`~/.pi/agent/models.json`（JSON，顶层 `providers`）
//!   - 密钥：`~/.pi/agent/auth.json`（`{ providerId: { type: "api_key", key } }`）
//!   - 默认模型：`~/.pi/agent/settings.json` 的 `defaultProvider` / `defaultModel`
//! - Oh-My-Pi：`~/.omp/agent/models.yml`（YAML；兼容读 `.yaml` 与旧版 `models.json`）
//!   - 密钥：`~/.omp/agent/auth.json`
//!   - 默认模型暂不支持可视化编辑（请在 omp 内用 `/model` 或 `omp config` 管理）
//!
//! 字段映射到通用 DTO：`contextWindow`→limitContext、`maxTokens`→limitOutput、
//! `input`→modalitiesInput；未建模字段经 extraOptions 原样往返。
//! 列表 DTO **永不**回传明文 API Key，与 OpenCode 后端一致。

use crate::agent_model_config::ModelConfigAgent;
use crate::opencode_config::{
    api_key_source, atomic_write, auth_get_key, auth_has_key, bool_opt, display_path, f64_opt,
    load_auth_file, save_auth_file, str_opt, string_list, AgentActionResult, AgentModelConfigView,
    AgentModelView, AgentProviderView, SetDefaultsPayload, UpsertModelPayload,
    UpsertProviderPayload,
};
use crate::platform;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;

/// Pi 家族模型条目的已知键；其余字段进 extraOptions 原样往返。
const MODEL_KNOWN_KEYS: &[&str] = &["id", "name", "reasoning", "input", "contextWindow", "maxTokens"];

/* ===== Paths ===== */

struct FamilyPaths {
    /// 模型配置文件（存在者优先，否则取默认名）。
    models: PathBuf,
    /// models 文件是否按 YAML 读写。
    yaml: bool,
    auth: PathBuf,
    /// 默认模型 settings（仅 Pi）。
    settings: Option<PathBuf>,
}

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "无法解析用户主目录".to_string())
}

fn family_paths(agent: ModelConfigAgent) -> Result<FamilyPaths, String> {
    let home = home_dir()?;
    match agent {
        ModelConfigAgent::Pi => Ok(FamilyPaths {
            models: home.join(".pi/agent/models.json"),
            yaml: false,
            auth: home.join(".pi/agent/auth.json"),
            settings: Some(home.join(".pi/agent/settings.json")),
        }),
        ModelConfigAgent::OhMyPi => {
            let base = home.join(".omp/agent");
            let yml = base.join("models.yml");
            let yaml = base.join("models.yaml");
            let legacy = base.join("models.json");
            let models = if yml.exists() {
                yml
            } else if yaml.exists() {
                yaml
            } else if legacy.exists() {
                legacy
            } else {
                yml
            };
            let yaml_flag = matches!(
                models.extension().and_then(|e| e.to_str()),
                Some("yml") | Some("yaml")
            );
            Ok(FamilyPaths {
                models,
                yaml: yaml_flag,
                auth: base.join("auth.json"),
                settings: None,
            })
        }
        ModelConfigAgent::Opencode => unreachable!("opencode 不走 Pi 家族后端"),
    }
}

/* ===== IO ===== */

fn load_models_root(paths: &FamilyPaths) -> Result<(Value, bool), String> {
    if !paths.models.exists() {
        return Ok((json!({}), false));
    }
    let raw = fs::read_to_string(&paths.models)
        .map_err(|e| format!("读取 {} 失败: {e}", paths.models.display()))?;
    let v: Value = if paths.yaml {
        serde_yaml::from_str(&raw)
            .map_err(|e| format!("解析 {} 失败: {e}", paths.models.display()))?
    } else {
        json5::from_str(&raw).map_err(|e| format!("解析 {} 失败: {e}", paths.models.display()))?
    };
    match v {
        Value::Object(_) => Ok((v, true)),
        _ => Err(format!("{} 根节点必须是对象", paths.models.display())),
    }
}

fn write_models_root(paths: &FamilyPaths, root: &Value) -> Result<(), String> {
    let text = if paths.yaml {
        serde_yaml::to_string(root).map_err(|e| format!("序列化 YAML 失败: {e}"))?
    } else {
        let t = serde_json::to_string_pretty(root).map_err(|e| format!("序列化失败: {e}"))?;
        format!("{t}\n")
    };
    atomic_write(&paths.models, &text)
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

fn providers_map_mut(root: &mut Value) -> Result<&mut Map<String, Value>, String> {
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| "配置根节点无效".to_string())?;
    let providers = root_obj
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !providers.is_object() {
        return Err("providers 节点必须是对象".into());
    }
    Ok(providers.as_object_mut().unwrap())
}

fn provider_object_mut<'a>(root: &'a mut Value, id: &str) -> Result<&'a mut Map<String, Value>, String> {
    let map = providers_map_mut(root)?;
    let entry = map
        .entry(id.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    entry
        .as_object_mut()
        .ok_or_else(|| format!("providers.{id} 必须是对象"))
}

/* ===== Parse view ===== */

fn parse_model(raw: &Value) -> AgentModelView {
    let mut extra = Map::new();
    if let Some(obj) = raw.as_object() {
        for (k, v) in obj {
            if !MODEL_KNOWN_KEYS.contains(&k.as_str()) {
                extra.insert(k.clone(), v.clone());
            }
        }
    }
    AgentModelView {
        id: str_opt(raw, "id").unwrap_or_default(),
        name: str_opt(raw, "name"),
        limit_context: f64_opt(raw, "contextWindow"),
        limit_input: None,
        limit_output: f64_opt(raw, "maxTokens"),
        modalities_input: string_list(raw, "input"),
        modalities_output: Vec::new(),
        reasoning: bool_opt(raw, "reasoning"),
        tool_call: None,
        attachment: None,
        status: None,
        thinking_type: None,
        thinking_budget_tokens: None,
        reasoning_effort: None,
        text_verbosity: None,
        variants: Vec::new(),
        extra_options: extra,
    }
}

fn parse_provider(id: &str, raw: &Value, auth: &Map<String, Value>) -> AgentProviderView {
    let in_auth = auth_has_key(auth, id);
    // Pi 家族 config 侧 apiKey 常见为环境变量名，非空即视为已配置。
    let in_config = raw
        .get("apiKey")
        .and_then(|k| k.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    let models = raw
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            let mut list: Vec<_> = arr.iter().map(parse_model).collect();
            list.sort_by(|a, b| a.id.cmp(&b.id));
            list
        })
        .unwrap_or_default();

    AgentProviderView {
        id: id.to_string(),
        name: str_opt(raw, "name"),
        npm: None,
        api: str_opt(raw, "api"),
        has_api_key: in_auth || in_config,
        api_key_source: api_key_source(in_auth, in_config),
        base_url: str_opt(raw, "baseUrl").or_else(|| str_opt(raw, "baseURL")),
        set_cache_key: None,
        timeout: None,
        chunk_timeout: None,
        whitelist: Vec::new(),
        blacklist: Vec::new(),
        models,
    }
}

fn read_defaults(paths: &FamilyPaths) -> (Option<String>, Option<String>) {
    let Some(sp) = &paths.settings else {
        return (None, None);
    };
    if !sp.exists() {
        return (None, None);
    }
    let Ok(raw) = fs::read_to_string(sp) else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return (None, None);
    };
    let provider = str_opt(&v, "defaultProvider");
    let model = str_opt(&v, "defaultModel");
    let combined = match (provider, model) {
        (Some(p), Some(m)) => Some(format!("{p}/{m}")),
        (None, Some(m)) => Some(m),
        _ => None,
    };
    (combined, None)
}

/* ===== Public API ===== */

pub fn get_config(agent: ModelConfigAgent) -> Result<AgentModelConfigView, String> {
    let paths = family_paths(agent)?;
    let installed = crate::sniff::is_agent_installed(agent.id());
    let exists = paths.models.exists();
    let mut warnings = Vec::new();
    if paths.yaml {
        warnings.push("当前配置为 YAML；保存后注释会丢失。".into());
    }

    let (model, small_model) = read_defaults(&paths);

    let not_installed_view = || AgentModelConfigView {
        agent: agent.id().into(),
        config_path: display_path(&paths.models),
        config_exists: exists,
        is_jsonc: false,
        installed: false,
        defaults_supported: agent == ModelConfigAgent::Pi,
        small_model_supported: false,
        model: None,
        small_model: None,
        enabled_providers: None,
        disabled_providers: None,
        providers: Vec::new(),
        warnings: Vec::new(),
    };
    if !installed {
        return Ok(not_installed_view());
    }

    let (root, _) = load_models_root(&paths)?;
    let auth = load_auth_file(&paths.auth).unwrap_or_default();
    let providers = root
        .get("providers")
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

    Ok(AgentModelConfigView {
        agent: agent.id().into(),
        config_path: display_path(&paths.models),
        config_exists: exists,
        is_jsonc: false,
        installed: true,
        defaults_supported: agent == ModelConfigAgent::Pi,
        small_model_supported: false,
        model,
        small_model,
        enabled_providers: None,
        disabled_providers: None,
        providers,
        warnings,
    })
}

pub fn upsert_provider(
    agent: ModelConfigAgent,
    payload: UpsertProviderPayload,
) -> Result<AgentActionResult, String> {
    let id = payload.id.trim().to_string();
    if id.is_empty() {
        return Err("提供商 ID 不能为空".into());
    }
    if id.contains('/') || id.contains(' ') {
        return Err("提供商 ID 不能包含空格或 /".into());
    }

    let paths = family_paths(agent)?;
    let (mut root, _) = load_models_root(&paths)?;

    // Rename: move previous_id → id（连同 auth 条目）
    if let Some(prev) = payload
        .previous_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && *s != id)
    {
        let map = providers_map_mut(&mut root)?;
        if let Some(old) = map.remove(&prev) {
            if map.contains_key(&id) {
                return Err(format!("提供商 `{id}` 已存在，无法重命名"));
            }
            map.insert(id.clone(), old);
        }
        let mut auth = load_auth_file(&paths.auth)?;
        if let Some(a) = auth.remove(&prev) {
            auth.insert(id.clone(), a);
            save_auth_file(&paths.auth, &auth)?;
        }
    }

    {
        let p = provider_object_mut(&mut root, &id)?;
        set_or_remove_str(p, "name", &payload.name);
        set_or_remove_str(p, "api", &payload.api);
        if let Some(ref bu) = payload.base_url {
            if bu.is_empty() {
                p.remove("baseUrl");
            } else {
                p.insert("baseUrl".into(), Value::String(bu.clone()));
            }
        }
        // OpenCode 特有字段（npm/setCacheKey/timeout/whitelist/blacklist）在 Pi 家族中忽略。
    }

    // 密钥只写 auth.json；config 内 apiKey 多为环境变量名，保持原样不动。
    if let Some(ref key) = payload.api_key {
        let mut auth = load_auth_file(&paths.auth)?;
        if key.is_empty() {
            auth.remove(&id);
        } else {
            auth.insert(
                id.clone(),
                json!({
                    "type": "api_key",
                    "key": key,
                }),
            );
        }
        save_auth_file(&paths.auth, &auth)?;
    }

    write_models_root(&paths, &root)?;
    Ok(AgentActionResult {
        ok: true,
        message: format!("提供商 `{id}` 已保存"),
        view: Some(get_config(agent)?),
    })
}

pub fn delete_provider(
    agent: ModelConfigAgent,
    provider_id: String,
    delete_auth: bool,
) -> Result<AgentActionResult, String> {
    let id = provider_id.trim().to_string();
    if id.is_empty() {
        return Err("提供商 ID 不能为空".into());
    }
    let paths = family_paths(agent)?;
    let (mut root, exists) = load_models_root(&paths)?;
    if !exists {
        return Err("配置文件不存在".into());
    }
    let removed = root
        .as_object_mut()
        .and_then(|o| o.get_mut("providers"))
        .and_then(|p| p.as_object_mut())
        .and_then(|m| m.remove(&id))
        .is_some();
    if !removed {
        return Err(format!("未找到提供商 `{id}`"));
    }
    write_models_root(&paths, &root)?;
    if delete_auth {
        let mut auth = load_auth_file(&paths.auth)?;
        auth.remove(&id);
        save_auth_file(&paths.auth, &auth)?;
    }
    Ok(AgentActionResult {
        ok: true,
        message: format!("已删除提供商 `{id}`"),
        view: Some(get_config(agent)?),
    })
}

/// 在 provider 的 models 数组中按 id 定位条目下标。
fn find_model_index(arr: &[Value], id: &str) -> Option<usize> {
    arr.iter().position(|m| m.get("id").and_then(|x| x.as_str()) == Some(id))
}

pub fn upsert_model(
    agent: ModelConfigAgent,
    payload: UpsertModelPayload,
) -> Result<AgentActionResult, String> {
    let pid = payload.provider_id.trim().to_string();
    let mid = payload.id.trim().to_string();
    if pid.is_empty() || mid.is_empty() {
        return Err("提供商 ID 与模型 ID 均不能为空".into());
    }

    let paths = family_paths(agent)?;
    let (mut root, _) = load_models_root(&paths)?;

    // Ensure provider exists
    {
        let _ = provider_object_mut(&mut root, &pid)?;
    }

    let p = provider_object_mut(&mut root, &pid)?;
    let models_val = p
        .entry("models".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !models_val.is_array() {
        return Err("models 必须是数组".into());
    }
    let arr = models_val.as_array_mut().unwrap();

    // Rename: previous_id → mid
    if let Some(prev) = payload
        .previous_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && *s != mid)
    {
        if let Some(idx) = find_model_index(arr, &prev) {
            if find_model_index(arr, &mid).is_some() {
                return Err(format!("模型 `{mid}` 已存在"));
            }
            arr[idx].as_object_mut().unwrap().insert("id".into(), Value::String(mid.clone()));
        }
    }

    let idx = match find_model_index(arr, &mid) {
        Some(i) => i,
        None => {
            arr.push(json!({ "id": mid }));
            arr.len() - 1
        }
    };
    let m = arr[idx]
        .as_object_mut()
        .ok_or_else(|| format!("模型 `{mid}` 必须是对象"))?;

    set_or_remove_str(m, "name", &payload.name);

    if let Some(r) = payload.reasoning {
        m.insert("reasoning".into(), Value::Bool(r));
    }
    if let Some(ref inputs) = payload.modalities_input {
        if inputs.is_empty() {
            m.remove("input");
        } else {
            m.insert(
                "input".into(),
                Value::Array(inputs.iter().map(|s| Value::String(s.clone())).collect()),
            );
        }
    }
    match payload.limit_context {
        Some(v) if v > 0.0 => {
            m.insert("contextWindow".into(), json!(v));
        }
        Some(_) => {
            m.remove("contextWindow");
        }
        None => {}
    }
    match payload.limit_output {
        Some(v) if v > 0.0 => {
            m.insert("maxTokens".into(), json!(v));
        }
        Some(_) => {
            m.remove("maxTokens");
        }
        None => {}
    }

    if let Some(ref extra) = payload.extra_options {
        if payload.replace_extra_options.unwrap_or(false) {
            let retained: Map<String, Value> = m
                .iter()
                .filter(|(k, _)| MODEL_KNOWN_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            *m = retained;
        }
        for (k, v) in extra {
            m.insert(k.clone(), v.clone());
        }
    }

    write_models_root(&paths, &root)?;
    Ok(AgentActionResult {
        ok: true,
        message: format!("模型 `{pid}/{mid}` 已保存"),
        view: Some(get_config(agent)?),
    })
}

pub fn delete_model(
    agent: ModelConfigAgent,
    provider_id: String,
    model_id: String,
) -> Result<AgentActionResult, String> {
    let pid = provider_id.trim().to_string();
    let mid = model_id.trim().to_string();
    let paths = family_paths(agent)?;
    let (mut root, exists) = load_models_root(&paths)?;
    if !exists {
        return Err("配置文件不存在".into());
    }
    let removed = root
        .as_object_mut()
        .and_then(|o| o.get_mut("providers"))
        .and_then(|p| p.as_object_mut())
        .and_then(|pm| pm.get_mut(&pid))
        .and_then(|p| p.as_object_mut())
        .and_then(|p| p.get_mut("models"))
        .and_then(|m| m.as_array_mut())
        .map(|arr| match find_model_index(arr, &mid) {
            Some(i) => {
                arr.remove(i);
                true
            }
            None => false,
        })
        .unwrap_or(false);
    if !removed {
        return Err(format!("未找到模型 `{pid}/{mid}`"));
    }
    write_models_root(&paths, &root)?;
    Ok(AgentActionResult {
        ok: true,
        message: format!("已删除模型 `{pid}/{mid}`"),
        view: Some(get_config(agent)?),
    })
}

pub fn get_provider_secret(agent: ModelConfigAgent, provider_id: String) -> Result<String, String> {
    let id = provider_id.trim().to_string();
    let paths = family_paths(agent)?;
    let auth = load_auth_file(&paths.auth)?;
    if let Some(k) = auth_get_key(&auth, &id) {
        return Ok(k);
    }
    // fallback config 侧 apiKey（可能是字面密钥，也可能是环境变量名）
    let (root, _) = load_models_root(&paths)?;
    if let Some(k) = root
        .pointer(&format!("/providers/{id}/apiKey"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        return Ok(k.to_string());
    }
    Ok(String::new())
}

pub fn set_provider_secret(
    agent: ModelConfigAgent,
    provider_id: String,
    api_key: String,
) -> Result<AgentActionResult, String> {
    upsert_provider(
        agent,
        UpsertProviderPayload {
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
        },
    )
}

pub fn set_defaults(agent: ModelConfigAgent, payload: SetDefaultsPayload) -> Result<AgentActionResult, String> {
    match agent {
        ModelConfigAgent::OhMyPi => Err(
            "Oh-My-Pi 暂不支持可视化编辑默认模型，请在 omp 内使用 /model 或 omp config 命令管理。".into(),
        ),
        ModelConfigAgent::Pi => {
            let paths = family_paths(agent)?;
            let sp = paths
                .settings
                .as_ref()
                .ok_or_else(|| "Pi settings 路径缺失".to_string())?;
            let mut root = if sp.exists() {
                crate::opencode_config::read_json_value(sp)?
            } else {
                json!({})
            };
            if !root.is_object() {
                root = json!({});
            }
            // payload.model: None=不改动；Some("")=删除；Some("provider/model")=拆分写入
            if let Some(ref m) = payload.model {
                let obj = root.as_object_mut().unwrap();
                if m.is_empty() {
                    obj.remove("defaultProvider");
                    obj.remove("defaultModel");
                } else if let Some((p, mid)) = m.split_once('/') {
                    obj.insert("defaultProvider".into(), Value::String(p.to_string()));
                    obj.insert("defaultModel".into(), Value::String(mid.to_string()));
                } else {
                    obj.remove("defaultProvider");
                    obj.insert("defaultModel".into(), Value::String(m.clone()));
                }
            }
            let text =
                serde_json::to_string_pretty(&root).map_err(|e| format!("序列化失败: {e}"))?;
            atomic_write(sp, &format!("{text}\n"))?;
            Ok(AgentActionResult {
                ok: true,
                message: "默认模型已更新".into(),
                view: Some(get_config(agent)?),
            })
        }
        ModelConfigAgent::Opencode => unreachable!("opencode 不走 Pi 家族后端"),
    }
}

pub fn reveal_config(agent: ModelConfigAgent) -> Result<AgentActionResult, String> {
    let paths = family_paths(agent)?;
    if !paths.models.exists() {
        if let Some(parent) = paths.models.parent() {
            fs::create_dir_all(parent).ok();
            platform::open_path(parent).map_err(|e| format!("打开目录失败: {e}"))?;
            return Ok(AgentActionResult {
                ok: true,
                message: format!("已打开目录 {}", display_path(parent)),
                view: None,
            });
        }
        return Err("配置文件尚不存在".into());
    }
    platform::reveal_path(&paths.models).map_err(|e| format!("打开文件管理器失败: {e}"))?;
    Ok(AgentActionResult {
        ok: true,
        message: format!("已在文件管理器中显示 {}", display_path(&paths.models)),
        view: None,
    })
}

/* ===== Tests ===== */

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // 触碰 HOME 环境的测试串行化：使用跨模块共享锁（见 config::TEST_HOME_LOCK）。

    struct TempHome {
        path: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TempHome {
        fn new(subdirs: &[&str]) -> Self {
            let guard = crate::config::lock_home_for_test();
            let path = std::env::temp_dir().join(format!(
                "agentbuddy-pifam-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            for d in subdirs {
                fs::create_dir_all(path.join(d)).unwrap();
            }
            std::env::set_var("HOME", &path);
            Self {
                path,
                _guard: guard,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_yaml(path: &Path, v: &Value) {
        fs::write(path, serde_yaml::to_string(v).unwrap()).unwrap();
    }

    #[test]
    fn pi_provider_model_upsert_roundtrip() {
        let h = TempHome::new(&[".pi/agent"]);
        let models = h.path.join(".pi/agent/models.json");
        fs::write(
            &models,
            r#"{"providers":{"deepseek":{"baseUrl":"https://api.deepseek.com","api":"openai-completions","apiKey":"$DEEPSEEK_API_KEY","models":[{"id":"deepseek-chat","name":"DeepSeek Chat","reasoning":false,"input":["text"],"contextWindow":64000,"maxTokens":8192,"extraKeep":{"a":1}}]}}}"#,
        )
        .unwrap();

        let view = get_config(ModelConfigAgent::Pi).unwrap();
        assert_eq!(view.agent, "pi");
        assert_eq!(view.providers.len(), 1);
        let p = &view.providers[0];
        assert_eq!(p.base_url.as_deref(), Some("https://api.deepseek.com"));
        assert!(p.has_api_key);
        assert_eq!(p.api_key_source, "config");
        let m = &p.models[0];
        assert_eq!(m.limit_context, Some(64000.0));
        assert_eq!(m.limit_output, Some(8192.0));
        assert!(m.extra_options.contains_key("extraKeep"));

        // Rename + update model
        upsert_model(
            ModelConfigAgent::Pi,
            UpsertModelPayload {
                provider_id: "deepseek".into(),
                id: "deepseek-v3".into(),
                previous_id: Some("deepseek-chat".into()),
                name: Some("DeepSeek V3".into()),
                limit_context: Some(128000.0),
                limit_input: None,
                limit_output: None,
                modalities_input: Some(vec!["text".into()]),
                modalities_output: None,
                reasoning: Some(true),
                tool_call: None,
                attachment: None,
                status: None,
                thinking_type: None,
                thinking_budget_tokens: None,
                reasoning_effort: None,
                text_verbosity: None,
                variants: None,
                extra_options: None,
                replace_extra_options: None,
            },
        )
        .unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&models).unwrap()).unwrap();
        let m0 = &raw.pointer("/providers/deepseek/models/0").unwrap();
        assert_eq!(m0.get("id").and_then(|x| x.as_str()), Some("deepseek-v3"));
        assert_eq!(m0.get("contextWindow").and_then(|x| x.as_f64()), Some(128000.0));
        // Unmodeled field preserved
        assert!(m0.pointer("/extraKeep/a").is_some());
        // apiKey untouched
        assert_eq!(
            raw.pointer("/providers/deepseek/apiKey").and_then(|x| x.as_str()),
            Some("$DEEPSEEK_API_KEY")
        );
    }

    #[test]
    fn pi_auth_secret_and_defaults() {
        let h = TempHome::new(&[".pi/agent"]);
        upsert_provider(
            ModelConfigAgent::Pi,
            UpsertProviderPayload {
                id: "deepseek".into(),
                previous_id: None,
                name: None,
                npm: None,
                api: Some("openai-completions".into()),
                base_url: Some("https://api.deepseek.com".into()),
                set_cache_key: None,
                timeout: None,
                chunk_timeout: None,
                whitelist: None,
                blacklist: None,
                api_key: Some("sk-test".into()),
            },
        )
        .unwrap();

        let auth: Value = serde_json::from_str(
            &fs::read_to_string(h.path.join(".pi/agent/auth.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(auth.pointer("/deepseek/type").and_then(|x| x.as_str()), Some("api_key"));
        assert_eq!(get_provider_secret(ModelConfigAgent::Pi, "deepseek".into()).unwrap(), "sk-test");

        set_defaults(
            ModelConfigAgent::Pi,
            SetDefaultsPayload {
                model: Some("deepseek/deepseek-chat".into()),
                small_model: None,
                enabled_providers: None,
                disabled_providers: None,
            },
        )
        .unwrap();
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(h.path.join(".pi/agent/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings.get("defaultProvider").and_then(|x| x.as_str()), Some("deepseek"));
        assert_eq!(settings.get("defaultModel").and_then(|x| x.as_str()), Some("deepseek-chat"));

        let view = get_config(ModelConfigAgent::Pi).unwrap();
        assert_eq!(view.model.as_deref(), Some("deepseek/deepseek-chat"));

        // Clear secret removes auth entry
        set_provider_secret(ModelConfigAgent::Pi, "deepseek".into(), String::new()).unwrap();
        let auth: Value = serde_json::from_str(
            &fs::read_to_string(h.path.join(".pi/agent/auth.json")).unwrap(),
        )
        .unwrap();
        assert!(auth.get("deepseek").is_none());
    }

    #[test]
    fn omp_yaml_roundtrip_preserves_unknown_fields() {
        let h = TempHome::new(&[".omp/agent"]);
        let models = h.path.join(".omp/agent/models.yml");
        let initial = json!({
            "providers": {
                "qiniu": {
                    "baseUrl": "https://api.qnaigc.com/v1",
                    "api": "openai-completions",
                    "authHeader": true,
                    "models": [
                        {
                            "id": "gpt-x",
                            "name": "GPT X",
                            "reasoning": true,
                            "input": ["text", "image"],
                            "contextWindow": 400000,
                            "maxTokens": 128000,
                            "compat": { "supportsToolChoice": false }
                        }
                    ]
                }
            }
        });
        write_yaml(&models, &initial);

        let view = get_config(ModelConfigAgent::OhMyPi).unwrap();
        assert_eq!(view.agent, "oh-my-pi");
        assert!(view.defaults_supported == false);
        let p = &view.providers[0];
        assert_eq!(p.models[0].limit_context, Some(400000.0));
        assert!(p.models[0].extra_options.contains_key("compat"));

        // Delete model; provider-level unknown fields must survive
        delete_model(ModelConfigAgent::OhMyPi, "qiniu".into(), "gpt-x".into()).unwrap();
        let raw = fs::read_to_string(&models).unwrap();
        let v: Value = serde_yaml::from_str(&raw).unwrap();
        let providers = v.get("providers").and_then(|p| p.as_object()).unwrap();
        let q = providers.get("qiniu").and_then(|q| q.as_object()).unwrap();
        let models_arr = q.get("models").and_then(|m| m.as_array()).unwrap();
        assert!(models_arr.is_empty());
        assert_eq!(q.get("authHeader").and_then(|x| x.as_bool()), Some(true));

        // Delete provider entirely
        delete_provider(ModelConfigAgent::OhMyPi, "qiniu".into(), false).unwrap();
        let raw = fs::read_to_string(&models).unwrap();
        let v: Value = serde_yaml::from_str(&raw).unwrap();
        let providers = v.get("providers").and_then(|p| p.as_object()).unwrap();
        assert!(providers.get("qiniu").is_none());
        let _ = &h;
    }

    #[test]
    fn omp_defaults_rejected() {
        let _h = TempHome::new(&[".omp/agent"]);
        let err = set_defaults(
            ModelConfigAgent::OhMyPi,
            SetDefaultsPayload {
                model: Some("a/b".into()),
                small_model: None,
                enabled_providers: None,
                disabled_providers: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("omp"));
    }
}
