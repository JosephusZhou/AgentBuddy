//! Claude Code device profile parsing, validation, and stabilization.
//!
//! Reference: CLIProxyAPI `helps/claude_device_profile.go`.
//! AgentBuddy keeps the same observable profile rules while using an in-process
//! scoped cache instead of the upstream home KV store.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_CLAUDE_VERSION: &str = "2.1.220";
const DEFAULT_PACKAGE_VERSION: &str = "0.94.0";
const DEFAULT_RUNTIME_VERSION: &str = "v26.3.0";
const DEFAULT_OS: &str = "MacOS";
const DEFAULT_ARCH: &str = "arm64";
const PROFILE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClaudeCliVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ClaudeCliVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let value = value.strip_prefix("claude-cli/").unwrap_or(value);
        let value = value.split_whitespace().next().unwrap_or(value);
        let mut parts = value.split('.');
        let version = Self::new(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        if parts.next().is_some() {
            return None;
        }
        Some(version)
    }

    pub fn as_string(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfile {
    pub user_agent: String,
    pub package_version: String,
    pub runtime_version: String,
    pub os: String,
    pub arch: String,
    pub version: ClaudeCliVersion,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self::for_version(ClaudeCliVersion::parse(DEFAULT_CLAUDE_VERSION).unwrap())
    }
}

impl DeviceProfile {
    pub fn for_version(version: ClaudeCliVersion) -> Self {
        Self {
            user_agent: format!("claude-cli/{} (external, cli)", version.as_string()),
            package_version: DEFAULT_PACKAGE_VERSION.to_string(),
            runtime_version: DEFAULT_RUNTIME_VERSION.to_string(),
            os: DEFAULT_OS.to_string(),
            arch: DEFAULT_ARCH.to_string(),
            version,
        }
    }

    #[allow(dead_code)]
    pub fn from_user_agent(user_agent: &str) -> Option<Self> {
        let version = ClaudeCliVersion::parse(user_agent)?;
        if !user_agent.trim().starts_with("claude-cli/") {
            return None;
        }
        Some(Self {
            user_agent: user_agent.trim().to_string(),
            ..Self::for_version(version)
        })
    }

    /// Normalize a candidate profile against the configured baseline.
    /// Software versions must match exactly; platform values are always pinned
    /// to the baseline so a caller cannot leak a third-party fingerprint.
    pub fn normalize(candidate: Self, baseline: &Self) -> Self {
        if candidate.version == baseline.version
            && candidate.package_version == baseline.package_version
            && candidate.runtime_version == baseline.runtime_version
        {
            Self {
                os: baseline.os.clone(),
                arch: baseline.arch.clone(),
                ..candidate
            }
        } else {
            baseline.clone()
        }
    }
}

#[derive(Clone)]
struct CacheEntry {
    profile: DeviceProfile,
    created: Instant,
}

static PROFILE_CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();

fn profile_cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    PROFILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn requested_version(version: &str) -> ClaudeCliVersion {
    ClaudeCliVersion::parse(version)
        .or_else(|| ClaudeCliVersion::parse(DEFAULT_CLAUDE_VERSION))
        .expect("the built-in Claude version must be valid")
}

/// Return a stable profile for the default route scope.
#[allow(dead_code)]
pub fn get_stable_profile(version: &str) -> DeviceProfile {
    get_stable_profile_for_scope("default", version)
}

/// Return a stable profile isolated by route/auth scope.
pub fn get_stable_profile_for_scope(scope: &str, version: &str) -> DeviceProfile {
    let requested = requested_version(version);
    let key = format!("{}:{}", scope.trim(), requested.as_string());
    let baseline = DeviceProfile::for_version(requested);
    let mut cache = profile_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(entry) = cache.get(&key) {
        if entry.created.elapsed() < PROFILE_TTL {
            return entry.profile.clone();
        }
    }

    let profile = DeviceProfile::normalize(baseline.clone(), &baseline);
    cache.insert(
        key,
        CacheEntry {
            profile: profile.clone(),
            created: Instant::now(),
        },
    );
    profile
}

#[allow(dead_code)]
pub fn clear_profile_cache() {
    profile_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

#[cfg(test)]
mod tests {
    use super::{
        clear_profile_cache, get_stable_profile_for_scope, ClaudeCliVersion, DeviceProfile,
    };

    #[test]
    fn parses_and_orders_cli_versions() {
        assert_eq!(
            ClaudeCliVersion::parse("claude-cli/2.1.220 (external, cli)"),
            Some(ClaudeCliVersion::new(2, 1, 220))
        );
        assert_eq!(
            DeviceProfile::from_user_agent("claude-cli/2.1.220 (external, cli)")
                .unwrap()
                .version,
            ClaudeCliVersion::new(2, 1, 220)
        );
        assert!(ClaudeCliVersion::new(2, 1, 221) > ClaudeCliVersion::new(2, 1, 220));
        assert!(ClaudeCliVersion::parse("claude-cli/not-a-version").is_none());
    }

    #[test]
    fn normalizes_software_and_platform_fingerprint() {
        let baseline = DeviceProfile::default();
        let candidate = DeviceProfile {
            os: "Linux".into(),
            arch: "x64".into(),
            ..baseline.clone()
        };
        let normalized = DeviceProfile::normalize(candidate, &baseline);
        assert_eq!(normalized.os, "MacOS");
        assert_eq!(normalized.arch, "arm64");

        let stale = DeviceProfile::for_version(ClaudeCliVersion::new(2, 1, 63));
        assert_eq!(DeviceProfile::normalize(stale, &baseline), baseline);
    }

    #[test]
    fn cache_isolated_by_scope_and_version() {
        clear_profile_cache();
        let first = get_stable_profile_for_scope("account-a", "2.1.220");
        let second = get_stable_profile_for_scope("account-b", "2.1.221");
        assert_ne!(first.version, second.version);
        assert_eq!(first.user_agent, "claude-cli/2.1.220 (external, cli)");
        assert_eq!(second.user_agent, "claude-cli/2.1.221 (external, cli)");
    }
}
