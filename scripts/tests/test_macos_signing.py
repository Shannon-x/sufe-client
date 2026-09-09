"""Offline trust-gate checks; real codesign stability is tested only on macOS CI."""
import hashlib
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

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
