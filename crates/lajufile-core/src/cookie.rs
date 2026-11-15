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
    use rand::RngCore;
    use std::os::unix::fs::PermissionsExt;

    use super::CookieError;
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
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        std::fs::write(&key_path, key).map_err(|e| CookieError::Dpapi(e.to_string()))?;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
        Ok(key)
    }

    fn keystream_byte(key: &[u8; 32], nonce: &[u8; 16], index: usize) -> u8 {
        let mut acc = (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        for (i, &k) in key.iter().enumerate() {
            let n = nonce[i % 16] as u64;
            acc = acc
                .wrapping_add((k as u64) ^ n.rotate_left((i as u32) * 3))
                .rotate_left(7)
                .wrapping_mul(0xBF58_476D_1CE4_E5B9);
        }
        (acc ^ (acc >> 32)) as u8
    }

    fn compute_tag(key: &[u8; 32], nonce: &[u8; 16], body: &[u8]) -> [u8; 16] {
        let mut state = [0u64; 2];
        for i in 0..16 {
            state[i / 8] |= (nonce[i] as u64) << ((i % 8) * 8);
        }
        for (idx, &b) in body.iter().chain(key.iter()).enumerate() {
            let lane = idx & 1;
            state[lane] = state[lane]
                .wrapping_add((b as u64).wrapping_mul(0x94D0_49BB_1331_11EB))
                .rotate_left(13)
                ^ state[1 - lane];
        }
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&state[0].to_le_bytes());
        out[8..].copy_from_slice(&state[1].to_le_bytes());
        out
    }

    pub fn protect(cookie: &str) -> Result<Vec<u8>, CookieError> {
        let key = user_key()?;
        let mut nonce = [0u8; 16];
        rand::rng().fill_bytes(&mut nonce);
        let body: Vec<u8> = cookie
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ keystream_byte(&key, &nonce, i))
            .collect();
        let tag = compute_tag(&key, &nonce, &body);
        let mut out = Vec::with_capacity(32 + body.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&tag);
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn unprotect(ciphertext: &[u8]) -> Result<String, CookieError> {
        if ciphertext.len() <= 32 {
            return Err(CookieError::Dpapi("invalid cookie blob".to_string()));
        }
        let key = user_key()?;
        let nonce: [u8; 16] = ciphertext[..16].try_into().unwrap();
        let tag = &ciphertext[16..32];
        let body = &ciphertext[32..];
        if compute_tag(&key, &nonce, body) != tag {
            return Err(CookieError::Dpapi(
                "cookie authentication failed".to_string(),
            ));
        }
        let plain: Vec<u8> = body
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ keystream_byte(&key, &nonce, i))
            .collect();
        String::from_utf8(plain).map_err(|_| CookieError::Utf8)
    }
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
