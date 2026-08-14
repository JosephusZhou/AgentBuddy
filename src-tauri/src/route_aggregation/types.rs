//! Data structures for route aggregation (DTOs + internal types).

use serde::{Deserialize, Serialize};

/// Upstream protocol used by a route group. Route aggregation only supports
/// same-protocol passthrough; this is intentionally not a protocol registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderFormat {
    Anthropic,
    OpenAiResponses,
}

/// API format — determines which API paths and cloaking strategy to use.
/// Internal only: the aggregated endpoint always serves both formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteGroup {
    ClaudeCode,
    Codex,
}

impl RouteGroup {
    pub const ALL: [RouteGroup; 2] = [RouteGroup::ClaudeCode, RouteGroup::Codex];
}

/// Cloaking mode for Claude Code rectifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloakingMode {
    Auto,
    Always,
    Never,
}

impl Default for CloakingMode {
    fn default() -> Self {
        CloakingMode::Auto
    }
}

/// Route aggregation runtime status (returned to frontend).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAggregationStatus {
    pub server_running: bool,
    pub listen_address: String,
    pub listen_port: u16,
    /// Merged provider statuses across both API formats.
    pub providers: Vec<ProviderRouteStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRouteStatus {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub circuit_state: String, // "closed" | "open" | "half_open"
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
    pub request_count: u64,
    pub success_count: u64,
}

/// Provider toggle (persisted in SQLite, unified across both API formats).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRouteToggle {
    pub provider_id: String,
    pub enabled: bool,
    pub sort_order: i32,
}

/// Internal representation of a provider in a route group's active pool.
#[derive(Debug, Clone)]
pub struct RouteProvider {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    /// Decrypted API key (never exposed via Tauri commands).
    pub api_key: String,
    /// Effective model IDs of this provider (custom models' alias/ID, if any).
    /// Empty means the provider has no custom model list.
    pub model_ids: Vec<String>,
    /// Whether this provider is enabled.
    pub enabled: bool,
    /// Known supported model IDs used to short-circuit the failover pool when a
    /// request names a model none of the providers actually offer. Populated at
    /// `refresh_pool_fast` time:
    /// - If `model_ids` is non-empty (custom list), this is `Some(model_ids)`
    ///   and the provider participates in failover unfiltered iff the model
    ///   is in the list.
    /// - Empty list → `None` (no filter, every request is allowed).
    pub supported_model_ids: Option<Vec<String>>,
}
