//! Billing header forging + CCH (xxHash64) signing.
//! Reference: CLIProxyAPI internal/runtime/executor/claude_signing.go

use sha2::{Digest, Sha256};
use xxhash_rust::xxh64;

/// Claude Code fingerprint salt (extracted from Claude Code source).
const FINGERPRINT_SALT: &str = "59cf53e54c78";

/// CCH signing seed (extracted from CLIProxyAPI).
const CCH_SEED: u64 = 0x6E52736AC806831E;

/// Generate the x-anthropic-billing-header value.
/// Format: cc_version=<ver>; cc_entrypoint=cli; cch=<hash>;
pub fn generate_billing_header(version: &str, body_text: &str) -> String {
    let fingerprint = compute_fingerprint(body_text, version);
    let cch = compute_cch(body_text);
    format!(
        "cc_version={}.{}; cc_entrypoint=cli; cch={:05x};",
        version, fingerprint, cch
    )
}

/// Compute the fingerprint: SHA256(salt + selected message chars + version)[:3]
/// This is a simplified version of the real algorithm.
fn compute_fingerprint(body_text: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_SALT.as_bytes());

    // Extract characters at positions 4, 7, 20 from the body text
    let chars: Vec<char> = body_text.chars().collect();
    if chars.len() > 20 {
        hasher.update(chars[4].to_string().as_bytes());
        hasher.update(chars[7].to_string().as_bytes());
        hasher.update(chars[20].to_string().as_bytes());
    } else if !chars.is_empty() {
        // Fallback: use what we have
        for c in chars.iter().take(3) {
            hasher.update(c.to_string().as_bytes());
        }
    }

    hasher.update(version.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..3])
}

/// Compute CCH: xxHash64(body_text, seed=CCH_SEED), take first 5 hex chars.
fn compute_cch(body_text: &str) -> u64 {
    let full_hash = xxh64::xxh64(body_text.as_bytes(), CCH_SEED);
    // Take the low 20 bits (5 hex chars = 20 bits)
    full_hash & 0xFFFFF
}
