#!/usr/bin/env python3
"""Stage pinned fx release assets in platform crates. This never publishes crates."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "crates/harnel-sys/native-release.json"
TARGETS = [
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin", "x86_64-pc-windows-msvc",
]


def read_asset(name, release, directory):
    if directory:
        return (directory / name).read_bytes()
    url = f"https://github.com/{release['repository']}/releases/download/{release['tag']}/{name}"
    with urllib.request.urlopen(url, timeout=120) as response:
        data = response.read(64 * 1024 * 1024 + 1)
    if len(data) > 64 * 1024 * 1024:
        raise ValueError(f"Native release asset exceeds 64 MiB: {name}")
    return data


def stage(target, release, directory, record):
    name = f"fx-native-{target}.tar.gz"
    data = read_asset(name, release, directory)
    checksum = hashlib.sha256(data).hexdigest()
    expected = (read_asset(name + ".sha256", release, directory).decode().split()[0]
                if record else release["targets"][target]["archive_sha256"])
    if checksum != expected:
        raise ValueError(f"Native archive checksum mismatch: {target}")
    library = "lib/fx_core.lib" if "windows" in target else "lib/libfx_core.a"
    allowed = {library, "include/fx.h", "manifest.json", "LICENSE", "THIRD_PARTY_NOTICES.md"}
    package = ROOT / "crates" / f"harnel-native-{target}"
    with tempfile.TemporaryDirectory(prefix="stage-", dir=package) as temporary:
        staged = Path(temporary)
        seen = set()
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            for member in archive:
                if member.isdir() and member.name in {"lib", "include"}:
                    continue
                if (not member.isfile() or member.name not in allowed or member.name in seen
                        or member.size > 64 * 1024 * 1024):
                    raise ValueError(f"Unexpected native archive member: {member.name}")
                seen.add(member.name)
                destination = staged / member.name
                destination.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, destination.open("wb") as output:
                    shutil.copyfileobj(source, output)
        if seen != allowed:
            raise ValueError(f"Incomplete native archive: {target}")
        manifest = json.loads((staged / "manifest.json").read_text())
        for key, value in {"schema_version": 1, "target": target, "revision": release["revision"],
                           "version": release["tag"].removeprefix("v"), "abi_version": release["abi_version"],
                           "linkage": "static", "library": library}.items():
            if manifest.get(key) != value:
                raise ValueError(f"Native {key} mismatch: {target}")
        library_checksum = hashlib.sha256((staged / library).read_bytes()).hexdigest()
        if library_checksum != manifest["library_sha256"]:
            raise ValueError(f"Native library checksum mismatch: {target}")
        if not record and library_checksum != release["targets"][target]["library_sha256"]:
            raise ValueError(f"Native library differs from release lock: {target}")
        destination = package / "native"
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(staged, destination)
    print(f"Staged {target} from {release['tag']} ({release['revision']})")
    return {"archive_sha256": checksum, "library_sha256": library_checksum}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--all", action="store_true")
    parser.add_argument("--artifacts-dir", type=Path)
    parser.add_argument("--record", action="store_true", help="Record checksums when updating the pinned release")
    args = parser.parse_args()
    release = json.loads(LOCK.read_text())
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    targets = TARGETS if args.all else [args.target or host]
    for target in targets:
        if target not in TARGETS:
            raise SystemExit(f"Unsupported target: {target}")
        record = stage(target, release, args.artifacts_dir, args.record)
        if args.record:
            release["targets"][target] = record
    if args.record:
        LOCK.write_text(json.dumps(release, indent=2) + "\n")


if __name__ == "__main__":
    main()
