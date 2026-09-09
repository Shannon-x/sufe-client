# 本轮交付范围与验证记录

> 本轮桌面默认 TUN 修复：Windows 最终安装包已实际安装并通过 TUN 验证；macOS Apple Silicon / Intel 和 Linux deb/AppImage 均已完成原生构建、包验证与真实 TUN 创建/清理测试。最新结果见 [桌面构建记录](DESKTOP-NATIVE-BUILDS.md) 和 [TUN 说明](DESKTOP-TUN.md)。

日期：2026-09-09。本记录分别列出已实现代码、已取得的验证结果与尚待完成的设备验收。Android 完整 Debug APK 已生成并完成签名与原生库检查；Windows 原生界面诊断烟测已通过。

## 交付代码

- 桌面统一应用外壳、连接仪表盘、节点、套餐/订单、账户、规则、日志、公告和客服页面，显式开发预览独立于真实客户端调用。
- 桌面和移动购买流程先生成订单，核对服务端实际金额后再确认付款，支持免费订单、周期优惠券、支付取消/超时与既有订单续付。iOS 补充支付前渠道/金额复核与支付后绑定金额检查；绑定订单的 null/0 手续费保持为零，条款变化时要求重新确认。支付成功必须由真实订单状态确认。
- 配置支持运营方内嵌品牌/功能开关、多个加密OSS各自多个API；远程对象必须Ed25519签名与AES-GCM验证，Stealth-v1匹配真实中间件源码，无自动明文降级。
- Android/iOS 已接入礼品卡查询、兑换、历史及邀请的原生界面与共享 FFI；奖励按真实类型展示，盲盒说明最终以兑换结果为准。独立充值、签到因本地源码无可验证路由而关闭；线上可能有额外插件，需要插件契约后再对接。
- 桌面、Android/iOS 均已接入 Chatwoot 原生聊天界面与受信任 Public API 桥接，默认未配置时关闭；工单保持可用。运营 inbox、实际消息收发与通知授权仍需验证，应用关闭后的推送不属于已交付能力。
- 自定义规则持久化、校验和配置注入已接入移动 UI。移动规则按账户保存在 SecureStore，Android 连接时自动注入，iOS 在 YAML 转换为 sing-box 配置前注入；保存后重连生效。
- Android/iOS 登录、注册、找回密码及发送邮件接入动态 Turnstile、reCAPTCHA v2/v3 控件，处理 token 过期、错误和消费，限制 WebView 导航与原生桥接来源。真实运营密钥和设备挑战仍需验收，见 [验证码说明](mobile-captcha.md)。
- 桌面和移动补充会话世代保护，拒绝旧账户的异步结果覆盖新会话；iOS 另修启动恢复与登录竞争、退出期间的认证忙碌状态、权益/工单/订单及订阅连接的异步返回检查。连接生命周期和数据面修复详见源码与移动 README。

## 平台边界

| 项目 | Windows | macOS | Linux | Android | iOS |
|---|---|---|---|---|---|
| 同一套完整桌面UI | 是 | 是 | 是 | 否，Compose原生壳 | 否，SwiftUI原生壳 |
| 内核 | mihomo | mihomo | mihomo | mihomo子进程 | sing-box/Libbox |
| 礼品卡/邀请/Chatwoot 界面与接口 | 已接入 | 共用代码 | 共用代码 | Compose UI 与共享 FFI 已接入 | SwiftUI 与共享 FFI 已接入 |
| 自定义规则/订单复核/动态验证码 | 已接入 | 共用代码 | 共用代码 | 原生 UI 与核心调用已接入 | 原生 UI 与核心调用已接入 |
| 中间件/OSS动态配置壳接线 | 桌面已接入 | 共用代码 | 共用代码 | 共享部署构造已接入 | 共享部署构造已接入 |
| TUN与网络恢复 | 默认 TUN；实际 SYSTEM 服务、网卡创建/清理、地址冲突拒绝通过 | 默认 TUN；两种架构的真实 root helper、受限 IPC、TUN 创建/清理及签名验证通过 | 默认 TUN；普通用户通过 capability 实际创建/清理网卡 | 需设备VPN数据流验收 | 需签名设备NE验收 |
| 当前产物与验证 | 最终 debug NSIS 已实际静默安装；权限、内核与安装升级钩子通过 | 两种架构 release DMG / app.zip 已生成并验收，使用 ad-hoc 签名，无 Apple 公证 | 原生 release deb/AppImage 已生成；运行依赖与两包 GUI 启动通过 | 三 ABI Debug APK 已生成，签名/ELF/16 KB 检查通过，设备启动与 VPN 待验收 | Swift 语法检查通过，无 Xcode 编译和 Apple 签名 |

