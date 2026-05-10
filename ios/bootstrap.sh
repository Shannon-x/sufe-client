#!/usr/bin/env bash
# One-time bootstrap for the iOS project.
#
# Materialises:
#   - XboardClient.xcodeproj (via xcodegen, from project.yml)
#   - Vendor/Libbox.xcframework (sing-box NE engine)
#   - Vendor/XboardCore.xcframework (xboard-core Rust → static lib)
#   - Shared/Generated/xboard_core.swift (UniFFI bindings)
#
# Re-running is safe: each step skips work it doesn't need to redo.
# `just ios-bootstrap` invokes this — see ../justfile.

set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$(cd .. && pwd)"

# ---------------- xcodegen ----------------
if ! command -v xcodegen >/dev/null 2>&1; then
    echo "xcodegen missing. Install via:  brew install xcodegen" >&2
    exit 1
fi
echo "→ xcodegen generate"
xcodegen generate

# ---------------- Libbox.xcframework ----------------
if [ ! -d "Vendor/Libbox.xcframework" ]; then
    echo "→ install-libbox-ios.sh"
    bash "${REPO_ROOT}/ci/scripts/install-libbox-ios.sh"
else
    echo "→ Libbox.xcframework already present, skipping"
fi

# ---------------- XboardCore.xcframework ----------------
if [ ! -d "Vendor/XboardCore.xcframework" ]; then
    echo "→ build-core-ios.sh"
    bash "${REPO_ROOT}/ci/scripts/build-core-ios.sh"
else
    echo "→ XboardCore.xcframework already present (re-run via:  just core-ios)"
fi

# ---------------- UniFFI Swift bindings ----------------
if [ ! -f "Shared/Generated/xboard_core.swift" ]; then
    echo "→ build-uniffi-swift.sh"
    bash "${REPO_ROOT}/ci/scripts/build-uniffi-swift.sh"
else
    echo "→ Swift bindings already present (re-run via:  just ios-bindings)"
fi

echo
echo "Done. Open the workspace with:"
echo "  open XboardClient.xcodeproj"
echo
echo "Or build from the command line:"
echo "  just ios-build"
