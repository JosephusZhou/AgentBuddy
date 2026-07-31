//! Data structures for route aggregation (DTOs + internal types).

use serde::{Deserialize, Serialize};

/// Route group type — determines which API paths and cloaking strategy to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteGroup {
    ClaudeCode,
    Codex,
}

impl RouteGroup {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteGroup::ClaudeCode => "claude_code",
            RouteGroup::Codex => "codex",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "claude_code" => Some(RouteGroup::ClaudeCode),
            "codex" => Some(RouteGroup::Codex),
            _ => None,
        }
    }
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
    pub claude_code: GroupStatus,
    pub codex: GroupStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupStatus {
    pub enabled: bool,
    pub active_providers: Vec<ProviderRouteStatus>,
    pub total_providers: usize,
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

/// Provider toggle in a route group (persisted in SQLite).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRouteToggle {
    pub provider_id: String,
    pub group: String, // "claude_code" | "codex"
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
    /// Whether this provider is enabled in this route group.
    pub enabled: bool,
    pub sort_order: i32,
}

/// Result of a forwarding attempt for one provider.
#[derive(Debug)]
pub struct ForwardAttempt {
    pub provider_id: String,
    pub provider_name: String,
    pub success: bool,
    pub error: Option<String>,
    pub status_code: Option<u16>,
}
