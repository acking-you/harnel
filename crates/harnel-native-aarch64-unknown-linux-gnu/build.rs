use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let manifest_path = root.join("native/manifest.json");
    println!("cargo:rerun-if-changed={}", root.join("native").display());
    let text = fs::read_to_string(&manifest_path)
        .expect("Native release payload missing; maintainers must run scripts/prepare-native.py");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("native manifest JSON");
    let target = env::var("TARGET").unwrap();
    assert_eq!(manifest["schema_version"], 1, "unsupported native manifest");
    assert_eq!(manifest["abi_version"], 1, "unsupported native ABI");
    assert_eq!(
        manifest["target"].as_str(),
        Some(target.as_str()),
        "native target mismatch"
    );
    assert_eq!(manifest["linkage"], "static", "expected a static archive");
    let archive = if target.ends_with("windows-msvc") {
        "fx_core.lib"
    } else {
        "libfx_core.a"
    };
    let directory = root.join("native/lib");
    let bytes = fs::read(directory.join(archive)).expect("native static archive");
    let digest = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        manifest["library_sha256"].as_str(),
        Some(digest.as_str()),
        "native checksum mismatch"
    );
    println!("cargo:lib_dir={}", directory.display());
    println!(
        "cargo:revision={}",
        manifest["revision"].as_str().expect("native revision")
    );
}
