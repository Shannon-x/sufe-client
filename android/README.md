# Sufe Android client

Native Kotlin + Jetpack Compose shell on top of the shared `xboard-core`
Rust crate (via UniFFI). Mirrors the [desktop](../desktop/) feature set —
auth, plans, orders, tickets, notices, plus a `VpnService`-backed TUN
toggle that runs the bundled mihomo executable in a supervised child process.


## 2026-09 可用性更新

- 默认 Sufe 浅色主题：#f6f7fb 背景、#7165e9 主色、#242539 正文；小屏首页/购买弹层可滚动，表单在宽屏居中限宽。
- VpnService 保留 ParcelFileDescriptor 所有权，Rust 仅借用 fd。launcher 在子进程 exec 前清除 FD_CLOEXEC，保留父进程的关闭责任。
- VPN 是 started foreground service，Activity 旋转不会关闭隧道；撤销 VPN 权限/服务销毁会停止内核；底层网络变化同步至 setUnderlyingNetworks。
- 自身 UID 从 VPN 路由中排除以避免 mihomo 出站回环；同时配置 IPv4/IPv6 路由。
- JNI libraries 使用 useLegacyPackaging=true，确保 libmihomo.so 实际解压到 nativeLibraryDir，供 exec 使用。
- checkout 网络失败不会显示支付成功。支付状态最多自动查询 10 分钟，订单完成时刷新套餐/订单；未完成的订单可从历史订单继续付款。
- 共享内核订阅缓存按订阅 URL 的 SHA256 隔离账号，无效 HTML/空节点不会覆盖好缓存；节点选择写入持久化 profile。

账户支持礼品卡、邀请与充值入口，规则页面支持增删和启用自定义规则。功能开放由部署配置与后台站点配置共同决定；验证码支持受限 HTTPS WebView 挑战。完整安装包仍须进行真机 VPN 与支付验收。

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
| Android NDK | r27d (27.3.13750724) | `sdkmanager "ndk;27.3.13750724"`; Gradle pins this version |
| Rust | stable | Plus the three Android targets — `just bootstrap` adds them |
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
just kernel-android v1.19.30        # ABI omitted = all three

# 5. Build the debug APK
just android-debug

# 6. Install on a connected device / emulator
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`compileDebugKotlin` automatically runs `generateUniffiBindings` first, which
shells out to the in-tree `uniffi-bindgen` cargo bin against `core/src/ffi.udl`
and writes `app/src/main/kotlin/com/xboard/client/core/xboard_core.kt`.

## Backend deployment

Android uses `Client.forDeployment`, with the same native deployment policy as
desktop and iOS. Set `SUFE_DEPLOYMENT_JSON` when compiling the Rust libraries;
it defines trusted API endpoints, feature flags, middleware encryption, encrypted
OSS discovery and Chatwoot settings. See [deployment configuration](../docs/client-deployment.md).
The default trusted endpoint is `https://www.isufe.me`. A nonempty build-time
`SUFE_DEPLOYMENT_JSON` takes precedence over `SUFE_BACKEND_URL` and the default.
Rebuild the Rust libraries for every packaged ABI to change an existing APK;
changing a Gradle property or secure-store URL does not override the native deployment policy.

## Why `.so` disguise

