use lajufile_core::cookie::{protect_cookie, unprotect_cookie};

#[test]
fn protects_and_unprotects_cookie() {
    let encrypted = protect_cookie("session=abc; theme=dark").unwrap();
    assert!(!encrypted.is_empty());
    assert!(!encrypted.windows(11).any(|window| window == b"session=abc"));
    assert_eq!(
        unprotect_cookie(&encrypted).unwrap(),
        "session=abc; theme=dark"
    );
}

#[test]
fn rejects_empty_cookie() {
    assert!(protect_cookie("").is_err());
    assert!(protect_cookie("   ").is_err());
}

#[test]
fn rejects_corrupt_ciphertext() {
    assert!(unprotect_cookie(&[1, 2, 3, 4]).is_err());
    assert!(unprotect_cookie(&[]).is_err());
}
