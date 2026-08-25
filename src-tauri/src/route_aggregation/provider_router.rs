//! Provider router — selects providers for a route group and manages circuit breakers.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::circuit_breaker::CircuitBreaker;
use super::types::{ProviderRouteStatus, RouteGroup, RouteProvider};
use super::CircuitBreakerSnapshot;

/// Claude Code 的 1M 上下文模型语法后缀。
///
/// 客户端配置 `claude-opus-5[1m]` 时，Claude Code 会**剥掉**该后缀并改发
/// `context-1m-2025-08-07` beta 头 + 裸模型名（8/24 日志证实）。而中转渠道
/// 通常以完整变体 ID（`claude-opus-5[1m]`）声明该能力，因此供应商解析层
/// 需要做 `model → model[1m]` 的变体匹配并在转发时回写完整 ID。
pub const CONTEXT_1M_SUFFIX: &str = "[1m]";

/// ProviderRouter manages the provider pool and circuit breaker state.
pub struct ProviderRouter {
    /// Provider pools: (group) → list of providers (with decrypted API keys).
    pools: RwLock<HashMap<RouteGroup, Vec<RouteProvider>>>,
    /// Circuit breakers: (provider_id, group) → breaker.
    breakers: RwLock<HashMap<(String, RouteGroup), Arc<CircuitBreaker>>>,
}

/// Intermediate row used while building a `RouteProvider` from the DB row.
/// Keeps DB parsing and route-provider construction separate.
struct ProviderBuild {
    id: String,
    name: String,
    provider_type: String,
    base_url: String,
    api_key: String,
    model_ids: Vec<String>,
    enabled: bool,
    sort_order: i32,
}

