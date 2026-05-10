#!/bin/sh
# Tauri 2 rpm post-install hook for xboard-client. Mirrors postinst.sh
# (kept as a separate file so deb/rpm can diverge if rpmlint ever flags
# anything specific). Same goal: grant cap_net_admin to the bundled mihomo
# binary so TUN-mode connections work without the UI process needing root.
#
# rpm tolerates a non-zero exit (it logs and continues) but we keep the
# same exit-0-on-failure behaviour as the deb postinst for consistency.

set -u

APP_DIR=/usr/lib/Xboard
TARGET="$(find "$APP_DIR" -maxdepth 1 -name 'mihomo-*' -type f 2>/dev/null | head -n1 || true)"

if [ -z "${TARGET:-}" ]; then
    echo "xboard postinst: no mihomo binary under $APP_DIR — TUN mode will be unavailable"
    exit 0
fi

if ! command -v setcap >/dev/null 2>&1; then
    echo "xboard postinst: 'setcap' missing — install libcap (Fedora) or libcap2-bin (openSUSE) to enable TUN mode" >&2
    exit 0
fi

if setcap 'cap_net_admin,cap_net_bind_service+ep' "$TARGET" 2>/dev/null; then
    echo "xboard postinst: granted cap_net_admin to $TARGET"
else
    echo "xboard postinst: setcap on $TARGET failed (filesystem may not support xattrs)" >&2
fi

exit 0
