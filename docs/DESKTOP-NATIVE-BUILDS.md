# macOS / Linux 构建与验证

`Build Desktop Artifacts` 工作流可手动运行，也可由专用 `codex/tun-native-build` 分支的 push 触发。它只上传 GitHub Actions Artifacts，权限为 `contents: read`，不创建或发布 Release，不需要 updater 私钥。`SUFE_DEPLOYMENT_JSON` 仅在需要定制机场部署配置时使用现有仓库 secret。

| 目标 | 原生 runner | 产物 |
| --- | --- | --- |
| Apple Silicon | macos-14 | DMG、保留权限的 app.zip |
| Intel Mac | macos-15-intel | DMG、保留权限的 app.zip |
| Linux x86_64 | ubuntu-22.04 | deb、AppImage |

选择 Ubuntu 22.04 保持 glibc / WebKitGTK 4.1 基线。macOS 两个架构分别原生编译，内核和 helper 必须与 UI 架构匹配。macOS 使用无需证书的 ad-hoc 签名并校验签名，**未做 Apple 公证**；系统仍可能要求用户确认来源。

默认内核版本来自 `ci/mihomo-version.txt`。2026-09-09 官方 `releases/latest` 仍为稳定 `v1.19.30`，下载需匹配 `ci/checksums/v1.19.30.sha256`。Windows/Linux amd64 默认 compatible，macOS 使用对应官方 arm64/amd64 归档。

## 产物必须通过的检查

- macOS：app 内存在 UI、mihomo 和 helper；三者架构匹配；整个 app 的 ad-hoc 签名有效；内核 `-v` 对应固定版本。
- Linux deb：解包后的 `/usr/bin/mihomo` 与 UI 可执行文件存在；实际安装 deb 后 `getcap` 必须包含 `cap_net_admin`。
- Linux AppImage：真实解包后 UI 与内核齐全；内核版本正确。deb 与 AppImage 各在 Xvfb 中运行 12 秒，无提前退出。这个检查验证未登录启动，不代表真实 VPN 已经过连接验证。
- 每个平台产出 `SHA256SUMS`、源 commit / 架构 / 内核版本验证报告；Linux 另保留启动日志。

Linux deb/RPM 安装脚本针对 Tauri 的真实 sidecar 位置 `/usr/bin/mihomo` 授权，拒绝符号链接与非 root 所有者，再去掉组/其他用户写权限。AppImage 的只读挂载不能保存文件 capability；首次 TUN 会通过 polkit 的系统授权窗口，将随包内核安装到 `/usr/local/lib/sufe/mihomo`。复制后的临时文件必须匹配构建时嵌入的 SHA256，才会获得网络 capability 并原子替换旧版本。目录和文件必须 root 所有、不可由普通用户写入，拒绝符号链接。已安装的相同版本通过复核后无需再次授权；更新内核需重新授权。

AppImage 所在桌面需提供 `pkexec` / polkit 认证代理及 `setcap`（Debian/Ubuntu 的 `libcap2-bin`）。用户取消、授权超时、哈希失败或缺少工具都会明确停止 TUN，不自动切换系统代理。客户端和内核配置仍以用户身份运行，只有受保护内核文件具有 `CAP_NET_ADMIN` / `CAP_NET_BIND_SERVICE`。deb 仍是无需额外准备工具的推荐安装包。CI 未登录启动检查不会弹授权，也不能替代在实际 Linux 桌面上的首次授权和 TUN 网络验收。

## 本地与云端命令

```sh
python -m unittest discover -s scripts/tests -v
bash ci/scripts/install-mihomo-sidecar.sh
# macOS 再运行（TARGET_TRIPLE 可明确指定架构）
bash ci/scripts/install-helper-sidecar.sh release
cd desktop
npm ci
npm run tauri build -- --ci --target aarch64-apple-darwin --bundles app,dmg --config src-tauri/tauri.artifacts.conf.json -- --locked
# Linux 将 target 换成 x86_64-unknown-linux-gnu，bundles 换成 deb,appimage。
```

原 `Release Desktop` 工作流继续用于有 updater 签名的正式发布。验证普通安装包请选 `Build Desktop Artifacts`。尚未取得成功的 Actions 运行记录前，只能报告脚本与静态检查通过，不能报告 macOS/Linux 成品构建通过。

依据：[mihomo 官方稳定版](https://github.com/MetaCubeX/mihomo/releases/latest)、[Tauri GitHub 构建](https://v2.tauri.app/distribute/pipelines/github/)、[Tauri Linux 基线说明](https://v2.tauri.app/distribute/appimage/)、[GitHub runner 列表](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。
