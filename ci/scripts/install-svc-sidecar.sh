#!/usr/bin/env bash
# Build xboard-svc for the host triple and drop it into
# desktop/src-tauri/binaries/ where Tauri's `externalBin` resolver expects
# it (suffix = target triple). Also fetches wintun.dll into the same
# directory — `tauri.windows.conf.json` lists it under `bundle.resources`
# so MSI/NSIS bundles ship it next to xboard-svc.exe.
#
# Windows only. The svc crate is gated behind `cfg(target_os = "windows")`
# at the source level, but cross-compiling to MSVC from a non-Windows host
# requires a separately-installed toolchain (cargo-xwin etc.); rather than
# carry that complexity, this script no-ops off-Windows so CI can call it
# unconditionally and it only does real work on the windows-latest runner.
#
# Usage (run from repo root or anywhere — script computes its own paths):
#   ci/scripts/install-svc-sidecar.sh           # debug build
#   ci/scripts/install-svc-sidecar.sh release   # release build for shipping

set -euo pipefail

UNAME="$(uname -s 2>/dev/null || echo unknown)"
case "${UNAME}" in
    MINGW*|MSYS*|CYGWIN*) ;;
    *)
        echo "→ install-svc-sidecar: host is ${UNAME}, skipping (xboard-svc is Windows-only)"
        exit 0
        ;;
esac

PROFILE="${1:-debug}"
case "${PROFILE}" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 1 ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="${REPO_ROOT}/desktop/src-tauri/binaries"
mkdir -p "${DEST}"

# Target triple the matrix is building for. We support both x86_64 and the
# experimental aarch64 windows job so the same script works when M3+ adds
# Windows on ARM. CARGO_BUILD_TARGET wins, then falls back to the host
# default which on windows-latest runners is x86_64-pc-windows-msvc.
TRIPLE="${CARGO_BUILD_TARGET:-x86_64-pc-windows-msvc}"
echo "→ build xboard-svc (${PROFILE}) for ${TRIPLE}"

cd "${REPO_ROOT}"
if [[ "${PROFILE}" == "release" ]]; then
    cargo build -p xboard-svc --release --target "${TRIPLE}"
    SRC="${REPO_ROOT}/target/${TRIPLE}/release/xboard-svc.exe"
else
    cargo build -p xboard-svc --target "${TRIPLE}"
    SRC="${REPO_ROOT}/target/${TRIPLE}/debug/xboard-svc.exe"
fi

OUT="${DEST}/xboard-svc-${TRIPLE}.exe"
cp "${SRC}" "${OUT}"
echo "  installed → ${OUT}"

# wintun.dll — wintun.net publishes a single ZIP that bundles dll's for all
# Windows architectures. Pick the right one based on TRIPLE. The version is
# pinned in WINTUN_VERSION (set via env or repo variable; default below
# matches the version mihomo's own CI was last verified against).
WINTUN_VERSION="${WINTUN_VERSION:-0.14.1}"
WINTUN_ZIP_URL="https://www.wintun.net/builds/wintun-${WINTUN_VERSION}.zip"
WINTUN_DEST="${DEST}/wintun.dll"
case "${TRIPLE}" in
    x86_64-pc-windows-*)   WINTUN_INNER="wintun/bin/amd64/wintun.dll" ;;
    aarch64-pc-windows-*)  WINTUN_INNER="wintun/bin/arm64/wintun.dll" ;;
    i686-pc-windows-*)     WINTUN_INNER="wintun/bin/x86/wintun.dll"   ;;
    *) echo "unsupported triple for wintun: ${TRIPLE}" >&2; exit 3 ;;
esac

if [[ -f "${WINTUN_DEST}" ]]; then
    echo "  wintun.dll already present → ${WINTUN_DEST} (delete to refresh)"
else
    TMP="$(mktemp -d)"
    trap 'rm -rf "${TMP}"' EXIT
    echo "→ fetch wintun ${WINTUN_VERSION} from ${WINTUN_ZIP_URL}"
    curl -fsSL "${WINTUN_ZIP_URL}" -o "${TMP}/wintun.zip"
    # Use bsdtar (built in to git-bash on Windows) to extract a single file.
    if command -v unzip >/dev/null 2>&1; then
        unzip -j -o "${TMP}/wintun.zip" "${WINTUN_INNER}" -d "${DEST}"
    else
        tar -xf "${TMP}/wintun.zip" -C "${TMP}" "${WINTUN_INNER}"
        cp "${TMP}/${WINTUN_INNER}" "${WINTUN_DEST}"
    fi
    echo "  installed → ${WINTUN_DEST}"
fi
