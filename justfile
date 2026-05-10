# xboard-client top-level command runner
# `cargo install just` then `just <recipe>`

set shell := ["bash", "-uc"]
set dotenv-load := true

# ---------------- Default ----------------
default:
    @just --list

# ---------------- Bootstrap ----------------
bootstrap:
    @echo "→ Rust toolchain"
    rustup show active-toolchain || rustup default stable
    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
    @echo "→ cargo-ndk for Android cross-compile"
    cargo install cargo-ndk uniffi-bindgen-cli || true
    @echo "→ Tauri prerequisites"
    cargo install tauri-cli --version "^2.0" || true
    @echo "Done. Now install bun (https://bun.sh) for the desktop frontend."

# ---------------- core (Rust) ----------------
core-build:
    cargo build -p xboard-core

core-test:
    cargo test -p xboard-core --all-features

core-check:
    cargo check -p xboard-core --all-features
    cargo clippy -p xboard-core --all-features -- -D warnings

core-fmt:
    cargo fmt -p xboard-core

# Android cross-compile (run from monorepo root)
core-android:
    cd core && cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o ../android/app/src/main/jniLibs build --release

# ---------------- desktop (Tauri) ----------------
# `npm` is the baseline; swap in bun/pnpm if available.
# `--cache=$PWD/.npm-cache` sidesteps a recurring permission issue on
# globally shared ~/.npm caches.
desktop-install:
    cd desktop && npm install --no-audit --fund=false --cache=$PWD/.npm-cache

desktop-check:
    cargo check -p xboard-desktop
    cargo clippy -p xboard-desktop --all-targets -- -D warnings
    cd desktop && ./node_modules/.bin/vue-tsc --noEmit

desktop-dev:
    cd desktop && npm run tauri -- dev

desktop-build:
    cd desktop && npm run tauri -- build

# ---------------- android ----------------
# One-time fetch of gradle-wrapper.jar / gradlew / gradlew.bat from the
# upstream gradle/gradle repo (we don't check those in). Run after a fresh
# clone before any other android-* recipe.
android-bootstrap:
    bash android/bootstrap-wrapper.sh

android-debug:
    cd android && ./gradlew assembleDebug

android-release:
    cd android && ./gradlew assembleRelease

# ---------------- kernel mirror ----------------
mirror-kernel version:
    bash ci/scripts/mirror-mihomo.sh {{version}}

# Drop the mihomo sidecar binary into desktop/src-tauri/binaries/ for the
# current host triple. Tauri's bundler validates externalBin at build time,
# so this must run before `desktop-check` / `desktop-dev` / `desktop-build`.
# Pass `--all` to fetch all four desktop triples (use this in CI).
kernel-fetch version *args:
    bash ci/scripts/install-mihomo-sidecar.sh {{version}} {{args}}

# Pull mihomo Android binaries (3 ABIs disguised as libmihomo.so) into
# android/app/src/main/jniLibs/ so Gradle packages them with the APK.
# Run before `just android-debug` / `android-release`. Pass an ABI as
# the second arg to limit the fetch (e.g. `just kernel-android v1.19.10 arm64-v8a`).
kernel-android version *args:
    bash ci/scripts/install-mihomo-android.sh {{version}} {{args}}

# Build the macOS root helper for the current host triple and drop it
# under desktop/src-tauri/binaries/ so Tauri's externalBin resolver finds
# it. Pass `release` for a stripped/optimized build.
helper-build profile="debug":
    bash ci/scripts/install-helper-sidecar.sh {{profile}}

# ---------------- ios ----------------
# One-time / on-demand bootstrap: pulls Libbox.xcframework, cross-compiles
# xboard-core for ios + ios-sim, builds XboardCore.xcframework, generates
# UniFFI Swift bindings, and runs xcodegen to materialize the .xcodeproj.
# Run after a fresh clone (or whenever ffi.udl changes).
ios-bootstrap:
    bash ios/bootstrap.sh

# Cross-compile xboard-core for iOS device (aarch64-apple-ios) plus the
# arm64 + x86_64 simulator slices, lipo them, and produce
# `ios/Vendor/XboardCore.xcframework`. The xcodegen project.yml references
# this path directly.
core-ios:
    bash ci/scripts/build-core-ios.sh

# Generate UniFFI Swift bindings for the current ffi.udl into
# `ios/Shared/Generated/`. Re-run whenever the FFI surface changes.
ios-bindings:
    bash ci/scripts/build-uniffi-swift.sh

# Pull (or refresh) the Libbox.xcframework binary into ios/Vendor/.
# Optional version arg pins to a specific sing-box-for-apple release.
ios-libbox version="latest":
    bash ci/scripts/install-libbox-ios.sh {{version}}

# Compile the iOS app for the simulator (smoke check). Requires
# `just ios-bootstrap` to have run at least once. The destination string
# uses a generic iPhone 15 Pro sim — adjust if your toolbox differs.
ios-build:
    cd ios && xcodebuild -project XboardClient.xcodeproj \
        -scheme XboardClient -configuration Debug \
        -destination 'platform=iOS Simulator,name=iPhone 15 Pro' \
        -allowProvisioningUpdates \
        build

# ---------------- release ----------------
# One-time setup: generate the ed25519 keypair Tauri's updater uses to
# verify release manifests. Paste the *private* key into the GitHub
# repo secret `TAURI_SIGNING_PRIVATE_KEY` and copy the *public* key into
# `desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.
# The keyfile (`xboard-updater.key{,.pub}`) lands in the current dir —
# **do not commit it**, the repo .gitignore already excludes it.
release-keygen:
    cd desktop && npm run tauri -- signer generate -w ../xboard-updater.key

# Cut a release: bumps the package version, tags it, and reminds you to
# push. The actual build runs in GitHub Actions on tag push (see
# `.github/workflows/release.yml`).
release-tag version:
    git tag -a "v{{version}}" -m "Release v{{version}}"
    @echo
    @echo "→ Now push the tag to fire the release workflow:"
    @echo "   git push origin v{{version}}"

# ---------------- clean ----------------
clean:
    cargo clean
    rm -rf desktop/dist desktop/src-tauri/target
    cd android && ./gradlew clean || true
