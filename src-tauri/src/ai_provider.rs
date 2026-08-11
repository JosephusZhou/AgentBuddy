//! AI provider (上游模型供应商) registry: encrypted API key storage + CRUD.
//!
//! 独立的供应商库：类型（anthropic / openai）+ Base URL + API Key + 默认模型；
//! Anthropic 类型额外支持按档位（haiku/sonnet/opus/fable）覆盖模型。
//! 本期仅做管理，不做下发到 Agent/环境（后续可扩展为一键填充）。

use crate::config;
use crate::crypto;
use crate::db;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TYPE_ANTHROPIC: &str = "anthropic";
pub const TYPE_OPENAI: &str = "openai";
/// 通用类型：同时配置 Anthropic 与 OpenAI 两种接入；OpenAI Base URL 由
/// Anthropic Base URL 自动派生（追加 /v1），并同时持有两套默认模型。
pub const TYPE_UNIVERSAL: &str = "universal";

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

/// Public list item — never includes API key material.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    /// 通用类型下由 base_url 派生（追加 /v1）；其他类型恒为空串。
    pub openai_base_url: String,
    pub default_model: String,
    /// 通用类型的 OpenAI 默认模型；其他类型恒为空串。
    pub openai_default_model: String,
    /// Anthropic 档位模型覆盖；OpenAI 类型恒为空 map。
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
    pub provider_type: String,
    pub base_url: String,
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

/// 通用类型下由 Anthropic Base URL 派生 OpenAI 形式 Base URL（追加 /v1）。
pub fn derive_openai_base_url(provider_type: &str, base_url: &str) -> String {
    if provider_type != TYPE_UNIVERSAL {
        return String::new();
    }
    format!("{}/v1", base_url.trim_end_matches('/'))
}

