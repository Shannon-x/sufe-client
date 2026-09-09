# 下载校验记录

构建脚本在解压前验证这里登记的 SHA256。新版本没有登记即失败，不能用刚下载文件计算出来的哈希作为该次下载的“预期值”。

- `v1.18.7.sha256`：2026-09-09 从 [MetaCubeX/mihomo 官方 v1.18.7 release](https://github.com/MetaCubeX/mihomo/releases/tag/v1.18.7) 经 HTTPS 获取原始归档后登记。该旧 release 的 GitHub asset metadata 没有 `digest`，也未提供独立 checksum 资产；这是首次官方来源信任后的固定哈希，不是厂商签名或独立来源交叉认证。后续构建都必须匹配本仓库固定值。
- `wintun-0.14.1.sha256`：对照 [Wintun 官方页面](https://www.wintun.net/) 公开的 SHA2-256 值登记；归档内预编译 DLL 带官方签名。

Windows v1.18.7 解压后 `mihomo-windows-amd64.exe` SHA256：`3e759402423ee81abccfed9d967b683fac5616b4d66729f1b5811068520408e9`。

Wintun 0.14.1 AMD64 DLL SHA256：`e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce`。

校验仅确认文件与固定资产一致，不代表该旧版本当前没有漏洞。升级内核需要先确认官方 release、更新固定哈希，再跑配置/连接与对应平台回归。

## v1.19.30

2026-09-09 再次核对官方 `releases/latest`：最新稳定 tag 仍是 `v1.19.30`（发布时间 2026-08-16，非 prerelease）。`ci/mihomo-version.txt` 记录本地安装脚本与独立桌面产物 CI 的默认版本；现有 CI / mobile / release 中的默认值也均为这一版本。

新增 Linux amd64-compatible 的官方 `assets[].digest` 固定值，并实际下载核验该归档及 macOS arm64/Intel 归档。Windows 和 Linux 的新安装默认选 compatible；`--standard` 可以明确选择普通 amd64 归档。macOS 保持官方对应架构归档。镜像生成脚本同样先核对固定值再解压。

Android 和四个桌面目标的 SHA256 来自 MetaCubeX/mihomo 官方 GitHub release API `assets[].digest`（2026-09-09 获取）；Windows 常规/compatible 归档均已下载核验。Windows 常规/compatible 均已通过主任务的隔离控制器与规则烟测，默认采用 compatible 以兼顾旧 CPU。`scripts/install-kernel.py --version v1.19.30 --target x86_64-pc-windows-msvc --cache-only` 只准备缓存；默认选择 compatible，加 `--standard` 选择标准归档，不会覆盖打包 sidecar。
