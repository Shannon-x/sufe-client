# SUFE Client

为自己的 Xboard-sh 服务构建的专属客户端。桌面端使用 Vue 3 + Tauri 2 + Rust + mihomo，包含连接仪表盘、节点管理、套餐购买、订单、账户、公告、客服、日志与自定义分流规则。终端用户只需账户登录，不需输入面板地址。

桌面、Android 与 iOS 共用 Rust 业务与安全配置层，移动端分别使用 Compose 和 SwiftUI。两端已接入礼品卡、邀请、自定义规则、原生 Chatwoot、真实订单金额复核、动态验证码及会话保护；界面统一浅色、紫色和卡片风格。以下分别列出实现范围与平台验收状态。

| 平台 | 界面/核心 | 当前范围 | 验收边界 |
|---|---|---|---|
| Windows | Vue/Tauri + mihomo | 桌面业务界面；默认 TUN，首次连接安装专用权限服务；保留手动系统代理 | perMachine 安装到 Program Files；真实账号支付与订阅仍需对应环境 |
| macOS | 同一 Vue/Tauri + mihomo | 共用桌面代码；默认 TUN，首次连接系统授权安装 helper | Apple Silicon / Intel 原生 DMG 与 app.zip 已构建，实际 helper / TUN 验证通过；尚无 Apple 公证 |
| Linux | 同一 Vue/Tauri + mihomo | 共用桌面代码；默认 TUN，deb 自动授予内核网络权限，AppImage 一次 polkit 授权 | 原生 deb / AppImage 构建与启动验证结果见平台记录 |
| Android | Compose + Rust + mihomo | 登录/节点/VPN、套餐/订单、公告/工单、礼品卡/邀请、规则、原生客服和验证码 | 三 ABI APK 编译、签名、ELF/16 KB 检查通过；仍需设备启动、真实订阅、VPN 数据面与运营业务验收 |
| iOS | SwiftUI + NetworkExtension + **sing-box/Libbox** | 同类原生业务入口、订单复核、验证码及 PacketTunnel 适配 | 27 个 Swift 文件通过语法解析；无 Xcode 类型编译与 Apple 签名，兼容内核仍需升级/真机验证 |

## 先运行桌面

准备 Node.js 24、Rust stable、Python 3.10+ 及对应系统的 Tauri 原生编译依赖。

```text
cd desktop
npm ci
npm run dev
```

这是网页开发服务。完整真实客户端需要 Tauri 宿主；仅在开发时显式启用的预览数据不能连接 VPN、充值或付款。

Windows 从仓库根目录准备经过固定 SHA256 校验的官方 mihomo 与 Wintun：

```text
python scripts/install-kernel.py --target x86_64-pc-windows-msvc
cargo build -p xboard-svc
```

将 `target/debug/xboard-svc.exe` 复制为 `desktop/src-tauri/binaries/xboard-svc-x86_64-pc-windows-msvc.exe`，再进入 `desktop` 执行 `npm run tauri -- dev`。macOS/Linux 的目标、构建依赖与发布步骤见 [SETUP.md](SETUP.md)。

## 专属后端与功能开关

[部署文档](docs/client-deployment.md) 描述 `SUFE_DEPLOYMENT_JSON` 构建配置、品牌、功能开关、多个 OSS 每个包含多个 API、签名与加密配置发布方法，以及对真实 `sufe-middleware-rs` Stealth-v1 的兼容。

桌面与移动已通过共享核心接入套餐、优惠券、订单、礼品卡、邀请、公告与工单。购买流程先生成订单并展示服务端实际应付，再确认支付；既有订单可继续付款，成功以真实订单状态为准。移动端规则按账户保存，Android 在连接时注入，iOS 在转换为 sing-box 配置前注入。

Chatwoot 需配置运营方自己的 Public API Inbox，默认关闭；已接入原生消息界面和受信任 REST 桥接，应用关闭后的推送不属于已完成能力。登录、注册、找回密码和发送邮件支持按站点配置启用 Turnstile、reCAPTCHA v2/v3，真实站点密钥仍需设备验收。当前本地 Xboard-sh-1 源码没有可验证的独立充值/签到接口，因此关闭这些入口；线上若安装不同插件，需要补齐该插件的真实契约后启用。

