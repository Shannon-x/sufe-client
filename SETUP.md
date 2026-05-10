# Sufe Client — 首次部署指南

零付费方案：用 GitHub 自带 CDN 做客户端自动更新。
仓库地址：`git@github.com:Shannon-x/sufe-client.git`。

发布产物挂在 GitHub Releases，更新 manifest 推到 GitHub Pages。
客户端的 updater endpoint 已配成
`https://shannon-x.github.io/sufe-client/desktop/latest.json`。

---

## 1. 本地一次性生成签名密钥

只跑一次，跑完把生成的 secret 贴进 GitHub。

```sh
cd xboard-client
bash ci/scripts/generate-release-keys.sh all
```

脚本会：

- 生成 Tauri ed25519 密钥对 → 提示你把私钥粘到
  `TAURI_SIGNING_PRIVATE_KEY`、密码粘到
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，**公钥**填进
  [`desktop/src-tauri/tauri.conf.json`](desktop/src-tauri/tauri.conf.json)
  → `plugins.updater.pubkey`
- 生成 Android Release Keystore → 提示你把四个值贴进
  `ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、
  `ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD`

**这两份密钥务必离线备份**：丢了 Tauri 私钥老用户收不到桌面更新；丢了
Android keystore 老用户必须卸载重装 APK。

## 2. 提交本仓库到 GitHub

如果 [`xboard-client/`](.) 还没有 git 历史，第一次推送：

```sh
cd xboard-client
git init -b main
git add .
git commit -m "init: sufe-client first push"
git remote add origin git@github.com:Shannon-x/sufe-client.git
git push -u origin main
```

如果项目已经在 git 里只是换 remote：

```sh
git remote set-url origin git@github.com:Shannon-x/sufe-client.git
git push -u origin main
```

## 3. GitHub 仓库一次性配置

仓库 → Settings 里完成三件事：

1. **Secrets and variables → Actions → New repository secret** — 把上面 6
   个 secret 全部贴进去。

2. **Actions → General → Workflow permissions** — 选 “Read and write
   permissions”，否则 `publish-desktop-manifest` 没权限 push 到 gh-pages。

3. **Pages → Build and deployment** — Source 选 “Deploy from a branch”，
   分支选 `gh-pages`、目录 `/ (root)`。第一次没 gh-pages 分支也没关系，
   release.yml 跑完会自动创建。

## 4. 触发首次发布

```sh
git tag v0.1.0
git push origin v0.1.0
```

一推 tag 同时触发两个 workflow：

- [`Release Desktop`](.github/workflows/release.yml) — 矩阵跑
  `aarch64-apple-darwin` / `x86_64-apple-darwin` /
  `x86_64-pc-windows-msvc` / `x86_64-unknown-linux-gnu` 四个 target，
  产物（`.dmg` / `.msi` / `.exe` / `.deb` / `.rpm` / `.AppImage`）+ 每个
  bundle 的 `.sig` 上传到草稿 Release。末步 `publish-desktop-manifest`
  把统一的 `latest.json` 提交到 gh-pages。
- [`Release Mobile`](.github/workflows/mobile.yml) — 跨编 4 个 ABI 的
  `libxboard_core.so` + 拉 mihomo 三架构二进制 + `assembleRelease` +
  签名 → APK 上传到同一个 Release。

第一次跑大概 25-40 分钟。GitHub Actions tab 里能看实时日志。

## 5. 检查清单

跑完 workflow 后逐项确认：

| 检查项 | URL / 命令 |
|---|---|
| 草稿 Release 包含 5+ 文件（dmg/msi/exe/deb/AppImage/apk） | https://github.com/Shannon-x/sufe-client/releases |
| gh-pages 分支已经创建并含 `desktop/latest.json` | https://github.com/Shannon-x/sufe-client/tree/gh-pages |
| Pages 站点能访问着陆页 | https://shannon-x.github.io/sufe-client/ |
| Tauri updater manifest 能拉到 | `curl https://shannon-x.github.io/sufe-client/desktop/latest.json \| jq .` |
| Android updater manifest 能拉到 | `curl https://shannon-x.github.io/sufe-client/mobile/android/latest.json \| jq .` |

通过后回到 Release 页点 **Publish release**（草稿状态客户端的
`releases/latest/download/...` 备份链接是空的，发布后才生效）。

## 6. 用户首次安装的提示

零付费方案的代价 — 系统会把没签名的安装包当成不可信来源：

- **macOS** — 双击 `.dmg` 打开后，右键 `.app` → “打开” → 弹窗里再点
  “打开”。只有第一次，之后就是普通应用。
- **Windows** — `.msi` 双击会弹 SmartScreen“未知发布者”，点
  “更多信息” → “仍要运行”。`.exe` 类似。
- **Linux** — `deb` / `rpm` 没强制签名要求，正常 `apt install ./xxx.deb`
  即可；AppImage `chmod +x && ./xxx.AppImage`。
- **Android** — 启用“未知来源”一次后正常装。

**重要** — 上述只发生在**首次**安装。一旦装上，自动更新走 Tauri 的
ed25519 校验和 Android in-app updater 的 sha256 校验，路径全自动、不再
弹窗。

## 7. 下次发版

```sh
git tag v0.1.1
git push origin v0.1.1
```

无需再做任何配置 — 同一组 secret 永久复用。

---

## 出问题排查

| 现象 | 原因 / 修法 |
|---|---|
| `publish-desktop-manifest` 失败：`Permission denied (publickey)` | Settings → Actions → Workflow permissions 没改 “Read and write” |
| Pages 404 | gh-pages 分支不存在；先跑一次 release.yml 让它创建 |
| 客户端检查更新提示 “update signature is invalid” | tauri.conf.json 的 pubkey 和 secret 里的私钥不是一对；重生成 |
| Android 安装提示 “应用未签名” | `ANDROID_KEYSTORE_BASE64` 没配，APK 是 unsigned 状态；补 secret 重新发版 |
| Release.yml 卡在 mihomo 下载 | mihomo GitHub 限速；切到自建镜像或重试 |
| Tag 推上去但 workflow 没跑 | tag 不是 `v*` 前缀（如 `release-1.0`），workflow 不会触发 |

需要还原 iOS 端构建（等 99 美元开发者账号到位后），从 git 历史 revert
对 [`.github/workflows/ci.yml`](.github/workflows/ci.yml) 和
[`.github/workflows/mobile.yml`](.github/workflows/mobile.yml) 的 iOS 段
删除即可。
