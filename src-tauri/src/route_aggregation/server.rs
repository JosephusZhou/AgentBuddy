//! Axum HTTP server lifecycle management for route aggregation.

use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, RwLock};

use super::config::RouteAggregationConfig;
use super::log::LogStore;
use super::provider_router::ProviderRouter;

/// Route aggregation HTTP server. Owns the tokio task and shutdown signal.
pub struct RouteAggregationServer {
    /// Shutdown sender — inside Mutex so `stop()` works through `&self`
    /// (the server is stored as `Arc<RouteAggregationServer>` in app state).
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// Join handle of the axum serve task — awaited in `stop()` to ensure
    /// the port is fully released before a new server can bind it.
    join_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    listen_address: String,
    listen_port: u16,
}

impl RouteAggregationServer {
    /// Start the local proxy server.
    ///
    /// Binds to `listen_address:listen_port` from the config. All routes are
    /// always registered; enabling/disabling a group is handled at runtime by
    /// the handler reading the shared config. Returns an error if the port is
    /// in use.
    pub async fn start(
        config: Arc<RwLock<RouteAggregationConfig>>,
        provider_router: Arc<ProviderRouter>,
        log_store: LogStore,
    ) -> Result<Self, String> {
        let (listen_addr, listen_port) = {
            let cfg = config.read().await;
            (cfg.listen_address.clone(), cfg.listen_port)
        };

        let addr = format!("{}:{}", listen_addr, listen_port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "端口 {} 已被占用，请在路由聚合设置中更改监听端口",
                        listen_port
                    )
                } else {
                    format!("绑定地址 {} 失败: {}", addr, e)
                }
            })?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let app = super::router::build_router(config, provider_router, log_store);

        let join_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        eprintln!(
            "[route-aggregation] server started on {}:{}",
            listen_addr, listen_port
        );

        Ok(Self {
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            join_handle: Mutex::new(Some(join_handle)),
            listen_address: listen_addr,
            listen_port,
        })
    }

    /// Gracefully stop the server. Works through `&self` (interior mutability).
    ///
    /// This is async because it awaits the axum serve task to complete,
    /// ensuring the listening port is fully released before returning.
    /// Without this, a subsequent `start()` on the same port would fail with
    /// "AddrInUse" because the old task hasn't released the port yet.
    pub async fn stop(&self) {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        // Wait for the server task to finish — this releases the port.
        let handle = self.join_handle.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.await;
        }
        eprintln!(
            "[route-aggregation] server stopped on {}:{}",
            self.listen_address, self.listen_port
        );
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.shutdown_tx.lock().unwrap().is_some()
    }
}
