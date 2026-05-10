#!/usr/bin/env bash
# Build xboard-helper for the host triple and drop it into
# desktop/src-tauri/binaries/ where Tauri's `externalBin` resolver expects
# it (suffix = target triple, no extension on unix).
#
# macOS only — `tauri.macos.conf.json` is the single config file that lists
# `binaries/xboard-helper` in externalBin, so non-mac builds neither need
# nor want this sidecar. The helper crate itself is also Unix-socket based
# and won't compile on Windows. This script no-ops on non-Darwin hosts so
# CI can call it unconditionally.
#
# Usage:
#   ci/scripts/install-helper-sidecar.sh           # debug build (fast iteration)
#   ci/scripts/install-helper-sidecar.sh release   # release build for shipping

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "→ install-helper-sidecar: host is $(uname -s), skipping (helper is macOS-only)"
    exit 0
fi

PROFILE="${1:-debug}"
case "${PROFILE}" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 1 ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="${REPO_ROOT}/desktop/src-tauri/binaries"
mkdir -p "${DEST}"

host_triple() {
    local arch os
    arch="$(uname -m)"
    os="$(uname -s)"
    case "${os}-${arch}" in
        Darwin-arm64)  echo "aarch64-apple-darwin" ;;
        Darwin-x86_64) echo "x86_64-apple-darwin" ;;
        Linux-x86_64)  echo "x86_64-unknown-linux-gnu" ;;
        MINGW*-x86_64|MSYS*-x86_64) echo "x86_64-pc-windows-msvc" ;;
        *) echo "unsupported host: ${os}-${arch}" >&2; exit 2 ;;
    esac
}

TRIPLE="$(host_triple)"
echo "→ build xboard-helper (${PROFILE}) for ${TRIPLE}"
cd "${REPO_ROOT}"
if [[ "${PROFILE}" == "release" ]]; then
    cargo build -p xboard-helper --release
    SRC="${REPO_ROOT}/target/release/xboard-helper"
else
    cargo build -p xboard-helper
    SRC="${REPO_ROOT}/target/debug/xboard-helper"
fi

OUT="${DEST}/xboard-helper-${TRIPLE}"
if [[ "${TRIPLE}" == *windows* ]]; then
    OUT="${OUT}.exe"
    SRC="${SRC}.exe"
fi

cp "${SRC}" "${OUT}"
chmod +x "${OUT}"
echo "  installed → ${OUT}"
