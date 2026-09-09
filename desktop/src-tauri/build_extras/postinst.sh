#!/bin/sh
# Tauri externalBin is installed in /usr/bin with the target suffix removed.
# Only dpkg configure should grant capabilities; never search user directories.
set -eu
case "${1:-configure}" in configure) ;; *) exit 0 ;; esac
TARGET=/usr/bin/mihomo
if [ ! -f "$TARGET" ] || [ -L "$TARGET" ]; then
    echo "Sufe: packaged mihomo missing or a symlink; TUN unavailable" >&2
    exit 0
fi
if [ "$(stat -c %u "$TARGET")" != 0 ]; then
    echo "Sufe: refusing capabilities on a non-root-owned kernel" >&2
    exit 0
fi
# dpkg owns this exact packaged path. Ensure users cannot replace its contents.
chmod go-w "$TARGET"
if command -v setcap >/dev/null 2>&1 && setcap 'cap_net_admin,cap_net_bind_service+ep' "$TARGET"; then
    echo 'Sufe: enabled TUN capabilities on /usr/bin/mihomo'
else
    echo 'Sufe: TUN capabilities unavailable; check libcap2-bin and filesystem xattrs' >&2
fi
exit 0