Android 10 blocks execution from the app's writable home directory for apps
targeting API 29 or higher ([Android documentation](https://developer.android.com/about/versions/10/behavior-changes-10#execute-permission)).
The executable therefore ships inside the APK as `libmihomo.so` and runs from
`applicationInfo.nativeLibraryDir`. AGP extracts it at install time thanks to
`packaging.jniLibs.useLegacyPackaging = true` in `app/build.gradle.kts`.

This is the same workaround used by clash-for-android, mihomo-party, etc.

## Common tasks

| Goal | Command |
|------|---------|
| Run unit tests | `cd android && ./gradlew test` |
| Run Android lint | `cd android && ./gradlew lint` |
| Strip x86_64 from a release | flip `abiFilters` in `app/build.gradle.kts` |
| Regenerate UniFFI bindings only | `cd android && ./gradlew generateUniffiBindings` |
| Wipe everything | `just clean` |

## CI

`/.github/workflows/mobile.yml` runs `just core-android`, `just kernel-android`,
`./gradlew assembleRelease`, then signs the APK with the keystore stored in
the `ANDROID_KEYSTORE_BASE64` repo secret. See the workflow file for the
exact keystore property names.

打包前会检查三个 ABI 的 libxboard_core.so 与 libmihomo.so 均为有效 ELF 文件，避免输出缺少实际内核、安装后无法启动的 APK。Kotlin 编译可以单独执行；它并不代表 APK 或 VPN 数据面已验证。

## 本次 Windows 构建验证

已输出 [Sufe-0.1.0-android-debug.apk](../artifacts/Sufe-0.1.0-android-debug.apk)，91,792,092 字节，三个 ABI 均确认内嵌 `https://www.isufe.me` 并移除旧默认域名。包名 com.xboard.client.debug，最低 Android 7.0（API 24），目标 API 34。APK 使用本机 Android Debug 证书签名；正式发行由 CI 使用运营方签名密钥。

SHA256：40951fb9c8133d7c5abc695fe1149448527796925071de3d5fa84359ea69abce。

本轮三个 Rust ABI release 重编分别耗时 1 分 48 秒、1 分 41 秒、1 分 48 秒；assembleDebug 耗时 3 分 2 秒。Kotlin/Java 界面及 FFI 签名未改，编译任务复用，未重新生成 UniFFI 绑定。直接检查成品 APK，15 个原生库的 ELF 架构与页对齐全部通过，apksigner v2 签名及 zipalign 通过，extractNativeLibs=true。AGP 对 mihomo 执行 NDK strip；本轮逐 ABI 复现转换并确认包内哈希一致，官方输入与成包哈希分别记录。详细证据见 [Android 构建验收记录](../artifacts/Android-build-verification.md)。

已安装并验证 Android SDK 34、Build Tools 34、Platform Tools、NDK r27d 和 Gradle 8.7。Windows 中文路径在 Kotlin daemon 的参数传递中可能编码错误，本次使用英文路径构建副本，并设置 kotlin.compiler.execution.strategy=in-process。第三方依赖镜像仅放于本机临时 Gradle init script，未改变仓库正式仓库地址。

当前宿主未开放 CPU 虚拟化扩展；官方 Android Emulator 37.1.11 与 API 34 系统镜像的三次软件模拟尝试均未启动 Android，已停止测试进程。adb 当前无设备，APK 安装、冷启动、4 KB/16 KB 真机 VPN 流量、实际支付尚未进行运行验收。连接测试必须使用有效账号与可用订阅，确认系统 VPN 授权、节点访问、网络切换及权限撤销后的断开行为。

## 16 KB 原生库兼容性

Android 固定使用官方 mihomo v1.19.30，压缩包 SHA256 取自 GitHub release asset digest 并存于 ci/checksums/v1.19.30.sha256。arm64 与 x86_64 的 PT_LOAD 对齐均已验证为 16384；32 位 ARM 为 4096。桌面内核版本可能不同，以其发行清单为准。

JNA 升至 5.18.0，包含上游 Android 16 KB 页面的两轮修复与后续死锁修复。Rust 的 .cargo/config.toml 为两个 Android 64 位目标配置 -Wl,-z,max-page-size=16384。打包后执行 python android/verify-native.py <APK路径>，检查 APK 中三个 ABI 的 Rust、mihomo、JNA，检测任何混入的不兼容 ELF；仍须在 4 KB/16 KB 设备上验证实际运行。

参考：[mihomo v1.19.30发布说明](https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.30)、[JNA变更记录](https://github.com/java-native-access/jna/blob/master/CHANGES.md)、[Android 16 KB要求](https://developer.android.com/guide/practices/page-sizes)。

本机验证工具：NDK r27d（27.3.13750724，Google 官方 SHA1 56607cbccd3642d4a1991f6bb3114a00f884f426）、cargo-ndk 4.1.2（官方 Windows 资产 SHA256 e2687c647748a8fb1c06981a4fc39376a5b5f640474db3f73de7ae247b7c55e0）。构建命令使用 cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 --platform 24 -o android/app/src/main/jniLibs build --release --lib -p xboard-core -j 2；这样只构建共享核心，不把仅供开发的绑定生成 CLI 打入 Android。
