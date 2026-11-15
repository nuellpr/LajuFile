use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=lajufile.rc");
    println!("cargo:rerun-if-changed=../../assets/lajufile.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let Some(rc) = find_rc() else {
        return;
    };
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let res = out_dir.join("lajufile.res");
    let status = Command::new(rc)
        .args(["/nologo", "/fo"])
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
