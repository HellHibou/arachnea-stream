//! Windows Credential Manager storage, with secrets kept out of command lines.

use anyhow::Result;
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_NOT_FOUND},
    Security::Credentials::*,
};

/// Encodes a credential target as a NUL-terminated UTF-16 string.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Reads one Credential Manager entry.
///
/// # Arguments
/// * `service` - Installation-specific vault namespace.
/// * `account` - Account UUID or the active-account reference.
///
/// # Errors
/// Returns unavailable/not-found/invalid-encoding errors without exposing secrets.
pub(super) fn read(service: &str, account: &str) -> Result<String> {
    let target = wide(&format!("{service}/{account}"));
    let mut credential = std::ptr::null_mut();
    // SAFETY: Windows owns the returned allocation until CredFree; target is NUL terminated.
    let bytes = unsafe {
        anyhow::ensure!(
            CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) != 0,
            "Credential Manager read failed"
        );
        let entry = &*credential;
        let value = if entry.CredentialBlobSize == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(entry.CredentialBlob, entry.CredentialBlobSize as usize)
                .to_vec()
        };
        CredFree(credential.cast());
        value
    };
    Ok(String::from_utf8(bytes)?)
}

/// Writes one user-scoped Credential Manager entry.
///
/// # Arguments
/// * `service` - Installation-specific vault namespace.
/// * `account` - Account UUID or the active-account reference.
/// * `value` - Secret or non-secret reference stored by the OS.
///
/// # Errors
/// Returns a vault error without including the supplied value.
pub(super) fn write(service: &str, account: &str, value: &str) -> Result<()> {
    let mut target = wide(&format!("{service}/{account}"));
    let mut username = wide(account);
    let mut bytes = value.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        UserName: username.as_mut_ptr(),
        CredentialBlobSize: u32::try_from(bytes.len())?,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: All buffers outlive this synchronous call and contain valid pointers/lengths.
    anyhow::ensure!(
        unsafe { CredWriteW(&credential, 0) } != 0,
        "Credential Manager write failed"
    );
    Ok(())
}

/// Deletes one credential, accepting an already absent entry.
///
/// # Arguments
/// * `service` - Installation-specific namespace.
/// * `account` - Account UUID or active-account reference.
///
/// # Errors
/// Returns a vault error other than not-found.
pub(super) fn delete(service: &str, account: &str) -> Result<()> {
    let target = wide(&format!("{service}/{account}"));
    // SAFETY: The target is NUL terminated and lives through both OS calls.
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        anyhow::ensure!(
            unsafe { GetLastError() } == ERROR_NOT_FOUND,
            "Credential Manager delete failed"
        );
    }
    Ok(())
}
