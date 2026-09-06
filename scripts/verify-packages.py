#!/usr/bin/env python3
"""Verify actual .crate packages through an offline registry substitute, without publishing."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target/verified-packages"
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def run(arguments, **options):
    subprocess.run(arguments, cwd=ROOT, check=True, **options)


def add_to_registry(archive, vendor):
    with tarfile.open(archive) as package:
        package.extractall(vendor, filter="data")
    directory = vendor / archive.name.removesuffix(".crate")
    files = {str(path.relative_to(directory)).replace(os.sep, "/"): hashlib.sha256(path.read_bytes()).hexdigest()
             for path in directory.rglob("*") if path.is_file()}
    (directory / ".cargo-checksum.json").write_text(json.dumps({
        "files": files, "package": hashlib.sha256(archive.read_bytes()).hexdigest()}))
    shutil.copyfile(archive, OUTPUT / archive.name)
    return directory


def verify(work):
    # A failed compiler path also detects accidental provisioning when Zig is
    # installed on a maintainer's machine. CI additionally starts without Zig.
    os.environ["ZIG"] = str(work / "zig-must-not-be-used")
    os.environ["HARNEL_OFFLINE"] = "1"
    os.environ.pop("HARNEL_FX_LIB_DIR", None)
    os.environ.pop("HARNEL_FX_SOURCE", None)
    if OUTPUT.exists():
        shutil.rmtree(OUTPUT)
    OUTPUT.mkdir(parents=True)
    vendor = work / "registry"
    run(["cargo", "vendor", "--locked", "--respect-source-config", str(vendor)], stdout=subprocess.DEVNULL)
    configuration = work / "registry.toml"
    configuration.write_text('[source.crates-io]\nreplace-with = "harnel-verification"\n'
                             '[source.harnel-verification]\ndirectory = ' + json.dumps(str(vendor)) + '\n')
    cargo_config = ["--config", str(configuration), "--offline"]
    packages = sorted(ROOT.glob("crates/harnel-native-*"))
    if len(packages) != 5:
        raise RuntimeError("Expected all five native platform crates")
    for directory in packages:
        if not (directory / "native/manifest.json").is_file():
            raise RuntimeError("Run scripts/prepare-native.py --all before packaging")
        run(["cargo", "package", "--manifest-path", str(directory / "Cargo.toml"),
             "--target-dir", str(work / "build"), "--allow-dirty", "--no-verify", *cargo_config])
        archive = work / "build/package" / f"{directory.name}-{VERSION}.crate"
        add_to_registry(archive, vendor)
    for name in ["harnel-sys", "harnel"]:
        run(["cargo", "package", "-p", name, "--target-dir", str(work / "build"), "--allow-dirty", *cargo_config])
        add_to_registry(work / "build/package" / f"{name}-{VERSION}.crate", vendor)
    consumer = work / "consumer"
    (consumer / "src").mkdir(parents=True)
    (consumer / "Cargo.toml").write_text(f'''[package]
name = "harnel-release-consumer"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
harnel = "={VERSION}"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
''')
    (consumer / "src/main.rs").write_text('''use harnel::Harness;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = std::env::current_dir()?;
    let harness = Harness::builder(workspace).native_tools(false).build().await?;
    let session = harness.session().await?;
    assert_eq!(harness.bash_first(true).await?["bashFirst"], true);
    assert_eq!(session.status().await?["state"], "idle");
    harness.shutdown().await?;
    println!("Packaged Harnel SDK initialized, created a session, changed shared state, and shut down.");
    Ok(())
}
''')
    run(["cargo", "run", "--manifest-path", str(consumer / "Cargo.toml"), *cargo_config])
    packaged = vendor / f"harnel-{VERSION}"
    run(["cargo", "build", "--manifest-path", str(packaged / "Cargo.toml"), "--bin", "harnel", "--examples",
         "--target-dir", str(work / "consumer-build"), *cargo_config])
    binary = work / "consumer-build/debug" / ("harnel.exe" if os.name == "nt" else "harnel")
    revision = json.loads((ROOT / "crates/harnel-sys/native-release.json").read_text())["revision"]
    actual = subprocess.check_output([str(binary), "--version"], text=True)
    if revision not in actual:
        raise RuntimeError(f"Packaged CLI linked an unexpected native revision: {actual}")
    run([sys.executable, "scripts/smoke-cli.py", str(binary)])
    run([sys.executable, "scripts/smoke-examples.py", "--examples", str(work / "consumer-build/debug/examples")])
    notebook = work / "notebook"
    shutil.copytree(ROOT / "examples/notebook", notebook, ignore=shutil.ignore_patterns("target", ".harnel-state"))
    run(["cargo", "build", "--manifest-path", str(notebook / "Cargo.toml"), *cargo_config])
    run(["cargo", "clippy", "--manifest-path", str(notebook / "Cargo.toml"), "--all-targets", *cargo_config, "--", "-D", "warnings"])
    notebook_binary = notebook / "target/debug" / ("harnel-notebook.exe" if os.name == "nt" else "harnel-notebook")
    run([sys.executable, "scripts/smoke-examples.py", "--notebook", str(notebook_binary)])
    print(f"Verified seven .crate packages from fx {revision}. No crates were published.")


def main():
    # Extracted registry packages must not inherit the repository workspace.
    with tempfile.TemporaryDirectory(prefix="harnel-package-verification-") as directory:
        verify(Path(directory))


if __name__ == "__main__":
    main()
