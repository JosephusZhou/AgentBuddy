//! AI provider (上游模型供应商) registry: encrypted API key storage + CRUD.
//!
//! 独立的供应商库：API 类型集合（anthropic / openai_chat / openai_responses，
//! 可多选）+ Anthropic 侧 Base URL + OpenAI 侧 Base URL + API Key + 两侧默认模型；
//! Anthropic 类型额外支持按档位（haiku/sonnet/opus/fable）覆盖模型。
//! 本期仅做管理，不做下发到 Agent/环境（后续可扩展为一键填充）。

use crate::config;
use crate::crypto;
use crate::db;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// API 类型：Anthropic Messages 协议。
pub const API_TYPE_ANTHROPIC: &str = "anthropic";
/// API 类型：OpenAI Chat Completions 协议。
pub const API_TYPE_OPENAI_CHAT: &str = "openai_chat";
/// API 类型：OpenAI Responses 协议。
pub const API_TYPE_OPENAI_RESPONSES: &str = "openai_responses";

/// 全部合法 API 类型。
pub const API_TYPES: [&str; 3] = [
    API_TYPE_ANTHROPIC,
    API_TYPE_OPENAI_CHAT,
    API_TYPE_OPENAI_RESPONSES,
];

/// 解析 `provider_type` 列（新版为 JSON 数组；兼容旧版单值标量）为规范化、
/// 去重后的合法 API 类型列表。无法解析出任何合法类型时返回空 Vec。
pub fn parse_api_types(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    // 旧版单值标量（多类型升级前）→ 映射到新类型集合。
    let mapped: Option<&[&str]> = match raw {
        "anthropic" => Some(&[API_TYPE_ANTHROPIC][..]),
        "openai" => Some(&[API_TYPE_OPENAI_CHAT, API_TYPE_OPENAI_RESPONSES][..]),
        "universal" => Some(
            &[
                API_TYPE_ANTHROPIC,
                API_TYPE_OPENAI_CHAT,
                API_TYPE_OPENAI_RESPONSES,
            ][..],
        ),
        _ => None,
    };
    if let Some(m) = mapped {
        return m.iter().map(|s| s.to_string()).collect();
    }
    let parsed: Vec<String> = serde_json::from_str(raw).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for item in parsed {
        let t = item.trim().to_lowercase();
        if API_TYPES.contains(&t.as_str()) && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// `provider_type` 列是否包含指定 API 类型。
pub fn provider_types_contain(raw: &str, target: &str) -> bool {
    parse_api_types(raw).iter().any(|t| t == target)
}

/// `provider_type` 列是否包含任一 OpenAI 协议类型（chat 或 responses）。
pub fn provider_types_have_openai(raw: &str) -> bool {
    parse_api_types(raw)
        .iter()
        .any(|t| t == API_TYPE_OPENAI_CHAT || t == API_TYPE_OPENAI_RESPONSES)
}

/// Anthropic 档位模型覆盖允许使用的键（与 Claude 环境的
/// ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS,FABLE}_MODEL 概念对齐）。
pub const MODEL_TIERS: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];

/// 一条已加密的 API Key（salt + nonce + cipher 三元组）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKey {
    pub salt: String,
    pub nonce: String,
    pub cipher: String,
}

/// 自定义模型条目：从供应商端点拉取后用户筛选保留的模型，可自定义别名 ID。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomModel {
    pub model: String,
    pub alias_id: String,
}

