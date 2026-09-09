#!/usr/bin/env bash
# Compatibility engine for the checked-in 1.10 JSON translator and Swift ABI.
# This builds upstream source; sing-box-for-apple has no Libbox release asset.
set -euo pipefail
cd "$(dirname "$0")"
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Libbox iOS build requires macOS with Xcode.' >&2
  exit 1
fi
command -v go >/dev/null || { echo 'Install Go before building Libbox.' >&2; exit 1; }
xcrun --sdk iphoneos --show-sdk-path >/dev/null
source_dir="$PWD/.build/sing-box-1.10.7"
revision='253b41936ecd6ae17948d49d9c510d7100830927'
if [[ ! -d "$source_dir/.git" ]]; then
  mkdir -p "$PWD/.build"
  git clone --depth 1 --branch v1.10.7 https://github.com/SagerNet/sing-box.git "$source_dir"
fi
[[ "$(git -C "$source_dir" rev-parse HEAD)" == "$revision" ]] || { echo 'Libbox source revision mismatch.' >&2; exit 1; }
[[ -z "$(git -C "$source_dir" status --porcelain --untracked-files=no)" ]] || { echo 'Libbox source contains local modifications.' >&2; exit 1; }
(
  cd "$source_dir"
  go install github.com/sagernet/gomobile/cmd/gomobile@v0.1.4
  go install github.com/sagernet/gomobile/cmd/gobind@v0.1.4
  go run ./cmd/internal/build_libbox -target apple -platform ios
)
mkdir -p Vendor
if [[ -e Vendor/Libbox.xcframework ]]; then
  mv Vendor/Libbox.xcframework "Vendor/Libbox.previous.$(date +%s).xcframework"
fi
ditto "$source_dir/Libbox.xcframework" Vendor/Libbox.xcframework
printf '%s\n' "$revision" > Vendor/Libbox.revision
echo 'Libbox 1.10.7 compatibility engine built. Run the iOS device acceptance checklist before release.'
