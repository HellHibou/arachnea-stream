use aes::Aes128;
use anyhow::{bail, Result};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Decrypts hexadecimal AES-128-CBC values with UTF-8 key and IV strings.
///
/// Inputs with invalid hexadecimal data, a key or IV other than 16 bytes, invalid
/// PKCS#7 padding, or non-UTF-8 plaintext are discarded.
pub(super) fn apply(texts: Vec<String>, key: &str, iv: &str) -> Vec<String> {
    texts
        .into_iter()
        .filter_map(|value| decrypt(value.trim(), key.as_bytes(), iv.as_bytes()))
        .collect()
}

/// Validates that the configured AES-128-CBC key and IV have usable lengths.
pub(super) fn validate(key: &str, iv: &str) -> Result<()> {
    if key.len() != 16 {
        bail!("aes_cbc_decrypt key must contain exactly 16 UTF-8 bytes");
    }
    if iv.len() != 16 {
        bail!("aes_cbc_decrypt iv must contain exactly 16 UTF-8 bytes");
    }
    Ok(())
}

fn decrypt(value: &str, key: &[u8], iv: &[u8]) -> Option<String> {
    if key.len() != 16 || iv.len() != 16 || value.len() % 2 != 0 || !value.is_ascii() {
        return None;
    }

    let mut ciphertext = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        })
        .collect::<Option<Vec<_>>>()?;

    let plaintext = Aes128CbcDec::new_from_slices(key, iv)
        .ok()?
        .decrypt_padded_mut::<Pkcs7>(&mut ciphertext)
        .ok()?;

    String::from_utf8(plaintext.to_vec()).ok()
}
