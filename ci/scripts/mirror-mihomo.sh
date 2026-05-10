#!/usr/bin/env bash
# Mirror a mihomo release into a self-contained `dist/mirror/<version>/`
# tree, ready to upload to a CDN of choice (R2, S3, a separate GitHub
# Releases repo, …). The actual upload step is intentionally left out —
# pick your CDN tooling and call it after this script succeeds.
#
# Why this script exists: GitHub Releases enforces an unauthenticated
# rate limit (~60 req/h per IP) on the public binary downloads. CI on
# matrix machines + first-run users can quickly trip that limit, which
# breaks first connect (we fetch mihomo on demand at install time during
# packaging). Pinning a known-good mihomo build into our own CDN bucket
# isolates us from that.
#
# Usage:
#   ci/scripts/mirror-mihomo.sh <version>
#
# `<version>` is a mihomo Meta release tag, e.g. `v1.18.7`. The script
# downloads all four desktop targets, decompresses them, computes SHA-256
# digests, and writes:
#
#   dist/mirror/<version>/
#     ├── mihomo-aarch64-apple-darwin
#     ├── mihomo-x86_64-apple-darwin
#     ├── mihomo-x86_64-pc-windows-msvc.exe
#     ├── mihomo-x86_64-unknown-linux-gnu
#     └── manifest.json   { version, files:[{triple, name, size, sha256}] }
#
# Re-running with the same version is idempotent (overwrites in place).

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <mihomo-version>" >&2
    exit 1
fi

VERSION="$1"
case "${VERSION}" in
    v*) ;;
    *) echo "version must start with 'v', got: ${VERSION}" >&2; exit 1 ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT_DIR="${REPO_ROOT}/dist/mirror/${VERSION}"
mkdir -p "${OUT_DIR}"

TRIPLES=(
    aarch64-apple-darwin
    x86_64-apple-darwin
    x86_64-pc-windows-msvc
    x86_64-unknown-linux-gnu
)

mihomo_artifact() {
    case "$1" in
        x86_64-pc-windows-msvc)   echo "mihomo-windows-amd64-${VERSION}.zip" ;;
        aarch64-apple-darwin)     echo "mihomo-darwin-arm64-${VERSION}.gz" ;;
        x86_64-apple-darwin)      echo "mihomo-darwin-amd64-${VERSION}.gz" ;;
        x86_64-unknown-linux-gnu) echo "mihomo-linux-amd64-${VERSION}.gz" ;;
        *) echo "unsupported triple: $1" >&2; exit 2 ;;
    esac
}

# `shasum -a 256` (BSD) and `sha256sum` (GNU) both accept the same flags
# we care about; pick whichever is on PATH.
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

declare -a manifest_entries=()

for triple in "${TRIPLES[@]}"; do
    artifact="$(mihomo_artifact "${triple}")"
    url="https://github.com/MetaCubeX/mihomo/releases/download/${VERSION}/${artifact}"
    out="mihomo-${triple}"
    [[ "${triple}" == *windows* ]] && out="${out}.exe"

    echo "→ ${triple}: ${url}"
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' EXIT
    curl -fsSL "${url}" -o "${tmp}/${artifact}"

    case "${artifact}" in
        *.gz)  gunzip -c "${tmp}/${artifact}" > "${OUT_DIR}/${out}" ;;
        *.zip) unzip -p "${tmp}/${artifact}" > "${OUT_DIR}/${out}" ;;
        *)     cp "${tmp}/${artifact}" "${OUT_DIR}/${out}" ;;
    esac
    chmod +x "${OUT_DIR}/${out}"
    rm -rf "${tmp}"
    trap - EXIT

    size="$(stat -f '%z' "${OUT_DIR}/${out}" 2>/dev/null || stat -c '%s' "${OUT_DIR}/${out}")"
    digest="$(sha256 "${OUT_DIR}/${out}")"
    echo "  ${out}: ${size} bytes, sha256=${digest}"
    manifest_entries+=("    {\"triple\": \"${triple}\", \"name\": \"${out}\", \"size\": ${size}, \"sha256\": \"${digest}\"}")
done

manifest="${OUT_DIR}/manifest.json"
{
    printf '{\n'
    printf '  "version": "%s",\n' "${VERSION}"
    printf '  "files": [\n'
    # Join entries with comma+newline.
    n=${#manifest_entries[@]}
    for i in "${!manifest_entries[@]}"; do
        if [[ $((i + 1)) -lt $n ]]; then
            printf '%s,\n' "${manifest_entries[$i]}"
        else
            printf '%s\n' "${manifest_entries[$i]}"
        fi
    done
    printf '  ]\n'
    printf '}\n'
} > "${manifest}"

echo
echo "✓ mirror staged at ${OUT_DIR}"
echo "  upload step (pick one):"
echo "    rclone copy '${OUT_DIR}' my-cdn:xboard-mirror/${VERSION}"
echo "    aws s3 sync '${OUT_DIR}' s3://xboard-mirror/${VERSION}/"
echo "    gh release upload <our-mirror-tag> '${OUT_DIR}'/* --repo your-org/xboard-mirror"
