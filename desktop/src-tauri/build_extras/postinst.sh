#!/bin/sh
# Tauri 2 deb post-install hook for xboard-client.
#
# Goal: grant cap_net_admin (and cap_net_bind_service for high-privilege
# port binding) to the bundled mihomo binary, so the unprivileged UI process
# doesn't have to run as root for TUN-mode connections to work.
#
# Run as root by dpkg. Failures are logged and swallowed (exit 0) so a
# package install never fails on a missing libcap2-bin or an exotic
# filesystem — the UI itself will detect the missing capability via
# DirectLauncher::ensure_privileged and downgrade to system-proxy mode.

set -u

APP_DIR=/usr/lib/Xboard
TARGET="$(find "$APP_DIR" -maxdepth 1 -name 'mihomo-*' -type f 2>/dev/null | head -n1 || true)"

if [ -z "${TARGET:-}" ]; then
    echo "xboard postinst: no mihomo binary under $APP_DIR — TUN mode will be unavailable"
    exit 0
fi

if ! command -v setcap >/dev/null 2>&1; then
    echo "xboard postinst: 'setcap' missing — install libcap2-bin to enable TUN mode" >&2
    exit 0
fi

if setcap 'cap_net_admin,cap_net_bind_service+ep' "$TARGET" 2>/dev/null; then
    echo "xboard postinst: granted cap_net_admin to $TARGET"
else
    echo "xboard postinst: setcap on $TARGET failed (filesystem may not support xattrs)" >&2
fi

exit 0