/// 返回供应商模型对外展示/路由使用的最终 ID。
///
/// `alias_id` 有值时覆盖原始 `model`，并按首次出现顺序去重。这样同一供应商
/// 的多个原始模型映射到同一个别名时，不会在 UI 或路由池中产生重复条目。
pub fn effective_custom_model_ids(models: impl IntoIterator<Item = CustomModel>) -> Vec<String> {
    let mut seen = HashSet::new();
    models
        .into_iter()
        .map(|model| {
            let alias = model.alias_id.trim();
            if alias.is_empty() {
                model.model.trim().to_string()
            } else {
                alias.to_string()
            }
        })
        .filter(|id| !id.is_empty())
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// Public list item — never includes API key material.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    /// 已勾选的 API 类型集合（anthropic / openai_chat / openai_responses）。
    pub provider_types: Vec<String>,
    /// Anthropic 侧 Base URL（勾选 anthropic 时有值）。
    pub base_url: String,
    /// OpenAI 侧 Base URL（勾选任一 OpenAI 类型时有值），用户独立填写、落库。
    pub openai_base_url: String,
    /// Anthropic 侧默认模型。
    pub default_model: String,
    /// OpenAI 侧默认模型。
    pub openai_default_model: String,
    /// Anthropic 档位模型覆盖；未勾选 anthropic 时恒为空 map。
    pub models: HashMap<String, String>,
    pub has_api_key: bool,
    /// 已存储的 API Key 数量（支持多 Key）。
    pub api_key_count: usize,
    /// 自定义模型列表（从端点拉取后筛选保留，可自定义别名 ID）。
    pub custom_models: Vec<CustomModel>,
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub sort_order: i64,
}

/// Internal DB row including encrypted API key fields.
#[derive(Debug, Clone)]
pub struct AiProviderRow {
    pub id: String,
    pub name: String,
    /// 新版为 JSON 数组字符串（如 `["anthropic","openai_chat"]`）；
    /// 旧版单值（anthropic/openai/universal）由 db 启动迁移与 parse_api_types 兼容。
    pub provider_type: String,
    /// Anthropic 侧 Base URL。
    pub base_url: String,
    /// OpenAI 侧 Base URL（独立填写、落库）。
    pub openai_base_url: String,
    pub api_key_salt: String,
    pub api_key_nonce: String,
    pub api_key_cipher: String,
    pub default_model: String,
    pub openai_default_model: String,
    pub models_json: String,
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub sort_order: i64,
    /// 多 API Key 的加密存储（JSON 数组，每项为 {salt,nonce,cipher}）。
    /// 为空数组时回退到旧的单 Key 列。
    pub api_keys_json: String,
    /// 自定义模型列表（JSON 数组，每项为 {model,aliasId}）。
    pub custom_models_json: String,
}

