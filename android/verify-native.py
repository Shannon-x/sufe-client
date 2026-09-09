#!/usr/bin/env python3
"""Verify packaged Android native dependencies, CPU ABI and 16 KB ELF alignment.

Usage: python android/verify-native.py path/to/app-debug.apk
Also accepts a directory containing <abi>/*.so (e.g. merged native libraries).
"""
import argparse
from pathlib import Path
import struct
import zipfile


ABIS = {"arm64-v8a": (2, 183, 16384), "armeabi-v7a": (1, 40, 4096), "x86_64": (2, 62, 16384)}
REQUIRED = {"libxboard_core.so", "libmihomo.so", "libjnidispatch.so"}


def inspect(data: bytes, abi: str, label: str) -> str:
    elf_class, machine, page = ABIS[abi]
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[4] != elf_class or data[5] != 1:
        raise ValueError(f"{label}: wrong ELF class or byte order")
    if struct.unpack_from("<H", data, 18)[0] != machine:
        raise ValueError(f"{label}: CPU machine does not match {abi}")
    wide = elf_class == 2
    phoff = struct.unpack_from("<Q" if wide else "<I", data, 32 if wide else 28)[0]
    size, count = struct.unpack_from("<HH", data, 54 if wide else 42)
    if size < (56 if wide else 32) or phoff + size * count > len(data):
        raise ValueError(f"{label}: malformed ELF program headers")
    loads = 0
    for index in range(count):
        offset = phoff + index * size
        if struct.unpack_from("<I", data, offset)[0] != 1:
            continue
        loads += 1
        file_offset, address = struct.unpack_from("<QQ" if wide else "<II", data, offset + (8 if wide else 4))
        alignment = struct.unpack_from("<Q" if wide else "<I", data, offset + (48 if wide else 28))[0]
        if alignment < page or (address - file_offset) % page:
            raise ValueError(f"{label}: PT_LOAD does not support {page}-byte pages (alignment={alignment})")
    if not loads:
        raise ValueError(f"{label}: no loadable ELF segments")
    return f"OK {label}: {len(data)} bytes, {loads} segments, {page}-byte pages"


def verify(path: Path) -> None:
    seen = {abi: set() for abi in ABIS}
    if path.is_dir():
        entries = [(str(p.relative_to(path)).replace("\\", "/"), p.read_bytes()) for p in path.rglob("*.so")]
    else:
        with zipfile.ZipFile(path) as archive:
            entries = [(name.removeprefix("lib/"), archive.read(name)) for name in archive.namelist() if name.startswith("lib/") and name.endswith(".so")]
    for name, data in entries:
        parts = name.split("/")
        if len(parts) != 2 or parts[0] not in ABIS:
            continue
        abi, library = parts
        print(inspect(data, abi, name))
        seen[abi].add(library)
    for abi, libraries in seen.items():
        missing = REQUIRED - libraries
        if missing:
            raise ValueError(f"{abi}: missing native dependencies: {', '.join(sorted(missing))}")
    print("All three ABIs contain the Rust core, mihomo and JNA; native ELF validation passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=Path)
    args = parser.parse_args()
    try:
        verify(args.artifact)
    except (ValueError, OSError, struct.error, zipfile.BadZipFile) as error:
        parser.exit(1, f"Native validation failed: {error}\n")
