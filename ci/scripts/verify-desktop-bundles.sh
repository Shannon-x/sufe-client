#!/usr/bin/env bash
# Run on the native CI host after tauri build. Validate what will be uploaded,
# not just whether a compiler returned success.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TRIPLE="${1:?usage: verify-desktop-bundles.sh <target-triple>}"
VERSION="${MIHOMO_VERSION:-$(cat "${ROOT}/ci/mihomo-version.txt")}"
BUNDLES="${ROOT}/target/${TRIPLE}/release/bundle"
OUT="${ROOT}/artifacts/desktop/${TRIPLE}"
mkdir -p "${OUT}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
shopt -s nullglob
REPORT="${OUT}/verification.txt"
printf 'Source: %s\nTarget: %s\nKernel: %s\n' "$(git -C "$ROOT" rev-parse HEAD)" "$TRIPLE" "$VERSION" > "$REPORT"

case "$TRIPLE" in
  aarch64-apple-darwin|x86_64-apple-darwin)
    APP="${BUNDLES}/macos/Sufe.app"
    ARCH=x86_64
    [[ "$TRIPLE" == aarch64-* ]] && ARCH=arm64
    for binary in xboard-desktop mihomo xboard-helper; do
      path="${APP}/Contents/MacOS/${binary}"
      test -x "$path"
      lipo -verify_arch "$ARCH" "$path"
      file "$path" >> "$REPORT"
    done
    codesign --verify --deep --strict "$APP"
    "${APP}/Contents/MacOS/mihomo" -v | tee -a "$REPORT" | grep -F "$VERSION"
    python3 "$ROOT/ci/scripts/probe-macos-signatures.py" "$APP" "$ROOT" "$TRIPLE" "$OUT/codesign-stability.json" | tee -a "$REPORT"
    bash "$ROOT/ci/scripts/smoke-helper-macos.sh" "$APP" "$OUT"
    printf 'Verified real root helper installation, owner IPC, legacy path rejection, restricted controller and isolated TUN lifecycle without changing default routes; test helper uninstalled.\n' >> "$REPORT"
    # Bash 3.2 + nounset cannot inspect an empty array after nullglob.
    set -- "${BUNDLES}/dmg/"*.dmg
    [[ $# -gt 0 ]] || { echo 'No macOS DMG was produced' >&2; exit 1; }
    cp "$@" "$OUT/"
    ditto -c -k --sequesterRsrc --keepParent "$APP" "${OUT}/Sufe-${TRIPLE}.app.zip"
    printf 'Verified both sidecars, native architecture and ad-hoc code signature. No Apple notarization.\n' >> "$REPORT"
    ;;
  x86_64-unknown-linux-gnu)
    debs=("${BUNDLES}/deb/"*.deb)
    images=("${BUNDLES}/appimage/"*.AppImage)
    [[ ${#debs[@]} -eq 1 && ${#images[@]} -eq 1 ]]
    # Inspect the package metadata: CI's build dependencies alone cannot prove
    # a clean desktop installation will receive GTK, WebKit and tray runtimes.
    depends="$(dpkg-deb -f "${debs[0]}" Depends)"
    printf 'deb Depends: %s\n' "$depends" | tee -a "$REPORT"
    for dependency in libcap2-bin iproute2 libgtk-3-0 libwebkit2gtk-4.1-0 libayatana-appindicator3-1; do
      if ! printf '%s\n' "$depends" | tr ',' '\n' | awk '{print $1}' | grep -Fxq "$dependency"; then
        echo "deb is missing required runtime dependency: $dependency" >&2
        exit 1
      fi
    done
    dpkg-deb -R "${debs[0]}" "${TMP}/deb"
    for binary in xboard-desktop mihomo; do
      test -x "${TMP}/deb/usr/bin/${binary}"
      file "${TMP}/deb/usr/bin/${binary}" | tee -a "$REPORT" | grep 'x86-64'
    done
    test -x "${TMP}/deb/DEBIAN/postinst"
    grep -F 'TARGET=/usr/bin/mihomo' "${TMP}/deb/DEBIAN/postinst"
    sudo apt-get install -y "${debs[0]}"
    getcap /usr/bin/mihomo | tee -a "$REPORT" | grep cap_net_admin
    /usr/bin/mihomo -v | tee -a "$REPORT" | grep -F "$VERSION"
    python3 "$ROOT/ci/scripts/smoke-linux-tun.py" --version "$VERSION" --report "$OUT/linux-tun-smoke.json"
    printf 'Verified real non-root deb capability TUN device lifecycle at 198.18.88.1/30 with default routes unchanged; no airport data-plane traffic.\n' >> "$REPORT"
    chmod +x "${images[0]}"
    (cd "$TMP" && "${images[0]}" --appimage-extract >/dev/null)
    test -x "${TMP}/squashfs-root/usr/bin/xboard-desktop"
    test -x "${TMP}/squashfs-root/usr/bin/mihomo"
    "${TMP}/squashfs-root/usr/bin/mihomo" -v | tee -a "$REPORT" | grep -F "$VERSION"
    # Fresh, unauthenticated UI launches under Xvfb; no VPN or proxy is started.
    # Reaching the timeout proves it stayed alive, while a crash fails the job.
    for format in deb appimage; do
      executable=/usr/bin/xboard-desktop
      [[ "$format" == appimage ]] && executable="${TMP}/squashfs-root/AppRun"
      set +e
      WEBKIT_DISABLE_DMABUF_RENDERER=1 timeout 12s dbus-run-session -- xvfb-run -a "$executable" >"${OUT}/${format}-startup.log" 2>&1
      status=$?
      set -e
      if [[ "$status" != 124 ]]; then cat "${OUT}/${format}-startup.log"; echo "$format exited unexpectedly: $status" >&2; exit 1; fi
    done
    cp "${debs[0]}" "${images[0]}" "$OUT/"
    printf 'Verified deb layout, installed TUN capability, AppImage extraction and 12-second unauthenticated GUI startup for both packages.\nAppImage is read-only and does not gain TUN capabilities from deb postinst.\n' >> "$REPORT"
    ;;
  *) echo "unsupported target: $TRIPLE" >&2; exit 2 ;;
esac

(
  cd "$OUT"
  for package in *.dmg *.zip *.deb *.AppImage; do
    [[ -f "$package" ]] || continue
    if command -v sha256sum >/dev/null; then sha256sum "$package"; else shasum -a 256 "$package"; fi
  done > SHA256SUMS
)
cat "$REPORT"
