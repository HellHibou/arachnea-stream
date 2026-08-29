//! Administration authentication: Argon2id password hashing, opaque sessions,
//! login rate limiting, and access-control helpers.

use anyhow::{Context, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString};
use argon2::{Argon2, PasswordVerifier};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL, Engine as _};
use rand::Rng;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Name of the session cookie sent to authenticated administrators.
pub const ADMIN_SESSION_COOKIE: &str = "arachnea_admin_session";

/// Lifetime of one administrator session.
const SESSION_TTL: Duration = Duration::from_secs(2 * 60 * 60);

/// Number of failed logins accepted per peer inside the rate-limit window.
const MAX_FAILED_LOGINS: usize = 10;

/// Window during which failed logins are counted per peer.
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Hashes a password with Argon2id and a random salt.
///
/// # Arguments
/// * `password` - Plain password to hash.
///
/// # Returns
/// The PHC-encoded hash string, ready for persistent storage.
///
/// # Errors
/// Returns an error when the hashing parameters or output serialization fail.
pub fn hash_admin_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .context("Failed to hash the administrator password.")
}

/// Verifies a plain password against a stored PHC-encoded Argon2id hash.
///
/// # Arguments
/// * `password` - Plain password candidate.
/// * `encoded_hash` - PHC-encoded hash previously produced by [`hash_admin_password`].
///
/// # Returns
/// `true` when the password matches the stored hash.
pub fn verify_admin_password(password: &str, encoded_hash: &str) -> bool {
    PasswordHash::new(encoded_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Generates an opaque, URL-safe session token.
///
/// # Returns
/// A 256-bit random token encoded as unpadded base64url.
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    BASE64_URL.encode(bytes)
}

/// Returns whether two strings match in constant time.
///
/// Used for plain-text comparisons against the in-memory temporary password so
/// the comparison duration does not leak the matching prefix length.
///
/// # Arguments
/// * `left` - First string.
/// * `right` - Second string.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// In-memory store of active administrator sessions.
///
/// Sessions are opaque tokens mapped to their expiry instant. Tokens never
/// leave the process: only the cookie value reaches the client, and the store
/// dies with the process.
#[derive(Default)]
pub struct SessionStore {
    sessions: Mutex<HashMap<String, Instant>>,
}

impl SessionStore {
    /// Creates a new empty session store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a session and returns its opaque token.
    ///
    /// # Returns
    /// The freshly generated session token.
    pub fn create(&self) -> String {
        let token = generate_session_token();
        let mut sessions = self.sessions.lock().expect("session store poisoned");
        sessions.retain(|_, expires_at| *expires_at > Instant::now());
        sessions.insert(token.clone(), Instant::now() + SESSION_TTL);
        token
    }

    /// Returns whether a session token is valid, renewing its expiry.
    ///
    /// # Arguments
    /// * `token` - Opaque session token presented by the client.
    pub fn validate(&self, token: &str) -> bool {
        let mut sessions = self.sessions.lock().expect("session store poisoned");
        sessions.retain(|_, expires_at| *expires_at > Instant::now());
        sessions
            .get_mut(token)
            .map(|expires_at| {
                *expires_at = Instant::now() + SESSION_TTL;
                true
            })
            .unwrap_or(false)
    }

    /// Removes one session token.
    ///
    /// # Arguments
    /// * `token` - Opaque session token to invalidate.
    pub fn remove(&self, token: &str) {
        self.sessions
            .lock()
            .expect("session store poisoned")
            .remove(token);
    }
}

/// Builds the `Set-Cookie` header value for one session token.
///
/// The cookie is `HttpOnly` and `SameSite=Strict` so scripts cannot read it
/// and cross-site requests never carry it. It is scoped to the whole origin so
/// it keeps working behind an entrypoint root.
///
/// # Arguments
/// * `token` - Opaque session token to store in the cookie.
///
/// # Returns
/// The `Set-Cookie` header value.
pub fn session_cookie(token: &str) -> String {
    format!(
        "{ADMIN_SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        SESSION_TTL.as_secs()
    )
}

/// Builds the `Set-Cookie` header value clearing the session cookie.
///
/// # Returns
/// The expiring `Set-Cookie` header value.
pub fn cleared_session_cookie() -> String {
    format!("{ADMIN_SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// Extracts the session token from the `Cookie` request header.
///
/// # Arguments
/// * `cookie_header` - Raw value of the `Cookie` header.
///
/// # Returns
/// The session token when the header carries the administration cookie.
pub fn session_token_from_cookie(cookie_header: Option<&str>) -> Option<String> {
    cookie_header?
        .split(';')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(name, value)| {
            (name.trim() == ADMIN_SESSION_COOKIE).then(|| value.trim().to_string())
        })
        .filter(|token| !token.is_empty())
}

/// Per-peer sliding-window limiter for failed login attempts.
#[derive(Default)]
pub struct LoginRateLimiter {
    failures: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl LoginRateLimiter {
    /// Creates a new empty limiter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether the peer reached the failure limit.
    ///
    /// # Arguments
    /// * `peer` - Remote peer IP address.
    pub fn is_limited(&self, peer: IpAddr) -> bool {
        let mut failures = self.failures.lock().expect("login limiter poisoned");
        Self::retain_current(&mut failures);
        failures
            .get(&peer)
            .map(|entries| entries.len() >= MAX_FAILED_LOGINS)
            .unwrap_or(false)
    }

    /// Records one failed login attempt for the peer.
    ///
    /// # Arguments
    /// * `peer` - Remote peer IP address.
    pub fn record_failure(&self, peer: IpAddr) {
        let mut failures = self.failures.lock().expect("login limiter poisoned");
        Self::retain_current(&mut failures);
        failures.entry(peer).or_default().push(Instant::now());
    }

    /// Clears the failure history of one peer, used after a successful login.
    ///
    /// # Arguments
    /// * `peer` - Remote peer IP address.
    pub fn clear(&self, peer: IpAddr) {
        self.failures
            .lock()
            .expect("login limiter poisoned")
            .remove(&peer);
    }

    fn retain_current(failures: &mut HashMap<IpAddr, Vec<Instant>>) {
        failures.retain(|_, entries| {
            entries.retain(|instant| instant.elapsed() < RATE_LIMIT_WINDOW);
            !entries.is_empty()
        });
    }
}