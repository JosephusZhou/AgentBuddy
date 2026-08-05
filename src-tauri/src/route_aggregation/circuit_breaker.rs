//! Three-state circuit breaker (Closed → Open → HalfOpen → Closed).
//!
//! Each provider × route group gets an independent breaker.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

use super::RouteGroup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    pub fn as_str(&self) -> &'static str {
        match self {
            CircuitState::Closed => "closed",
            CircuitState::Open => "open",
            CircuitState::HalfOpen => "half_open",
        }
    }
}

/// Per-provider×group circuit breaker.
pub struct CircuitBreaker {
    state: AsyncMutex<CircuitState>,
    consecutive_failures: AtomicU32,
    success_count: AtomicU32,
    opened_at: AsyncMutex<Option<Instant>>,
    total_requests: AtomicU64,
    total_failures: AtomicU64,
    last_error: Mutex<Option<String>>,
    last_error_at: AtomicI64,

    // Configurable thresholds
    failure_threshold: u32,
    success_threshold: u32,
    timeout: Duration,
    error_rate_threshold: f64,
    min_requests: u64,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: AsyncMutex::new(CircuitState::Closed),
            consecutive_failures: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            opened_at: AsyncMutex::new(None),
            total_requests: AtomicU64::new(0),
            total_failures: AtomicU64::new(0),
            last_error: Mutex::new(None),
            last_error_at: AtomicI64::new(0),
            failure_threshold: 4,
            success_threshold: 2,
            timeout: Duration::from_secs(60),
            error_rate_threshold: 0.6,
            min_requests: 10,
        }
    }

    #[allow(dead_code)]
    pub async fn state(&self) -> CircuitState {
        *self.state.lock().await
    }

    pub async fn can_attempt(&self) -> bool {
        let mut state = self.state.lock().await;
        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let opened_at = self.opened_at.lock().await;
                if let Some(t) = *opened_at {
                    if t.elapsed() >= self.timeout {
                        // Transition to half-open, allow a probe
                        *state = CircuitState::HalfOpen;
                        self.success_count.store(0, Ordering::Relaxed);
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => {
                // Only allow one probe at a time
                self.consecutive_failures.load(Ordering::Relaxed) == 0
            }
        }
    }

    pub async fn record_success(&self) {
        let mut state = self.state.lock().await;
        match *state {
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if successes >= self.success_threshold {
                    *state = CircuitState::Closed;
                    self.consecutive_failures.store(0, Ordering::Relaxed);
                }
            }
            CircuitState::Closed => {
                self.consecutive_failures.store(0, Ordering::Relaxed);
            }
            _ => {}
        }
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn record_failure(&self, error: &str) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        self.total_failures.fetch_add(1, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        {
            let mut last_err = self.last_error.lock().unwrap();
            *last_err = Some(error.to_string());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.last_error_at.store(now, Ordering::Relaxed);

        let mut state = self.state.lock().await;
        match *state {
            CircuitState::HalfOpen => {
                // Probe failed — re-open
                *state = CircuitState::Open;
                *self.opened_at.lock().await = Some(Instant::now());
            }
            CircuitState::Closed => {
                if failures >= self.failure_threshold {
                    *state = CircuitState::Open;
                    *self.opened_at.lock().await = Some(Instant::now());
                } else {
                    // Also check error rate
                    let total = self.total_requests.load(Ordering::Relaxed);
                    if total >= self.min_requests {
                        let fail_rate =
                            self.total_failures.load(Ordering::Relaxed) as f64 / total as f64;
                        if fail_rate >= self.error_rate_threshold {
                            *state = CircuitState::Open;
                            *self.opened_at.lock().await = Some(Instant::now());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        *state = CircuitState::Closed;
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        self.total_requests.store(0, Ordering::Relaxed);
        self.total_failures.store(0, Ordering::Relaxed);
        *self.opened_at.lock().await = None;
        {
            let mut last_err = self.last_error.lock().unwrap();
            *last_err = None;
        }
        self.last_error_at.store(0, Ordering::Relaxed);
    }

    pub async fn snapshot(&self) -> CircuitBreakerSnapshot {
        CircuitBreakerSnapshot {
            state: self.state.lock().await.as_str().to_string(),
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            request_count: self.total_requests.load(Ordering::Relaxed),
            success_count: self.total_requests.load(Ordering::Relaxed)
                - self.total_failures.load(Ordering::Relaxed),
            last_error: self.last_error.lock().unwrap().clone(),
            last_error_at: {
                let ts = self.last_error_at.load(Ordering::Relaxed);
                if ts == 0 { None } else { Some(ts) }
            },
        }
    }
}

/// Readable snapshot for status reporting.
#[derive(Debug, Clone)]
pub struct CircuitBreakerSnapshot {
    pub state: String,
    pub consecutive_failures: u32,
    pub request_count: u64,
    pub success_count: u64,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

/// Manager holding all circuit breakers keyed by (provider_id, group).
#[allow(dead_code)]
pub struct CircuitBreakerManager {
    breakers: Mutex<HashMap<(String, RouteGroup), CircuitBreaker>>,
}

#[allow(dead_code)]
impl CircuitBreakerManager {
    pub fn new() -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_or_create(&self, provider_id: &str, group: RouteGroup) -> CircuitBreakerRef {
        let mut map = self.breakers.lock().unwrap();
        let key = (provider_id.to_string(), group);
        map.entry(key).or_insert_with(CircuitBreaker::new);
        // We return a snapshot approach since we can't return a reference through a Mutex
        // Instead, the provider_router holds the actual breakers
        CircuitBreakerRef
    }

    /// Get a snapshot of all breakers for a given group.
    pub fn snapshots_for_group(
        &self,
        group: RouteGroup,
        provider_ids: &[String],
    ) -> Vec<(String, CircuitBreakerSnapshot)> {
        let map = self.breakers.lock().unwrap();
        let mut result = Vec::new();
        for pid in provider_ids {
            let key = (pid.clone(), group);
            if map.get(&key).is_some() {
                // We need to get the snapshot synchronously — but CircuitBreaker.snapshot is async.
                // For now, return a placeholder; the real snapshot is obtained via ProviderRouter.
                // This synchronous path is only used for quick checks.
                result.push((pid.clone(), CircuitBreakerSnapshot {
                    state: "closed".to_string(),
                    consecutive_failures: 0,
                    request_count: 0,
                    success_count: 0,
                    last_error: None,
                    last_error_at: None,
                }));
            }
        }
        result
    }
}

#[allow(dead_code)]
pub struct CircuitBreakerRef;

impl Default for CircuitBreakerManager {
    fn default() -> Self {
        Self::new()
    }
}
