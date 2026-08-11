//! Provider router — selects providers for a route group and manages circuit breakers.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::circuit_breaker::CircuitBreaker;
use super::types::{ProviderRouteStatus, RouteGroup, RouteProvider};
use super::CircuitBreakerSnapshot;

/// ProviderRouter manages the provider pool and circuit breaker state.
pub struct ProviderRouter {
    /// Provider pools: (group) → list of providers (with decrypted API keys).
    pools: RwLock<HashMap<RouteGroup, Vec<RouteProvider>>>,
    /// Circuit breakers: (provider_id, group) → breaker.
    breakers: RwLock<HashMap<(String, RouteGroup), Arc<CircuitBreaker>>>,
}

impl ProviderRouter {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            breakers: RwLock::new(HashMap::new()),
        }
    }

    /// Refresh the provider pool for an API format from the database.
    /// Loads AI providers, filters by type compatibility, decrypts API keys,
    /// and merges with the unified provider toggle settings.
    pub async fn refresh_pool(&self, group: RouteGroup) -> Result<usize, String> {
        let provider_rows = crate::db::load_ai_provider_rows()?;
        let toggles = crate::db::load_provider_route_toggles()?;

        let mut providers: Vec<RouteProvider> = Vec::new();

        for row in &provider_rows {
            // Filter by provider type compatibility for this API format
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
            let model_ids: Vec<String> = serde_json::from_str::<
                Vec<crate::ai_provider::CustomModel>,
            >(&row.custom_models_json)
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                if m.alias_id.trim().is_empty() {
                    m.model
                } else {
                    m.alias_id
                }
            })
            .filter(|id| !id.trim().is_empty())
            .collect();

            // For universal type in Codex group, use the derived OpenAI base URL
            let base_url = if row.provider_type == crate::ai_provider::TYPE_UNIVERSAL
                && group == RouteGroup::Codex
            {
                crate::ai_provider::derive_openai_base_url(&row.provider_type, &row.base_url)
            } else {
                row.base_url.clone()
            };

            providers.push(RouteProvider {
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

        // Sort by sort_order
        providers.sort_by_key(|p| p.sort_order);

        let count = providers.len();
        let mut pools = self.pools.write().await;
        pools.insert(group, providers);

        // Ensure circuit breakers exist for all providers
        let mut breakers = self.breakers.write().await;
        let pool = pools.get(&group).unwrap();
        for p in pool {
            let key = (p.id.clone(), group);
            breakers.entry(key).or_insert_with(|| Arc::new(CircuitBreaker::new()));
        }

        Ok(count)
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

    /// Get merged info for all enabled providers across both API formats:
    /// (id, name, custom_model_ids, fetch_base_url, api_key).
    /// Codex-format entries are visited first so universal providers expose
    /// their OpenAI-style base URL (used for remote /v1/models fetching).
    pub async fn get_enabled_provider_model_infos(
        &self,
    ) -> Vec<(String, String, Vec<String>, String, String)> {
        let pools = self.pools.read().await;
        let mut result: Vec<(String, String, Vec<String>, String, String)> = Vec::new();
        for group in [RouteGroup::Codex, RouteGroup::ClaudeCode] {
            let pool = match pools.get(&group) {
                Some(p) => p,
                None => continue,
            };
            for p in pool.iter().filter(|p| p.enabled) {
                if !result.iter().any(|(id, _, _, _, _)| *id == p.id) {
                    result.push((
                        p.id.clone(),
                        p.name.clone(),
                        p.model_ids.clone(),
                        p.base_url.clone(),
                        p.api_key.clone(),
                    ));
                }
            }
        }
        result
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
