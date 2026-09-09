#!/usr/bin/env bash
# Download reviewed official sidecars. Python owns archive naming, checksum
# validation and atomic extraction for local and CI builds alike.
# Usage: install-mihomo-sidecar.sh [version] [--all]
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${1:-$(cat "${REPO_ROOT}/ci/mihomo-version.txt")}"
MODE="${2:-host}"
case "${MODE}" in host|--all) ;; *) echo 'usage: install-mihomo-sidecar.sh [version] [--all]' >&2; exit 2 ;; esac
PYTHON="${PYTHON:-python3}"
command -v "${PYTHON}" >/dev/null || PYTHON=python

host_triple() {
    if [[ -n "${TARGET_TRIPLE:-}" ]]; then printf '%s\n' "${TARGET_TRIPLE}"; return; fi
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) echo aarch64-apple-darwin ;;
        Darwin-x86_64) echo x86_64-apple-darwin ;;
        Linux-x86_64) echo x86_64-unknown-linux-gnu ;;
        MINGW*-x86_64|MSYS*-x86_64) echo x86_64-pc-windows-msvc ;;
        *) echo 'Unsupported host; set TARGET_TRIPLE explicitly' >&2; exit 2 ;;
    esac
}

install_one() {
    local triple="$1"
    # macOS ships Bash 3.2, where nounset rejects an empty array expansion.
    local args=(--version "${VERSION}" --target "${triple}")
    if [[ "${triple}" == *windows* && "${MIHOMO_WINDOWS_STANDARD:-false}" == true ]] ||
       [[ "${triple}" == *linux* && "${MIHOMO_LINUX_STANDARD:-false}" == true ]]; then
        args+=(--standard)
    fi
    "${PYTHON}" "${REPO_ROOT}/scripts/install-kernel.py" "${args[@]}"
}

if [[ "${MODE}" == --all ]]; then
    for triple in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
        install_one "${triple}"
    done
else
    install_one "$(host_triple)"
fi
