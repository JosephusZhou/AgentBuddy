//! Route aggregation (local proxy server): aggregates multiple AI providers
//! behind a single local endpoint with automatic failover and request cloaking.
//!
//! Architecture:
//! - `server` — Axum HTTP server lifecycle (start/stop/config)
//! - `router` — Route registration + request dispatch
//! - `handler` — Request entry points (Claude messages, Codex chat/responses)
//! - `forwarder` — Request forwarding with failover + header injection + SSE passthrough
//! - `provider_router` — Provider selection + circuit breaker management
//! - `circuit_breaker` — Three-state circuit breaker (Closed/Open/HalfOpen)
//! - `cloaking` — Claude Code rectifier + Codex client simulation
//! - `translator` — Multi-protocol request/response translation
//!   (Phase 0: registry + stream state machine + shared common tools;
//!    Phase 1+: per-pair translators)
//! - `config` — RouteAggregationConfig load/save (stored in config.json)
//! - `types` — Data structures (DTOs for Tauri commands)

pub mod circuit_breaker;
pub mod cloaking;
pub mod config;
pub mod forwarder;
pub mod handler;
pub mod provider_router;
pub mod router;
pub mod server;
pub mod translator;
pub mod types;

use std::sync::Arc;
use tokio::sync::RwLock;

pub use types::*;

// Re-export key types used across modules and in lib.rs/config.rs.
pub use config::RouteAggregationConfig;
pub use circuit_breaker::CircuitBreakerSnapshot;

/// Global route aggregation state, shared across Tauri commands via `app.manage()`.
pub struct RouteAggregationState {
    /// Server instance (present when running, None when stopped).
    pub server: RwLock<Option<Arc<server::RouteAggregationServer>>>,
    /// Config snapshot — shared with Axum handlers via AppState so that runtime
    /// config updates (e.g. toggling a route group) are visible without restart.
    pub config: Arc<RwLock<RouteAggregationConfig>>,
    /// Provider router (holds circuit breaker state, survives server stop).
    pub provider_router: Arc<provider_router::ProviderRouter>,
    /// Multi-protocol translator registry. Populated by `populate_default_translators`
    /// during `setup()`. Forwarder reads `(source, target)` → translator here.
    pub translator_registry: Arc<translator::TranslatorRegistry>,
}

impl RouteAggregationState {
    pub fn new(config: RouteAggregationConfig) -> Self {
        Self {
            server: RwLock::new(None),
            config: Arc::new(RwLock::new(config)),
            provider_router: Arc::new(provider_router::ProviderRouter::new()),
            translator_registry: Arc::new(translator::TranslatorRegistry::new()),
        }
    }

    /// 注册默认的 pair 翻译器。setup 阶段调用一次（线程安全，内部 Mutex）。
    pub fn populate_default_translators(&self) {
        use std::sync::Arc as StdArc;
        use translator::claude_gemini::ClaudeToGeminiTranslator;
        use translator::gemini_openai_chat::GeminiToOpenaiChatTranslator;
        use translator::gemini_openai_responses::GeminiToOpenaiResponsesTranslator;
        use translator::openai_gemini::OpenaiToGeminiTranslator;
        use translator::openai_openai_responses::OpenaiResponsesToGeminiTranslator;

        // Phase 1: Claude Messages → Gemini generateContent
        self.translator_registry.register(
            translator::Format::Anthropic,
            translator::Format::Gemini,
            StdArc::new(ClaudeToGeminiTranslator),
        );
        // Phase 2: OpenAI Chat Completions → Gemini generateContent
        self.translator_registry.register(
            translator::Format::OpenAiChat,
            translator::Format::Gemini,
            StdArc::new(OpenaiToGeminiTranslator),
        );
        // Phase 2: Gemini → OpenAI Chat Completions (响应方向)
        self.translator_registry.register(
            translator::Format::Gemini,
            translator::Format::OpenAiChat,
            StdArc::new(GeminiToOpenaiChatTranslator),
        );
        // Phase 4: OpenAI Responses API → Gemini generateContent
        self.translator_registry.register(
            translator::Format::OpenAiResponses,
            translator::Format::Gemini,
            StdArc::new(OpenaiResponsesToGeminiTranslator),
        );
        // Phase 4: Gemini → OpenAI Responses API (响应方向)
        self.translator_registry.register(
            translator::Format::Gemini,
            translator::Format::OpenAiResponses,
            StdArc::new(GeminiToOpenaiResponsesTranslator),
        );
    }

    /// Build a status snapshot for the frontend.
    pub async fn get_status(&self) -> RouteAggregationStatus {
        let config = self.config.read().await;
        let server_running = self.server.read().await.is_some();
        let providers = self.provider_router.get_merged_statuses().await;

        RouteAggregationStatus {
            server_running,
            listen_address: config.listen_address.clone(),
            listen_port: config.listen_port,
            providers,
        }
    }
}
