# 移动端验证码

Android Compose 和 iOS SwiftUI 的认证页面读取共享 `fetchSiteConfig()`。`isCaptcha` 关闭时不要求 token；开启时根据 `captchaType` 选择 Turnstile、reCAPTCHA v2（`recaptcha`）或 v3（`recaptcha-v3`）。站点配置尚未取得时禁止提交；未知类型、缺少站点密钥或无有效 HTTPS 站点地址时显示可解释错误，不猜测验证码配置。

## 运营配置

后端 `app_url` 必须是受信任的 HTTPS 站点地址，其 hostname 必须已在验证码服务商登记。控件从该地址提取 origin，作为本地固定 HTML 的 base URL；不会加载任意服务端 HTML，也不会通过禁用域名校验解决错误。站点密钥来自对应 `turnstileSiteKey`、`recaptchaSiteKey` 或 `recaptchaV3SiteKey`。中间件强制 Turnstile 时，共享核心合并该要求；中间件和后端不得重复消费同一个 token。

WebView 保留标准 User-Agent、JavaScript 和正常 Cookie/存储能力。只有固定的 HTTPS 验证码服务域名与验证所需的空白子框架获准导航，不开放文件访问、任意主页面跳转或忽略 TLS 错误。Turnstile 的移动 WebView 要求见 [Cloudflare 官方说明](https://developers.cloudflare.com/turnstile/get-started/mobile-implementation/)。reCAPTCHA 使用官方支持的 `www.recaptcha.net` 入口，保留品牌、隐私政策和服务条款入口，见 [Google FAQ](https://developers.google.com/recaptcha/docs/faq)。

## Token 生命周期

登录、注册、重置密码、发送邮件分别申请并消费 token。发送邮件后必须重新完成注册或重置密码所需的验证。v2/Turnstile 使用显式渲染与成功、过期、错误回调；v3 仅在用户提交时执行，固定 action 为 `login`、`register`、`reset_password` 或 `send_email`，不会在页面初次加载时预取即将过期的 token。官方行为见 [Google v2](https://developers.google.com/recaptcha/docs/display)、[Google v3](https://developers.google.com/recaptcha/docs/v3) 和 [Turnstile 控件配置](https://developers.cloudflare.com/turnstile/get-started/client-side-rendering/widget-configurations/)。

Token 只存于当前表单内存，不写入日志或持久化存储；请求结束、错误、过期、重载和离开页面时清理。原生端另设提前过期边界：Turnstile 270 秒，reCAPTCHA 110 秒。v3 的异步旧结果在 reset 后被丢弃。最终有效性由后端验证，前端完成验证不能代替服务端验证。

iOS `CaptchaView.swift` 使用 `WKScriptMessageHandler`，仅接受当前主框架、当前随机 nonce、正确 HTTPS hostname/port 的消息。第三方子框架不能向原生层提交 token。配置只以转义 JSON 进入固定页面，并设置 CSP。使用的 Apple API 见 [WKScriptMessage](https://developer.apple.com/documentation/webkit/wkscriptmessage)、[WKSecurityOrigin](https://developer.apple.com/documentation/webkit/wksecurityorigin) 与 [loadHTMLString](https://developer.apple.com/documentation/webkit/wkwebview/loadhtmlstring(_:baseurl:))。

Android `CaptchaWidget.kt` 不暴露 `addJavascriptInterface`，由应用读取当前主文档中固定结果字段；原生端一次消费后清空，并在页面替换或错误后拒绝旧结果。

共享 FFI 的登录、注册、找回密码会依据受信任配置将旧的 `recaptcha` 参数分派为后端的 `recaptcha_data` 或 `recaptcha_v3_token`；发送邮件同样分派，不会把 v3 token 错放到 v2 字段。该内部修复不改变 UDL，但必须重新编译移动 Rust 库。

## 已验证与待验收

- iOS 四个 CAPTCHA/认证 Swift 文件通过 tree-sitter 语法解析；Swift/Kotlin 绑定重新生成，核对了 AppModel 四种调用的参数标签。
- 从实际 Swift 源码抽取的 JavaScript 通过 Node 语法检查，三个 provider 的回调、过期/错误与 v3 reset 丢弃旧结果通过模拟 provider 测试。证据见 `artifacts/ios-captcha-script-check.json`。
- 上述检查不等同于 Xcode 类型编译、真实 WKWebView 渲染或验证码厂商验收。最终必须用运营方授权域名/站点密钥在 iPhone 与 Android 设备验证交互挑战、错误重试、过期、发送邮件后的第二次挑战及四种认证请求。后台关闭验证码后也应验证表单不再等待 token。

生产站点目前公开配置关闭验证码；本轮不擅自开启后台安全设置或创建测试账号。因此真实厂商挑战仍需运营环境验收。
