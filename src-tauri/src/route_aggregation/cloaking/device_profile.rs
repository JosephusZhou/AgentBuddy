//! Device profile stabilization.
//! Reference: CLIProxyAPI claude_device_profile.go

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Stable device profile for Claude Code client simulation.
/// Cached for a TTL period to ensure consistency across requests.
#[allow(dead_code)]
pub struct DeviceProfile {
    pub user_agent: String,
    pub package_version: String,
    pub runtime_version: String,
    pub os: String,
    pub arch: String,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self {
            user_agent: "claude-cli/2.1.63 (external, cli)".to_string(),
            package_version: "0.74.0".to_string(),
            runtime_version: "v24.3.0".to_string(),
            os: "MacOS".to_string(),
            arch: "arm64".to_string(),
        }
    }
}

/// Global cached device profile.
#[allow(dead_code)]
static CACHED_PROFILE: Mutex<Option<(DeviceProfile, Instant)>> = Mutex::new(None);

/// TTL for the cached profile: 7 days.
#[allow(dead_code)]
const PROFILE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Get the stable device profile, creating or refreshing if needed.
#[allow(dead_code)]
pub fn get_stable_profile(version: &str) -> DeviceProfile {
    let mut cached = CACHED_PROFILE.lock().unwrap();
    if let Some((profile, created)) = cached.as_ref() {
        if created.elapsed() < PROFILE_TTL {
            // Check if the requested version is newer
            if version == profile.user_agent.split('/').nth(1).unwrap_or("").split(' ').next().unwrap_or("") {
                return profile.clone_profile();
            }
        }
    }

    // Create new profile
    let profile = DeviceProfile {
        user_agent: format!("claude-cli/{} (external, cli)", version),
        ..Default::default()
    };
    *cached = Some((profile.clone_profile(), Instant::now()));
    profile.clone_profile()
}

impl DeviceProfile {
    #[allow(dead_code)]
    fn clone_profile(&self) -> Self {
        Self {
            user_agent: self.user_agent.clone(),
            package_version: self.package_version.clone(),
            runtime_version: self.runtime_version.clone(),
            os: self.os.clone(),
            arch: self.arch.clone(),
        }
    }
}
