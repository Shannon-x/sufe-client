"""Exercise the real installer in an isolated filesystem, mocking root tools.

No elevation, real setcap, VPN, or system file writes occur in these tests.
"""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "desktop/src-tauri/build_extras/install-linux-kernel.sh"


def posix(path):
    text = Path(path).as_posix()
    return '/' + text[0].lower() + text[2:] if os.name == 'nt' else text


def function(source, name):
    start = source.index('fn ' + name + '(')
    brace = source.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        if source[end] == '{': depth += 1
        if source[end] == '}': depth -= 1
        end += 1
    return source[start:end]


class LinuxInstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.shell = 'C:/Program Files/Git/bin/bash.exe' if os.name == 'nt' else '/bin/sh'
        if not Path(self.shell).exists(): self.skipTest('A POSIX shell is required')
        self.bin = self.root / 'mock-bin'
        self.bin.mkdir()
        self.destdir = self.root / 'protected'
        self.dest = self.destdir / 'mihomo'
        self.source = self.root / 'bundled-kernel'
        self.source.write_bytes(b'known kernel snapshot')
        self.digest = hashlib.sha256(self.source.read_bytes()).hexdigest()
        for name, body in {
            'id': "echo 0\n",
            'stat': 'case "$2" in %u) echo "${TEST_OWNER:-0}";; %a) echo "${TEST_MODE:-755}";; *) exit 1;; esac\n',
            'chown': 'exit 0\n',
            'chmod': 'exit 0\n',
            'mkdir': '[ "$1" != -m ] || shift 2\nexec /usr/bin/mkdir "$@"\n',
            'setcap': 'printf "%s\\n" "$*" >> "$TEST_CAP_LOG"\nexit "${TEST_CAP_EXIT:-0}"\n',
        }.items():
            path = self.bin / name
            path.write_text('#!/bin/sh\n' + body, newline='\n', encoding='utf-8')
            path.chmod(0o755)
        source = SCRIPT.read_text(encoding="utf-8").replace('@SUFE_KERNEL_SHA256@', self.digest)
        # MSYS dd lacks Linux O_NOFOLLOW/O_NONBLOCK. Keep real hardened flags
        # on Linux CI; Windows exercises copy/hash/atomic-replacement decisions.
        if os.name == 'nt': source = source.replace('iflag=nofollow,nonblock,fullblock', 'iflag=fullblock')
        source = source.replace('PATH=/usr/sbin:/usr/bin:/sbin:/bin', f'PATH="{posix(self.bin)}:/usr/bin:/bin"')
        source = source.replace('DEST_DIR=/usr/local/lib/sufe', f'DEST_DIR="{posix(self.destdir)}"')
        source = source.replace('DEST=/usr/local/lib/sufe/mihomo', f'DEST="{posix(self.dest)}"')
        source = source.replace('for directory in / /usr /usr/local /usr/local/lib;', f'for directory in "{posix(self.root)}";')
        self.script = self.root / 'installer.sh'
        self.script.write_text(source, newline='\n', encoding='utf-8')
        self.log = self.root / 'cap.log'

    def run_installer(self, **env):
        if sys.platform == 'darwin': self.skipTest('GNU/Linux installer runs on Linux CI; pure Rust decoder checks still run on macOS')
        return subprocess.run([self.shell, posix(self.script), posix(self.source)], capture_output=True, text=True, encoding="utf-8", errors="replace",
                              env={**os.environ, 'TEST_CAP_LOG': posix(self.log), **env})

    def test_verified_copy_installed_before_any_execution(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.dest.read_bytes(), self.source.read_bytes())
        self.assertIn('cap_net_admin,cap_net_bind_service+ep', self.log.read_text(encoding="utf-8"))
        self.assertEqual(list(self.destdir.glob('.mihomo.*')), [])

    def test_production_copy_has_timeout_and_safe_open_flags(self):
        source = SCRIPT.read_text(encoding='utf-8')
        self.assertIn('/usr/bin/timeout --kill-after=2s 20s /usr/bin/dd', source)
        self.assertIn('count=128 iflag=nofollow,nonblock,fullblock', source)

    def test_hash_mismatch_keeps_old_kernel_and_grants_no_caps(self):
        self.destdir.mkdir()
        self.dest.write_bytes(b'old installed kernel')
        self.source.write_bytes(b'tampered source')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('checksum mismatch', result.stderr)
        self.assertEqual(self.dest.read_bytes(), b'old installed kernel')
        self.assertFalse(self.log.exists())
        self.assertEqual(list(self.destdir.glob('.mihomo.*')), [])

    def test_failed_setcap_does_not_replace_existing_kernel(self):
        self.destdir.mkdir()
        self.dest.write_bytes(b'old installed kernel')
        result = self.run_installer(TEST_CAP_EXIT='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.log.exists(), result.stderr)
        self.assertEqual(self.dest.read_bytes(), b'old installed kernel')
        self.assertEqual(list(self.destdir.glob('.mihomo.*')), [])

    def test_unsafe_ancestor_permissions_fail_closed(self):
        result = self.run_installer(TEST_MODE='777')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.dest.exists())
        self.assertFalse(self.log.exists())

    def test_nonroot_ancestor_fails_closed(self):
        result = self.run_installer(TEST_OWNER='1000')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.dest.exists())
        self.assertFalse(self.log.exists())

    def test_actual_rust_capability_and_consent_decoders(self):
        if not shutil.which('rustc'): self.skipTest('rustc required')
        caps = (ROOT / 'core/src/kernel/launcher.rs').read_text(encoding="utf-8")
        launcher = (ROOT / 'desktop/src-tauri/src/linux_launcher.rs').read_text(encoding="utf-8")
        # Extract only pure functions to test the exact Rust implementations on
        # Windows too, without libc/Linux linking or a native desktop build.
        code = '#![allow(dead_code)]\n#[derive(Debug)] enum LauncherError { NeedsConsent(String), NotPermitted(String), Other(String) }\n'
        code += function(caps, 'grants_net_admin') + '\n'
        code += '#[test]\n' + function(caps, 'capability_blob_requires_effective_net_admin') + '\n'
        code += function(launcher, 'authorization_result') + '\n'
        code += '#[test]\n' + function(launcher, 'cancelled_and_unavailable_authorizations_are_not_success') + '\n'
        harness = self.root / 'harness.rs'
        harness.write_text(code, encoding="utf-8")
        exe = self.root / ('harness.exe' if os.name == 'nt' else 'harness')
        result = subprocess.run(['rustc', '--edition=2021', '--test', str(harness), '-o', str(exe)], capture_output=True, text=True, encoding='utf-8', errors='replace')
        self.assertEqual(result.returncode, 0, result.stderr)
        subprocess.run([str(exe)], check=True, capture_output=True)


if __name__ == '__main__': unittest.main()
