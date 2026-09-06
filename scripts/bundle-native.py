#!/usr/bin/env python3
"""Stage a self-contained fx source bundle for cargo package; never publish."""
import pathlib
import shutil
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[1]
source = root / "vendor/fx"
destination = root / "crates/harnel-sys/native"


def git(*arguments):
    return subprocess.check_output(["git", "-C", str(source), *arguments])


if git("status", "--porcelain").strip():
    sys.exit("Commit the fx source before staging a reproducible native bundle.")
if destination.exists():
    sys.exit(f"Bundle already exists at {destination}; remove that generated directory before restaging.")

revision = git("rev-parse", "HEAD").decode().strip()
files = git("ls-files", "-z", "src", "include", "build.zig", "build.zig.zon", "LICENSE").decode().split("\0")
destination.mkdir(parents=True)
try:
    for name in filter(None, files):
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / name, target)
    (destination / "REVISION").write_text(revision + "\n")
except Exception:
    shutil.rmtree(destination)
    raise
print(f"Staged fx {revision} in {destination}")
print("Run cargo package -p harnel-sys to verify the source package. No publication was performed.")
