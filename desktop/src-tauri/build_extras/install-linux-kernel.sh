#!/bin/sh
# Embedded in the desktop executable. pkexec receives this constant script,
# with the build-time digest substituted before invocation. Only $1 is input.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077
EXPECTED_SHA256='@SUFE_KERNEL_SHA256@'
DEST_DIR=/usr/local/lib/sufe
DEST=/usr/local/lib/sufe/mihomo
SOURCE=${1:?missing bundled kernel}

[ "$(id -u)" = 0 ] || { echo 'Administrator authorization required' >&2; exit 1; }
[ -f "$SOURCE" ] && [ ! -L "$SOURCE" ] || { echo 'Invalid bundled kernel' >&2; exit 1; }
check_directory() {
    [ -d "$1" ] && [ ! -L "$1" ] || { echo 'Unsafe install directory' >&2; exit 1; }
    [ "$(stat -c %u "$1")" = 0 ] || { echo 'Install directory is not root-owned' >&2; exit 1; }
    mode=$(stat -c %a "$1")
    [ "$((0$mode & 022))" = 0 ] || { echo 'Install directory is writable by other users' >&2; exit 1; }
}
for directory in / /usr /usr/local /usr/local/lib; do check_directory "$directory"; done
if [ ! -e "$DEST_DIR" ]; then mkdir -m 0755 "$DEST_DIR"; fi
check_directory "$DEST_DIR"
[ ! -L "$DEST" ] || { echo 'Kernel destination is a symlink' >&2; exit 1; }
command -v setcap >/dev/null || { echo 'Install libcap2-bin (Debian/Ubuntu) or libcap (Fedora) first' >&2; exit 1; }
STAGED=$(mktemp "$DEST_DIR/.mihomo.XXXXXXXX")
trap 'rm -f "$STAGED"' EXIT HUP INT TERM
# Read a bounded snapshot, then validate that exact inode before granting caps.
/usr/bin/timeout --kill-after=2s 20s /usr/bin/dd if="$SOURCE" of="$STAGED" bs=1048576 count=128 iflag=nofollow,nonblock,fullblock status=none
actual=$(sha256sum "$STAGED")
actual=${actual%% *}
[ "$actual" = "$EXPECTED_SHA256" ] || { echo 'Bundled kernel checksum mismatch' >&2; exit 1; }
chown 0:0 "$STAGED"
chmod 0755 "$STAGED"
setcap 'cap_net_admin,cap_net_bind_service+ep' "$STAGED"
mv -Tf "$STAGED" "$DEST"
trap - EXIT HUP INT TERM
echo 'Sufe TUN kernel installed'
