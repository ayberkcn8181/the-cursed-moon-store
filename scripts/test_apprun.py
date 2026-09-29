#!/usr/bin/env python3
"""Exercise the real launcher with a fake executable; no display or GTK needed."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class AppRunTests(unittest.TestCase):
    def test_relocation_arguments_and_host_environment(self):
        with tempfile.TemporaryDirectory(prefix="tcms appdir ") as tmp:
            appdir = Path(tmp)
            (appdir / "usr/bin").mkdir(parents=True)
            data = appdir / "usr/share/the-cursed-moon-store"
            data.mkdir(parents=True)
            shutil.copy(ROOT / "packaging/appimage/environment.keys", data)
            shutil.copy(ROOT / "packaging/appimage/AppRun", appdir)
            executable = appdir / "usr/bin/the-cursed-moon-store"
            executable.write_text(
                f"#!{sys.executable}\nimport json, os, sys\n"
                "print(json.dumps({'env': dict(os.environ), 'args': sys.argv[1:]}))\n"
            )
            executable.chmod(0o755)
            env = dict(os.environ)
            for key in (ROOT / "packaging/appimage/environment.keys").read_text().splitlines():
                env.pop(key, None)
                env[f"TCMS_HOST_{key}"] = "stale value"
            env.pop("APPDIR", None)
            env.update(XDG_DATA_DIRS="/custom data:/usr/share", GTK_PATH="", GDK_BACKEND="wayland")
            out = subprocess.check_output(
                ["bash", str(appdir / "AppRun"), "arg with spaces", "--version"], env=env, text=True
            )
            result = json.loads(out)
            child = result["env"]
            self.assertEqual(result["args"], ["arg with spaces", "--version"])
            self.assertEqual(child["TCMS_APPIMAGE"], "1")
            self.assertEqual(child["TCMS_HOST_XDG_DATA_DIRS"], env["XDG_DATA_DIRS"])
            self.assertEqual(child["TCMS_HOST_GTK_PATH"], "")
            self.assertNotIn("TCMS_HOST_LD_LIBRARY_PATH", child)
            self.assertEqual(child["LD_LIBRARY_PATH"], str(appdir / "usr/lib"))
            self.assertEqual(child["PATH"], env["PATH"])
            self.assertEqual(child["GDK_BACKEND"], "wayland")
            self.assertNotIn("GIO_EXTRA_MODULES", child)


if __name__ == "__main__":
    unittest.main()
