#!/usr/bin/env python3
"""Check the release asset boundary before any archive is installed."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("prepare", Path(__file__).with_name("prepare-native.py"))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)
TARGET = "x86_64-unknown-linux-gnu"


class ReleaseAssets(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        original = prepare.ROOT
        prepare.ROOT = self.root
        self.addCleanup(setattr, prepare, "ROOT", original)
        self.package = self.root / "crates" / f"harnel-native-{TARGET}"
        self.package.mkdir(parents=True)
        self.library = b"test static archive"
        self.release = {"tag": "v0.0.8", "revision": "a" * 40, "abi_version": 1, "targets": {}}
        self.manifest = {
            "schema_version": 1, "version": "0.0.8", "revision": "a" * 40,
            "target": TARGET, "abi_version": 1, "linkage": "static",
            "library": "lib/libfx_core.a", "library_sha256": hashlib.sha256(self.library).hexdigest(),
        }

    def archive(self, extra=None):
        members = {"lib/libfx_core.a": self.library, "include/fx.h": b"header",
                   "manifest.json": json.dumps(self.manifest).encode(), "LICENSE": b"license",
                   "THIRD_PARTY_NOTICES.md": b"notices"}
        members.update(extra or {})
        path = self.root / f"fx-native-{TARGET}.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            for name, value in members.items():
                info = tarfile.TarInfo(name)
                info.size = len(value)
                archive.addfile(info, io.BytesIO(value))
        self.release["targets"][TARGET] = {
            "archive_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "library_sha256": hashlib.sha256(self.library).hexdigest(),
        }
        return path

    def test_pinned_archive_installs_exact_library(self):
        self.archive()
        prepare.stage(TARGET, self.release, self.root, False)
        self.assertEqual((self.package / "native/lib/libfx_core.a").read_bytes(), self.library)

    def test_changed_archive_does_not_replace_existing_payload(self):
        path = self.archive()
        prepare.stage(TARGET, self.release, self.root, False)
        path.write_bytes(path.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
            prepare.stage(TARGET, self.release, self.root, False)
        self.assertEqual((self.package / "native/lib/libfx_core.a").read_bytes(), self.library)

    def test_other_target_and_revision_are_rejected(self):
        for key, value in [("target", "aarch64-unknown-linux-gnu"), ("revision", "b" * 40)]:
            with self.subTest(key=key):
                old = self.manifest[key]
                self.manifest[key] = value
                self.archive()
                with self.assertRaisesRegex(ValueError, f"Native {key} mismatch"):
                    prepare.stage(TARGET, self.release, self.root, False)
                self.manifest[key] = old
                self.assertFalse((self.package / "native").exists())

    def test_unexpected_archive_paths_are_rejected(self):
        self.archive({"../../outside": b"unexpected"})
        with self.assertRaisesRegex(ValueError, "Unexpected native archive member"):
            prepare.stage(TARGET, self.release, self.root, False)
        self.assertFalse((self.package / "native").exists())


if __name__ == "__main__":
    unittest.main()
