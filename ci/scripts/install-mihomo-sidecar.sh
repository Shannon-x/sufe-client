#!/usr/bin/env bash
# Drop mihomo binaries into desktop/src-tauri/binaries/ with the
# target-triple suffix Tauri expects for `externalBin` resolution.
#
# Usage:
#   ci/scripts/install-mihomo-sidecar.sh <version>           # current host triple
#   ci/scripts/install-mihomo-sidecar.sh <version> --all      # all four desktop triples
#
# `<version>` is a mihomo Meta release tag, e.g. `v1.18.7`. The script
# fetches the official archive from MetaCubeX/mihomo, extracts the binary,
# strips it, and renames it for Tauri.
#
# This script is intentionally simple — replace the URL with your own CDN
# mirror once the kernel-mirror CI workflow is wired up.

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <version> [--all]" >&2
    exit 1
fi

VERSION="$1"
shift || true
MODE="host"
if [[ "${1:-}" == "--all" ]]; then
    MODE="all"
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="${REPO_ROOT}/desktop/src-tauri/binaries"
mkdir -p "${DEST}"

# Map host triple -> mihomo release artifact name (without .gz/.zip).
mihomo_artifact() {
    case "$1" in
        x86_64-pc-windows-msvc)   echo "mihomo-windows-amd64-${VERSION}.zip" ;;
        aarch64-apple-darwin)     echo "mihomo-darwin-arm64-${VERSION}.gz" ;;
        x86_64-apple-darwin)      echo "mihomo-darwin-amd64-${VERSION}.gz" ;;
        x86_64-unknown-linux-gnu) echo "mihomo-linux-amd64-${VERSION}.gz" ;;
        *) echo "unsupported triple: $1" >&2; exit 2 ;;
    esac
}

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

install_one() {
    local triple="$1"
    local artifact url tmp out
    artifact="$(mihomo_artifact "${triple}")"
    url="https://github.com/MetaCubeX/mihomo/releases/download/${VERSION}/${artifact}"
    tmp="$(mktemp -d)"
    out="${DEST}/mihomo-${triple}"
    if [[ "${triple}" == *windows* ]]; then
        out="${out}.exe"
    fi

    echo "→ ${triple}: ${url}"
    curl -fsSL "${url}" -o "${tmp}/${artifact}"

    case "${artifact}" in
        *.gz)
            gunzip -c "${tmp}/${artifact}" > "${out}"
            ;;
        *.zip)
            unzip -p "${tmp}/${artifact}" >"${out}"
            ;;
        *)
            cp "${tmp}/${artifact}" "${out}"
            ;;
    esac
    chmod +x "${out}"
    rm -rf "${tmp}"
    echo "  installed → ${out}"
}

if [[ "${MODE}" == "all" ]]; then
    install_one x86_64-pc-windows-msvc
    install_one aarch64-apple-darwin
    install_one x86_64-apple-darwin
    install_one x86_64-unknown-linux-gnu
else
    install_one "$(host_triple)"
fi
