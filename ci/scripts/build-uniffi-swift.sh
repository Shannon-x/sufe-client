#!/usr/bin/env bash
# Generate the UniFFI Swift bindings into `ios/Shared/Generated/`.
#
# Output:
#   ios/Shared/Generated/xboard_core.swift
#
# The accompanying C header + modulemap are baked into XboardCore.xcframework
# by `build-core-ios.sh`, so we only keep the Swift file here.
#
# Re-run after editing core/src/ffi.udl. `just ios-bindings` is the
# canonical entry point.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${REPO_ROOT}"

OUT="${REPO_ROOT}/ios/Shared/Generated"
mkdir -p "${OUT}"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

cargo run -p xboard-core --bin uniffi-bindgen -- generate \
    core/src/ffi.udl \
    --language swift \
    --out-dir "${tmp}"

mv "${tmp}/xboard_core.swift" "${OUT}/xboard_core.swift"

echo "  wrote → ${OUT}/xboard_core.swift"
