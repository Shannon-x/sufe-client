#!/usr/bin/env python3
"""Download pinned official kernels; validate archives before extracting.

Works in PowerShell, macOS and Linux without Bash. Does not start a kernel,
install a service, request elevation or change the system proxy.
"""
import argparse
import gzip
import hashlib
import os
from pathlib import Path
import re
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".tools" / "kernel-downloads"
DEFAULT_VERSION = (ROOT / "ci" / "mihomo-version.txt").read_text().strip()
TARGET_STEMS = {"x86_64-pc-windows-msvc": "windows-amd64", "aarch64-apple-darwin": "darwin-arm64",
                "x86_64-apple-darwin": "darwin-amd64", "x86_64-unknown-linux-gnu": "linux-amd64"}


def artifact_for(version: str, target: str, standard: bool = False) -> str:
    if not re.fullmatch(r"v\d+\.\d+\.\d+", version):
        raise ValueError("Kernel version must be a stable release tag such as v1.19.30")
    stem = TARGET_STEMS[target]
    if ("windows" in target or "linux" in target) and not standard and version != "v1.18.7":
        stem += "-compatible"
    return f"mihomo-{stem}-{version}.{'zip' if 'windows' in target else 'gz'}"


def fetch(version: str, artifact: str, url: str) -> Path:
    pins_file = ROOT / "ci" / "checksums" / f"{version}.sha256"
    if not pins_file.is_file():
        raise ValueError(f"No reviewed checksum pins for {version}")
    pins = dict((parts[1], parts[0]) for line in pins_file.read_text().splitlines()
                if len(parts := line.split()) == 2)
    expected = pins.get(artifact)
    if expected is None or len(expected) != 64:
        raise ValueError(f"Missing checksum pin: {artifact}")
    CACHE.mkdir(parents=True, exist_ok=True)
    target = CACHE / artifact
    if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() == expected:
        return target
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=CACHE, delete=False) as output:
            temporary = Path(output.name)
            request = urllib.request.Request(url, headers={"User-Agent": "SUFE-verified-installer"})
            with urllib.request.urlopen(request, timeout=60) as source:
                digest = hashlib.sha256()
                while chunk := source.read(1024 * 1024):
                    digest.update(chunk)
                    output.write(chunk)
        if digest.hexdigest() != expected:
            raise ValueError(f"SHA256 mismatch: {artifact}")
        os.replace(temporary, target)
        return target
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def install(data: bytes, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as output:
        temporary = Path(output.name)
        output.write(data)
    os.replace(temporary, destination)
    if os.name != "nt":
        destination.chmod(0o755)
    print(f"Verified and installed {destination.name}: {hashlib.sha256(data).hexdigest()}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", default=DEFAULT_VERSION)
    parser.add_argument("--cache-only", action="store_true", help="Verify and extract into .tools/kernel-downloads without changing the bundled sidecar")
    cpu = parser.add_mutually_exclusive_group()
    cpu.add_argument("--compatible", action="store_true", help="Use the reviewed Windows/Linux amd64-compatible archive (default)")
    cpu.add_argument("--standard", action="store_true", help="Use the standard Windows/Linux amd64 archive instead of the compatible default")
    parser.add_argument("--target", choices=TARGET_STEMS, required=True)
    args = parser.parse_args()
    windows = "windows" in args.target
    if (args.compatible or args.standard) and "darwin" in args.target:
        parser.error("--compatible/--standard are supported only for Windows/Linux amd64")
    artifact = artifact_for(args.version, args.target, args.standard)
    archive = fetch(args.version, artifact, f"https://github.com/MetaCubeX/mihomo/releases/download/{args.version}/{artifact}")
    if windows:
        with zipfile.ZipFile(archive) as package:
            members = [name for name in package.namelist() if name.endswith(".exe") and "/" not in name and "\\" not in name]
            if len(members) != 1:
                raise ValueError("Official archive must contain one root-level mihomo executable")
            data = package.read(members[0])
    else:
        with gzip.open(archive, "rb") as package:
            data = package.read()
    if args.cache_only:
        name = artifact.removesuffix(".zip").removesuffix(".gz") + (".exe" if windows else "")
        install(data, CACHE / name)
        return
    install(data, ROOT / "desktop" / "src-tauri" / "binaries" / f"mihomo-{args.target}{'.exe' if windows else ''}")
    if windows:
        archive = fetch("wintun-0.14.1", "wintun-0.14.1.zip", "https://www.wintun.net/builds/wintun-0.14.1.zip")
        with zipfile.ZipFile(archive) as package:
            data = package.read("wintun/bin/amd64/wintun.dll")
        install(data, ROOT / "desktop" / "src-tauri" / "binaries" / "wintun.dll")


if __name__ == "__main__":
    main()
