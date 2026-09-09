"""Offline trust-gate checks; real codesign stability is tested only on macOS CI."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "ci/scripts" / f"{name}.py")
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


presign = module("presign-macos-sidecars")
probe = module("probe-macos-signatures")


class MacSigningGateTests(unittest.TestCase):
    def test_signing_identity_or_certificate_override_is_rejected(self):
        for variable, value in (("APPLE_SIGNING_IDENTITY", "Developer ID Application: Other"),
                                ("APPLE_CERTIFICATE", "certificate"), ("APPLE_ENTITLEMENTS", "/tmp/custom.plist")):
            with self.subTest(variable=variable), patch.dict(os.environ, {variable: value}, clear=True):
                with self.assertRaises(ValueError):
                    presign.validate_configuration()

    def test_reviewed_ad_hoc_configuration_is_accepted(self):
        with patch.dict(os.environ, {}, clear=True):
            presign.validate_configuration()

    def test_signature_layout_preparation_still_requires_two_equal_full_hashes(self):
        for unstable in (False, True):
            with self.subTest(unstable=unstable), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                binaries = root / "desktop/src-tauri/binaries"
                binaries.mkdir(parents=True)
                (root / "desktop/package-lock.json").write_text(json.dumps({"packages": {
                    "node_modules/@tauri-apps/cli": {"version": "2.11.1"}}}))
                triple = "aarch64-apple-darwin"
                calls = {}
                for name in ("mihomo", "xboard-helper"):
                    (binaries / f"{name}-{triple}").write_bytes(b"official input")

                def codesign(arguments, **_):
                    path = Path(arguments[-1])
                    if "--force" in arguments:
                        count = calls.get(path.name, 0) + 1
                        calls[path.name] = count
                        if count == 1:
                            path.write_bytes(b"new LC_CODE_SIGNATURE layout")
                        elif unstable:
                            path.write_bytes(f"continually changing signature {count}".encode())
                        else:
                            path.write_bytes(b"stable allocated layout and default identifier")
                    return SimpleNamespace(stdout="", stderr="Identifier=reviewed-sidecar\n")

                with patch.object(presign, "ROOT", root), patch.object(presign.sys, "platform", "darwin"), \
                     patch.object(presign, "validate_configuration"), patch.object(presign.subprocess, "run", side_effect=codesign):
                    if unstable:
                        with self.assertRaisesRegex(ValueError, "not byte-stable"):
                            presign.main(triple)
                        self.assertFalse((binaries / f"macos-sidecar-pins-{triple}.sha256").exists())
                        for name in ("mihomo", "xboard-helper"):
                            self.assertEqual((binaries / f"{name}-{triple}").read_bytes(), b"official input")
                    else:
                        presign.main(triple)
                        self.assertEqual(calls, {"mihomo": 3, "xboard-helper": 3})
                        pins = (binaries / f"macos-sidecar-pins-{triple}.sha256").read_text()
                        for name in ("mihomo", "xboard-helper"):
                            self.assertIn(f"{presign.sha256(binaries / f'{name}-{triple}')}  {name}\n", pins)

    def test_bundle_cannot_replace_compiled_pin_with_its_own_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "desktop/src-tauri/binaries"
            bundled = root / "Sufe.app/Contents/MacOS"
            binaries.mkdir(parents=True)
            bundled.mkdir(parents=True)
            triple = "aarch64-apple-darwin"
            contents = b"changed sidecar bytes whose freshly calculated digest is not trusted"
            for name in ("mihomo", "xboard-helper"):
                (binaries / f"{name}-{triple}").write_bytes(contents)
                (bundled / name).write_bytes(contents)
            pins = binaries / f"macos-sidecar-pins-{triple}.sha256"
            pins.write_text("0" * 64 + "  mihomo\n" + "0" * 64 + "  xboard-helper\n")
            # Mock only Apple signing; the production report/pin comparison is
            # real, and rejects consistent self-hashes that differ from pins.
            with patch.object(probe, "sign", side_effect=probe.digest):
                with self.assertRaisesRegex(ValueError, "does not match compiled"):
                    probe.probe(root / "Sufe.app", root, triple, root / "report.json")
                trusted = hashlib.sha256(contents).hexdigest()
                pins.write_text(f"{trusted}  mihomo\n{trusted}  xboard-helper\n")
                probe.probe(root / "Sufe.app", root, triple, root / "report.json")