impl ProviderRouter {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            breakers: RwLock::new(HashMap::new()),
        }
    }

    /// Refresh the provider pool from DB rows. Every provider's
    /// `supported_model_ids` is filled synchronously from
    /// `custom_models_json` (non-empty → `Some(list)`, empty → `None`
    /// to allow manually specified model IDs).
    ///
    /// 适用于热路径：启动、状态轮询、provider toggle、路由聚合重启等。
    pub async fn refresh_pool_fast(&self, group: RouteGroup) -> Result<usize, String> {
        let providers = self.build_pool_from_db(group).await?;
        let count = providers.len();
        let mut pools = self.pools.write().await;
        pools.insert(group, providers);
        let mut breakers = self.breakers.write().await;
        let pool = pools.get(&group).unwrap();
        for p in pool {
            let key = (p.id.clone(), group);
            breakers
                .entry(key)
                .or_insert_with(|| Arc::new(CircuitBreaker::new()));
        }
        Ok(count)
    }

    /// Build the provider list for a group from the DB, decrypting API keys
    /// and applying toggles. `supported_model_ids` is filled synchronously from
    /// `custom_models_json` (non-empty → `Some(list)`, empty → `None`).
    async fn build_pool_from_db(&self, group: RouteGroup) -> Result<Vec<RouteProvider>, String> {
        let provider_rows = crate::db::load_ai_provider_rows()?;
        let toggles = crate::db::load_provider_route_toggles()?;

        let mut builds: Vec<ProviderBuild> = Vec::new();

        for row in &provider_rows {
            // Filter by provider type compatibility for this API format.
            // Phase 5+：路由聚合只接受 Anthropic / OpenAI / Universal 三类 backend。
            let matches = match group {
                RouteGroup::ClaudeCode => {
                    row.provider_type == crate::ai_provider::TYPE_ANTHROPIC
                        || row.provider_type == crate::ai_provider::TYPE_UNIVERSAL
                }
                RouteGroup::Codex => {
                    row.provider_type == crate::ai_provider::TYPE_OPENAI
                        || row.provider_type == crate::ai_provider::TYPE_UNIVERSAL
                }
            };
            if !matches {
                continue;
            }

            // Decrypt API key
            let api_key = if row.api_key_cipher.is_empty() {
                String::new()
            } else {
                let master_key = match crate::config::load_secrets_key() {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!(
                            "[route-aggregation] failed to load secrets key: {}, skipping provider {}",
                            e, row.name
                        );
                        continue;
                    }
                };
                match crate::crypto::decrypt_secret(
                    &master_key,
                    &row.api_key_salt,
                    &row.api_key_nonce,
                    &row.api_key_cipher,
                ) {
                    Ok(key) => key,
                    Err(_) => {
                        eprintln!(
                            "[route-aggregation] failed to decrypt API key for provider {}, skipping",
                            row.name
                        );
                        continue;
                    }
                }
            };

            // Find toggle for this provider
            let toggle = toggles.iter().find(|t| t.provider_id == row.id);
            let (enabled, sort_order) = match toggle {
                Some(t) => (t.enabled, t.sort_order),
                None => (true, row.sort_order as i32), // default enabled
            };

            // Effective model IDs from the provider's custom model list.
            // Each entry is {model, aliasId}; aliasId takes precedence.
            let model_ids = crate::ai_provider::effective_custom_model_ids(
                serde_json::from_str::<Vec<crate::ai_provider::CustomModel>>(
                    &row.custom_models_json,
                )
                .unwrap_or_default(),
            );

            // For universal type in Codex group, append /v1 so the upstream OpenAI Responses
            // endpoint at `{base}/v1/responses` is reachable. In ClaudeCode group the
            // raw Anthropic base URL is used (Anthropic Messages endpoint already
            // lives at `{base}/v1/messages`).
            let base_url = if row.provider_type == crate::ai_provider::TYPE_UNIVERSAL
                && group == RouteGroup::Codex
            {
                crate::ai_provider::derive_openai_base_url(&row.provider_type, &row.base_url)
            } else {
                row.base_url.clone()
            };

            builds.push(ProviderBuild {
                id: row.id.clone(),
                name: row.name.clone(),
                provider_type: row.provider_type.clone(),
                base_url,
                api_key,
                model_ids,
                enabled,
                sort_order,
            });
        }

        // Sort by sort_order so the in-memory order matches the user's intent.
        builds.sort_by_key(|b| b.sort_order);

        let providers: Vec<RouteProvider> = builds
            .iter()
            .map(|b| RouteProvider {
                id: b.id.clone(),
                name: b.name.clone(),
                provider_type: b.provider_type.clone(),
                base_url: b.base_url.clone(),
                api_key: b.api_key.clone(),
                model_ids: b.model_ids.clone(),
                enabled: b.enabled,
                // 模型列表唯一来源：用户在 AI 供应商编辑页配置的 `custom_models_json`。
                // 非空时直接作为 supported_model_ids；空列表保持 None（failover 不按模型过滤）。
                supported_model_ids: if b.model_ids.is_empty() {
                    None
                } else {
                    Some(b.model_ids.clone())
                },
            })
            .collect();

        Ok(providers)
    }

    /// Select providers for forwarding. If failover is enabled, returns all enabled
    /// providers (skipping open-circuit ones). If not, returns only the first.
    pub async fn select_providers(
        &self,
        group: RouteGroup,
        auto_failover: bool,
    ) -> Vec<RouteProvider> {
        let pools = self.pools.read().await;
        let pool = match pools.get(&group) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let enabled: Vec<&RouteProvider> = pool.iter().filter(|p| p.enabled).collect();
        if enabled.is_empty() {
            return Vec::new();
        }

        if !auto_failover {
            // Return only the first provider
            return vec![enabled[0].clone()];
        }

        // With failover: return all enabled providers whose circuit breaker allows
        let mut result = Vec::new();
        for p in enabled.iter().copied() {
            let breakers = self.breakers.read().await;
            let key = (p.id.clone(), group);
            if let Some(breaker) = breakers.get(&key) {
                if breaker.can_attempt().await {
                    result.push(p.clone());
                }
            } else {
                // No breaker yet — allow
                result.push(p.clone());
            }
        }
        result
    }

    /// Resolve the candidate provider list against the requested model.
    ///
    /// 返回 `(provider, upstream_model_override)`：override 为 `Some` 时调用方
    /// 必须在转发前把 body 的 model 重写为该值。
    ///
    /// 匹配规则：
    /// - `model = None`: 不过滤（用于无模型请求）。
    /// - Provider 无自定义模型列表（`supported_model_ids = None`）：直接保留。
    /// - 精确命中请求模型：保留，原样转发。
    /// - 命中 `[1m]` 变体（见 CONTEXT_1M_SUFFIX 说明）：保留并返回 override，
    ///   转发时重写为中转声明的完整变体 ID，否则只声明变体渠道的中转无法路由。
    /// - 其余：剔除，避免浪费 round-trip。
    ///
    /// 排序保持不变（沿用 build_pool_from_db 的 sort_order）。
    pub fn resolve_providers_for_model(
        providers: Vec<RouteProvider>,
        model: Option<&str>,
    ) -> Vec<(RouteProvider, Option<String>)> {
        let Some(model) = model else {
            return providers.into_iter().map(|p| (p, None)).collect();
        };
        let variant = format!("{model}{CONTEXT_1M_SUFFIX}");
        providers
            .into_iter()
            .filter_map(|p| match &p.supported_model_ids {
                None => Some((p, None)),
                Some(ids) => {
                    if ids.iter().any(|id| id == model) {
                        Some((p, None))
                    } else if ids.iter().any(|id| id == &variant) {
                        Some((p, Some(variant.clone())))
                    } else {
                        None
                    }
                }
            })
            .collect()
    }

    /// Record a successful request for a provider.
    pub async fn record_success(&self, provider_id: &str, group: RouteGroup) {
        let breakers = self.breakers.read().await;
        let key = (provider_id.to_string(), group);
        if let Some(breaker) = breakers.get(&key) {
            breaker.record_success().await;
        }
    }

    /// Record a failed request for a provider.
    pub async fn record_failure(&self, provider_id: &str, group: RouteGroup, error: &str) {
        let breakers = self.breakers.read().await;
        let key = (provider_id.to_string(), group);
        if let Some(breaker) = breakers.get(&key) {
            breaker.record_failure(error).await;
        }
    }

    /// Reset a provider's circuit breaker.
    pub async fn reset_breaker(&self, provider_id: &str, group: RouteGroup) {
        let breakers = self.breakers.read().await;
        let key = (provider_id.to_string(), group);
        if let Some(breaker) = breakers.get(&key) {
            breaker.reset().await;
        }
    }

    /// Get the union of custom model IDs for all enabled providers.
    pub async fn get_enabled_model_ids(&self) -> Vec<String> {
        let pools = self.pools.read().await;
        let mut model_ids = std::collections::BTreeSet::new();
        // 合并两个入口的启用 provider 模型；BTreeSet 自动去重。
        for group in [RouteGroup::Codex, RouteGroup::ClaudeCode] {
            let pool = match pools.get(&group) {
                Some(p) => p,
                None => continue,
            };
            for p in pool.iter().filter(|p| p.enabled) {
                model_ids.extend(p.model_ids.iter().cloned());
            }
        }
        model_ids.into_iter().collect()
    }

    /// Get status snapshots for all providers in a group.
    pub async fn get_provider_statuses(&self, group: RouteGroup) -> Vec<ProviderRouteStatus> {
        let pools = self.pools.read().await;
        let breakers = self.breakers.read().await;

        let pool = match pools.get(&group) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        for p in pool {
            let key = (p.id.clone(), group);
            let snap = if let Some(b) = breakers.get(&key) {
                b.snapshot().await
            } else {
                CircuitBreakerSnapshot {
                    state: "closed".to_string(),
                    consecutive_failures: 0,
                    request_count: 0,
                    success_count: 0,
                    last_error: None,
                    last_error_at: None,
                }
            };

            result.push(ProviderRouteStatus {
                id: p.id.clone(),
                name: p.name.clone(),
                provider_type: p.provider_type.clone(),
                enabled: p.enabled,
                circuit_state: snap.state,
                consecutive_failures: snap.consecutive_failures,
                last_error: snap.last_error,
                last_error_at: snap.last_error_at,
                request_count: snap.request_count,
                success_count: snap.success_count,
            });
        }
        result
    }

    /// Get merged status snapshots across both API formats (for the UI).
    /// A provider appears once; circuit state is the worst of both formats,
    /// counters are summed, and the most recent error is kept.
    pub async fn get_merged_statuses(&self) -> Vec<ProviderRouteStatus> {
        let cc = self.get_provider_statuses(RouteGroup::ClaudeCode).await;
        let codex = self.get_provider_statuses(RouteGroup::Codex).await;

        let rank = |state: &str| -> u8 {
            match state {
                "open" => 2,
                "half_open" => 1,
                _ => 0,
            }
        };

        let mut merged: Vec<ProviderRouteStatus> = cc;
        for item in codex {
            match merged.iter_mut().find(|m| m.id == item.id) {
                Some(existing) => {
                    if rank(&item.circuit_state) > rank(&existing.circuit_state) {
                        existing.circuit_state = item.circuit_state;
                    }
                    existing.consecutive_failures =
                        existing.consecutive_failures.max(item.consecutive_failures);
                    existing.request_count += item.request_count;
                    existing.success_count += item.success_count;
                    let newer = match (existing.last_error_at, item.last_error_at) {
                        (_, None) => false,
                        (None, Some(_)) => true,
                        (Some(a), Some(b)) => b > a,
                    };
                    if newer {
                        existing.last_error = item.last_error;
                        existing.last_error_at = item.last_error_at;
                    }
                }
                None => merged.push(item),
            }
        }
        merged
    }
}