impl From<AiProviderRow> for AiProvider {
    fn from(row: AiProviderRow) -> Self {
        let models: HashMap<String, String> =
            serde_json::from_str(&row.models_json).unwrap_or_default();
        let openai_base_url = derive_openai_base_url(&row.provider_type, &row.base_url);
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
            provider_type: row.provider_type,
            base_url: row.base_url,
            openai_base_url,
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
    pub provider_type: String,
    pub base_url: String,
    /// Required on create; empty/omitted on edit keeps existing ciphertext.
    /// （旧字段，仅单个 Key；优先使用 `api_keys`。）
    pub api_key: Option<String>,
    /// 多 API Key（明文数组）；Some 时替换全部密钥，None 时保持旧密钥。
    pub api_keys: Option<Vec<String>>,
    pub default_model: Option<String>,
    /// 通用类型的 OpenAI 默认模型；其他类型忽略并清空。
    pub openai_default_model: Option<String>,
    /// Anthropic 档位模型覆盖（haiku/sonnet/opus/fable）；anthropic/universal 有效。
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

fn normalize_provider_type(raw: &str) -> Result<String, String> {
    let t = raw.trim().to_lowercase();
    match t.as_str() {
        TYPE_ANTHROPIC => Ok(TYPE_ANTHROPIC.to_string()),
        TYPE_OPENAI => Ok(TYPE_OPENAI.to_string()),
        TYPE_UNIVERSAL => Ok(TYPE_UNIVERSAL.to_string()),
        _ => Err("供应商类型仅支持 anthropic、openai 或 universal".to_string()),
    }
}

/// 过滤档位模型映射：仅保留允许的档位键、去除空值；仅 anthropic/universal 保留。
fn normalize_models(provider_type: &str, models: &HashMap<String, String>) -> HashMap<String, String> {
    if provider_type == TYPE_OPENAI {
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
    Ok(rows.into_iter().map(AiProvider::from).collect())
}

pub fn upsert_provider(payload: AiProviderUpsertPayload) -> Result<AiProviderActionResult, String> {
    let id = payload.id.trim().to_string();
    let name = payload.name.trim().to_string();
    let provider_type = normalize_provider_type(&payload.provider_type)?;
    let base_url = payload.base_url.trim().to_string();
    let default_model = payload
        .default_model
        .unwrap_or_default()
        .trim()
        .to_string();
    // OpenAI 默认模型仅通用类型保留
    let openai_default_model = if provider_type == TYPE_UNIVERSAL {
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
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    validate_base_url(&base_url)?;

    let empty_models = HashMap::new();
    let models = normalize_models(&provider_type, payload.models.as_ref().unwrap_or(&empty_models));
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
        if s.is_empty() { Vec::new() } else { vec![s] }
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
            (first.salt.clone(), first.nonce.clone(), first.cipher.clone(), json)
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

        // 根据 provider_type 决定写入 Claude 环境 / Codex 环境的值。
        let (claude_base, claude_model, codex_base, codex_model): (String, String, String, String) = match row.provider_type.as_str() {
            TYPE_UNIVERSAL => {
                let openai_base = derive_openai_base_url(&row.provider_type, &row.base_url);
                (
                    row.base_url.clone(),
                    row.default_model.clone(),
                    openai_base,
                    row.openai_default_model.clone(),
                )
            }
            TYPE_ANTHROPIC => (
                row.base_url.clone(),
                row.default_model.clone(),
                String::new(),
                String::new(),
            ),
            TYPE_OPENAI => (
                String::new(),
                String::new(),
                row.base_url.clone(),
                row.default_model.clone(),
            ),
            _ => (String::new(), String::new(), String::new(), String::new()),
        };

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
                        format!("已同步 {} 个 Claude 环境（失败: {}）", synced, errs.join("、"))
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
                        format!("已同步 {} 个 Codex 环境（失败: {}）", synced, errs.join("、"))
                    };
                    sync_parts.push(part);
                }
                Ok(_) => {}
                Err(e) => {
                    sync_parts.push(format!("Codex 环境同步失败: {}", e));
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
    let row = db::get_ai_provider_row(id.trim())?
        .ok_or_else(|| "供应商不存在".to_string())?;
    let master = config::load_secrets_key()?;
    // 优先解析多 Key JSON
    let enc_keys: Vec<EncryptedKey> =
        serde_json::from_str(&row.api_keys_json).unwrap_or_default();
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
            provider_type: "anthropic".to_string(),
            base_url: "https://api.example.com".to_string(),
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
    fn provider_crud_and_secret_roundtrip() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("crud");

        // Create
        let created = upsert_provider(payload(&id, Some("sk-test-123"))).expect("create");
        assert!(created.ok);
        let p = created.provider.expect("provider");
        assert!(p.has_api_key);
        assert_eq!(p.provider_type, "anthropic");
        assert_eq!(p.default_model, "claude-sonnet-x");
        // 只允许合法档位键、丢弃空值
        assert_eq!(p.models.len(), 1);
        assert_eq!(p.models.get("haiku").map(|s| s.as_str()), Some("claude-haiku-x"));

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
    fn openai_type_clears_tier_models() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("openai");
        let mut pl = payload(&id, Some("sk-openai"));
        pl.provider_type = "openai".to_string();
        let created = upsert_provider(pl).expect("create");
        let p = created.provider.expect("provider");
        assert_eq!(p.provider_type, "openai");
        assert!(p.models.is_empty());
        delete_provider(id).expect("cleanup");
    }

    #[test]
    fn universal_type_derives_openai_url_and_keeps_both_models() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = unique_id("universal");
        let mut pl = payload(&id, Some("sk-universal"));
        pl.provider_type = "universal".to_string();
        pl.base_url = "https://gateway.example.com/".to_string();
        pl.openai_default_model = Some("gpt-5".to_string());
        let created = upsert_provider(pl).expect("create");
        let p = created.provider.expect("provider");
        assert_eq!(p.provider_type, "universal");
        // 尾部斜杠被规范化后追加 /v1
        assert_eq!(p.openai_base_url, "https://gateway.example.com/v1");
        assert_eq!(p.default_model, "claude-sonnet-x");
        assert_eq!(p.openai_default_model, "gpt-5");
        // 通用类型保留 Anthropic 档位模型
        assert_eq!(p.models.get("haiku").map(|s| s.as_str()), Some("claude-haiku-x"));
        delete_provider(id).expect("cleanup");

        // 非通用类型的 openai_default_model 被清空
        let id2 = unique_id("universal-clear");
        let mut pl2 = payload(&id2, Some("sk-x"));
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

        let mut bad_type = payload(&id, Some("sk-x"));
        bad_type.provider_type = "gemini".to_string();
        assert!(upsert_provider(bad_type).is_err());
    }
}
