//! Native session broker with OS-backed storage and explicit memory fallback.

use super::credentials;
use anyhow::{Context, Result};
use std::sync::Mutex;
use uuid::Uuid;

/// Native session shared only by transport-verified public webviews.
pub(super) struct NativeSessions {
    service: String,
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    loaded: bool,
    token: Option<String>,
    persistent: bool,
    user: Option<Uuid>,
}

impl NativeSessions {
    /// Builds a vault namespace tied to this installation's canonical data root.
    ///
    /// # Arguments
    /// * `installation` - Stable installation data-root identity.
    pub(super) fn new(installation: &str) -> Self {
        Self {
            service: format!(
                "{}.{}",
                crate::PUBLIC_NATIVE_VAULT_SERVICE_PREFIX,
                credentials::digest(installation)
            ),
            state: Mutex::default(),
        }
    }
    /// Restores the native secret once, without exposing it to JavaScript.
    pub(super) fn current(&self) -> Result<(Option<String>, bool)> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("native session broker poisoned"))?;
        if !state.loaded {
            state.loaded = true;
            if let Ok(user) = vault_read(&self.service, crate::PUBLIC_NATIVE_VAULT_ACTIVE_ACCOUNT) {
                if Uuid::parse_str(&user).is_ok() {
                    if let Ok(token) = vault_read(&self.service, &user) {
                        state.user = Uuid::parse_str(&user).ok();
                        state.token = Some(token);
                        state.persistent = true;
                    }
                }
            }
        }
        Ok((state.token.clone(), state.persistent))
    }
    /// Binds trusted public tabs to an account, falling back to memory visibly.
    ///
    /// # Arguments
    /// * `user` - Target account UUID.
    /// * `token` - Opaque session secret used only in memory.
    pub(super) fn set(&self, user: Uuid, token: String) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("native session broker poisoned"))?;
        // Clear the previous resume pointer before switching accounts. Never
        // restore a stale credential when a later vault write is unavailable.
        if let Some(previous) = state.user {
            let _ = vault_delete(&self.service, &previous.to_string());
        }
        let cleared = vault_delete(&self.service, crate::PUBLIC_NATIVE_VAULT_ACTIVE_ACCOUNT);
        let stored = cleared
            .and_then(|_| vault_write(&self.service, &user.to_string(), &token))
            .and_then(|_| {
                vault_write(
                    &self.service,
                    crate::PUBLIC_NATIVE_VAULT_ACTIVE_ACCOUNT,
                    &user.to_string(),
                )
            });
        state.loaded = true;
        state.user = Some(user);
        state.persistent = stored.is_ok();
        state.token = Some(token);
        if stored.is_err() {
            tracing::warn!(
                "Native session credential storage unavailable; session remains in memory"
            );
        }
        Ok(state.persistent)
    }
    /// Removes a matching native session without clearing a newer login.
    ///
    /// # Arguments
    /// * `token` - Opaque session secret used only in memory.
    pub(super) fn clear(&self, token: &str) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("native session broker poisoned"))?;
        if state.token.as_deref() == Some(token) {
            if let Some(user) = state.user.take() {
                let _ = vault_delete(&self.service, &user.to_string());
            }
            let _ = vault_delete(&self.service, crate::PUBLIC_NATIVE_VAULT_ACTIVE_ACCOUNT);
            state.token = None;
            state.persistent = false;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn vault_read(service: &str, account: &str) -> Result<String> {
    Ok(String::from_utf8(
        security_framework::passwords::get_generic_password(service, account)?,
    )?)
}
#[cfg(target_os = "macos")]
fn vault_write(service: &str, account: &str, value: &str) -> Result<()> {
    security_framework::passwords::set_generic_password(service, account, value.as_bytes())
        .context("saving native session in Keychain")
}
#[cfg(target_os = "macos")]
fn vault_delete(service: &str, account: &str) -> Result<()> {
    match security_framework::passwords::delete_generic_password(service, account) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == -25300 => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "linux")]
fn secret_tool(args: &[&str], input: Option<&str>) -> Result<String> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut child = Command::new("secret-tool")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(value) = input {
        child
            .stdin
            .take()
            .context("credential input unavailable")?
            .write_all(value.as_bytes())?;
    } else {
        drop(child.stdin.take());
    }
    let result = child.wait_with_output()?;
    anyhow::ensure!(result.status.success(), "OS credential storage unavailable");
    Ok(String::from_utf8(result.stdout)?
        .trim_end_matches('\n')
        .to_string())
}
#[cfg(target_os = "linux")]
fn vault_read(service: &str, account: &str) -> Result<String> {
    secret_tool(
        &["lookup", "application", service, "account", account],
        None,
    )
}
#[cfg(target_os = "linux")]
fn vault_write(service: &str, account: &str, value: &str) -> Result<()> {
    secret_tool(
        &[
            "store",
            "--label=Arachnea public session",
            "application",
            service,
            "account",
            account,
        ],
        Some(value),
    )
    .map(|_| ())
}
#[cfg(target_os = "linux")]
fn vault_delete(service: &str, account: &str) -> Result<()> {
    // An absent item is already cleared; failure to contact the service is
    // detected on the following write and falls back to memory.
    let _ = secret_tool(&["clear", "application", service, "account", account], None);
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn vault_read(_service: &str, _account: &str) -> Result<String> {
    anyhow::bail!("OS credential storage unavailable on this platform")
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn vault_write(_service: &str, _account: &str, _value: &str) -> Result<()> {
    anyhow::bail!("OS credential storage unavailable on this platform")
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn vault_delete(_service: &str, _account: &str) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn vault_read(service: &str, account: &str) -> Result<String> {
    super::windows_vault::read(service, account)
}
#[cfg(windows)]
fn vault_write(service: &str, account: &str, value: &str) -> Result<()> {
    super::windows_vault::write(service, account, value)
}
#[cfg(windows)]
fn vault_delete(service: &str, account: &str) -> Result<()> {
    super::windows_vault::delete(service, account)
}
