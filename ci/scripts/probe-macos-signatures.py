#!/usr/bin/env python3
"""Observe Tauri's pinned ad-hoc signing on disposable copies only.

This probe never mutates the application, sidecars or system state. Its report
decides whether a later build can bind full-file SHA256 after pre-signing.
"""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def sign(path):
    # Exact executable arguments in tauri-cli-v2.11.1 Keychain::sign, with
    # current signingIdentity="-", hardenedRuntime=true and no entitlements.
    result = subprocess.run(
        ["/usr/bin/codesign", "--force", "-s", "-", "--options", "runtime", str(path)],
        capture_output=True, text=True, timeout=30,
    )
    if result.returncode:
        raise RuntimeError(result.stderr[-4096:])
    return digest(path)


def probe(app, root, triple, report_path):
    report = {"cli_version": "2.11.1", "signing_arguments": ["--force", "-s", "-", "--options", "runtime"],
              "target": triple, "copies_only": True, "binaries": {}, "stable": False}
    with tempfile.TemporaryDirectory(prefix="sufe-codesign-probe-") as temporary:
        stage = Path(temporary)
        for name in ("mihomo", "xboard-helper"):
            result = {}
            report["binaries"][name] = result
            try:
                original = root / "desktop/src-tauri/binaries" / f"{name}-{triple}"
                bundled = app / "Contents/MacOS" / name
                result["raw_sha256"] = digest(original)
                result["bundled_sha256"] = digest(bundled)
                for kind, source in (("raw", original), ("bundled", bundled)):
                    destination = stage / kind / name
                    destination.parent.mkdir(exist_ok=True)
                    shutil.copyfile(source, destination)
                    destination.chmod(0o755)
                    result[f"{kind}_signed_once_sha256"] = sign(destination)
                    result[f"{kind}_signed_twice_sha256"] = sign(destination)
                result["presign_matches_bundle"] = result["raw_signed_once_sha256"] == result["bundled_sha256"]
                result["raw_resign_stable"] = result["raw_signed_once_sha256"] == result["raw_signed_twice_sha256"]
                result["bundle_resign_stable"] = result["bundled_sha256"] == result["bundled_signed_once_sha256"] == result["bundled_signed_twice_sha256"]
                result["stable"] = all(result[field] for field in ("presign_matches_bundle", "raw_resign_stable", "bundle_resign_stable"))
            except Exception as error:
                result.update(stable=False, error=str(error))
        report["stable"] = all(item["stable"] for item in report["binaries"].values())
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Ad-hoc sidecar signing stability observed: {report['stable']} (report: {report_path.name})")
    # The GUI build embeds these pre-signed full-file digests. Re-signing in the
    # final bundle must preserve them, or first TUN installation would reject.
    pins_path = root / "desktop/src-tauri/binaries" / f"macos-sidecar-pins-{triple}.sha256"
    pins = {}
    for line in pins_path.read_text().splitlines():
        expected, name = line.split()
        if name in pins:
            raise ValueError("duplicate pre-signed sidecar pin")
        pins[name] = expected
    if set(pins) != {"mihomo", "xboard-helper"}:
        raise ValueError("missing pre-signed sidecar pins")
    if not report["stable"] or any(pins[name] != item.get("bundled_sha256") or pins[name] != item.get("raw_sha256")
                                   for name, item in report["binaries"].items()):
        raise ValueError("final bundle does not match compiled pre-signed sidecar hashes; refusing delivery")
    print("Final bundle sidecars match the full-file SHA256 pins embedded by the desktop build")


if __name__ == "__main__":
    if sys.platform != "darwin":
        raise SystemExit("macOS signing probe only")
    if len(sys.argv) != 5:
        raise SystemExit("usage: probe-macos-signatures.py APP REPOSITORY TARGET REPORT_JSON")
    probe(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4]))
