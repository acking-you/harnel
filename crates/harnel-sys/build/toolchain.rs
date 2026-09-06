//! Compiler provisioning is used only by the explicit source-build feature.
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn valid(path: &Path) -> bool {
    Command::new(path)
        .arg("version")
        .output()
        .is_ok_and(|output| output.status.success() && output.stdout.trim_ascii() == b"0.16.0")
}

pub fn resolve() -> PathBuf {
    if let Some(path) = env::var_os("ZIG") {
        let path = PathBuf::from(path);
        assert!(
            valid(&path),
            "ZIG must name a working Zig 0.16.0 executable"
        );
        return path;
    }
    if valid(Path::new("zig")) {
        return PathBuf::from("zig");
    }
    let host = env::var("HOST").expect("Cargo host");
    let toolchains: serde_json::Value =
        serde_json::from_str(include_str!("zig-toolchains.json")).expect("pinned Zig toolchains");
    let toolchain = &toolchains[&host];
    let url = toolchain["tarball"]
        .as_str()
        .expect("No automatic Zig distribution for this host; set ZIG");
    let checksum = toolchain["shasum"].as_str().expect("Zig checksum");
    let filename = url.rsplit('/').next().unwrap();
    let directory = filename
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".zip");
    let cache = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("toolchain");
    let executable = cache.join(directory).join(if host.contains("windows") {
        "zig.exe"
    } else {
        "zig"
    });
    if cache.join("verified.sha256").is_file()
        && fs::read_to_string(cache.join("verified.sha256"))
            .ok()
            .as_deref()
            == Some(checksum)
        && valid(&executable)
    {
        return executable;
    }
    assert!(
        env::var_os("HARNEL_OFFLINE").is_none(),
        "Zig is not cached and HARNEL_OFFLINE disables downloads; set ZIG or prepare the source build online first"
    );
    println!("cargo:warning=Preparing Zig 0.16.0 for the explicit source build ({host})");
    download(url, checksum, &cache)
        .expect("download and verify Zig 0.16.0; set ZIG for an offline compiler");
    assert!(
        valid(&executable),
        "Downloaded Zig 0.16.0 did not run on this host"
    );
    executable
}

fn download(url: &str, checksum: &str, cache: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let staging = cache.with_extension("partial");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut response = ureq::get(url)
            .config()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .call()?;
        let archive_path = staging.join("download");
        let mut archive = fs::File::create(&archive_path)?;
        let mut reader = response.body_mut().as_reader();
        let mut hash = Sha256::new();
        let mut bytes = [0; 64 * 1024];
        let mut total = 0usize;
        loop {
            let count = reader.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            total += count;
            if total > 200 * 1024 * 1024 {
                return Err("Zig archive exceeds 200 MiB".into());
            }
            hash.update(&bytes[..count]);
            archive.write_all(&bytes[..count])?;
        }
        drop(archive);
        if format!("{:x}", hash.finalize()) != checksum {
            return Err("Zig archive checksum mismatch".into());
        }
        if url.ends_with(".zip") {
            zip::ZipArchive::new(fs::File::open(&archive_path)?)?.extract(&staging)?;
        } else {
            let tar_path = staging.join("download.tar");
            let mut input = io::BufReader::new(fs::File::open(&archive_path)?);
            let mut output = fs::File::create(&tar_path)?;
            lzma_rs::xz_decompress(&mut input, &mut output)?;
            drop(output);
            tar::Archive::new(fs::File::open(&tar_path)?).unpack(&staging)?;
            fs::remove_file(tar_path)?;
        }
        fs::remove_file(archive_path)?;
        fs::write(staging.join("verified.sha256"), checksum)?;
        if cache.exists() {
            fs::remove_dir_all(cache)?;
        }
        fs::rename(&staging, cache)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}
