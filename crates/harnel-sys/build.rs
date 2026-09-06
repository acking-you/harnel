use std::{env, path::PathBuf, process::Command};

fn main() {
    for name in ["ZIG", "HARNEL_FX_SOURCE", "HARNEL_FX_LIB_DIR"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let target = env::var("TARGET").expect("Cargo target");
    let zig_target = match target.as_str() {
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-apple-darwin" => "x86_64-macos",
        "aarch64-apple-darwin" => "aarch64-macos",
        "x86_64-pc-windows-msvc" => "x86_64-windows-msvc",
        _ => {
            panic!(
                "Harnel supports Linux GNU/macOS on x86_64/aarch64 and Windows MSVC on x86_64; got {target}"
            )
        }
    };
    let library = if let Some(path) = env::var_os("HARNEL_FX_LIB_DIR") {
        PathBuf::from(path)
    } else {
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let source = env::var_os("HARNEL_FX_SOURCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let bundled = manifest.join("native");
                let checkout = manifest.join("../../vendor/fx");
                if checkout.join("src/c_api_main.zig").is_file() {
                    checkout
                } else {
                    bundled
                }
            })
            .canonicalize()
            .expect("fx sources missing: run git submodule update --init --recursive");
        for entry in ["src", "build.zig", "build.zig.zon"] {
            println!("cargo:rerun-if-changed={}", source.join(entry).display());
        }
        let revision_file = source.join("REVISION");
        let revision = if revision_file.is_file() {
            println!("cargo:rerun-if-changed={}", revision_file.display());
            std::fs::read_to_string(revision_file)
                .expect("read native source revision")
                .trim()
                .to_owned()
        } else {
            let metadata = Command::new("git")
                .current_dir(&source)
                .args(["rev-parse", "HEAD", "--absolute-git-dir"])
                .output()
                .expect("read fx Git metadata");
            assert!(
                metadata.status.success(),
                "fx archive needs a REVISION file"
            );
            let metadata = String::from_utf8(metadata.stdout).expect("UTF-8 Git metadata");
            let mut lines = metadata.lines();
            let revision = lines.next().expect("fx revision").to_owned();
            let git_dir = PathBuf::from(lines.next().expect("fx Git directory"));
            for entry in ["HEAD", "refs", "packed-refs"] {
                println!("cargo:rerun-if-changed={}", git_dir.join(entry).display());
            }
            revision
        };
        assert!(
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid fx source revision"
        );
        let zig = env::var_os("ZIG").unwrap_or_else(|| "zig".into());
        let version = Command::new(&zig)
            .arg("version")
            .output()
            .expect("Zig 0.16.0 is required; set ZIG to its executable");
        assert!(
            version.status.success() && String::from_utf8_lossy(&version.stdout).trim() == "0.16.0",
            "Harnel requires Zig 0.16.0"
        );
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        let prefix = out.join("native");
        let status = Command::new(zig)
            .current_dir(&source)
            .args([
                "build",
                "libfx",
                "-j1",
                &format!("-Dtarget={zig_target}"),
                &format!("-Dgit-revision={revision}"),
                "--prefix",
            ])
            .arg(&prefix)
            .arg("--cache-dir")
            .arg(out.join("zig-cache"))
            .status()
            .expect("start Zig build");
        assert!(status.success(), "Native fx build failed");
        prefix.join("lib")
    };
    let archive = if target.ends_with("windows-msvc") {
        "fx_core.lib"
    } else {
        "libfx_core.a"
    };
    assert!(
        library.join(archive).is_file(),
        "{archive} missing from {}",
        library.display()
    );
    println!("cargo:rerun-if-changed={}", library.join(archive).display());
    println!("cargo:rustc-link-search=native={}", library.display());
    println!("cargo:rustc-link-lib=static=fx_core");
    if target.contains("linux") {
        for lib in ["pthread", "dl", "m"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    } else if target.ends_with("windows-msvc") {
        // A static archive does not propagate Zig's Windows import libraries.
        for lib in ["kernel32", "ntdll", "ws2_32", "crypt32"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
