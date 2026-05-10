#!/usr/bin/env bash
# Pull mihomo binaries from MetaCubeX/mihomo release archive and drop
# them into android/app/src/main/jniLibs/<abi>/libmihomo.so.
#
# Usage:
#   ci/scripts/install-mihomo-android.sh <version>            # all 3 ABIs
#   ci/scripts/install-mihomo-android.sh <version> arm64-v8a  # one ABI
#
# `<version>` is a mihomo Meta release tag, e.g. `v1.19.10`.
#
# Why "libmihomo.so" — Android 12+ blocks execve from /data/data, but
# the OS's nativeLibraryDir (where jniLibs land) keeps the exec bit.
# Disguising the binary as a .so is the standard workaround used by
# clash-for-android, mihomo-party, etc. AGP packages it uncompressed
# (see app/build.gradle.kts → packaging.jniLibs.useLegacyPackaging).

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <version> [<abi>]" >&2
    exit 1
fi

VERSION="$1"
ABI="${2:-all}"

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST_ROOT="${REPO_ROOT}/android/app/src/main/jniLibs"

# Map Android ABI -> MetaCubeX release artifact name.
mihomo_artifact() {
    case "$1" in
        arm64-v8a)    echo "mihomo-android-arm64-v8a-${VERSION}.gz" ;;
        armeabi-v7a)  echo "mihomo-android-armv7-${VERSION}.gz" ;;
        x86_64)       echo "mihomo-android-amd64-${VERSION}.gz" ;;
        *) echo "unsupported abi: $1" >&2; exit 2 ;;
    esac
}

install_one() {
    local abi="$1"
    local artifact url tmp out_dir out
    artifact="$(mihomo_artifact "${abi}")"
    url="https://github.com/MetaCubeX/mihomo/releases/download/${VERSION}/${artifact}"
    tmp="$(mktemp -d)"
    out_dir="${DEST_ROOT}/${abi}"
    out="${out_dir}/libmihomo.so"

    mkdir -p "${out_dir}"
    echo "→ ${abi}: ${url}"
    curl -fsSL "${url}" -o "${tmp}/${artifact}"
    gunzip -c "${tmp}/${artifact}" > "${out}"
    chmod +x "${out}"
    rm -rf "${tmp}"
    echo "  installed → ${out}"
}

if [[ "${ABI}" == "all" ]]; then
    install_one arm64-v8a
    install_one armeabi-v7a
    install_one x86_64
else
    install_one "${ABI}"
fi
