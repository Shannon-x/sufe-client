#!/usr/bin/env python3
"""Pre-sign reviewed sidecars with Tauri's exact ad-hoc signing arguments.

After preparing the Mach-O signature layout, two identical signatures are
required before pinning full-file SHA256. The desktop build embeds those pins
and the final bundle must still match them.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def signature_details(path):
    result = subprocess.run(
        ["/usr/bin/codesign", "-d", "--verbose=4", str(path)],
        capture_output=True, text=True, check=True, timeout=30,
    )
    details = result.stdout + result.stderr
    identifier = next((line.removeprefix("Identifier=") for line in details.splitlines()
                       if line.startswith("Identifier=")), None)
    if not identifier:
        raise ValueError("codesign did not report an identifier for the sidecar")
    return {"identifier": identifier, "size": path.stat().st_size}


def validate_configuration():
    config = json.loads((ROOT / "desktop/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    platform = json.loads((ROOT / "desktop/src-tauri/tauri.macos.conf.json").read_text(encoding="utf-8"))
    options = {**config.get("bundle", {}).get("macOS", {}), **platform.get("bundle", {}).get("macOS", {})}
    if options.get("signingIdentity") != "-" or options.get("hardenedRuntime", True) is not True or options.get("entitlements"):
        raise ValueError("pre-sign requires signingIdentity '-', hardenedRuntime true and no entitlements")
    if os.environ.get("APPLE_SIGNING_IDENTITY", "-") not in ("", "-"):
        raise ValueError("APPLE_SIGNING_IDENTITY overrides ad-hoc signing; configure a matching reviewed pre-sign chain first")
    for variable in ("APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_ENTITLEMENTS"):
        if os.environ.get(variable):
            raise ValueError(f"{variable} conflicts with the reviewed ad-hoc pre-sign configuration")


def main(triple):
    if sys.platform != "darwin" or triple not in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
        raise ValueError("pre-sign runs only on macOS for a reviewed native target")
    validate_configuration()
    cli = json.loads((ROOT / "desktop/package-lock.json").read_text(encoding="utf-8"))["packages"]["node_modules/@tauri-apps/cli"]["version"]
    if cli != "2.11.1":
        raise ValueError("review new Tauri CLI codesign arguments before changing the pinned 2.11.1 signing chain")
    binaries = ROOT / "desktop/src-tauri/binaries"
    report_dir = ROOT / "artifacts/desktop" / triple
    report_dir.mkdir(parents=True, exist_ok=True)
    report = {"target": triple, "cli_version": cli, "binaries": {}}
    with tempfile.TemporaryDirectory(prefix="sufe-presign-") as temporary:
        stage = Path(temporary)
        for name in ("mihomo", "xboard-helper"):
            original = binaries / f"{name}-{triple}"
            if original.is_symlink() or not original.is_file() or not 0 < original.stat().st_size <= 256 * 1024 * 1024:
                raise ValueError(f"missing/unsafe {name} sidecar; download the reviewed kernel and build the matching helper first")
            destination = stage / name  # final bundle basename affects signing identity
            shutil.copyfile(original, destination)
            destination.chmod(0o755)
            entry = {"raw_sha256": sha256(original)}
            report["binaries"][name] = entry
            # Apple's MachORep::identificationFor hashes load commands when a
            # Go binary has no LC_UUID. Signing a bare basename then uses that
            # value in its default identifier. The first codesign allocation
            # changes LC_CODE_SIGNATURE, so comparing only the first two runs
            # incorrectly rejects the convergent result. Prepare that layout
            # once, then still demand TWO byte-identical signatures. There is
            # no retry loop which could accidentally bless a changing binary.
            # https://github.com/apple-oss-distributions/Security/blob/main/OSX/libsecurity_codesigning/lib/machorep.cpp
            for iteration in (0, 1, 2):
                subprocess.run(["/usr/bin/codesign", "--force", "-s", "-", "--options", "runtime", str(destination)], check=True, timeout=30)
                entry[f"signature_{iteration}_sha256"] = sha256(destination)
                entry[f"signature_{iteration}_details"] = signature_details(destination)
            (report_dir / "presigned-sidecars.json").write_text(json.dumps(report, indent=2) + "\n")
            if entry["signature_1_sha256"] != entry["signature_2_sha256"]:
                raise ValueError(f"{name} ad-hoc re-signing is not byte-stable; refusing an unreliable install pin")
            subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(destination)], check=True, timeout=30)
        # Commit only after both binaries pass. Existing production signing keys
        # are neither read nor created; '-' uses a deterministic ad-hoc signature.
        for name in report["binaries"]:
            with tempfile.NamedTemporaryFile(prefix=".sufe-presign-", dir=binaries, delete=False) as file:
                staged = Path(file.name)
                file.write((stage / name).read_bytes())
            try:
                staged.chmod(0o755)
                os.replace(staged, binaries / f"{name}-{triple}")
            finally:
                staged.unlink(missing_ok=True)
    pins = "".join(f"{entry['signature_2_sha256']}  {name}\n" for name, entry in report["binaries"].items())
    (binaries / f"macos-sidecar-pins-{triple}.sha256").write_text(pins)
    (report_dir / "presigned-sidecars.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Pinned stable ad-hoc signatures for both {triple} sidecars")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: presign-macos-sidecars.py <apple-target-triple>")
    main(sys.argv[1])
