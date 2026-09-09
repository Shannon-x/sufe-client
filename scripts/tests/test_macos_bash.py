"""Exercise production shell wrappers on the native /bin/bash (macOS 3.2).

Download and cargo are stubbed; no network, build or privileged installation.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix", "native /bin/bash regression runs in macOS/Linux CI")
class NativeBashWrapperTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "ci/scripts").mkdir(parents=True)
        (self.root / "scripts").mkdir()
        for name in ("install-mihomo-sidecar.sh", "install-helper-sidecar.sh"):
            shutil.copyfile(ROOT / "ci/scripts" / name, self.root / "ci/scripts" / name)
        (self.root / "scripts/install-kernel.py").write_text("import json, sys\nprint(json.dumps(sys.argv[1:]))\n")
        self.env = {key: value for key, value in os.environ.items()
                    if key not in ("TARGET_TRIPLE", "MIHOMO_WINDOWS_STANDARD", "MIHOMO_LINUX_STANDARD")}
        self.env["PYTHON"] = sys.executable

    def run_script(self, name, *args):
        result = subprocess.run(["/bin/bash", str(self.root / "ci/scripts" / name), *args],
                                env=self.env, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_download_without_optional_flags(self):
        self.env["TARGET_TRIPLE"] = "aarch64-apple-darwin"
        args = json.loads(self.run_script("install-mihomo-sidecar.sh", "v1.19.30"))
        self.assertEqual(args, ["--version", "v1.19.30", "--target", "aarch64-apple-darwin"])

    def test_download_with_explicit_standard_flag(self):
        self.env.update(TARGET_TRIPLE="x86_64-unknown-linux-gnu", MIHOMO_LINUX_STANDARD="true")
        args = json.loads(self.run_script("install-mihomo-sidecar.sh", "v1.19.30"))
        self.assertEqual(args, ["--version", "v1.19.30", "--target", "x86_64-unknown-linux-gnu", "--standard"])

    def test_helper_default_host_without_target_override(self):
        fakebin = self.root / "fakebin"
        fakebin.mkdir()
        for name, script in {
            "uname": '#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo arm64;; esac\n',
            "cargo": '#!/bin/sh\nprintf "%s\\n" "$@" > "$CARGO_ARGS_CAPTURE"\n',
        }.items():
            executable = fakebin / name
            executable.write_text(script)
            executable.chmod(0o755)
        source = self.root / "target/debug/xboard-helper"
        source.parent.mkdir(parents=True)
        source.write_text("fake helper copied, never executed\n")
        capture = self.root / "cargo-args.txt"
        self.env.update(PATH=str(fakebin) + os.pathsep + self.env["PATH"], CARGO_ARGS_CAPTURE=str(capture))
        self.run_script("install-helper-sidecar.sh")
        self.assertEqual(capture.read_text().splitlines(), ["build", "--locked", "-p", "xboard-helper"])
        self.assertTrue((self.root / "desktop/src-tauri/binaries/xboard-helper-aarch64-apple-darwin").is_file())
