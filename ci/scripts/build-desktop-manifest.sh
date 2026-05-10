#!/usr/bin/env bash
# Assemble the unified Tauri updater manifest (latest.json) from a
# GitHub Release's bundle assets. Tauri 2's matrix-built `latest.json`
# files are per-target — we walk all assets, identify each bundle by
# filename suffix, pull the matching `.sig` body, and emit a single
# platforms-keyed manifest.
#
# Usage:
#   ci/scripts/build-desktop-manifest.sh <tag> <out-path>
#
# Required env:
#   GH_TOKEN   - github token with `repo` scope (or GITHUB_TOKEN in Actions)
#
# This script is invoked from .github/workflows/release.yml after the
# matrix `build` job finishes; it doesn't touch git, just produces the
# manifest file.

set -euo pipefail

TAG="${1:?usage: $0 <tag> <out-path>}"
OUT="${2:?usage: $0 <tag> <out-path>}"
REPO="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY env required}"

mkdir -p "$(dirname "${OUT}")"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

# Pull every asset for the tag locally — easier than guessing names.
# Note: pattern '' would mean "all"; gh prefers omitting the flag for that.
gh release download "${TAG}" --repo "${REPO}" --dir "${TMP}" --skip-existing || true

# Map (filename suffix → tauri platform key). Order matters: longer
# suffixes first so the .tar.gz on linux doesn't shadow the .deb.
declare -a PATTERNS=(
    'tauri:darwin-aarch64:_aarch64.app.tar.gz'
    'tauri:darwin-x86_64:_x64.app.tar.gz'
    'tauri:darwin-aarch64:.aarch64.dmg'
    'tauri:darwin-x86_64:.x64.dmg'
    'tauri:windows-x86_64:_x64-setup.exe'
    'tauri:windows-x86_64:_x64_en-US.msi'
    'tauri:linux-x86_64:_amd64.deb'
    'tauri:linux-x86_64:_amd64.AppImage'
    'tauri:linux-x86_64:.x86_64.rpm'
)

VERSION="${TAG#v}"
NOTES="$(printf 'Tag %s' "${TAG}")"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# Build platforms.{key}.{url,signature} entries by walking the bundles.
PLATFORMS_JSON='{}'

for entry in "${PATTERNS[@]}"; do
    IFS=':' read -r kind key suffix <<<"${entry}"
    found=""
    for f in "${TMP}"/*"${suffix}"; do
        [ -f "${f}" ] || continue
        found="${f}"
        break
    done
    [ -z "${found}" ] && continue

    fname="$(basename "${found}")"
    sig=""
    if [ -f "${found}.sig" ]; then
        sig="$(cat "${found}.sig")"
    fi
    url="https://github.com/${REPO}/releases/download/${TAG}/${fname}"

    # Tauri prefers .app.tar.gz on macOS for the updater payload; .dmg
    # entries are useful for first-install download links but not
    # consumed by the updater. Skip non-updater assets.
    case "${suffix}" in
        *.app.tar.gz|*-setup.exe|*.AppImage)
            ;;
        *)
            continue
            ;;
    esac

    PLATFORMS_JSON="$(jq \
        --arg key "${key}" --arg url "${url}" --arg sig "${sig}" \
        '.[$key] = { url: $url, signature: $sig }' \
        <<<"${PLATFORMS_JSON}")"
done

jq -n \
    --arg version "${VERSION}" \
    --arg notes "${NOTES}" \
    --arg pub_date "${PUB_DATE}" \
    --argjson platforms "${PLATFORMS_JSON}" \
    '{
        version: $version,
        notes: $notes,
        pub_date: $pub_date,
        platforms: $platforms
    }' \
    > "${OUT}"

echo "→ wrote ${OUT}"
cat "${OUT}"
