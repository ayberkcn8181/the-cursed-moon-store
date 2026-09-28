#!/usr/bin/env python3
"""Package committed sources and generate a checksum-pinned Arch recipe."""

import argparse
import gzip
import hashlib
from pathlib import Path
import re
import subprocess
import tomllib
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parent.parent


def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args])


def prepare(output, tag=None):
    # Never silently ship a different snapshot than the files a maintainer sees.
    if git("status", "--porcelain", "--untracked-files=no").strip():
        raise ValueError("Commit tracked changes before preparing a release")
    manifest = tomllib.loads(git("show", "HEAD:Cargo.toml").decode())
    version = manifest["workspace"]["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Release version must be MAJOR.MINOR.PATCH")
    if tag is not None:
        if tag != f"v{version}":
            raise ValueError(f"Tag {tag!r} does not match Cargo version v{version}")
        if git("rev-parse", f"refs/tags/{tag}^{{commit}}") != git("rev-parse", "HEAD"):
            raise ValueError("Release tag must point to the checked-out commit")
    metainfo = ET.fromstring(git("show", "HEAD:data/com.cursedmoon.Store.metainfo.xml"))
    if metainfo.find("releases/release").attrib["version"] != version:
        raise ValueError("AppStream release version does not match Cargo version")
    lock = tomllib.loads(git("show", "HEAD:Cargo.lock").decode())
    if any(p["version"] != version for p in lock["package"] if p["name"].startswith("tcms-")):
        raise ValueError("Cargo.lock workspace versions do not match Cargo.toml")

    output = Path(output).resolve()
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"Output directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    name = f"the-cursed-moon-store-{version}"
    archive = gzip.compress(git("archive", "--format=tar", f"--prefix={name}/", "HEAD"), mtime=0)
    (output / f"{name}.tar.gz").write_bytes(archive)
    checksum = hashlib.sha256(archive).hexdigest()
    template = git("show", "HEAD:packaging/arch/PKGBUILD.in").decode()
    recipe = template.replace("@VERSION@", version).replace("@SHA256@", checksum)
    (output / "PKGBUILD").write_text(recipe)
    (output / "SOURCE_COMMIT").write_bytes(git("rev-parse", "HEAD"))
    print(f"Prepared {name}: {checksum}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--tag", help="Require this version tag to point at HEAD")
    args = parser.parse_args()
    try:
        prepare(args.output, args.tag)
    except (ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"error: {error}\n")
