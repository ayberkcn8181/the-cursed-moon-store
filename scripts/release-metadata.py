#!/usr/bin/env python3
"""Publish only version bumps on main or matching explicit release tags."""
import os
import re
import subprocess
import tomllib


def version_at(ref):
    manifest = subprocess.check_output(["git", "show", f"{ref}:Cargo.toml"])
    version = tomllib.loads(manifest.decode())["workspace"]["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Invalid release version")
    return version


def should_publish(event, ref, before, current, previous=None):
    if event != "push":
        return False
    if ref.startswith("refs/tags/"):
        if ref != f"refs/tags/v{current}":
            raise ValueError("Tag does not match package version")
        return True
    if ref != "refs/heads/main" or not before or set(before) == {"0"} or previous is None:
        return False
    return tuple(map(int, current.split("."))) > tuple(map(int, previous.split(".")))


if __name__ == "__main__":
    current = version_at("HEAD")
    event = os.environ.get("GITHUB_EVENT_NAME", "")
    ref = os.environ.get("GITHUB_REF", "")
    before = os.environ.get("TCMS_BEFORE", "")
    previous = None
    if event == "push" and ref == "refs/heads/main" and re.fullmatch(r"[0-9a-f]{40}", before) and set(before) != {"0"}:
        previous = version_at(before)
    publish = should_publish(event, ref, before, current, previous)
    print(f"version={current}\ntag=v{current}\npublish={str(publish).lower()}")
