#!/usr/bin/env bash
# Generate the two signing keys this project needs to ship releases:
#
#   1. Tauri ed25519 keypair  (desktop auto-update — required)
#   2. Android release keystore (APK signing — required for upgradeable APKs)
#
# Both run locally on your machine, never in CI. Output is plain-text
# secrets — keep the generated files out of git, and drop them into the
# GitHub repo's Settings → Secrets and variables → Actions.
#
# Usage:
#   bash ci/scripts/generate-release-keys.sh tauri      # only Tauri key
#   bash ci/scripts/generate-release-keys.sh android    # only Android keystore
#   bash ci/scripts/generate-release-keys.sh all        # both (default)
#
# Re-running with the same output paths refuses to overwrite — the
# Android keystore in particular is unrecoverable, since losing it locks
# every existing user out of in-place upgrades.

set -euo pipefail

MODE="${1:-all}"
OUT_DIR="${OUT_DIR:-${HOME}/.xboard-release-keys}"
mkdir -p "${OUT_DIR}"
chmod 700 "${OUT_DIR}"

case "${MODE}" in
  tauri|android|all) ;;
  *)
    echo "usage: $0 [tauri|android|all]" >&2
    exit 2
    ;;
esac

generate_tauri() {
  local key_path="${OUT_DIR}/tauri.key"
  if [[ -f "${key_path}" ]]; then
    echo "[skip] tauri key already exists at ${key_path} — refusing to overwrite" >&2
    return
  fi

  echo "==> generating Tauri ed25519 keypair"
  echo "    A password will be requested. Use a strong one and record it"
  echo "    separately — you'll paste it into TAURI_SIGNING_PRIVATE_KEY_PASSWORD."
  echo ""

  pushd "$(dirname "$0")/../../desktop" >/dev/null
  if ! command -v npx >/dev/null 2>&1; then
    echo "[fatal] npx not found — install Node.js first" >&2
    exit 1
  fi
  npx --yes @tauri-apps/cli signer generate -w "${key_path}"
  popd >/dev/null

  local pubkey
  pubkey="$(cat "${key_path}.pub")"

  cat <<EOF

Tauri keypair generated.
  Private key file : ${key_path}
  Public key file  : ${key_path}.pub

Action items:
  1. Paste the *contents* of ${key_path} into the GitHub repo secret:
       TAURI_SIGNING_PRIVATE_KEY
     The password you just typed goes into:
       TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  2. Replace the placeholder pubkey in
       desktop/src-tauri/tauri.conf.json
     -> plugins.updater.pubkey
     with the following string (NO quotes, single line):

${pubkey}

  3. Commit the tauri.conf.json change. NEVER commit ${key_path} itself.

EOF
}

generate_android() {
  local store_path="${OUT_DIR}/xboard-release.jks"
  if [[ -f "${store_path}" ]]; then
    echo "[skip] android keystore already exists at ${store_path} — refusing to overwrite" >&2
    return
  fi

  if ! command -v keytool >/dev/null 2>&1; then
    echo "[fatal] keytool not found — install JDK 17+ (e.g. brew install openjdk@17)" >&2
    exit 1
  fi

  local password alias_name
  read -rsp "Choose a keystore password (min 6 chars): " password
  echo ""
  if [[ ${#password} -lt 6 ]]; then
    echo "[fatal] password too short" >&2
    exit 1
  fi
  read -rp "Choose a key alias [xboard]: " alias_name
  alias_name="${alias_name:-xboard}"

  echo "==> generating Android release keystore (validity: 100 years)"
  keytool -genkey -v \
    -keystore "${store_path}" \
    -alias "${alias_name}" \
    -keyalg RSA -keysize 4096 -validity 36500 \
    -storepass "${password}" \
    -keypass "${password}" \
    -dname "CN=Xboard, O=Xboard, C=CN"

  local b64_path="${store_path}.base64"
  base64 -i "${store_path}" > "${b64_path}"

  cat <<EOF

Android keystore generated.
  Keystore file        : ${store_path}
  Base64 (for secret)  : ${b64_path}

Action items — paste each into the GitHub repo's Actions secrets:
  ANDROID_KEYSTORE_BASE64    <contents of ${b64_path}>
  ANDROID_KEYSTORE_PASSWORD  <the password you just chose>
  ANDROID_KEY_ALIAS          ${alias_name}
  ANDROID_KEY_PASSWORD       <the same password>

⚠ Back ${store_path} up offline. If you lose it, every Android user has to
  uninstall + reinstall to receive future updates.

EOF
}

case "${MODE}" in
  tauri)   generate_tauri ;;
  android) generate_android ;;
  all)     generate_tauri; generate_android ;;
esac

echo "Done. Generated material lives under ${OUT_DIR} (mode 700)."
