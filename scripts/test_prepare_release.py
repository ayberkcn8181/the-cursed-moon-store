#!/usr/bin/env python3
"""Exercise release snapshot and tag guards without requiring Rust or Arch."""

import gzip
import hashlib
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("prepare-release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.original_root = release.ROOT
        release.ROOT = self.root
        self.addCleanup(setattr, release, "ROOT", self.original_root)
        (self.root / "data").mkdir()
        (self.root / "packaging/arch").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.1"\n')
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "tcms-app"\nversion = "0.1.1"\n')
        (self.root / "data/com.cursedmoon.Store.metainfo.xml").write_text(
            '<component><releases><release version="0.1.1"/></releases></component>'
        )
        shutil.copy(self.original_root / "packaging/arch/PKGBUILD.in", self.root / "packaging/arch")
        self.git("init", "-q")
        self.git("config", "user.name", "Release test")
        self.git("config", "user.email", "test@example.invalid")
        self.commit()

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args])

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def test_snapshot_checksum_and_repeatability(self):
        self.git("tag", "v0.1.1")
        output = self.root / "out"
        release.prepare(output, "v0.1.1")
        archive = output / "the-cursed-moon-store-0.1.1.tar.gz"
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.assertIn(checksum, (output / "PKGBUILD").read_text())
        self.assertNotIn("@VERSION@", (output / "PKGBUILD").read_text())
        self.assertEqual((output / "SOURCE_COMMIT").read_bytes(), self.git("rev-parse", "HEAD"))
        with tarfile.open(archive) as source:
            self.assertIn("the-cursed-moon-store-0.1.1/Cargo.lock", source.getnames())
            self.assertFalse(any("/.git/" in p for p in source.getnames()))
        release.prepare(self.root / "second", "v0.1.1")
        self.assertEqual(archive.read_bytes(), (self.root / "second" / archive.name).read_bytes())
        self.assertTrue(gzip.decompress(archive.read_bytes()))

    def test_wrong_tag_version_rejected(self):
        with self.assertRaisesRegex(ValueError, "does not match"):
            release.prepare(self.root / "out", "v0.2.0")

    def test_tag_at_other_commit_rejected(self):
        self.git("tag", "v0.1.1")
        (self.root / "README").write_text("changed")
        self.commit()
        with self.assertRaisesRegex(ValueError, "checked-out commit"):
            release.prepare(self.root / "out", "v0.1.1")

    def test_dirty_sources_rejected(self):
        (self.root / "Cargo.toml").write_text("uncommitted change")
        with self.assertRaisesRegex(ValueError, "Commit tracked changes"):
            release.prepare(self.root / "out")

    def test_stale_lock_version_rejected(self):
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "tcms-app"\nversion = "0.1.0"\n')
        self.commit()
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            release.prepare(self.root / "out")

    def test_existing_output_preserved(self):
        output = self.root / "out"
        output.mkdir()
        (output / "keep").write_text("keep me")
        with self.assertRaisesRegex(ValueError, "not empty"):
            release.prepare(output)
        self.assertEqual((output / "keep").read_text(), "keep me")


if __name__ == "__main__":
    unittest.main()
