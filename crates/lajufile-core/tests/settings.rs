use lajufile_core::settings::{Settings, Theme, generate_token};

#[test]
fn defaults_match_spec() {
    let s = Settings::default();
    assert_eq!(s.max_connections, 8);
    assert_eq!(s.max_concurrent, 3);
    assert_eq!(s.global_speed_limit_kbps, 0);
    assert!(s.close_to_tray);
    assert!(!s.autostart);
    assert!(!s.auto_resume);
    assert_eq!(s.theme, Theme::Light);
    assert_eq!(s.bridge_port, 53841);
}

#[test]
fn first_run_creates_64_hex_token_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(s.bridge_token.len(), 64);
    assert!(s.bridge_token.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(dir.path().join("settings.json").exists());
}

#[test]
fn second_load_keeps_token() {
    let dir = tempfile::tempdir().unwrap();
    let a = Settings::load_or_create(dir.path()).unwrap();
    let b = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(a.bridge_token, b.bridge_token);
}

#[test]
fn empty_token_is_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Settings::default();
    s.bridge_token.clear();
    s.save(dir.path()).unwrap();
    let loaded = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(loaded.bridge_token.len(), 64);
    assert!(loaded.bridge_token.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn invalid_json_is_an_error_and_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(Settings::load_or_create(dir.path()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn save_is_atomic_and_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings {
        max_connections: 16,
        bridge_token: generate_token(),
        ..Settings::default()
    };
    s.save(dir.path()).unwrap();
    assert!(!dir.path().join("settings.json.tmp").exists());
    let loaded = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(loaded.max_connections, 16);
    assert_eq!(loaded.bridge_token, s.bridge_token);
}

#[test]
fn generated_tokens_differ_and_are_hex() {
    let a = generate_token();
    let b = generate_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn non_hex_token_is_repaired_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings {
        bridge_token: "zz".into(),
        ..Settings::default()
    };
    s.save(dir.path()).unwrap();

    let loaded = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(loaded.bridge_token.len(), 64);
    assert!(loaded.bridge_token.chars().all(|c| c.is_ascii_hexdigit()));

    let raw = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(!raw.contains(&loaded.bridge_token));
    assert!(
        json["bridge_token"]
            .as_str()
            .is_some_and(|v| v.starts_with("enc:v1:"))
    );

    let again = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(again.bridge_token, loaded.bridge_token);
}

#[test]
fn short_token_is_repaired_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings {
        bridge_token: "a".repeat(63),
        ..Settings::default()
    };
    s.save(dir.path()).unwrap();

    let loaded = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(loaded.bridge_token.len(), 64);
    assert!(loaded.bridge_token.chars().all(|c| c.is_ascii_hexdigit()));

    let raw = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(!raw.contains(&loaded.bridge_token));
    assert!(
        json["bridge_token"]
            .as_str()
            .is_some_and(|v| v.starts_with("enc:v1:"))
    );

    let again = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(again.bridge_token, loaded.bridge_token);
}
