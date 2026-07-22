//! Reversible secret encryption for app data (WebDAV passwords, etc.).
//!
//! Layout per secret:
//!   salt  = 16 random bytes (stored)
//!   nonce = 12 random bytes (stored, AES-GCM)
//!   key   = HKDF-SHA256(ikm=master_key, salt=row_salt, info=HKDF_INFO)
//!   cipher = AES-256-GCM(key, nonce, plaintext)

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

const MASTER_KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HKDF_INFO: &[u8] = b"agentbuddy/webdav/v1";
/// Backup whole-archive envelope (passphrase → key). Isolated from WebDAV info.
const BACKUP_HKDF_INFO: &[u8] = b"agentbuddy/backup/v1";
/// Magic prefix for `.abenc` files: ASCII "ABENC1" + NUL.
pub const BACKUP_MAGIC: &[u8] = b"ABENC1\0";

#[derive(Debug, Clone)]
pub struct EncryptedSecret {
    pub salt: String,
    pub nonce: String,
    pub cipher: String,
}

/// Generate a fresh 32-byte master key, base64-encoded for config.json.
pub fn generate_secrets_key() -> String {
    let mut bytes = [0u8; MASTER_KEY_LEN];
    rand::thread_rng().fill_bytes(&mut bytes);
    B64.encode(bytes)
}

/// Decode and validate a base64 master key (must decode to 32 bytes).
pub fn decode_master_key(encoded: &str) -> Result<[u8; MASTER_KEY_LEN], String> {
    let bytes = B64
        .decode(encoded.trim())
        .map_err(|e| format!("Invalid secretsKey encoding: {}", e))?;
    if bytes.len() != MASTER_KEY_LEN {
        return Err(format!(
            "Invalid secretsKey length: expected {}, got {}",
            MASTER_KEY_LEN,
            bytes.len()
        ));
    }
    let mut key = [0u8; MASTER_KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Encrypt plaintext with a fresh per-row salt and nonce.
pub fn encrypt_secret(master_key: &[u8; MASTER_KEY_LEN], plaintext: &str) -> Result<EncryptedSecret, String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(master_key, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to init cipher: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Failed to encrypt secret: {}", e))?;

    Ok(EncryptedSecret {
        salt: B64.encode(salt),
        nonce: B64.encode(nonce_bytes),
        cipher: B64.encode(ciphertext),
    })
}

/// Decrypt a previously encrypted secret.
pub fn decrypt_secret(
    master_key: &[u8; MASTER_KEY_LEN],
    salt_b64: &str,
    nonce_b64: &str,
    cipher_b64: &str,
) -> Result<String, String> {
    let salt = B64
        .decode(salt_b64.trim())
        .map_err(|e| format!("Invalid password salt: {}", e))?;
    let nonce_bytes = B64
        .decode(nonce_b64.trim())
        .map_err(|e| format!("Invalid password nonce: {}", e))?;
    let ciphertext = B64
        .decode(cipher_b64.trim())
        .map_err(|e| format!("Invalid password cipher: {}", e))?;

    if salt.len() != SALT_LEN {
        return Err(format!("Invalid password salt length: {}", salt.len()));
    }
    if nonce_bytes.len() != NONCE_LEN {
        return Err(format!("Invalid password nonce length: {}", nonce_bytes.len()));
    }

    let key = derive_key(master_key, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to init cipher: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "Failed to decrypt password (secretsKey mismatch or data corrupted)".to_string())?;

    String::from_utf8(plaintext)
        .map_err(|e| format!("Decrypted password is not valid UTF-8: {}", e))
}

fn derive_key(master_key: &[u8; MASTER_KEY_LEN], salt: &[u8]) -> Result<[u8; MASTER_KEY_LEN], String> {
    derive_key_with_info(master_key, salt, HKDF_INFO)
}

fn derive_key_with_info(
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
) -> Result<[u8; MASTER_KEY_LEN], String> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; MASTER_KEY_LEN];
    hk.expand(info, &mut okm)
        .map_err(|e| format!("HKDF expand failed: {}", e))?;
    Ok(okm)
}

/// Encrypt a whole backup archive (typically zip bytes) with a user passphrase.
///
/// Output layout:
///   magic(7) + salt(16) + nonce(12) + ciphertext+tag
pub fn encrypt_backup_blob(passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    if passphrase.is_empty() {
        return Err("备份口令不能为空".to_string());
    }
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce_bytes);

    let key = derive_key_with_info(passphrase.as_bytes(), &salt, BACKUP_HKDF_INFO)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to init backup cipher: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("Failed to encrypt backup: {}", e))?;

    let mut out = Vec::with_capacity(BACKUP_MAGIC.len() + SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(BACKUP_MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt_backup_blob`].
/// Used by unit tests and reserved for restore (phase 2).
#[allow(dead_code)]
pub fn decrypt_backup_blob(passphrase: &str, blob: &[u8]) -> Result<Vec<u8>, String> {
    if passphrase.is_empty() {
        return Err("备份口令不能为空".to_string());
    }
    let min = BACKUP_MAGIC.len() + SALT_LEN + NONCE_LEN + 16;
    if blob.len() < min {
        return Err("备份文件过短或已损坏".to_string());
    }
    if &blob[..BACKUP_MAGIC.len()] != BACKUP_MAGIC {
        return Err("不是有效的 AgentBuddy 加密备份（魔数不匹配）".to_string());
    }
    let salt_start = BACKUP_MAGIC.len();
    let nonce_start = salt_start + SALT_LEN;
    let ct_start = nonce_start + NONCE_LEN;
    let salt = &blob[salt_start..nonce_start];
    let nonce_bytes = &blob[nonce_start..ct_start];
    let ciphertext = &blob[ct_start..];

    let key = derive_key_with_info(passphrase.as_bytes(), salt, BACKUP_HKDF_INFO)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to init backup cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "解密失败：口令错误或文件已损坏".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_unique_ciphertext() {
        let master = decode_master_key(&generate_secrets_key()).unwrap();
        let a = encrypt_secret(&master, "same-password").unwrap();
        let b = encrypt_secret(&master, "same-password").unwrap();
        assert_ne!(a.cipher, b.cipher);
        assert_ne!(a.salt, b.salt);
        assert_eq!(decrypt_secret(&master, &a.salt, &a.nonce, &a.cipher).unwrap(), "same-password");
        assert_eq!(decrypt_secret(&master, &b.salt, &b.nonce, &b.cipher).unwrap(), "same-password");
    }

    #[test]
    fn backup_blob_roundtrip() {
        let plain = b"PK\x03\x04fake-zip-bytes-for-test";
        let enc = encrypt_backup_blob("test-pass-phrase", plain).unwrap();
        assert!(enc.starts_with(BACKUP_MAGIC));
        let dec = decrypt_backup_blob("test-pass-phrase", &enc).unwrap();
        assert_eq!(dec, plain);
        assert!(decrypt_backup_blob("wrong", &enc).is_err());
    }
}
