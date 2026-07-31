//! Axum HTTP server lifecycle management for route aggregation.

use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use super::config::RouteAggregationConfig;
use super::provider_router::ProviderRouter;
use super::RouteGroup;

/// Route aggregation HTTP server. Owns the tokio task and shutdown signal.
pub struct RouteAggregationServer {
    /// Shutdown sender — inside Mutex so `stop()` works through `&self`
    /// (the server is stored as `Arc<RouteAggregationServer>` in app state).
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    listen_address: String,
    listen_port: u16,
}

impl RouteAggregationServer {
    /// Start the local proxy server.
    ///
    /// Binds to `listen_address:listen_port` and serves routes based on which
    /// groups are enabled in the config. Returns an error if the port is in use.
    pub async fn start(
        config: RouteAggregationConfig,
        provider_router: Arc<ProviderRouter>,
    ) -> Result<Self, String> {
        let addr = format!("{}:{}", config.listen_address, config.listen_port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "端口 {} 已被占用，请在路由聚合设置中更改监听端口",
                        config.listen_port
                    )
                } else {
                    format!("绑定地址 {} 失败: {}", addr, e)
                }
            })?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let app = super::router::build_router(&config, provider_router.clone());

        let listen_addr = config.listen_address.clone();
        let listen_port = config.listen_port;

        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        eprintln!(
            "[route-aggregation] server started on {}:{} (CC={}, Codex={})",
            listen_addr,
            listen_port,
            config.claude_code_enabled,
            config.codex_enabled
        );

        Ok(Self {
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            listen_address: listen_addr,
            listen_port,
        })
    }

    /// Gracefully stop the server. Works through `&self` (interior mutability).
    pub fn stop(&self) {
        let mut guard = self.shutdown_tx.lock().unwrap();
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
        eprintln!(
            "[route-aggregation] server stopped on {}:{}",
            self.listen_address, self.listen_port
        );
    }

    pub fn is_running(&self) -> bool {
        self.shutdown_tx.lock().unwrap().is_some()
    }
}
