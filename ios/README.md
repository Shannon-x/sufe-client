# Xboard for iOS

Native SwiftUI shell over the shared `xboard-core` Rust kernel (via UniFFI)
plus sing-box (via `Libbox.xcframework`) running inside a NetworkExtension
PacketTunnelProvider.

## Layout

```
ios/
├── project.yml              # XcodeGen spec (single source of truth)
├── bootstrap.sh             # idempotent setup — run once after clone
├── XboardClient/            # the SwiftUI app target
│   ├── XboardClientApp.swift
│   ├── AppModel.swift       # @Observable @MainActor singleton
│   ├── ConnectionController.swift
│   ├── SecureStore.swift    # Keychain-backed UniFFI SecureStore
│   ├── Views/               # 12 SwiftUI screens
│   └── Resources/{en,zh-Hans}.lproj/Localizable.strings
├── PacketTunnel/            # NEPacketTunnelProvider target
│   └── PacketTunnelProvider.swift
├── Shared/Generated/        # UniFFI Swift bindings (regenerated)
└── Vendor/
    ├── Libbox.xcframework       # sing-box engine
    └── XboardCore.xcframework   # Rust kernel → static lib
```

`XboardClient.xcodeproj`, `Vendor/*.xcframework`, and `Shared/Generated/`
are **not** checked in — they're materialized by `bootstrap.sh`.

## Prerequisites

- Xcode 15+ on macOS 13+
- [`xcodegen`](https://github.com/yonaskolb/XcodeGen) — `brew install xcodegen`
- Rust toolchain with iOS targets:
  ```sh
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
  cargo install uniffi-bindgen-cli
  ```
- An Apple Developer account (any tier) — required because the app uses
  the `com.apple.developer.networking.networkextension` entitlement, which
  Apple only grants to signed builds. Set `DEVELOPMENT_TEAM` in your shell
  before building, or override via `xcodebuild DEVELOPMENT_TEAM=…`.

## Setup

```sh
# from repo root
just ios-bootstrap
open ios/XboardClient.xcodeproj
```

`bootstrap.sh` runs:

1. `xcodegen generate` → produces `XboardClient.xcodeproj`
2. `install-libbox-ios.sh` → downloads `Libbox.xcframework` from
   [SagerNet/sing-box-for-apple](https://github.com/SagerNet/sing-box-for-apple)
   into `Vendor/`
3. `build-core-ios.sh` → cross-compiles `xboard-core` for device + sim,
   lipo-merges the sim slices, and assembles `XboardCore.xcframework`
4. `build-uniffi-swift.sh` → emits Swift bindings into `Shared/Generated/`

Each step is idempotent — re-running only redoes missing artifacts. After
changing `core/src/ffi.udl` or any Rust code reachable from the FFI:

```sh
just core-ios          # rebuilds XboardCore.xcframework
just ios-bindings      # regenerates Swift bindings
```

## Build & run

Pure command-line smoke:

```sh
just ios-build         # xcodebuild -scheme XboardClient -destination iPhone 15 Pro sim
```

Day-to-day, open the project in Xcode and ⌘R. First launch on a real
device requires:

- a valid Apple Developer team selected for both `XboardClient` and
  `PacketTunnel` targets (XcodeGen sets `CODE_SIGN_STYLE = Automatic`)
- the `com.apple.developer.networking.networkextension` capability,
  which Apple grants automatically once the bundle id is registered
  with a paid team
- the App Group `group.com.xboard.client.ios` enabled on both targets
  (already declared in the entitlements files xcodegen produces)

The first VPN toggle prompts iOS to install the profile under
`Settings → General → VPN & Device Management → Xboard`.

## Architecture

- **App side (XboardClient)** owns the FFI `Client`. It logs in, fetches
  plans / orders / tickets / subscription, and — when the user toggles
  the VPN — calls `render_singbox_config(...)` over UniFFI to translate
  the mihomo subscription YAML into sing-box JSON. The result is dropped
  into the App Group's `UserDefaults` under
  `singbox.config.json`, then `NETunnelProviderManager.startVPNTunnel()`
  is called.
- **NE side (PacketTunnel)** has no FFI dependency at all (memory cap is
  50 MB on iOS, every kilobyte counts). It reads the JSON from the App
  Group, hands it to `LibboxNewService(...)`, and bridges packets between
  sing-box's `tun` inbound and `NEPacketTunnelFlow` via a thin
  `XboardPlatformInterface` conforming to `LibboxPlatformInterfaceProtocol`.
- **Keychain access group** `$(AppIdentifierPrefix)com.xboard.client`
  is shared between the two targets so the NE can read the bearer
  token if it ever needs to (today it doesn't — the app does all auth
  and only ships the rendered config).

## Customizing the backend URL

The default panel base is set in `Info.plist` via `XboardDefaultBackendURL`.
You can override it without rebuilding by writing to the Keychain key
`xboard.backend_base_url` (no in-app UI yet — wire one in if your build
ships against multiple panels).

## Protocol coverage

sing-box on iOS supports: VLESS, VMess, Shadowsocks, Trojan, Hysteria2,
TUIC. mihomo-only protocols (SSR, WireGuard, Hysteria 1, fingerprint
randomization) are dropped during translation in
`core/src/profile/inject_singbox.rs` with a `tracing::warn!` and won't
appear in the iOS node list.

## Troubleshooting

- **"xcodegen: command not found"** — `brew install xcodegen`
- **"DEVELOPMENT_TEAM is missing"** — set `DEVELOPMENT_TEAM=ABCD123456`
  in your shell, or hit "Signing & Capabilities" in Xcode and pick a team
- **"Could not find Libbox" linker error** — re-run `just ios-libbox`;
  the xcframework download may have failed mid-flight
- **"No NEVPNError 1"** — the `packet-tunnel-provider` entitlement isn't
  recognized for the bundle id; usually means the Apple Developer team
  hasn't been registered properly. Toggle to a free personal team and
  back to refresh entitlements.
- **NE crashes on tunnel start with "no sing-box config in app group"**
  — the main app didn't write the JSON. Verify `App Group` is enabled
  on *both* targets in Signing & Capabilities, or re-run xcodegen.