impl Default for ProviderRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::super::types::RouteProvider;
    use super::{ProviderRouter, CONTEXT_1M_SUFFIX};

    fn resolve(
        providers: Vec<RouteProvider>,
        model: Option<&str>,
    ) -> Vec<(RouteProvider, Option<String>)> {
        ProviderRouter::resolve_providers_for_model(providers, model)
    }

    fn provider(id: &str, models: Option<&[&str]>) -> RouteProvider {
        RouteProvider {
            id: id.into(),
            name: id.into(),
            provider_type: crate::ai_provider::TYPE_ANTHROPIC.into(),
            base_url: "https://relay.test".into(),
            api_key: String::new(),
            model_ids: Vec::new(),
            enabled: true,
            supported_model_ids: models.map(|ms| ms.iter().map(|m| m.to_string()).collect()),
        }
    }

    #[test]
    fn no_model_keeps_all_providers_without_override() {
        let providers = vec![provider("a", Some(&["m1"])), provider("b", None)];
        let resolved = resolve(providers, None);
        assert_eq!(resolved.len(), 2);
        assert!(resolved.iter().all(|(_, o)| o.is_none()));
    }

    #[test]
    fn exact_match_forwards_model_verbatim() {
        let providers = vec![provider("a", Some(&["claude-opus-5"]))];
        let resolved = resolve(providers, Some("claude-opus-5"));
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0.id, "a");
        assert!(resolved[0].1.is_none());
    }

    #[test]
    fn context_1m_variant_matches_and_rewrites_upstream_model() {
        // CC 发裸名（已把 [1m] 转为 beta 头），供应商只声明 [1m] 变体：
        // 必须命中并回写完整变体 ID，否则中转无法路由到 1M 渠道。
        let providers = vec![provider("any", Some(&["claude-opus-5[1m]"]))];
        let resolved = resolve(providers, Some("claude-opus-5"));
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0.id, "any");
        assert_eq!(
            resolved[0].1.as_deref(),
            Some(format!("claude-opus-5{}", CONTEXT_1M_SUFFIX).as_str())
        );
    }

    #[test]
    fn providers_without_matching_models_are_dropped() {
        let providers = vec![
            provider("x", Some(&["other-model"])),
            provider("y", Some(&["claude-opus-4"])),
        ];
        let resolved = resolve(providers, Some("claude-opus-5"));
        assert!(resolved.is_empty());
    }

    #[test]
    fn no_custom_list_passes_through_unfiltered() {
        let providers = vec![provider("b", None)];
        let resolved = resolve(providers, Some("anything"));
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].1.is_none());
    }

    #[test]
    fn resolver_is_associated_function() {
        // 防止误改为需要实例的方法：纯函数语义，供 forwarder 直接调用。
        let _ = ProviderRouter::resolve_providers_for_model;
    }
}
