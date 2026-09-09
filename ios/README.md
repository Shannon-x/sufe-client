# Sufe iOS 客户端

SwiftUI 界面、共享 Rust 业务层、NetworkExtension 隧道。界面使用与桌面/Android 相同的浅色、紫色和圆角卡片。最低 iOS 17（使用 Observation 与 ContentUnavailableView）。

## 当前边界

- iOS 使用 **嵌入式 sing-box/Libbox**，不会尝试在 iOS 沙盒内启动 mihomo 可执行文件。
- 已接入真实 NEVPNStatus：只有扩展报告 connected 才显示连接成功；系统断开同步到界面；启动超时、无支持节点、无效配置会报错。
- 隧道通过 LibboxGetTunnelFileDescriptor 借用 NetworkExtension 的 utun fd，Libbox 复制后持有自己的 fd；没有丢包的占位 read-loop。
- NWPathMonitor 将 Wi-Fi/蜂窝变化传给 Libbox，缓存与规则集在 App Group 的 kernel 目录保存。
- 订阅直接使用后端返回的 HTTPS subscribe_url，保留加密路径和订阅域名。
- 节点列表、切换和测速通过 NE provider message 到本地内核 API；仅允许固定回环地址，使用随机密钥，测速并发限制为 4。切换成功才更新所选节点。
- 购买先创建订单并核对服务端实际金额；确认前复查支付渠道和手续费，返回支付结果后再核对绑定金额，条款变化需重新确认。取消订单不会显示已支付；支付页最多自动查询 10 分钟。
- 登录、注册、找回密码、发送邮件验证码接入动态 Turnstile/reCAPTCHA v2/v3 控件，受限 WKWebView 只接收主框架可信来源消息。配置要求、Token 生命周期与设备验收边界见 [移动验证码说明](../docs/mobile-captcha.md)。

**本仓库的 Swift ABI 和 JSON 转换器目前固定兼容 sing-box 1.10.7。它是待验证的兼容开发基线，不能视为已经通过发布审核的当前内核。** 升级到受支持的新版 sing-box，需要一起迁移 PlatformInterface、DNS/route schema 与出站结构，并在 macOS/Xcode 和 iPhone 上验收。本次 Windows 环境不能执行 Xcode、签名或真机 VPN 验证。

## 在 macOS 构建

需要 Xcode 15.3+、Go、Rust iOS targets、XcodeGen。真机 NetworkExtension/App Group 签名需要具备相应能力的 Apple Developer 团队。

```sh
brew install go xcodegen
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
export DEVELOPMENT_TEAM=你的团队ID
just ios-bootstrap
open ios/XboardClient.xcodeproj
```

bootstrap 的 Libbox 步骤调用 `ios/build-libbox.sh`，从 SagerNet/sing-box 的 v1.10.7 构建 framework，并校验完整提交 `253b41936ecd6ae17948d49d9c510d7100830927`。不再依赖不存在的 sing-box-for-apple releases/Libbox.xcframework.zip 文件。Go module 与 gomobile 版本固定在该源码依赖中。

生成物：`Vendor/Libbox.xcframework`、`Vendor/XboardCore.xcframework`、`Shared/Generated/*`、`XboardClient.xcodeproj`，均由构建生成。更改 Rust/UDL 后执行 `just core-ios` 和 `just ios-bindings`。

## 发布前必须完成的验收

1. 对固定 Libbox framework 执行 Xcode 编译与链接，确认 Swift 导入签名；把 framework 版本/哈希写入发行清单。
2. 真机授予 VPN 权限后访问 HTTPS、UDP/DNS、IPv6 网站，观察实际流量，确认无假连接。
3. 切换 Wi-Fi/蜂窝、锁屏唤醒、撤销 VPN 权限、扩展异常退出，确认界面状态与系统一致。
4. 验证登录、验证码、订阅过期、支付中回到 App、取消订单与成功订单；确认后端动态配置覆盖移动端计划开放的功能。
5. 对新内核迁移完成后执行协议矩阵与内存压测，再进行签名分发。

协议转换覆盖 VLESS、VMess、Shadowsocks、Trojan、Hysteria2、TUIC。mihomo 专属协议不会被伪装成可用出口；没有兼容节点时拒绝启动。

## 官方实现参考

- [sing-box Apple NetworkExtension 特性](https://sing-box.sagernet.org/clients/apple/features/)
- [固定版本 PlatformInterface](https://github.com/SagerNet/sing-box/blob/v1.10.7/experimental/libbox/platform.go)
- [固定版本 fd 复制/生命周期](https://github.com/SagerNet/sing-box/blob/v1.10.7/experimental/libbox/service.go)
- [固定版本 framework 构建入口](https://github.com/SagerNet/sing-box/blob/v1.10.7/cmd/internal/build_libbox/main.go)