源码默认后端已按运营方指定改为 `https://www.isufe.me`，桌面、Android 与 iOS 读取同一份 Rust 部署配置。2026-09-09 该地址的公共配置 API 返回 HTTP 200 JSON；这仅证明公共接口可达，不证明账号、支付渠道或订阅连接已经验证。Windows、macOS 两种架构、Linux 与 Android 三 ABI 已重新编译并验包，当前下载包均内嵌新地址。登录接口实际返回“邮箱或密码错误”（HTTP 400），尚未取得有效会话和订阅；需要更正账号信息后继续实网验收。正在运行的旧 Windows 客户端保留原状，需从托盘退出后安装新包。

## 检查与文档

本轮 Rust 核心回归 **111 项通过、1 项默认忽略**，真实 mihomo 配置校验已另行显式执行。Windows 新安装包和内嵌地址校验通过；上一轮已安装版本的权限服务与实际 TUN 创建/清理验证通过，当前新包尚未升级安装；Linux 原生构建、25 项 Rust 定向检查、脚本检查、TUN 和两种包的 GUI 启动通过；macOS 两种架构已完成 DMG / app.zip、签名、真实 helper / TUN 及私密配置权限验收。原有桌面 Playwright **22/22**、账单 **3/3** 和生产构建通过，本轮另重跑连接与 CSP 的 10 项回归。iOS **27/27** Swift 语法解析及 **3/3** 验证码模拟检查不替代 Xcode 或设备验收。

本地产物包括 [Mac Apple Silicon DMG](artifacts/desktop/aarch64-apple-darwin/Sufe_0.1.0_aarch64.dmg)、[Mac Intel DMG](artifacts/desktop/x86_64-apple-darwin/Sufe_0.1.0_x64.dmg)、[Linux deb](artifacts/desktop/x86_64-unknown-linux-gnu/Sufe_0.1.0_amd64.deb) 和 [AppImage](artifacts/desktop/x86_64-unknown-linux-gnu/Sufe_0.1.0_amd64.AppImage)，均为原生 release 构建。另有 [Windows NSIS](artifacts/Sufe_0.1.0_x64-setup.exe) 与 [Android APK](artifacts/Sufe-0.1.0-android-debug.apk)，两者采用 debug 构建。SHA256、具体测试证据与设备验收边界见 [交付记录](docs/delivery-status.md)。macOS/Linux 的原生构建与安装说明见 [平台构建记录](docs/DESKTOP-NATIVE-BUILDS.md)；默认 TUN 的权限与升级机制见 [桌面 TUN](docs/DESKTOP-TUN.md)。

```text
cargo test -p xboard-core --lib
cargo check -p xboard-desktop
cd desktop
npm run build
```

- [部署与构建](SETUP.md)
- [加密配置与真实接口](docs/client-deployment.md)
- [交付范围和验证记录](docs/delivery-status.md)
- [移动端共享接口](docs/mobile-ffi-contract.md)
- [移动验证码配置与验收](docs/mobile-captcha.md)
- [固定下载哈希与来源](ci/checksums/README.md)
- [Android 开发说明](android/README.md)
- [iOS 开发说明](ios/README.md)
- [原始审计报告](审计报告.md)

仓库结构：`core/` 为共享 Rust 核心；`desktop/` 为桌面应用；`android/` 和 `ios/` 为原生移动壳；`ci/` 为构建与校验脚本；`scripts/` 为跨平台安装与加密配置工具。内核运行时更新模块、Android自动更新、iOS发行流水线不可视为已交付能力。

默认桌面/Android 内核已统一为 mihomo v1.19.30，Windows 采用 compatible 归档以覆盖更广的 CPU。归档按官方 release API SHA256 固定验证；可用 `python scripts/install-kernel.py --target x86_64-pc-windows-msvc --cache-only` 仅准备缓存，`--standard` 可显式选择标准构建。iOS 仍使用其独立的 Libbox 兼容基线。
