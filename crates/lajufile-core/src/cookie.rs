//! Windows DPAPI protection for browser cookies.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

#[derive(Debug, thiserror::Error)]
pub enum CookieError {
    #[error("DPAPI: {0}")]
    Dpapi(String),
    #[error("cookie is not valid UTF-8")]
    Utf8,
}

pub fn protect_cookie(cookie: &str) -> Result<Vec<u8>, CookieError> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err(CookieError::Dpapi("cookie is empty".to_string()));
    }
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

pub fn unprotect_cookie(ciphertext: &[u8]) -> Result<String, CookieError> {
    if ciphertext.is_empty() {
        return Err(CookieError::Dpapi("cookie blob is empty".to_string()));
    }
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
