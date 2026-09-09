#!/usr/bin/env bash
# Sourced by download scripts. Unknown versions and missing pins fail closed.
verify_download() {
    local version="$1" artifact="$2" file="$3" root pins expected actual
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
    pins="${root}/ci/checksums/${version}.sha256"
    if [[ ! -f "${pins}" ]]; then
        echo "No reviewed checksum pins for ${version}; add ci/checksums/${version}.sha256 first" >&2
        return 1
    fi
    expected="$(awk -v name="${artifact}" '$2 == name { print $1 }' "${pins}")"
    if [[ ! "${expected}" =~ ^[a-f0-9]{64}$ ]]; then
        echo "Missing or ambiguous SHA256 pin for ${artifact}" >&2
        return 1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "${file}" | awk '{print $1}')"
    else
        actual="$(shasum -a 256 "${file}" | awk '{print $1}')"
    fi
    if [[ "${actual}" != "${expected}" ]]; then
        echo "SHA256 verification failed for ${artifact}" >&2
        return 1
    fi
    echo "  SHA256 verified: ${artifact}"
}
