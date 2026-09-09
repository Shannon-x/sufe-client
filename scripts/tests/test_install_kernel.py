"""Offline regression tests for pinned kernel preparation (no native execution)."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "install-kernel.py"
SPEC = importlib.util.spec_from_file_location("install_kernel", SCRIPT)
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


class KernelDownloadTests(unittest.TestCase):
    def test_all_default_targets_have_reviewed_pins(self):
        pins = (installer.ROOT / "ci/checksums" / f"{installer.DEFAULT_VERSION}.sha256").read_text()
        for target in installer.TARGET_STEMS:
            self.assertIn(installer.artifact_for(installer.DEFAULT_VERSION, target), pins)
        self.assertIn("-compatible-", installer.artifact_for(installer.DEFAULT_VERSION, "x86_64-unknown-linux-gnu"))

    def test_standard_archive_is_explicit(self):
        name = installer.artifact_for(installer.DEFAULT_VERSION, "x86_64-pc-windows-msvc", standard=True)
        self.assertNotIn("compatible", name)

    def test_unreviewable_tags_rejected(self):
        for tag in ["latest", "alpha", "../../v1.19.30", "v1.19.30/extra"]:
            with self.assertRaises(ValueError):
                installer.artifact_for(tag, "aarch64-apple-darwin")

    def test_valid_cache_does_not_require_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / "cache"
            cache.mkdir()
            pins = root / "ci/checksums"
            pins.mkdir(parents=True)
            contents = b"reviewed archive"
            (cache / "kernel.gz").write_bytes(contents)
            (pins / "v1.0.0.sha256").write_text(hashlib.sha256(contents).hexdigest() + "  kernel.gz\n")
            with patch.object(installer, "ROOT", root), patch.object(installer, "CACHE", cache), patch.object(installer.urllib.request, "urlopen") as network:
                self.assertEqual(installer.fetch("v1.0.0", "kernel.gz", "https://example.invalid/kernel.gz"), cache / "kernel.gz")
                network.assert_not_called()

    def test_mismatched_download_is_not_cached(self):
        import io
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pins = root / "ci/checksums"
            pins.mkdir(parents=True)
            (pins / "v1.0.0.sha256").write_text("0" * 64 + "  kernel.gz\n")
            cache = root / "cache"
            with patch.object(installer, "ROOT", root), patch.object(installer, "CACHE", cache), patch.object(installer.urllib.request, "urlopen", return_value=io.BytesIO(b"corrupted")):
                with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
                    installer.fetch("v1.0.0", "kernel.gz", "https://example.invalid/kernel.gz")
                self.assertEqual(list(cache.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
