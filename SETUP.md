# SUFE Client 构建与部署

先完成专属后端配置、生成可审查安装包，再在对应平台验证登录、购买、连接、退出网络恢复。源代码修改或公共接口可达不等于已经完成发行。

## 1. 后端配置

参见 [client-deployment.md](docs/client-deployment.md)。完整 JSON 通过构建环境变量 `SUFE_DEPLOYMENT_JSON` 嵌入；简单部署可设置 `SUFE_BACKEND_URL`。所有备用 API 必须属于同一运营方和同一账户数据库。运行时不开放任意后端输入。

PowerShell 示例（配置文件应放在仓库外的受限运维目录）：

```powershell
$env:SUFE_DEPLOYMENT_JSON = Get-Content -Raw -LiteralPath 'C:\private\sufe-deployment.json'
```

OSS 私钥不能放入客户端；仅嵌入公钥与单独的解密口令。生成密文对象使用 `scripts/pack-client-config.py`。Chatwoot 需要 API Inbox identifier，不能把管理员令牌或 website token 填进该字段。

## 2. Windows

需要 Node.js 24、Rust stable（MSVC目标）、Python 3.10+、Visual Studio C++ Build Tools 和 Windows SDK。使用已配置 C++ 工具链的 Developer PowerShell。不要把安装脚本的退出码或目录存在当作工具链可用证明，先确认 `cargo --version` 与 `cl` 可执行。

```powershell
python scripts/install-kernel.py --target x86_64-pc-windows-msvc
cargo build -p xboard-svc
Copy-Item -LiteralPath 'target/debug/xboard-svc.exe' -Destination 'desktop/src-tauri/binaries/xboard-svc-x86_64-pc-windows-msvc.exe'
Set-Location desktop
npm ci
npm run build
npm run tauri -- dev
```

下载工具从官方 HTTPS 获取归档，先对比 `ci/checksums/` 固定 SHA256，再解压 mihomo/Wintun。它不会运行内核、安装服务或修改系统代理。当前开发版本恢复默认 TUN；Windows/macOS 首次使用需系统授权安装受保护服务，服务仅运行固定内核及经过限制的配置快照。构建成功与实际 TUN 验收结果分别记录在交付报告中。

构建可分发包前，把服务侧车换为 release 版本：从仓库根执行 `cargo build -p xboard-svc --release`，把 `target/release/xboard-svc.exe` 复制到相同侧车目标名，然后在 `desktop` 执行 `npm run tauri -- build`。Tauri updater 产物需要与配置公钥匹配的签名私钥。

若设置了 `CARGO_TARGET_DIR`，复制路径应使用该目录；不要机械复制陈旧的 `target/` 产物。若 Rust/C工具链在中文路径中出现原生依赖问题，可使用指向本项目的英文目录 junction，并把临时编译产物放英文路径；原始源文件仍保持在本工作区。

## 3. macOS 与 Linux

macOS 需要 Xcode Command Line Tools，Linux 需要 WebKitGTK 4.1、GTK3、AppIndicator、OpenSSL、DBus 开发包和 pkg-config。Ubuntu 示例：

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev pkg-config libcap2-bin
```

从根目录安装目标内核：

```sh
# Apple Silicon；Intel 使用 x86_64-apple-darwin
export TARGET_TRIPLE=aarch64-apple-darwin
python3 scripts/install-kernel.py --target "$TARGET_TRIPLE"
bash ci/scripts/install-helper-sidecar.sh release
python3 ci/scripts/presign-macos-sidecars.py "$TARGET_TRIPLE"

# Linux x86_64
python3 scripts/install-kernel.py --target x86_64-unknown-linux-gnu
```

然后在 `desktop` 中执行 `npm ci`、`npm run build`、`npm run tauri -- build`，明确目标时追加 `--target "$TARGET_TRIPLE"`。macOS helper 的架构必须与应用目标一致。预签步骤按当前 Tauri 的 ad-hoc 参数连续签名两次，只有 SHA256 完全一致才生成侧车校验清单；桌面构建把校验值编入程序。安装前及 root 复制时均验证完整文件 SHA256，打包验证还必须确认成包侧车与预签值相同。修改内核、helper 或签名设置后需重新预签和重建桌面。当前流程拒绝正式证书/entitlements 覆盖，正式签名需先配套验证预签流程。macOS 尚未做 Apple 公证；各平台实际运行结果以交付报告为准。

## 4. Android 与 iOS

Android 的确切 SDK、NDK、Gradle 和 UniFFI 流程见 [android/README.md](android/README.md)。先构建共享 Rust 原生库、生成绑定、安装已验证内核，再运行 `assembleDebug`；只有 Kotlin 编译通过仍不能证明 JNI打包与 VPN数据流可用。Release APK必须签名，缺少密钥时CI拒绝发布。

iOS见 [ios/README.md](ios/README.md)。当前使用 sing-box/Libbox兼容基线，不是mihomo。`ios/build-libbox.sh` 从固定源码commit构建框架，旧的不存在release下载链接已移除。需要macOS/Xcode、NetworkExtension签名配置、设备验收；不能通过“恢复旧CI片段”直接得到可发行客户端。

移动原生壳已接入共享动态配置、礼品卡、邀请、自定义规则、Chatwoot、订单复核及验证码；移动平台的编译、签名与真机流量验收边界见各自 README，不应将源码接线等同于全部平台完成运行验收。

## 5. 测试

```sh
cargo test -p xboard-core --lib
cargo check -p xboard-desktop
cd desktop
npm run build
```

前端端到端测试配置与本次结果以 [交付记录](docs/delivery-status.md) 和任务最终验证记录为准。生产验收需要真实测试账号、有效订阅、支付渠道测试环境和Chatwoot inbox；不要用预览数据代替这些验收。

## 6. GitHub发行流程

在确认仓库remote和发行目标后，由维护者创建版本tag触发草稿构建。当前workflow不在本地任务中推送或发布任何内容。

需要的配置：

- `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，公钥填写 `desktop/src-tauri/tauri.conf.json` 的 updater配置。
- `ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD`；Android脚本使用SDK原生 `zipalign` / `apksigner`。
- 可选的 `SUFE_DEPLOYMENT_JSON` secret，用于桌面专属部署配置。
- GitHub Actions写入release/gh-pages的权限；Pages选择gh-pages分支。

tag构建只生成草稿Release。检查安装包、签名和平台验证后，由维护者发布草稿；`release.published` 事件才生成公开更新清单，避免客户端收到指向私有草稿资产的下载地址。桌面与Android清单发布共用并发组，避免同时覆盖gh-pages。桌面清单必须包含四个目标和签名，并额外附加为release的 `latest.json`，使备用updater URL可用。

Android清单目前只作为分发元数据；客户端内自动升级逻辑没有交付。Tauri updater签名验证不等于Windows Authenticode或macOS公证，也不保证未来安装不会出现系统确认。iOS发行workflow仍未配置。

换内核版本之前，先独立确认官方资产并更新 `ci/checksums/<version>.sha256`。没有固定hash的版本会失败，不允许去掉校验继续打包。

内核下载默认 v1.19.30，Windows 默认 compatible。`--version v1.18.7` 保留旧版本复现能力，`--standard` 选择新版本标准 Windows 构建；两者只允许仓库已有固定 SHA256 的归档。`--cache-only` 不修改当前打包 sidecar。Bash CI 对应 `MIHOMO_WINDOWS_STANDARD=true` 可选标准 Windows 归档，默认 compatible。CI 中已有 `MIHOMO_VERSION` 仓库变量会覆盖工作流默认版本，运营方需同步检查该值。
