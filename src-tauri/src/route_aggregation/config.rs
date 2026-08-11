//! RouteAggregationConfig — stored in config.json under the `routeAggregation` key.

use serde::{Deserialize, Serialize};

/// Route aggregation configuration (persisted in config.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAggregationConfig {
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_true")]
    pub auto_failover: bool,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_stream_first_byte_timeout")]
    pub stream_first_byte_timeout: u64,
    #[serde(default = "default_stream_idle_timeout")]
    pub stream_idle_timeout: u64,
    #[serde(default = "default_non_stream_total_timeout")]
    pub non_stream_total_timeout: u64,
    #[serde(default)]
    pub cloaking_mode: super::CloakingMode,
    #[serde(default = "default_claude_code_version")]
    pub claude_code_version: String,
    #[serde(default = "default_codex_version")]
    pub codex_version: String,
    /// Endpoint API keys. The first key (index 0) is the primary key and can
    /// only be regenerated, never deleted; additional keys can be deleted.
    /// Empty list = no authentication required.
    #[serde(default)]
    pub api_keys: Vec<String>,
    /// Whether the proxy server should auto-start when the app launches.
    /// Kept in sync with the server's start/stop actions.
    #[serde(default)]
    pub auto_start: bool,
}

fn default_listen_address() -> String {
    "127.0.0.1".to_string()
}

fn default_listen_port() -> u16 {
    16888
}

fn default_true() -> bool {
    true
}

fn default_max_retries() -> u32 {
    3
}

fn default_stream_first_byte_timeout() -> u64 {
    60
}

fn default_stream_idle_timeout() -> u64 {
    120
}

fn default_non_stream_total_timeout() -> u64 {
    600
}

fn default_claude_code_version() -> String {
    "2.1.63".to_string()
}

fn default_codex_version() -> String {
    "0.146.0".to_string()
}

impl Default for RouteAggregationConfig {
    fn default() -> Self {
        Self {
            listen_address: default_listen_address(),
            listen_port: default_listen_port(),
            auto_failover: true,
            max_retries: 3,
            stream_first_byte_timeout: 60,
            stream_idle_timeout: 120,
            non_stream_total_timeout: 600,
            cloaking_mode: super::CloakingMode::Auto,
            claude_code_version: default_claude_code_version(),
            codex_version: default_codex_version(),
            api_keys: Vec::new(),
            auto_start: false,
        }
    }
}

/// Generate a random endpoint API key: `sk-` + 48 hex chars.
pub fn generate_api_key() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 24]; // 24 bytes → 48 hex chars
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("sk-{}", hex)
}

/// Load route aggregation config from config.json.
pub fn load_config() -> Result<super::RouteAggregationConfig, String> {
    let app_config = crate::config::load_app_config()?;
    Ok(app_config.route_aggregation)
}

/// Save route aggregation config to config.json.
pub fn save_config(config: &super::RouteAggregationConfig) -> Result<(), String> {
    crate::config::save_route_aggregation_config(config)
}

/// Normalize + validate config before write.
pub fn normalize_config(mut config: super::RouteAggregationConfig) -> Result<super::RouteAggregationConfig, String> {
    config.listen_address = config.listen_address.trim().to_string();
    if config.listen_address.is_empty() {
        config.listen_address = default_listen_address();
    }
    if config.listen_port == 0 {
        config.listen_port = default_listen_port();
    }
    if config.max_retries > 10 {
        config.max_retries = 10;
    }
    Ok(config)
}
