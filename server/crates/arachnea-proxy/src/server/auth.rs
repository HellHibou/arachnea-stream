use base64::Engine;
use serde::{Deserialize, Serialize};

/// Optional inbound proxy authentication configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyAuthConfig {
    /// Username expected from HTTP proxy or SOCKS5 clients.
    pub username: Option<String>,
    /// Environment variable containing the expected proxy password.
    pub password_env: Option<String>,
    /// HTTP Basic authentication realm advertised to clients.
    pub realm: String,
}

impl Default for ProxyAuthConfig {
    /// Creates an authentication configuration with no required password.
    fn default() -> Self {
        Self {
            username: None,
            password_env: None,
            realm: "Arachnea Proxy".to_string(),
        }
    }
}

impl ProxyAuthConfig {
    /// Returns whether inbound proxy authentication is fully configured.
    ///
    /// # Returns
    ///
    /// `true` when both username and password environment variable are set.
    pub fn is_required(&self) -> bool {
        self.username.is_some() && self.password_env.is_some()
    }

    /// Loads configured credentials from the environment.
    ///
    /// Authentication is disabled unless both `username` and `password_env` are
    /// configured. This keeps proxy authentication optional for local testing and
    /// prevents clients from being challenged when no password is defined.
    ///
    /// # Returns
    ///
    /// Loaded credentials when authentication is enabled.
    ///
    /// # Errors
    ///
    /// Returns an error when authentication is enabled but the password
    /// environment variable is not readable.
    pub fn credentials(&self) -> Result<Option<ProxyCredentials>, String> {
        let (Some(username), Some(password_env)) = (&self.username, &self.password_env) else {
            return Ok(None);
        };
        let password = std::env::var(password_env).map_err(|_| {
            format!("proxy authentication password env '{password_env}' is not set")
        })?;
        Ok(Some(ProxyCredentials {
            username: username.clone(),
            password,
        }))
    }
}

/// Loaded proxy credentials used for client authentication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyCredentials {
    /// Expected proxy username.
    pub username: String,
    /// Expected proxy password.
    pub password: String,
}

impl ProxyCredentials {
    /// Validates a username and password pair.
    ///
    /// # Parameters
    ///
    /// - `username`: Username supplied by the client.
    /// - `password`: Password supplied by the client.
    ///
    /// # Returns
    ///
    /// `true` when the supplied pair matches these credentials.
    pub fn matches(&self, username: &str, password: &str) -> bool {
        self.username == username && self.password == password
    }
}

/// Validates an HTTP `Proxy-Authorization` Basic header value.
///
/// # Parameters
///
/// - `header`: Header value sent by the HTTP proxy client.
/// - `credentials`: Expected proxy credentials.
///
/// # Returns
///
/// `true` when the header contains matching Basic credentials.
pub fn validate_http_basic_header(header: &str, credentials: &ProxyCredentials) -> bool {
    let Some((username, password)) = decode_http_basic_credentials(header) else {
        return false;
    };
    credentials.matches(&username, &password)
}

/// Decodes an HTTP `Proxy-Authorization` Basic header value.
///
/// # Parameters
///
/// - `header`: Header value sent by the HTTP proxy client.
///
/// # Returns
///
/// Decoded username and password when the header is valid Basic credentials.
fn decode_http_basic_credentials(header: &str) -> Option<(String, String)> {
    let Some(encoded) = header.trim().strip_prefix("Basic ") else {
        return None;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
        return None;
    };
    let Ok(decoded) = String::from_utf8(decoded) else {
        return None;
    };
    let Some((username, password)) = decoded.split_once(':') else {
        return None;
    };
    Some((username.to_string(), password.to_string()))
}
