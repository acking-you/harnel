#!/usr/bin/env python3
"""Build a separate application from crates.io using a fresh Cargo cache."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def verify(destination):
    # copytree deliberately refuses an existing directory to preserve user work.
    shutil.copytree(ROOT / "examples/notebook", destination,
                    ignore=shutil.ignore_patterns("target", ".harnel-state", "Cargo.lock"))
    with tempfile.TemporaryDirectory(prefix="harnel-registry-cache-") as cache:
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith(("HARNEL_", "CARGO_"))}
        environment.update(CARGO_HOME=cache, HARNEL_OFFLINE="1",
                           ZIG=str(destination / "zig-must-not-be-used"))

        def run(arguments, **options):
            directory = destination
            if arguments[0] == "cargo":
                # Cargo searches cwd ancestors even with a different CARGO_HOME.
                # Avoid inheriting a user's mirror from an ancestor .cargo directory.
                directory = Path(destination.anchor)
                arguments = [*arguments[:2], "--manifest-path", str(destination / "Cargo.toml"), *arguments[2:]]
            try:
                return subprocess.run(arguments, cwd=directory, env=environment, check=True, **options)
            except subprocess.CalledProcessError as error:
                if options.get("capture_output"):
                    print(error.stdout or "", end="")
                    print(error.stderr or "", end="", file=sys.stderr)
                raise

        run(["cargo", "build", "--release"])
        run(["cargo", "clippy", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"])
        run(["cargo", "build", "--release", "--locked", "--offline"])
        host = next(line.removeprefix("host: ") for line in subprocess.check_output(
            ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
        metadata = json.loads(run(["cargo", "metadata", "--format-version", "1", "--locked",
                                   "--offline", "--filter-platform", host], capture_output=True, text=True).stdout)
        packages = [{"name": p["name"], "version": p["version"], "source": p["source"]}
                    for p in metadata["packages"] if p["name"] == "harnel" or p["name"].startswith("harnel-")
                    if p["name"] != "harnel-notebook"]
        assert packages and any(p["name"] == "harnel" for p in packages), packages
        assert all(p["source"] == "registry+https://github.com/rust-lang/crates.io-index"
                   for p in packages), packages
        binary = destination / "target/release" / ("harnel-notebook.exe" if os.name == "nt" else "harnel-notebook")
        smoke = run([sys.executable, str(ROOT / "scripts/smoke-examples.py"), "--notebook", str(binary)],
                    capture_output=True, text=True)
        print(smoke.stdout, end="")
        revision = json.loads((ROOT / "crates/harnel-sys/native-release.json").read_text())["revision"]
        assert f"Native revision: {revision}" in smoke.stdout, smoke.stdout
        report = {"packages": packages, "host": host, "binary_bytes": binary.stat().st_size,
                  "native_revision": revision,
                  "fresh_cargo_cache": True, "offline_rebuild": True,
                  "native_compiler_disabled": True, "notebook_smoke": "passed"}
        (destination / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, help="New directory to keep the independent project and evidence")
    options = parser.parse_args()
    if options.destination:
        verify(options.destination.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix="harnel-registry-consumer-") as temporary:
            verify(Path(temporary) / "notebook")


if __name__ == "__main__":
    main()
