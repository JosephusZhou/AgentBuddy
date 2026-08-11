//! Data structures for route aggregation (DTOs + internal types).

use serde::{Deserialize, Serialize};

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
    pub sort_order: i32,
}

/// Result of a forwarding attempt for one provider.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ForwardAttempt {
    pub provider_id: String,
    pub provider_name: String,
    pub success: bool,
    pub error: Option<String>,
    pub status_code: Option<u16>,
}
