use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=lajufile.rc");
    println!("cargo:rerun-if-changed=lajufile.manifest");
    println!("cargo:rerun-if-changed=../../assets/lajufile.ico");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION_MAJOR");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION_MINOR");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION_PATCH");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    if std::env::var("PROFILE").as_deref() == Ok("release")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-bins=/DEBUG:NONE");
    }

    let Some(rc) = find_rc() else {
        return;
    };
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let res = out_dir.join("lajufile.res");
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap_or_else(|_| "1".into());
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap_or_else(|_| "0".into());
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap_or_else(|_| "0".into());
    let status = Command::new(rc)
        .args([
            "/nologo",
            &format!("/dLAJUFILE_VER_MAJOR={major}"),
            &format!("/dLAJUFILE_VER_MINOR={minor}"),
            &format!("/dLAJUFILE_VER_PATCH={patch}"),
            "/fo",
        ])
        .arg(&res)
        .arg("lajufile.rc")
        .status()
        .expect("failed to run rc.exe");
    assert!(status.success(), "rc.exe failed to compile lajufile.rc");
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

fn find_rc() -> Option<PathBuf> {
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(PathBuf::from("rc.exe"));
    }
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("x64").join("rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}