iOS 当前固定 Libbox 1.10.7 源码用于兼容现有接口，内核是 sing-box，不能宣称运行 mihomo，也不能据此宣称使用当前受支持或已无漏洞的内核。桌面与移动的核心账户、购买和新增权益功能均有界面接线；各平台仍存在内核、权限、后台运行与发布状态的差异。

上一轮因特权路径未完成而关闭 Windows/macOS TUN。本轮以 IPC v2 内联配置、服务私有目录、受保护内核和受限控制代理替换旧实现后启用；旧路径型 StartKernel 请求明确拒绝。默认 TUN 授权失败会停止，系统代理由用户手动选择。

## 已取得的独立证据

- 默认公共配置 `https://imitate.cnqq.de/api/v1/guest/comm/config` 只读GET返回HTTP200和成功信封。is_captcha为0；线上额外返回余额充值插件配置字段，本地源码没有相应路由。未尝试登录、注册或产生订单。
- 早期 v1.18.7 下载记录保留于 `ci/checksums/v1.18.7.sha256`；当前桌面/Android 使用官方稳定版 **v1.19.30**，官方 release API digest 固定于 `ci/checksums/v1.19.30.sha256`。
- Windows 包内 mihomo 与固定官方归档解压内容 SHA256 一致，Wintun 0.14.1 与官方公开 hash 一致且安装在内核旁。后续实际服务/TUN 验证没有改变用户的 Clash、系统代理、默认路由或 DNS。
- `scripts/install-kernel.py --target x86_64-pc-windows-msvc` 本机执行通过，验证并安装同hash资产。
- Python加密配置发布工具完成语法验证和独立测试fixture生成，Rust有相应跨语言签名/加密/篡改/过期/项目隔离测试。
- `codex/tun-native-build` 分支已运行原生构建工作流。macOS 两种架构和 Linux 最终包均来自 commit `3673087379afd5f687e7963a2356d13c87265c86`、[run 34327137901](https://github.com/Shannon-x/sufe-client/actions/runs/34327137901)，只保存 Actions Artifacts，没有创建 tag 或发布 Release。
- 五个修改过的Bash脚本通过逐文件 `bash -n`；下载校验函数对真实官方归档通过、对错误内容拒绝。工作区Shell文件改为LF，并新增Git属性防止Windows检出时重新变成CRLF。

## 已确认的构建与测试结果

以下结果由本次主任务及对应平台子任务确认。浏览器测试使用显式预览或受控原生调用替身，不证明实际 VPN、支付渠道或客服服务已打通。

| 检查 | 结果 | 实际覆盖与限制 |
|---|---|---|
| `cargo test -p xboard-core --lib` | **111 通过，1 默认忽略** | 本轮共享核心回归；另显式执行真实 mihomo 配置校验测试通过。实际权限与 TUN 验收另列 |
| 桌面 Playwright | **22/22 通过** | 18 项界面/购买/生命周期，加 4 项生产 CSP 回归；实际 Tauri 原生界面烟测另记 |
| `node --test tests/billing.test.ts`（desktop） | **3/3 通过** | 手续费、优惠计算与订单状态映射 |
| `npm run build`（desktop） | **通过** | Vue/TypeScript 检查和前端生产构建 |
| npm 依赖审计 | **0 项已知漏洞** | 本次锁定依赖的审计结果，不代表整个产品没有漏洞 |
| 生产内容安全策略 | **4/4 通过，包含在上述 22 项** | 三个认证表单显示与空提交校验正常；真实同源脚本的 `Function()` 仍被 CSP 拒绝 |
| Windows 最终 NSIS 打包及安装 | **通过** | debug 构建；实际静默安装退出码 0，旧服务停止、受保护安装路径与首次服务启动通过，无 Authenticode 签名 |
| Windows 默认 TUN 服务 | **通过** | 6 项服务权限测试与 2 项提权前路径检查通过；安装后 SYSTEM mihomo v1.19.30 创建/清理网卡，错误地址冲突被拒绝，IPC 权限及控制接口限制通过 |
| Linux 原生构建、包与 TUN | **通过** | 脚本检查和 25 项 Rust 定向检查通过，包含 5 项私密配置权限回归；deb 运行依赖完整，普通用户真实 TUN 创建/清理通过，两包未登录 GUI 均运行 12 秒 |
| Swift 源码 tree-sitter 解析 | **27/27 通过** | 仅语法解析，不是 Swift 类型检查、链接或 Xcode 编译；证据 `artifacts/ios-swift-source-check.json` |
| iOS 验证码 JavaScript | **3/3 通过** | 从实际 Swift 源码抽取脚本，模拟三种 provider 回调/过期/错误和 v3 reset；证据 `artifacts/ios-captcha-script-check.json`，不是真实厂商挑战 |
| Swift/Kotlin UniFFI 绑定生成 | **通过** | 生成成功并核对新增共享方法名称；平台链接和设备运行另行验证 |
| Windows 原生界面烟测 | **通过（本机诊断构建）** | 真实 WebView2/Tauri、嵌入资源、登录/注册/找回页面、空提交校验、`connection_state`/`fetch_client_config`/`fetch_site_config`；无运行时错误。没有登录账号、提交付款或启动隧道 |
| Android 最终 Kotlin/原生编译与 APK | **通过** | 最终 Kotlin 1 分 22 秒、assembleDebug 2 分 54 秒；三个 ABI 原生库完成 release 编译，构建副本关键源码哈希与最终冻结一致 |
| Android 成品签名/原生库检查 | **通过** | APK v2 Debug 签名和 zipalign 通过；包内 15 个原生库 ELF 类别/机器码通过，64 位全部满足 16 KB 对齐，extractNativeLibs=true |
| Android 安装/启动/VPN 数据面 | **未执行** | 宿主未开放虚拟化扩展，三次软件模拟均未启动 Android，最终 adb 无设备；未执行真机安装、冷启动、VPN 或支付验收 |
| macOS 原生构建、签名、helper 与 TUN | **两种架构均通过** | DMG / app.zip 已下载核对 SHA256；每架构私密配置 8 项、ACL 3 项通过；真实 root helper 安装、owner IPC、受限控制接口、TUN 创建/停止清理和默认路由保持通过。未做 Apple 公证 |
| iOS Xcode/Apple 签名 | **未执行** | Windows 环境没有执行 Xcode 类型编译、NetworkExtension 签名或 iPhone VPN 验收 |

### Windows 当前审阅包

| 字段 | 值 |
|---|---|
| 文件 | [`artifacts/Sufe_0.1.0_x64-setup.exe`](../artifacts/Sufe_0.1.0_x64-setup.exe) |
| 类型 | 本地 debug 构建，NSIS 安装包 |
| 应用签名 | Windows Authenticode `NotSigned`；这是本地测试产物，尚非正式签名发行包 |
| 大小 | **21,783,284 字节** |
| SHA256 | `BF1F0E28E048D4EFC5246DD74A7948863C03EAE5D39CD5F1A4E427EAF80A463B` |
| 打包入口 | `scripts/build-local-windows.ps1`；该流程不创建生产更新签名、不发布、不注册特权服务 |

文件大小与 SHA256 已通过本机文件检查复核。后续若重新打包，必须同步更新本表，不能沿用本次哈希。

原生界面烟测使用前一阶段本机诊断构建，额外启用本地回环调试端口。测试时终端以管理员身份运行，WebView2 会忽略环境变量中的调试参数，因此通过单独的本地构建配置启用诊断；没有修改系统策略。测试通过后进程正常退出，最终 NSIS 以默认配置重新构建，不包含该调试端口。界面诊断证据为 `artifacts/native-smoke.json` 和 `artifacts/sufe-native-windows.png`；最终 NSIS 的安装、权限与 TUN 结果见下段独立证据。

最终 NSIS 解包确认主程序为 Windows GUI（2），不含诊断端口，mihomo 与 Wintun 符合固定官方哈希，svc 与最终构建一致。安装后的正向 TUN、地址冲突拒绝和停止清理均实际执行；证据见 `artifacts/windows-package-verification.json`、`windows-nsis-install.json`、`windows-kernel-probe.json`。各平台校验值汇总于 `artifacts/SHA256SUMS`，机器可读记录见 `artifacts/delivery-manifest.json`。

### Android 当前测试包

| 字段 | 值 |
|---|---|
| 文件 | [`artifacts/Sufe-0.1.0-android-debug.apk`](../artifacts/Sufe-0.1.0-android-debug.apk) |
| 类型 | 完整三 ABI APK，本机 Android Debug 证书签名 |
| 大小 | **89,810,877 字节（85.65 MiB）** |
| SHA256 | `dec308afce17c0b538eece537a07497dfafbb5b8d24acd0e4e4bd1c24cf71c95` |
| 应用/包名 | Sufe / com.xboard.client.debug，版本 0.1.0-debug |
| 系统范围 | 最低 Android 7.0（API 24），目标 API 34 |
| ABI 与核心 | arm64-v8a、armeabi-v7a、x86_64；Rust xboard-core 0.1.0、mihomo v1.19.30、JNA 5.18.0 |
| 成品验证 | apksigner v2、zipalign、全部 15 个原生库 ELF/页对齐检查通过 |
| 详细证据 | [Android 构建验收记录](../artifacts/Android-build-verification.md)及其签名、原生库、包信息和 Manifest 日志 |

安装包完整性与源码编译已经验证，不能代替真机运行结论。当前无可用 Android 设备；软件模拟未启动系统，失败日志保留于 .tools/mobile-smoke/。正式发行使用运营方签名密钥。

## 仍需真实环境

真实机场账号、有效订阅、支付测试渠道和 Chatwoot API Inbox 仍需运营环境。独立 TUN 创建/清理验证不等于真实节点出口、DNS 和网络切换验收；macOS 的系统授权弹窗及原生 GUI 交互、Linux AppImage 的 polkit 弹窗尚未在实际桌面会话验收。Android 仍需设备 VPN/后台切换，iOS 仍需 Xcode、NetworkExtension 签名及设备验收。构建密钥、客服凭据和生产支付权限不得放入公开仓库。

## 后续复核补充

- 移动 UI 已调用共享配置、订单详情、礼品卡、邀请、规则和 Chatwoot 接口。Android 使用周期券检查接口；iOS 下单时携带优惠码和周期，由后端校验并在订单详情中展示最终优惠。详细字段见 [移动接口契约](mobile-ffi-contract.md)。
- 桌面 AppState 本轮默认 Tun；首次内核初始化采用原子 get-or-init，避免并发产生无法管理的第二个实例。连接/重连串行且以取消代数隔离登出中的请求；所有正常退出入口均清理内核与系统代理。异常强杀无法保证 Rust 清理函数执行，不能将正常退出修复解释为 SIGKILL 自动恢复。
- 功能开关在受保护操作前按五分钟刷新；关闭自定义规则会停止下一次连接注入并保留用户保存内容。
- Windows v1.19.30 常规/compatible 官方归档已按 GitHub release API 的 SHA256 digest 验证并缓存。该下载过程未覆盖旧 sidecar；随后主任务隔离烟测通过，默认升级为 v1.19.30 compatible，并由打包脚本安装。macOS/Linux v1.19.30 官方 digest 已固定，后续最终原生包和实际 TUN 验收也已完成。
- Android 默认内核更新为 v1.19.30；CI 增加最终签名 APK 原生库完整性与 64 位 ELF 16 KiB 对齐检查。32 位 ARM ELF 对齐按平台要求另验，不能将 64 位结果套用于所有 ABI。
- 原 update-server 文档错误宣称 Android 已有启动轮询/强制升级，已改为仅分发元数据。
- Windows 原生烟测发现并修复生产登录空白页：Vue I18n 原本运行时调用 `new Function`，与原生 CSP 冲突；改用官方支持的 JIT AST 解释执行，保留消息编译器，未放宽 CSP、未新增依赖。修复后独立生产 CSP 4 项及原有 18 项回归全部通过。
- 本地 Windows 审阅构建也使用 GUI subsystem，避免安装运行时额外弹出控制台；调试输出仍可通过继承/重定向句柄读取。
