//! Per-user protection for browser cookies (Windows DPAPI or Linux 0600 user key).

#[derive(Debug, thiserror::Error)]
pub enum CookieError {
    #[error("DPAPI: {0}")]
    Dpapi(String),
    #[error("cookie is not valid UTF-8")]
    Utf8,
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    use super::CookieError;

    pub fn protect(cookie: &str) -> Result<Vec<u8>, CookieError> {
        let mut input = cookie.as_bytes().to_vec();
        let input_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptProtectData(
                &input_blob,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(CookieError::Dpapi(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            LocalFree(output.pbData as *mut c_void);
        }
        Ok(bytes)
    }

    pub fn unprotect(ciphertext: &[u8]) -> Result<String, CookieError> {
        let mut input = ciphertext.to_vec();
        let input_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptUnprotectData(
                &input_blob,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(CookieError::Dpapi(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            LocalFree(output.pbData as *mut c_void);
        }
        String::from_utf8(bytes).map_err(|_| CookieError::Utf8)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::os::unix::fs::PermissionsExt;

    use super::{CookieError, open_with_key, seal_with_key};
    use crate::paths;

    fn user_key() -> Result<[u8; 32], CookieError> {
        let dir = paths::data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| CookieError::Dpapi(e.to_string()))?;
        let key_path = dir.join(".cookie.key");
        if let Ok(bytes) = std::fs::read(&key_path)
            && let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice())
        {
            return Ok(key);
        }
        let key = rand::random::<[u8; 32]>();
        std::fs::write(&key_path, key).map_err(|e| CookieError::Dpapi(e.to_string()))?;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
        Ok(key)
    }

    pub fn protect(cookie: &str) -> Result<Vec<u8>, CookieError> {
        let key = user_key()?;
        Ok(seal_with_key(&key, cookie))
    }

    pub fn unprotect(ciphertext: &[u8]) -> Result<String, CookieError> {
        let key = user_key()?;
        open_with_key(&key, ciphertext)
    }
}

#[cfg_attr(windows, allow(dead_code))]
const ENVELOPE_MAGIC: &[u8; 3] = b"LF2";

#[cfg_attr(windows, allow(dead_code))]
fn derive_subkeys(master: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    (
        crate::checksum::hmac_sha256(master, b"lajufile-cookie-enc-v2"),
        crate::checksum::hmac_sha256(master, b"lajufile-cookie-mac-v2"),
    )
}

#[cfg_attr(windows, allow(dead_code))]
fn xor_ctr_sha256(enc_key: &[u8; 32], nonce: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut block_in = [0u8; 20];
    block_in[..16].copy_from_slice(nonce);
    for (idx, chunk) in data.chunks(32).enumerate() {
        block_in[16..20].copy_from_slice(&(idx as u32).to_le_bytes());
        let mask = crate::checksum::hmac_sha256(enc_key, &block_in);
        for (&b, &m) in chunk.iter().zip(mask.iter()) {
            out.push(b ^ m);
        }
    }
    out
}

#[cfg_attr(windows, allow(dead_code))]
fn compute_cookie_mac(mac_key: &[u8; 32], nonce: &[u8], body: &[u8]) -> [u8; 32] {
    let mut signed = Vec::with_capacity(ENVELOPE_MAGIC.len() + nonce.len() + body.len());
    signed.extend_from_slice(ENVELOPE_MAGIC);
    signed.extend_from_slice(nonce);
    signed.extend_from_slice(body);
    crate::checksum::hmac_sha256(mac_key, &signed)
}

#[cfg_attr(windows, allow(dead_code))]
fn seal_with_key(key: &[u8; 32], cookie: &str) -> Vec<u8> {
    let (enc_key, mac_key) = derive_subkeys(key);
    let nonce = rand::random::<[u8; 16]>();
    let body = xor_ctr_sha256(&enc_key, &nonce, cookie.as_bytes());
    let tag = compute_cookie_mac(&mac_key, &nonce, &body);
    let mut out = Vec::with_capacity(ENVELOPE_MAGIC.len() + 16 + 32 + body.len());
    out.extend_from_slice(ENVELOPE_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&tag);
    out.extend_from_slice(&body);
    out
}

#[cfg_attr(windows, allow(dead_code))]
fn open_with_key(key: &[u8; 32], ciphertext: &[u8]) -> Result<String, CookieError> {
    const HDR_LEN: usize = 3 + 16 + 32;
    if ciphertext.len() <= HDR_LEN || !ciphertext.starts_with(ENVELOPE_MAGIC) {
        return Err(CookieError::Dpapi("invalid cookie blob".to_string()));
    }
    let (enc_key, mac_key) = derive_subkeys(key);
    let nonce: [u8; 16] = ciphertext[3..19].try_into().unwrap();
    let tag = &ciphertext[19..HDR_LEN];
    let body = &ciphertext[HDR_LEN..];
    let expected = compute_cookie_mac(&mac_key, &nonce, body);
    if !crate::checksum::constant_time_eq(&expected, tag) {
        return Err(CookieError::Dpapi(
            "cookie authentication failed".to_string(),
        ));
    }
    let plain = xor_ctr_sha256(&enc_key, &nonce, body);
    String::from_utf8(plain).map_err(|_| CookieError::Utf8)
}

pub fn protect_cookie(cookie: &str) -> Result<Vec<u8>, CookieError> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err(CookieError::Dpapi("cookie is empty".to_string()));
    }
    platform::protect(cookie)
}

pub fn unprotect_cookie(ciphertext: &[u8]) -> Result<String, CookieError> {
    if ciphertext.is_empty() {
        return Err(CookieError::Dpapi("cookie blob is empty".to_string()));
    }
    platform::unprotect(ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_cookie_roundtrip_and_tamper_check() {
        let key = [7u8; 32];
        let payload = "sid=123; session=αβγδεζηθ; path=/; HttpOnly; Secure; extra=0123456789abcdef0123456789abcdef0123456789abcdef";
        let sealed_a = seal_with_key(&key, payload);
        let sealed_b = seal_with_key(&key, payload);
        assert!(sealed_a.starts_with(b"LF2"));
        assert_ne!(sealed_a, sealed_b);
        assert_eq!(open_with_key(&key, &sealed_a).unwrap(), payload);
        assert_eq!(open_with_key(&key, &sealed_b).unwrap(), payload);

        for idx in [0usize, 5, 25, sealed_a.len() - 1] {
            let mut tampered = sealed_a.clone();
            tampered[idx] ^= 1;
            assert!(
                open_with_key(&key, &tampered).is_err(),
                "tampered byte {idx} must fail auth"
            );
        }
        assert!(open_with_key(&key, &sealed_a[..sealed_a.len() - 1]).is_err());
    }
}
