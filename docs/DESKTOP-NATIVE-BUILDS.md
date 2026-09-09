# macOS / Linux 构建与验证

`Build Desktop Artifacts` 工作流可手动运行，也可由专用 `codex/tun-native-build` 分支的 push 触发。它只上传 GitHub Actions Artifacts，权限为 `contents: read`，不创建或发布 Release，不需要 updater 私钥。`SUFE_DEPLOYMENT_JSON` 仅在需要定制机场部署配置时使用现有仓库 secret。

| 目标 | 原生 runner | 产物 |
| --- | --- | --- |
| Apple Silicon | macos-14 | DMG、保留权限的 app.zip |
| Intel Mac | macos-15-intel | DMG、保留权限的 app.zip |
| Linux x86_64 | ubuntu-22.04 | deb、AppImage |

选择 Ubuntu 22.04 保持 glibc / WebKitGTK 4.1 基线。macOS 两个架构分别原生编译，内核和 helper 必须与 UI 架构匹配。macOS 使用无需证书的 ad-hoc 签名并校验签名，**未做 Apple 公证**；系统仍可能要求用户确认来源。

默认内核版本来自 `ci/mihomo-version.txt`。2026-09-09 官方 `releases/latest` 仍为稳定 `v1.19.30`，下载需匹配 `ci/checksums/v1.19.30.sha256`。Windows/Linux amd64 默认 compatible，macOS 使用对应官方 arm64/amd64 归档。

## 本次产物记录

