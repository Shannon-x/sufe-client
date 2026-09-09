#!/usr/bin/env bash
# Destructive cleanup is limited to this test's previously absent namespace.
# Never run this against a developer or user Mac, even with explicit sudo.
set -euo pipefail
[[ "$(uname -s)" == Darwin && "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted && "${RUNNER_OS:-}" == macOS && "$(id -u)" != 0 ]] || {
  echo 'Helper smoke requires a disposable GitHub-hosted macOS runner and ordinary runner UID.' >&2; exit 2;
}
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="${1:?usage: smoke-helper-macos.sh APP REPORT_DIRECTORY}"
OUT="${2:?report directory required}"
BASE='/Library/Application Support/com.xboard.client'
PLIST='/Library/LaunchDaemons/com.xboard.client.helper.plist'
[[ ! -e "$BASE" && ! -L "$BASE" && ! -e "$PLIST" && ! -L "$PLIST" ]] || {
  echo 'Refusing to replace an existing helper installation.' >&2; exit 2;
}
if /sbin/ifconfig utun1989 >/dev/null 2>&1 || /bin/launchctl print system/com.xboard.client.helper >/dev/null 2>&1; then
  echo 'Refusing to touch an existing TUN or launch daemon.' >&2; exit 2
fi
sudo -n /usr/bin/true
mkdir -p "$OUT"
TMP="$(mktemp -d)"
cleanup() {
  status=$?
  trap - EXIT
  set +e
  python3 "$ROOT/ci/scripts/smoke-helper-macos.py" --stop >/dev/null 2>&1
  sudo -n /bin/launchctl bootout system/com.xboard.client.helper >/dev/null 2>&1
  # Collect only kernel logs, never private config snapshots or credentials.
  sudo -n /usr/bin/find "$BASE/state" -name '*.log' -type f -exec /usr/bin/tail -n 100 {} \; > "$OUT/helper-smoke-kernel.log" 2>/dev/null
  if [[ -x "$TMP/render-install" ]]; then
    "$TMP/render-install" uninstall > "$TMP/uninstall.sh"
    sudo -n /bin/bash "$TMP/uninstall.sh" >> "$OUT/helper-smoke-cleanup.log" 2>&1 || status=1
  fi
  # These exact paths were absent before this test. Never follow a replaced link.
  if [[ -L "$BASE" ]]; then status=1
  elif [[ -d "$BASE" ]]; then sudo -n /bin/rm -rf -- '/Library/Application Support/com.xboard.client' || status=1
  fi
  [[ ! -e "$PLIST" && ! -L "$PLIST" ]] || status=1
  for attempt in 1 2 3 4 5; do
    /sbin/ifconfig utun1989 >/dev/null 2>&1 || break
    sleep 1
  done
  if /sbin/ifconfig utun1989 >/dev/null 2>&1; then echo 'Test TUN survived cleanup' >&2; status=1; fi
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT
rustc --edition 2021 "$ROOT/helper/examples/render-install.rs" -o "$TMP/render-install"
# Like the application, snapshot both sidecars as the ordinary installation UID.
cp "$APP/Contents/MacOS/xboard-helper" "$TMP/xboard-helper"
cp "$APP/Contents/MacOS/mihomo" "$TMP/mihomo"
chmod 600 "$TMP/xboard-helper" "$TMP/mihomo"
HELPER_SHA="$(shasum -a 256 "$TMP/xboard-helper" | cut -d' ' -f1)"
KERNEL_SHA="$(shasum -a 256 "$TMP/mihomo" | cut -d' ' -f1)"
"$TMP/render-install" install "$TMP/xboard-helper" "$TMP/mihomo" "$HELPER_SHA" "$KERNEL_SHA" "$(id -u)" > "$TMP/install.sh"
# CI only: trusted checked-out renderer. Production passes the literal script
# directly to osascript and never elevates a user-writable script file.
sudo -n /bin/bash "$TMP/install.sh" > "$OUT/helper-smoke-install.log" 2>&1
python3 "$ROOT/ci/scripts/smoke-helper-macos.py" --report "$OUT/helper-smoke.json" --version "$(tr -d '\r\n' < "$ROOT/ci/mihomo-version.txt")"
