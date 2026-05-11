# Xboard Android client

Native Kotlin + Jetpack Compose shell on top of the shared `xboard-core`
Rust crate (via UniFFI). Mirrors the [desktop](../desktop/) feature set —
auth, plans, orders, tickets, notices, plus a `VpnService`-backed TUN
toggle that runs the bundled mihomo binary in-process.

## Layout

```
android/
├── app/                                    # :app module
│   ├── build.gradle.kts                    # AGP/Kotlin/Compose config
│   └── src/main/
│       ├── AndroidManifest.xml             # VpnService + ForegroundService
│       ├── kotlin/com/xboard/client/       # hand-written code
│       │   ├── ui/screens/                 # Compose screens
│       │   ├── ui/components/              # Card / EmptyState / Scaffold
│       │   ├── vm/AppViewModel.kt          # StateFlow-driven UI state
│       │   ├── vpn/XboardVpnService.kt     # VpnService.Builder + TunDelegate
│       │   ├── store/AndroidSecureStore.kt # EncryptedSharedPreferences impl
│       │   └── MainActivity.kt
│       ├── kotlin/com/xboard/client/core/  # *generated* — UniFFI Kotlin
│       │                                   #   bindings (gitignored)
│       ├── jniLibs/<abi>/libxboard_core.so # *generated* — Rust dylib
│       │                                   #   (gitignored, see §Build)
│       ├── jniLibs/<abi>/libmihomo.so      # *generated* — mihomo binary
│       │                                   #   disguised as .so (see §Why)
│       └── res/values{,-en}/strings.xml    # i18n (zh-CN default, en-US)
├── settings.gradle.kts                     # rootProject + include(":app")
├── build.gradle.kts                        # plugin alias declarations
├── gradle.properties                       # JVM args + AndroidX flags
├── gradle/wrapper/                         # *fetched at bootstrap*
└── bootstrap-wrapper.sh                    # see §Setup
```

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| JDK | 17 | Temurin or any Adoptium build; `JAVA_HOME` must point at it |
| Android SDK | API 34 | Install via Android Studio or `cmdline-tools` |
| Android NDK | r26 (26.x) | `sdkmanager "ndk;26.1.10909125"` — pin via `local.properties`'s `ndk.dir` |
| Rust | stable | Plus the four Android targets — `just bootstrap` adds them |
| `cargo-ndk` | latest | Installed by `just bootstrap` |
| `ktlint` (optional) | latest | UniFFI auto-formats Kotlin output if found on PATH |

`just bootstrap` (run from the repo root) handles the Rust side. The Android
SDK / NDK side is still manual — install Android Studio once, or follow the
[`cmdline-tools` instructions](https://developer.android.com/tools).

## Setup

```sh
# 1. Repo-level Rust toolchain + cargo-ndk (one-time, from repo root)
just bootstrap

# 2. Android Gradle wrapper (one-time, fetched from gradle/gradle repo)
just android-bootstrap

# 3. Cross-compile xboard-core for arm64-v8a / armeabi-v7a / x86_64
#    Output → android/app/src/main/jniLibs/<abi>/libxboard_core.so
just core-android

# 4. Pull mihomo binaries (3 ABIs) and drop them as libmihomo.so
#    (see "Why .so disguise" below)
just kernel-android v1.19.10        # ABI omitted = all three

# 5. Build the debug APK
just android-debug

# 6. Install on a connected device / emulator
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`compileDebugKotlin` automatically runs `generateUniffiBindings` first, which
shells out to the in-tree `uniffi-bindgen` cargo bin against `core/src/ffi.udl`
and writes `app/src/main/kotlin/com/xboard/client/core/xboard_core.kt`.

## Backend URL

The default backend baked into `BuildConfig.DEFAULT_BACKEND_URL` is
`https://imitate.cnqq.de` — override via:

- `gradle.properties` (do not commit): `xboard.defaultBackendUrl=https://panel.example.com`
- env var: `XBOARD_DEFAULT_BACKEND_URL=https://panel.example.com`
- runtime: write to the secure-store key `xboard.backend_base_url`
  (no in-app UI, used by QA / staging)

## Why `.so` disguise

Android 12+ blocks `execve` from `/data/data/<pkg>/`, but
`applicationInfo.nativeLibraryDir` (where AGP unpacks `jniLibs/`) keeps the
exec bit. Naming the mihomo binary `libmihomo.so` lets `Process.start()`
find and exec it. AGP packages it uncompressed thanks to
`packaging.jniLibs.useLegacyPackaging = true` in `app/build.gradle.kts`.

This is the same workaround used by clash-for-android, mihomo-party, etc.

## Common tasks

| Goal | Command |
|------|---------|
| Run unit tests | `cd android && ./gradlew test` |
| Run lint + detekt | `cd android && ./gradlew lint detekt` |
| Strip x86_64 from a release | flip `abiFilters` in `app/build.gradle.kts` |
| Regenerate UniFFI bindings only | `cd android && ./gradlew generateUniffiBindings` |
| Wipe everything | `just clean` |

## CI

`/.github/workflows/mobile.yml` runs `just core-android`, `just kernel-android`,
`./gradlew assembleRelease`, then signs the APK with the keystore stored in
the `ANDROID_KEYSTORE_BASE64` repo secret. See the workflow file for the
exact keystore property names.
