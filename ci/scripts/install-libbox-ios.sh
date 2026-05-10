#!/usr/bin/env bash
# Pull sing-box's official Libbox.xcframework (built from
# https://github.com/SagerNet/sing-box-for-apple) and drop it under
# `ios/Vendor/Libbox.xcframework` so the project.yml FRAMEWORK_SEARCH_PATHS
# entry resolves it for both the main app and the PacketTunnel target.
#
# Usage:
#   ci/scripts/install-libbox-ios.sh                  # latest stable
#   ci/scripts/install-libbox-ios.sh 1.10.7           # pinned version
#
# Notes
#   - Each release uploads `Libbox.xcframework.zip` (contains both
#     ios-arm64 device + ios-arm64-simulator slices).
#   - We do NOT pin the package via SwiftPM — the xcframework path is
#     simpler to wire up in xcodegen, and avoids tying the build to
#     network access at every Xcode launch.

set -euo pipefail

VERSION="${1:-latest}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="${REPO_ROOT}/ios/Vendor"

mkdir -p "${DEST}"

if [[ "${VERSION}" == "latest" ]]; then
    URL="https://github.com/SagerNet/sing-box-for-apple/releases/latest/download/Libbox.xcframework.zip"
else
    URL="https://github.com/SagerNet/sing-box-for-apple/releases/download/${VERSION}/Libbox.xcframework.zip"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

echo "→ ${URL}"
curl -fsSL "${URL}" -o "${tmp}/Libbox.xcframework.zip"

# Replace any prior copy atomically — cheap insurance against partial unzips.
rm -rf "${DEST}/Libbox.xcframework"
unzip -q "${tmp}/Libbox.xcframework.zip" -d "${DEST}"
echo "  installed → ${DEST}/Libbox.xcframework"
