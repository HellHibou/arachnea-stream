//! Validation and reusable cryptographic primitives for public credentials.

use super::reject;
use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

/// Normalizes a login without disclosing account existence on login failures.
///
/// # Arguments
/// * `value` - Input value to validate or encode.
pub(super) fn login(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    if !(crate::PUBLIC_LOGIN_MIN_LENGTH..=crate::PUBLIC_LOGIN_MAX_LENGTH).contains(&value.len())
        || !value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
    {
        return Err(super::for_field(
            "login",
            reject(
                "invalid_data",
                422,
                "Login must contain 3–64 ASCII letters, digits, dots, underscores or hyphens.",
            ),
        ));
    }
    Ok(value)
}

/// Checks password length without transforming the user's secret.
///
/// # Arguments
/// * `value` - Input value to validate or encode.
pub(super) fn password(value: &str) -> Result<()> {
    if !(crate::PUBLIC_PASSWORD_MIN_LENGTH..=crate::PUBLIC_PASSWORD_MAX_LENGTH)
        .contains(&value.chars().count())
    {
        return Err(reject(
            "invalid_data",
            422,
            "Password must contain 12–128 characters.",
        ));
    }
    Ok(())
}

/// Hashes a public password with the existing Argon2id primitive and fresh salt.
///
/// # Arguments
/// * `value` - Input value to validate or encode.
pub(super) fn hash(value: &str) -> Result<String> {
    password(value)?;
    arachnea_scrapyfy::admin::auth::hash_admin_password(value)
}

/// Verifies an Argon2id hash using the shared primitive.
///
/// # Arguments
/// * `value` - Input value to validate or encode.
/// * `hash` - Persisted Argon2id password hash.
pub(super) fn verify(value: &str, hash: &str) -> bool {
    arachnea_scrapyfy::admin::auth::verify_admin_password(value, hash)
}

/// Hashes an opaque high-entropy session secret; plaintext is never persisted.
///
/// # Arguments
/// * `value` - Input value to validate or encode.
pub(super) fn digest(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

/// Generates a cryptographically random 256-bit session secret.
pub(super) fn token() -> String {
    arachnea_scrapyfy::admin::auth::generate_session_token()
}
