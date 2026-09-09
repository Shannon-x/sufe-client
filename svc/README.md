# Sufe Windows TUN 服务

`xboard-svc` 由 Windows SCM 以 LocalSystem 启动，使用固定的、面向所有用户安装的 `Program Files\Sufe\mihomo.exe`。开发目录中的可执行文件不能注册为特权服务。安装器在提权前取得交互用户 SID，将其写入 SCM 启动参数；服务仅接受该 SID 的本地请求。

## 特权边界

- `StartKernelV2` 只传递大小受限的内联 YAML。旧版可执行文件、工作目录、配置文件和日志路径参数一律拒绝。
- 共享配置校验器重建允许的节点、DNS、TUN 和规则配置。禁止脚本、外部 UI、可执行路径、证书文件路径、外部控制管道和本地 provider；允许的 HTTPS 规则集写入服务决定的私有缓存路径。
- 安装文件的所有路径组件必须不是 reparse point，所有者必须是 SYSTEM、Administrators 或 TrustedInstaller，普通用户不能写入。服务持有不共享删除的路径句柄，防止校验后被替换。
- 桌面进程在调用 UAC 前也校验并锁定服务可执行文件及其父目录。开发路径和用户可写副本直接拒绝，不会先运行候选文件再让它自检。
- 配置和日志保存在 `ProgramData\Sufe\Service\<会话 UUID>`；内核缓存使用同一受保护目录中的 `kernel` 子目录。普通用户没有读取或写入权限，断开后删除会话配置。
- UI 密钥只认证受限 HTTP 网关。真实 mihomo 控制端口和独立密钥由服务生成；网关开放状态、流量、节点选择和断开连接等操作，拒绝配置写入、升级、重启等特权接口。发送私有密钥前，服务用 Windows TCP owner 表确认端口属于刚启动的内核 PID。
- 命名管道禁止远程连接。交互用户仅有数据读写权限，没有创建管道实例的权限；服务从实际管道 token 验证调用者。客户端先比对管道服务器 PID 与 SCM 服务 PID，再发送配置。
- 子进程使用固定 DLL 目录、清空继承环境、私有临时目录以及 kill-on-close Job Object。内核退出、TUN 网卡丢失或服务停止时会结束控制网关。
- 只有真正的 `Sufe` 网卡已变为 Up 后才返回 `Started`。mihomo 单独启动 REST 控制端口不能视为 TUN 连接成功。

## 管理和验证

安装包负责升级前停止本服务并等待文件句柄释放，不操作其他代理软件。正常用户通过应用触发一次 UAC 安装，无需手动执行命令。

管理员可对已安装文件执行：

```powershell
& 'C:\Program Files\Sufe\xboard-svc.exe' check-install
& 'C:\Program Files\Sufe\xboard-svc.exe' probe-ipc
& 'C:\Program Files\Sufe\xboard-svc.exe' probe-tun
```

`probe-ipc` 使用移除了 Administrators 权限的 token 验证合法用户通信、匿名拒绝、额外管道实例拒绝、旧协议拒绝和危险 YAML 拒绝。它不启动内核。

`probe-tun` 只创建名称包含随机 UUID 的临时 Wintun 适配器，打开 ring session 后关闭并移除。它不配置 IP、DNS、路由或系统代理。

完整的服务启动测试应使用 `auto-route: false`、`dns.enable: false` 和未被占用的 `dns.fake-ip-range`，验证 TUN、控制网关、停止清理以及前后网络设置。mihomo v1.19.30 的 IPv4 TUN 地址由 `dns.fake-ip-range` 推导，`tun.inet4-address` 已不生效。默认地址冲突时应返回失败，不能停止用户原有的 VPN。

Windows 本地检查：

```powershell
cargo test -p xboard-svc
cargo build -p xboard-svc
```

服务安全测试包含实际 Windows ACL、非可信所有者、reparse point、管道权限掩码以及 TCP listener 的 PID 归属。ACL 测试需要管理员权限；无管理员权限的 CI 不代表已验证本机安装权限。
