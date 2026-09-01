//! Field-level AES-256-GCM encryption helpers for typed persistence stores.

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

/// The size of the AES-GCM nonce in bytes.
const AES_GCM_NONCE_SIZE: usize = 12;

/// Encrypts one field value with AES-256-GCM.
///
/// The returned string is `base64(nonce || ciphertext)` with a fresh random
/// 12-byte nonce, so identical plaintexts never produce the same stored value.
///
/// # Arguments
/// * `key` - Static AES-256-GCM key used to encrypt the value.
/// * `plaintext` - UTF-8 value to encrypt.
///
/// # Returns
/// `Ok(String)` with the encoded nonce and ciphertext.
/// `Err(anyhow::Error)` when the value cannot be encrypted.
pub fn encrypt_value(key: &[u8; 32], plaintext: &str) -> Result<String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| anyhow!("Invalid AES-GCM value encryption key length."))?;

    let mut nonce = [0u8; AES_GCM_NONCE_SIZE];
    rand::fill(&mut nonce);

    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow!("Failed to encrypt field value."))?;

    let mut payload = Vec::with_capacity(AES_GCM_NONCE_SIZE + ciphertext.len());
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);

    Ok(BASE64_STANDARD.encode(payload))
}

/// Decrypts one field value produced by [`encrypt_value`].
///
/// # Arguments
/// * `key` - Static AES-256-GCM key used to decrypt the value.
/// * `encoded` - `base64(nonce || ciphertext)` payload to decrypt.
///
/// # Returns
/// `Ok(String)` with the decrypted UTF-8 value.
/// `Err(anyhow::Error)` when the payload is malformed or cannot be decrypted.
pub fn decrypt_value(key: &[u8; 32], encoded: &str) -> Result<String> {
    let payload = BASE64_STANDARD
        .decode(encoded.trim())
        .context("Field value is not valid base64.")?;
    if payload.len() <= AES_GCM_NONCE_SIZE {
        bail!("Encrypted field value is too short to contain a nonce and ciphertext.");
    }
    let (nonce, ciphertext) = payload.split_at(AES_GCM_NONCE_SIZE);

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| anyhow!("Invalid AES-GCM value encryption key length."))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("Failed to decrypt field value."))?;

    String::from_utf8(plaintext).context("Decrypted field value is not valid UTF-8.")
}
