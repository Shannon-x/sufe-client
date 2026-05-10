#!/usr/bin/env bash
# Build xboard-core as an iOS XCFramework — `Vendor/XboardCore.xcframework`
# in the iOS project root. Includes:
#   - ios-arm64           (device, --target aarch64-apple-ios)
#   - ios-arm64-simulator (Apple Silicon Mac simulator)
#   - ios-x86_64-simulator (Intel Mac simulator, optional)
#
# Bundles the C header + modulemap UniFFI emits so Swift's bridging
# `import xboard_coreFFI` works without manual SwiftPM glue.
#
# Re-run after any change to core/src/*.rs or core/src/ffi.udl.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${REPO_ROOT}"

PROFILE="${PROFILE:-release}"
INCLUDE_INTEL_SIM="${INCLUDE_INTEL_SIM:-1}"

# Cargo profile dir — `release` outputs to `target/<triple>/release/`,
# `debug` to `target/<triple>/debug/`.
case "${PROFILE}" in
    release) PROFILE_DIR="release"; PROFILE_FLAG="--release" ;;
    debug)   PROFILE_DIR="debug"; PROFILE_FLAG="" ;;
    *) echo "PROFILE must be release or debug, got: ${PROFILE}" >&2; exit 1 ;;
esac

LIB_NAME="libxboard_core.a"

build_target() {
    local target="$1"
    echo "→ cargo build -p xboard-core --target ${target} (${PROFILE})"
    rustup target add "${target}" >/dev/null 2>&1 || true
    # iOS toolchain doesn't link against a system-provided `xcrun`-resolved
    # SDK by default; CARGO_TARGET_*_LINKER stays unset and cargo falls back
    # to the host `cc`. That works for static libs (no actual linking).
    cargo build -p xboard-core --target "${target}" ${PROFILE_FLAG}
}

build_target aarch64-apple-ios
build_target aarch64-apple-ios-sim
if [[ "${INCLUDE_INTEL_SIM}" == "1" ]]; then
    build_target x86_64-apple-ios
fi

# ---------- assemble headers ----------
HEADERS="$(mktemp -d)"
trap 'rm -rf "${HEADERS}"' EXIT

echo "→ uniffi-bindgen → C headers"
cargo run -p xboard-core --bin uniffi-bindgen -- generate \
    core/src/ffi.udl \
    --language swift \
    --out-dir "${HEADERS}"

# UniFFI emits xboard_coreFFI.h + xboard_coreFFI.modulemap; the .swift
# file is consumed via build-uniffi-swift.sh, not here.
HEADER_DIR="${HEADERS}/Headers"
mkdir -p "${HEADER_DIR}"
mv "${HEADERS}/xboard_coreFFI.h" "${HEADER_DIR}/"
# The modulemap must reference the header by *bundle-relative* path, not the
# original `xboard_coreFFI.h`. Rewrite it inline.
cat > "${HEADER_DIR}/module.modulemap" <<'EOF'
framework module xboard_coreFFI {
    umbrella header "xboard_coreFFI.h"
    export *
    module * { export * }
}
EOF

# ---------- merge simulator slices ----------
SIM_LIB="$(mktemp -d)/libxboard_core_sim.a"
if [[ "${INCLUDE_INTEL_SIM}" == "1" ]]; then
    echo "→ lipo (sim arm64 + sim x86_64)"
    lipo -create -output "${SIM_LIB}" \
        "target/aarch64-apple-ios-sim/${PROFILE_DIR}/${LIB_NAME}" \
        "target/x86_64-apple-ios/${PROFILE_DIR}/${LIB_NAME}"
else
    cp "target/aarch64-apple-ios-sim/${PROFILE_DIR}/${LIB_NAME}" "${SIM_LIB}"
fi

# ---------- xcodebuild -create-xcframework ----------
DEST="${REPO_ROOT}/ios/Vendor/XboardCore.xcframework"
rm -rf "${DEST}"
echo "→ xcodebuild -create-xcframework"
xcodebuild -create-xcframework \
    -library "target/aarch64-apple-ios/${PROFILE_DIR}/${LIB_NAME}" \
    -headers "${HEADER_DIR}" \
    -library "${SIM_LIB}" \
    -headers "${HEADER_DIR}" \
    -output "${DEST}"

echo "  installed → ${DEST}"
