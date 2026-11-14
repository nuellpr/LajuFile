use lajufile_core::naming::{
    dedupe_path, resolve_file_name, sanitize_file_name, temp_path_for, truncate_file_name,
};
use url::Url;

#[test]
fn sanitizes_windows_invalid_characters() {
    assert_eq!(
        sanitize_file_name(r#"a<b>c:d"e/f\g|h?i*"#),
        "a_b_c_d_e_f_g_h_i_"
    );
}

#[test]
fn trims_trailing_dots_and_spaces_and_empty_becomes_download() {
    assert_eq!(sanitize_file_name("report.pdf... "), "report.pdf");
    assert_eq!(sanitize_file_name("   "), "download");
}

#[test]
fn truncates_to_180_chars_preserving_extension() {
    let name = format!("{}.pdf", "a".repeat(300));
    let out = truncate_file_name(&name, 180);
    assert!(out.chars().count() <= 180);
    assert!(out.ends_with(".pdf"));
}

#[test]
fn explicit_name_wins_over_content_disposition() {
    let url = Url::parse("https://example.com/from-url.bin").unwrap();
    let cd = Some("attachment; filename*=UTF-8''from-cd%20file.iso");
    assert_eq!(
        resolve_file_name(Some("explicit.zip"), cd, &url),
        "explicit.zip"
    );
}

#[test]
fn utf8_content_disposition_wins_over_url() {
    let url = Url::parse("https://example.com/from-url.bin").unwrap();
    let cd = Some("attachment; filename*=UTF-8''laporan%20annual.pdf");
    assert_eq!(resolve_file_name(None, cd, &url), "laporan annual.pdf");
}

#[test]
fn url_path_is_percent_decoded() {
    let url = Url::parse("https://example.com/my%20file.iso").unwrap();
    assert_eq!(resolve_file_name(None, None, &url), "my file.iso");
}

#[test]
fn content_disposition_path_traversal_is_sanitized() {
    let url = Url::parse("https://example.com/a").unwrap();
    let cd = Some(r#"attachment; filename="..\..\evil.exe""#);
    let out = resolve_file_name(None, cd, &url);
    assert_eq!(out, ".._.._evil.exe");
    assert!(!out.contains('/'));
    assert!(!out.contains('\\'));
}

#[test]
fn missing_name_falls_back_to_download() {
    let url = Url::parse("https://example.com/").unwrap();
    assert_eq!(resolve_file_name(None, None, &url), "download");
}

#[test]
fn dedupes_existing_file_and_temp() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file.zip"), b"x").unwrap();
    assert_eq!(
        dedupe_path(dir.path(), "file.zip").file_name().unwrap(),
        "file (1).zip"
    );
    std::fs::write(dir.path().join("file (1).zip.lajufile"), b"x").unwrap();
    assert_eq!(
        dedupe_path(dir.path(), "file.zip").file_name().unwrap(),
        "file (2).zip"
    );
}

#[test]
fn temp_path_appends_suffix() {
    let path = temp_path_for(std::path::Path::new(r"D:\Downloads\file.zip"));
    assert_eq!(
        path,
        std::path::PathBuf::from(r"D:\Downloads\file.zip.lajufile")
    );
}

#[test]
fn reserved_windows_device_names_are_prefixed() {
    assert_eq!(sanitize_file_name("CON"), "_CON");
    assert_eq!(sanitize_file_name("con.txt"), "_con.txt");
    assert_eq!(sanitize_file_name("AUX"), "_AUX");
    assert_eq!(sanitize_file_name("COM1"), "_COM1");
    assert_eq!(sanitize_file_name("COM0"), "COM0");
    assert_eq!(sanitize_file_name("LPT10"), "LPT10");
}