三个目标均在 [run 34327137901](https://github.com/Shannon-x/sufe-client/actions/runs/34327137901) 完成原生构建与验收，最终源码为 `3673087379afd5f687e7963a2356d13c87265c86`。全部安装包已下载到本地并核对 GitHub artifact digest 和包内 `SHA256SUMS`。

两种 Mac 均通过实际 root helper 安装、受限 IPC/控制接口、TUN 网卡创建与停止清理、完整侧车签名与哈希检查；每种架构的私密配置 8 项、ACL 解析 3 项测试通过。Linux 通过实际普通用户 capability TUN 和两种包的 GUI 启动检查；私密配置 5 项、capability 2 项、配置注入 15 项、launcher 3 项测试通过。各平台脚本检查也通过，详细计数保存在各自产物来源记录中。

| 平台 | 安装文件 | 字节数 | SHA256 |
| --- | --- | ---: | --- |
| Linux x64 | [Sufe_0.1.0_amd64.deb](../artifacts/desktop/x86_64-unknown-linux-gnu/Sufe_0.1.0_amd64.deb) | 24,864,492 | `66582fbf213b949a93c76bda41cdd4c606b3094e63361d3e7337f145c323fac6` |
| Linux x64 | [Sufe_0.1.0_amd64.AppImage](../artifacts/desktop/x86_64-unknown-linux-gnu/Sufe_0.1.0_amd64.AppImage) | 100,276,728 | `f660016666794740897eecaa84199367e7fc4338ceea23c7c3269cde24961b3c` |
| Mac Apple Silicon | [Sufe_0.1.0_aarch64.dmg](../artifacts/desktop/aarch64-apple-darwin/Sufe_0.1.0_aarch64.dmg) | 23,455,979 | `c0ed66c108378c6908b8a3c3ac04073332fb503b379eb73efdc0cdd0ae0a19d2` |
| Mac Apple Silicon | [Sufe-aarch64-apple-darwin.app.zip](../artifacts/desktop/aarch64-apple-darwin/Sufe-aarch64-apple-darwin.app.zip) | 23,436,484 | `4e6a307757c26c8bd93f07dc40eb28a9e3b81871ea6f4ca6fa0cf11122f24bd0` |
| Mac Intel | [Sufe_0.1.0_x64.dmg](../artifacts/desktop/x86_64-apple-darwin/Sufe_0.1.0_x64.dmg) | 25,404,641 | `7fc4d0bbd1e02288cc164e2ba61f37100f54f068c1456ab1d7e0e390db544cb5` |
| Mac Intel | [Sufe-x86_64-apple-darwin.app.zip](../artifacts/desktop/x86_64-apple-darwin/Sufe-x86_64-apple-darwin.app.zip) | 25,437,182 | `8c70db2d695b540fd936d134da4616dee286731f6553f7847916e3ae799898a8` |

每个目标目录的 `delivery-provenance.json`、`verification.txt`、`private-config-tests.txt` 和 TUN 烟测报告记录来源与实测结果。Mac 推荐打开对应架构 DMG，将 Sufe 拖入 Applications；`.app.zip` 是保留权限的同一应用备份。首次连接由系统请求管理员授权安装专用 helper。当前使用 ad-hoc 签名，未做 Apple 公证。

Linux 推荐通过 `sudo apt install ./Sufe_0.1.0_amd64.deb` 安装依赖。AppImage 下载后先执行 `chmod +x Sufe_0.1.0_amd64.AppImage`；首次 TUN 授权要求见下文。

## 产物必须通过的检查

- macOS：app 内存在 UI、mihomo 和 helper；三者架构匹配；整个 app 的 ad-hoc 签名有效；内核 `-v` 对应固定版本。两侧车预签必须连续两次产生相同 SHA256，桌面构建嵌入这些校验值，成包中的完整文件必须仍与它们相同。CI 在临时 runner 安装真实 helper，验证 owner IPC、旧路径协议拒绝、受限 controller、独立 TUN 创建及停止；关闭自动路由与 DNS，不使用真实订阅，并校验默认路由不变，最终卸载测试组件。
- Linux deb：解包后的 `/usr/bin/mihomo` 与 UI 可执行文件存在；实际安装 deb 后 `getcap` 必须包含 `cap_net_admin`。
- Linux AppImage：真实解包后 UI 与内核齐全；内核版本正确。deb 与 AppImage 各在 Xvfb 中运行 12 秒，无提前退出。这个检查验证未登录启动，不代表真实 VPN 已经过连接验证。
- 每个平台产出 `SHA256SUMS`、源 commit / 架构 / 内核版本验证报告；Linux 另保留启动日志。

Linux deb/RPM 安装脚本针对 Tauri 的真实 sidecar 位置 `/usr/bin/mihomo` 授权，拒绝符号链接与非 root 所有者，再去掉组/其他用户写权限。AppImage 的只读挂载不能保存文件 capability；首次 TUN 会通过 polkit 的系统授权窗口，将随包内核安装到 `/usr/local/lib/sufe/mihomo`。复制后的临时文件必须匹配构建时嵌入的 SHA256，才会获得网络 capability 并原子替换旧版本。目录和文件必须 root 所有、不可由普通用户写入，拒绝符号链接。已安装的相同版本通过复核后无需再次授权；更新内核需重新授权。

AppImage 所在桌面需提供 `pkexec` / polkit 认证代理及 `setcap`（Debian/Ubuntu 的 `libcap2-bin`）。用户取消、授权超时、哈希失败或缺少工具都会明确停止 TUN，不自动切换系统代理。客户端和内核配置仍以用户身份运行，只有受保护内核文件具有 `CAP_NET_ADMIN` / `CAP_NET_BIND_SERVICE`。deb 仍是无需额外准备工具的推荐安装包。CI 未登录启动检查不会弹授权，也不能替代在实际 Linux 桌面上的首次授权和 TUN 网络验收。

## 本地与云端命令

```sh
python3 -m unittest discover -s scripts/tests -v
# Apple Silicon；Intel 使用 x86_64-apple-darwin。
export TARGET_TRIPLE=aarch64-apple-darwin
bash ci/scripts/install-mihomo-sidecar.sh
bash ci/scripts/install-helper-sidecar.sh release
python3 ci/scripts/presign-macos-sidecars.py "$TARGET_TRIPLE"
cd desktop
npm ci
npm run tauri build -- --ci --target "$TARGET_TRIPLE" --bundles app,dmg --config src-tauri/tauri.artifacts.conf.json -- --locked
# Linux 将 target 换成 x86_64-unknown-linux-gnu，bundles 换成 deb,appimage；跳过helper和预签两个macOS专属步骤。
```

预签参数与锁定的 Tauri CLI 2.11.1 一致：`codesign --force -s - --options runtime`，以最终文件名 `mihomo` 和 `xboard-helper` 签名；不读取或创建生产签名私钥。`APPLE_SIGNING_IDENTITY` 正式身份、证书覆盖或 entitlements 与当前流程不兼容时明确停止。`presigned-sidecars.json`、`codesign-stability.json` 保存本次实际哈希，任何重复签名不稳定或成包改写都会阻止交付。安装器使用编入 GUI 的预期校验值，不接受“现场计算可写侧车的哈希作为信任依据”。修改 CLI/签名参数需重新审查并通过这组真实 macOS 检查。[Tauri 2.11.1 签名实现](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.1/crates/tauri-macos-sign/src/keychain.rs)

原 `Release Desktop` 工作流继续用于有 updater 签名的正式发布。验证普通安装包请选 `Build Desktop Artifacts`。尚未取得成功的 Actions 运行记录前，只能报告脚本与静态检查通过，不能报告 macOS/Linux 成品构建通过。

依据：[mihomo 官方稳定版](https://github.com/MetaCubeX/mihomo/releases/latest)、[Tauri GitHub 构建](https://v2.tauri.app/distribute/pipelines/github/)、[Tauri Linux 基线说明](https://v2.tauri.app/distribute/appimage/)、[GitHub runner 列表](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。