impl From<AiProviderRow> for AiProvider {
    fn from(row: AiProviderRow) -> Self {
        let models: HashMap<String, String> =
            serde_json::from_str(&row.models_json).unwrap_or_default();
        let provider_types = parse_api_types(&row.provider_type);
        // 解析多 Key JSON；为空时回退到旧的单 Key 列。
        let enc_keys: Vec<EncryptedKey> =
            serde_json::from_str(&row.api_keys_json).unwrap_or_default();
        let api_key_count = if !enc_keys.is_empty() {
            enc_keys.len()
        } else if !row.api_key_cipher.is_empty() {
            1
        } else {
            0
        };
        let custom_models: Vec<CustomModel> =
            serde_json::from_str(&row.custom_models_json).unwrap_or_default();
        Self {
            id: row.id,
            name: row.name,
            provider_types,
            base_url: row.base_url,
            openai_base_url: row.openai_base_url,
            default_model: row.default_model,
            openai_default_model: row.openai_default_model,
            models,
            has_api_key: api_key_count > 0,
            api_key_count,
            custom_models,
            notes: row.notes,
            created_at: row.created_at,
            updated_at: row.updated_at,
            sort_order: row.sort_order,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderUpsertPayload {
    pub id: String,
    pub name: String,
    /// 勾选的 API 类型集合（anthropic / openai_chat / openai_responses）。
    pub provider_types: Vec<String>,
    /// Anthropic 侧 Base URL（勾选 anthropic 时必填）。
    pub base_url: String,
    /// OpenAI 侧 Base URL（勾选任一 OpenAI 类型时必填）。
    pub openai_base_url: Option<String>,
    /// Required on create; empty/omitted on edit keeps existing ciphertext.
    /// （旧字段，仅单个 Key；优先使用 `api_keys`。）
    pub api_key: Option<String>,
    /// 多 API Key（明文数组）；Some 时替换全部密钥，None 时保持旧密钥。
    pub api_keys: Option<Vec<String>>,
    /// Anthropic 侧默认模型。
    pub default_model: Option<String>,
    /// OpenAI 侧默认模型。
    pub openai_default_model: Option<String>,
    /// Anthropic 档位模型覆盖（haiku/sonnet/opus/fable）；勾选 anthropic 时有效。
    pub models: Option<HashMap<String, String>>,
    /// 自定义模型列表（含别名 ID）；None 时保持旧列表，Some 时替换。
    pub custom_models: Option<Vec<CustomModel>>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderActionResult {
    pub ok: bool,
    pub message: String,
    pub provider: Option<AiProvider>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn validate_base_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("Base URL 必须以 http:// 或 https:// 开头".to_string());
    }
    Ok(())
}

/// 规范化前端提交的 API 类型集合：去空白、小写、去重；至少保留一个合法类型。
fn normalize_api_types(raw: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for item in raw {
        let t = item.trim().to_lowercase();
        if API_TYPES.contains(&t.as_str()) && !out.contains(&t) {
            out.push(t);
        }
    }
    if out.is_empty() {
        return Err(
            "请至少勾选一个 API 类型（Anthropic / OpenAI Chat Completions / OpenAI Responses）"
                .to_string(),
        );
    }
    Ok(out)
}

/// 过滤档位模型映射：仅保留允许的档位键、去除空值；仅勾选了 anthropic 时保留。
/// OpenAI 协议没有 `haiku`/`sonnet`/`opus`/`fable` 这类档位概念，
/// 无论前端是否误传都强制清空，避免在 Claude/Codex 环境回写时混入无效档位。
fn normalize_models(
    has_anthropic: bool,
    models: &HashMap<String, String>,
) -> HashMap<String, String> {
    if !has_anthropic {
        return HashMap::new();
    }
    models
        .iter()
        .filter_map(|(k, v)| {
            let key = k.trim().to_lowercase();
            let value = v.trim().to_string();
            if MODEL_TIERS.contains(&key.as_str()) && !value.is_empty() {
                Some((key, value))
            } else {
                None
            }
        })
        .collect()
}

pub fn list_providers() -> Result<Vec<AiProvider>, String> {
    let rows = db::load_ai_provider_rows()?;
    Ok(rows
        .into_iter()
        .filter(|row| !parse_api_types(&row.provider_type).is_empty())
        .map(AiProvider::from)
        .collect())
}

pub fn upsert_provider(payload: AiProviderUpsertPayload) -> Result<AiProviderActionResult, String> {
    let id = payload.id.trim().to_string();
    let name = payload.name.trim().to_string();
    let types = normalize_api_types(&payload.provider_types)?;
    let has_anthropic = types.iter().any(|t| t == API_TYPE_ANTHROPIC);
    let has_openai = types
        .iter()
        .any(|t| t == API_TYPE_OPENAI_CHAT || t == API_TYPE_OPENAI_RESPONSES);
    let provider_type = serde_json::to_string(&types).unwrap_or_else(|_| "[]".into());

    // 勾选了哪侧才接收哪侧的 URL / 默认模型；未勾选侧强制清空，保持库内一致。
    let base_url = if has_anthropic {
        payload.base_url.trim().to_string()
    } else {
        String::new()
    };
    let openai_base_url = if has_openai {
        payload
            .openai_base_url
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        String::new()
    };
    let default_model = if has_anthropic {
        payload.default_model.unwrap_or_default().trim().to_string()
    } else {
        String::new()
    };
    let openai_default_model = if has_openai {
        payload
            .openai_default_model
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        String::new()
    };
    let notes = payload.notes.unwrap_or_default().trim().to_string();

    if id.is_empty() {
        return Err("供应商 ID 不能为空".to_string());
    }
    if id == crate::codex_env::OFFICIAL_OAUTH_PROVIDER_ID {
        return Err("该供应商 ID 保留给 Codex 官方 OAuth 登录，不能用于 AI 供应商".to_string());
    }
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if has_anthropic {
        validate_base_url(&base_url)?;
    }
    if has_openai {
        validate_base_url(&openai_base_url)?;
    }

    let empty_models = HashMap::new();
    let models = normalize_models(
        has_anthropic,
        payload.models.as_ref().unwrap_or(&empty_models),
    );
    let models_json = serde_json::to_string(&models).unwrap_or_else(|_| "{}".into());

    let existing = db::get_ai_provider_row(&id)?;
    let now = now_secs();
    let master = config::load_secrets_key()?;

    // 自定义模型列表：None 时保持旧列表，Some 时替换。
    let custom_models_json = match payload.custom_models.as_ref() {
        Some(list) => {
            let cleaned: Vec<CustomModel> = list
                .iter()
                .map(|cm| CustomModel {
                    model: cm.model.trim().to_string(),
                    alias_id: cm.alias_id.trim().to_string(),
                })
                .filter(|cm| !cm.model.is_empty())
                .collect();
            serde_json::to_string(&cleaned).unwrap_or_else(|_| "[]".into())
        }
        None => existing
            .as_ref()
            .map(|r| r.custom_models_json.clone())
            .unwrap_or_else(|| "[]".to_string()),
    };

    // 确定明文 API Key 列表：优先使用 api_keys（数组），回退到 api_key（单个）
    let keep_existing_keys = payload.api_keys.is_none() && payload.api_key.is_none();
    let plain_keys: Vec<String> = if let Some(keys) = payload.api_keys.as_ref() {
        keys.iter()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect()
    } else if let Some(single) = payload.api_key.as_ref() {
        let s = single.trim().to_string();
        if s.is_empty() {
            Vec::new()
        } else {
            vec![s]
        }
    } else {
        Vec::new()
    };

    let (api_key_salt, api_key_nonce, api_key_cipher, api_keys_json) = if keep_existing_keys {
        let row = existing
            .as_ref()
            .ok_or_else(|| "新建供应商时至少需要一个 API Key".to_string())?;
        (
            row.api_key_salt.clone(),
            row.api_key_nonce.clone(),
            row.api_key_cipher.clone(),
            row.api_keys_json.clone(),
        )
    } else {
        if plain_keys.is_empty() && existing.is_none() {
            return Err("新建供应商时至少需要一个 API Key".to_string());
        }
        let encrypted: Vec<EncryptedKey> = plain_keys
            .iter()
            .map(|k| {
                crypto::encrypt_secret(&master, k).map(|es| EncryptedKey {
                    salt: es.salt,
                    nonce: es.nonce,
                    cipher: es.cipher,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let json = serde_json::to_string(&encrypted).unwrap_or_else(|_| "[]".into());
        if let Some(first) = encrypted.first() {
            (
                first.salt.clone(),
                first.nonce.clone(),
                first.cipher.clone(),
                json,
            )
        } else {
            (String::new(), String::new(), String::new(), json)
        }
    };

    let created_at = existing.as_ref().map(|r| r.created_at).unwrap_or(now);
    let is_new = existing.is_none();

    let row = AiProviderRow {
        id: id.clone(),
        name,
        provider_type,
        base_url,
        openai_base_url,
        api_key_salt,
        api_key_nonce,
        api_key_cipher,
        default_model,
        openai_default_model,
        models_json,
        notes,
        created_at,
        updated_at: now,
        sort_order: existing.as_ref().map(|r| r.sort_order).unwrap_or(0),
        api_keys_json,
        custom_models_json,
    };

    db::upsert_ai_provider_row(&row)?;

    let mut message = if is_new {
        "供应商已创建".to_string()
    } else {
        "供应商已更新".to_string()
    };

    // 更新路径：反向同步所有关联此供应商的环境。
    if !is_new {
        // 解密明文 API Key 用于同步写入。
        let plain_key = if !row.api_key_cipher.is_empty() {
            crypto::decrypt_secret(
                &master,
                &row.api_key_salt,
                &row.api_key_nonce,
                &row.api_key_cipher,
            )
            .unwrap_or_default()
        } else {
            String::new()
        };

        // 解析档位模型映射
        let tier_models: std::collections::HashMap<String, String> =
            serde_json::from_str(&row.models_json).unwrap_or_default();

        // 根据勾选的 API 类型决定写入 Claude 环境 / Codex 环境的值：
        // 勾了 anthropic → Anthropic 侧 URL/默认模型；勾了任一 openai → OpenAI 侧。
        let has_anthropic_row = provider_types_contain(&row.provider_type, API_TYPE_ANTHROPIC);
        let has_openai_row = provider_types_have_openai(&row.provider_type);
        let (claude_base, claude_model, codex_base, codex_model): (String, String, String, String) = (
            if has_anthropic_row {
                row.base_url.clone()
            } else {
                String::new()
            },
            if has_anthropic_row {
                row.default_model.clone()
            } else {
                String::new()
            },
            if has_openai_row {
                row.openai_base_url.clone()
            } else {
                String::new()
            },
            if has_openai_row {
                row.openai_default_model.clone()
            } else {
                String::new()
            },
        );

        let mut sync_parts: Vec<String> = Vec::new();

        if !claude_base.is_empty() {
            match crate::claude_env::sync_provider_to_envs(
                &row.id,
                &claude_base,
                &plain_key,
                &claude_model,
                &tier_models,
            ) {
                Ok((synced, errs)) if synced > 0 || !errs.is_empty() => {
                    let part = if errs.is_empty() {
                        format!("已同步 {} 个 Claude 环境", synced)
                    } else {
                        format!(
                            "已同步 {} 个 Claude 环境（失败: {}）",
                            synced,
                            errs.join("、")
                        )
                    };
                    sync_parts.push(part);
                }
                Ok(_) => {} // 无关联环境
                Err(e) => {
                    sync_parts.push(format!("Claude 环境同步失败: {}", e));
                }
            }
        }

        if !codex_base.is_empty() {
            match crate::codex_env::sync_provider_to_envs(
                &row.id,
                &codex_model,
                &codex_base,
                &plain_key,
            ) {
                Ok((synced, errs)) if synced > 0 || !errs.is_empty() => {
                    let part = if errs.is_empty() {
                        format!("已同步 {} 个 Codex 环境", synced)
                    } else {
                        format!(
                            "已同步 {} 个 Codex 环境（失败: {}）",
                            synced,
                            errs.join("、")
                        )
                    };
                    sync_parts.push(part);
                }
                Ok(_) => {}
                Err(e) => {
                    sync_parts.push(format!("Codex 环境同步失败: {}", e));
                }
            }
        }

        // 同步到绑定的 opencode / pi provider
        match db::load_agent_provider_bindings(&row.id) {
            Err(e) => {
                sync_parts.push(format!("加载 Agent 绑定失败: {}", e));
            }
            Ok(bindings) => {
                use crate::agent_model_config::ModelConfigAgent;
                use crate::opencode_config::{AgentModelConfigView, UpsertProviderPayload};

                // 预加载各 agent 的配置（每个 agent 只读一次文件，避免循环里重复读）
                let mut opencode_view: Option<Result<AgentModelConfigView, String>> = None;
                let mut pi_view: Option<Result<AgentModelConfigView, String>> = None;

                for binding in &bindings {
                    // 步骤 1：从缓存里找目标 provider，确定它的 api 格式
                    let target_api: Option<String> = match binding.agent_type.as_str() {
                        "opencode" => {
                            if opencode_view.is_none() {
                                opencode_view = Some(crate::opencode_config::get_config());
                            }
                            match opencode_view.as_ref().unwrap() {
                                Ok(view) => view
                                    .providers
                                    .iter()
                                    .find(|p| p.id == binding.agent_provider_id)
                                    .and_then(|p| p.api.clone()),
                                // 读配置失败：跳过这个绑定，不要硬同步
                                Err(_) => continue,
                            }
                        }
                        "pi" => {
                            if pi_view.is_none() {
                                pi_view =
                                    Some(crate::pi_model_config::get_config(ModelConfigAgent::Pi));
                            }
                            match pi_view.as_ref().unwrap() {
                                Ok(view) => view
                                    .providers
                                    .iter()
                                    .find(|p| p.id == binding.agent_provider_id)
                                    .and_then(|p| p.api.clone()),
                                Err(_) => continue,
                            }
                        }
                        other => {
                            sync_parts.push(format!("未知的 agent 类型: {}", other));
                            continue;
                        }
                    };

                    // 步骤 2：根据目标 provider 的 api 格式，从全局供应商选对应侧的 base_url
                    let base_url = match target_api.as_deref() {
                        Some("openai") => codex_base.clone(),
                        Some("anthropic") => claude_base.clone(),
                        // provider 不存在或 api 字段为空（新建场景）：
                        // 优先 anthropic 侧，否则走 openai 侧。
                        None => {
                            if has_anthropic_row {
                                claude_base.clone()
                            } else {
                                codex_base.clone()
                            }
                        }
                        // 其他 api 类型（google/azure 等）：不同步 base_url，只同步 api_key
                        _ => String::new(),
                    };

                    // 步骤 3：构造 payload，不传 api 字段（不覆盖用户已选的格式）
                    let payload = UpsertProviderPayload {
                        id: binding.agent_provider_id.clone(),
                        previous_id: None,
                        name: None,
                        npm: None,
                        api: None, // 不动用户已选的 api 格式
                        base_url: if base_url.is_empty() {
                            None
                        } else {
                            Some(base_url)
                        },
                        set_cache_key: None,
                        timeout: None,
                        chunk_timeout: None,
                        whitelist: None,
                        blacklist: None,
                        api_key: Some(plain_key.clone()),
                    };

                    let result = match binding.agent_type.as_str() {
                        "opencode" => crate::opencode_config::upsert_provider(payload)
                            .map(|_| "OpenCode".to_string()),
                        "pi" => {
                            crate::pi_model_config::upsert_provider(ModelConfigAgent::Pi, payload)
                                .map(|_| "Pi".to_string())
                        }
                        _ => unreachable!(),
                    };
                    match result {
                        Ok(label) => {
                            sync_parts
                                .push(format!("已同步 {} 的 {}", label, binding.agent_provider_id));
                        }
                        Err(e) => {
                            sync_parts.push(format!(
                                "{} 的 {} 同步失败: {}",
                                binding.agent_type, binding.agent_provider_id, e
                            ));
                        }
                    }
                }
            }
        }

        if !sync_parts.is_empty() {
            message.push_str("；");
            message.push_str(&sync_parts.join("；"));
        }
    }

    Ok(AiProviderActionResult {
        ok: true,
        message,
        provider: Some(AiProvider::from(row)),
    })
}

pub fn delete_provider(id: String) -> Result<AiProviderActionResult, String> {
    let id = id.trim().to_string();
    if db::get_ai_provider_row(&id)?.is_none() {
        return Err("供应商不存在".to_string());
    }
    // 清除关联此供应商的环境的 provider_id，防止悬挂引用。
    let _ = db::clear_provider_id_on_envs(&id);
    db::delete_ai_provider_row(&id)?;
    Ok(AiProviderActionResult {
        ok: true,
        message: "供应商已删除".to_string(),
        provider: None,
    })
}

/// 按需读取明文 API Key（编辑表单用）。列表接口永不调用此函数。
/// 返回第一个（主）Key，向后兼容。
pub fn get_provider_secret(id: String) -> Result<String, String> {
    let secrets = get_provider_secrets(id)?;
    Ok(secrets.into_iter().next().unwrap_or_default())
}

/// 按需读取全部明文 API Key（编辑表单用）。
/// 优先返回多 Key JSON 中的所有密钥；为空时回退到旧的单 Key 列。
pub fn get_provider_secrets(id: String) -> Result<Vec<String>, String> {
    let row = db::get_ai_provider_row(id.trim())?.ok_or_else(|| "供应商不存在".to_string())?;
    let master = config::load_secrets_key()?;
    // 优先解析多 Key JSON
    let enc_keys: Vec<EncryptedKey> = serde_json::from_str(&row.api_keys_json).unwrap_or_default();
    if !enc_keys.is_empty() {
        return enc_keys
            .iter()
            .map(|ek| crypto::decrypt_secret(&master, &ek.salt, &ek.nonce, &ek.cipher))
            .collect();
    }
    // 回退到旧的单 Key 列
    if row.api_key_cipher.is_empty() {
        return Ok(Vec::new());
    }
    let plain = crypto::decrypt_secret(
        &master,
        &row.api_key_salt,
        &row.api_key_nonce,
        &row.api_key_cipher,
    )?;
    Ok(vec![plain])
}

/// 批量更新供应商排序。`ids` 按目标顺序排列，索引即为新 sort_order。
pub fn reorder_providers(ids: Vec<String>) -> Result<(), String> {
    let orders: Vec<(String, i64)> = ids
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i as i64))
        .collect();
    db::reorder_ai_provider_rows(&orders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_custom_model_ids_prefers_alias_and_deduplicates() {
        let models = vec![
            CustomModel {
                model: "A1".to_string(),
                alias_id: "".to_string(),
            },
            CustomModel {
                model: "A2".to_string(),
                alias_id: "A1".to_string(),
            },
            CustomModel {
                model: "A3".to_string(),
                alias_id: " A1 ".to_string(),
            },
            CustomModel {
                model: " A4 ".to_string(),
                alias_id: "  ".to_string(),
            },
            CustomModel {
                model: "  ".to_string(),
                alias_id: "".to_string(),
            },
        ];

        assert_eq!(effective_custom_model_ids(models), vec!["A1", "A4"]);
    }

    fn unique_id(tag: &str) -> String {
        format!(
            "__agentbuddy_test_provider_{}_{}",
            tag,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn payload(id: &str, api_key: Option<&str>) -> AiProviderUpsertPayload {
        let mut models = HashMap::new();
        models.insert("haiku".to_string(), "claude-haiku-x".to_string());
        models.insert("bogus".to_string(), "should-be-dropped".to_string());
        models.insert("opus".to_string(), "   ".to_string());
        AiProviderUpsertPayload {
            id: id.to_string(),
            name: "测试供应商".to_string(),
            provider_types: vec![API_TYPE_ANTHROPIC.to_string()],
            base_url: "https://api.example.com".to_string(),
            openai_base_url: None,
            api_key: api_key.map(|s| s.to_string()),
            api_keys: None,
            default_model: Some("claude-sonnet-x".to_string()),
            openai_default_model: None,
            models: Some(models),
            custom_models: None,
            notes: Some("note".to_string()),
        }
    }

    #[test]
    fn parse_api_types_handles_new_and_legacy_values() {
        assert_eq!(
            parse_api_types("[\"anthropic\",\"openai_chat\"]"),
            vec!["anthropic", "openai_chat"]
        );
        // 去重 + 忽略非法项
        assert_eq!(
            parse_api_types("[\"openai_chat\",\"openai_chat\",\"bogus\"]"),
            vec!["openai_chat"]
        );
        // 旧版标量映射
        assert_eq!(parse_api_types("anthropic"), vec!["anthropic"]);
        assert_eq!(
            parse_api_types("openai"),
            vec!["openai_chat", "openai_responses"]
        );
        assert_eq!(
            parse_api_types("universal"),
            vec!["anthropic", "openai_chat", "openai_responses"]
        );
        assert!(parse_api_types("").is_empty());
        assert!(parse_api_types("gemini").is_empty());
    }

    #[test]
    fn provider_crud_and_secret_roundtrip() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("crud");

        // Create
        let created = upsert_provider(payload(&id, Some("sk-test-123"))).expect("create");
        assert!(created.ok);
        let p = created.provider.expect("provider");
        assert!(p.has_api_key);
        assert_eq!(p.provider_types, vec!["anthropic"]);
        assert_eq!(p.default_model, "claude-sonnet-x");
        // 只允许合法档位键、丢弃空值
        assert_eq!(p.models.len(), 1);
        assert_eq!(
            p.models.get("haiku").map(|s| s.as_str()),
            Some("claude-haiku-x")
        );

        // Secret roundtrip
        let secret = get_provider_secret(id.clone()).expect("secret");
        assert_eq!(secret, "sk-test-123");

        // Edit with empty apiKey keeps existing ciphertext
        let mut edit = payload(&id, None);
        edit.name = "改名".to_string();
        let updated = upsert_provider(edit).expect("update");
        let p2 = updated.provider.expect("provider2");
        assert_eq!(p2.name, "改名");
        assert_eq!(p2.created_at, p.created_at);
        assert!(p2.updated_at >= p.updated_at);
        let secret2 = get_provider_secret(id.clone()).expect("secret2");
        assert_eq!(secret2, "sk-test-123");

        // List never leaks key material, includes has_api_key
        let list = list_providers().expect("list");
        let item = list.iter().find(|x| x.id == id).expect("in list");
        assert!(item.has_api_key);

        // Delete
        delete_provider(id.clone()).expect("delete");
        assert!(db::get_ai_provider_row(&id).expect("get").is_none());
    }

    #[test]
    fn openai_only_provider_clears_tier_models() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("openai");
        let mut pl = payload(&id, Some("sk-openai"));
        pl.provider_types = vec!["openai_chat".into(), "openai_responses".into()];
        pl.base_url = String::new();
        pl.openai_base_url = Some("https://api.openai.com/v1".into());
        pl.default_model = None;
        pl.openai_default_model = Some("gpt-5".into());
        let created = upsert_provider(pl).expect("create");
        let p = created.provider.expect("provider");
        assert_eq!(p.provider_types, vec!["openai_chat", "openai_responses"]);
        assert!(p.models.is_empty());
        // 未勾选 anthropic：anthropic 侧字段被清空
        assert!(p.base_url.is_empty());
        assert!(p.default_model.is_empty());
        assert_eq!(p.openai_base_url, "https://api.openai.com/v1");
        assert_eq!(p.openai_default_model, "gpt-5");
        delete_provider(id).expect("cleanup");
    }

    #[test]
    fn multi_type_provider_keeps_both_sides() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("multi");
        let mut pl = payload(&id, Some("sk-multi"));
        pl.provider_types = vec![
            "anthropic".into(),
            "openai_chat".into(),
            "openai_responses".into(),
        ];
        pl.base_url = "https://gateway.example.com".to_string();
        pl.openai_base_url = Some("https://gateway.example.com/v1".to_string());
        pl.openai_default_model = Some("gpt-5".to_string());
        let created = upsert_provider(pl).expect("create");
        let p = created.provider.expect("provider");
        assert_eq!(
            p.provider_types,
            vec!["anthropic", "openai_chat", "openai_responses"]
        );
        assert_eq!(p.base_url, "https://gateway.example.com");
        assert_eq!(p.openai_base_url, "https://gateway.example.com/v1");
        assert_eq!(p.default_model, "claude-sonnet-x");
        assert_eq!(p.openai_default_model, "gpt-5");
        // 勾选了 anthropic：保留档位模型
        assert_eq!(
            p.models.get("haiku").map(|s| s.as_str()),
            Some("claude-haiku-x")
        );
        delete_provider(id).expect("cleanup");

        // 只勾选 anthropic 的供应商：openai 侧字段被清空
        let id2 = unique_id("multi-clear");
        let mut pl2 = payload(&id2, Some("sk-x"));
        pl2.openai_base_url = Some("https://should-be-cleared/v1".to_string());
        pl2.openai_default_model = Some("should-be-cleared".to_string());
        let created2 = upsert_provider(pl2).expect("create2");
        let p2 = created2.provider.expect("provider2");
        assert!(p2.openai_default_model.is_empty());
        assert!(p2.openai_base_url.is_empty());
        delete_provider(id2).expect("cleanup2");
    }

    #[test]
    fn create_requires_api_key_and_valid_url() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("validate");
        assert!(upsert_provider(payload(&id, None)).is_err());

        let mut bad_url = payload(&id, Some("sk-x"));
        bad_url.base_url = "ftp://nope".to_string();
        assert!(upsert_provider(bad_url).is_err());

        // 空类型集合被拒绝
        let mut bad_type = payload(&id, Some("sk-x"));
        bad_type.provider_types = vec![];
        assert!(upsert_provider(bad_type).is_err());

        // 只勾 openai 时，anthropic URL 可空但 openai URL 必须合法
        let mut openai_only = payload(&id, Some("sk-x"));
        openai_only.provider_types = vec!["openai_chat".into()];
        openai_only.base_url = String::new();
        openai_only.openai_base_url = Some("ftp://nope".into());
        assert!(upsert_provider(openai_only).is_err());
    }

    #[test]
    fn official_oauth_provider_id_is_reserved() {
        let mut pl = payload(crate::codex_env::OFFICIAL_OAUTH_PROVIDER_ID, Some("sk-x"));
        pl.provider_types = vec!["openai_chat".into()];
        pl.base_url = String::new();
        pl.openai_base_url = Some("https://api.openai.com/v1".into());
        assert!(upsert_provider(pl)
            .unwrap_err()
            .contains("保留给 Codex 官方 OAuth"));
    }
}
