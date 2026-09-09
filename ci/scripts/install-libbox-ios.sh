#!/usr/bin/env bash
# Compatibility wrapper. Build the exact revision required by our Swift bridge.
# Upstream does not publish the zip assumed by the former download script.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
if [[ $# -gt 0 && "$1" != "1.10.7" && "$1" != "v1.10.7" ]]; then
  echo "This bridge supports only the 1.10.7 development baseline. See ios/README.md." >&2
  exit 2
fi
exec bash "${REPO_ROOT}/ios/build-libbox.sh"
