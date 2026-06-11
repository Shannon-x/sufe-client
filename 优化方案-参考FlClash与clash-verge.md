# Xboard-Client 优化落地方案（首席架构师版）

> 定位锚点：Xboard 是「登录面板 → 购买套餐 → 一键连接」的**单订阅机场客户端**。本方案只借鉴 clash-verge-rev / FlClash 的**连接可用性、可观测性、跨平台底座**，主动剔除通用 Clash 的多订阅/配置编辑/脚本/WebDAV 等过度工程。所有路径基于真实仓库结构。

---

## 一、总体差距判断

xboard-client 的**核心层（core/ crate）工程质量已接近成熟客户端**：重试退避（`manager.rs` retry_with_backoff）、心跳监督（5s `/version` + 阈值拆除）、崩溃清代理、TUN-first 透明降级、随机控制器 secret、订阅 ETag 缓存与空节点硬拦截、单测齐全。差距**不在业务正确性，而在三个面**：

1. **可观测性 UI 几乎为零（最关键）**
   core 已经具备能力但**没接出来**：`MihomoDriver` 有 `log_stream()`、`KernelManager::live_logs()` 在跑（`manager.rs:377`），但 FFI 和桌面命令层**都没暴露日志流**；`/connections` 只取了累计数字（`mihomo.rs:352`）没做活动连接列表；`/traffic` 只读首帧给个数字没有时间序列折线图；规则完全不暴露。结果是用户「连不上 / 连上不走代理」时**没有任何排障手段**，只能 tail 静态 `mihomo.log`。verge/FlClash 把实时日志、连接表、流量图、规则视图当成核心排障面，这是我们与它们成熟度差距最大的地方。

2. **桌面便利性 + 运行期可靠性缺失**
   缺 sysproxy guard（设代理后无 30s 巡检重断言，被别的程序改掉就「连上不走代理」——机场客诉第一名）；缺全局热键、开机自启、启动即连；缺独立设置页（模式/语言/主题/DNS/bypass/测速URL/端口散落各处，无 home_cards 式集中）；缺自动重连（心跳能发现内核死并拆除，但**不会重连**）；托盘极简（只有「显示/退出」，无状态变色/速率/模式快切/节点快切）；控制通道仍是 TCP external-controller（`DEFAULT_CONTROLLER_ADDR`），有端口冲突风险。

3. **跨平台成熟度参差**
   Windows TUN 特权进程 `xboard-svc` 实体/安装链路未完全打通，实际多走系统代理降级；Android 分应用代理缺失（仅 `addDisallowedApplication(self)`）；移动端网络切换（WiFi↔蜂窝）/唤醒后无自动恢复；iOS 无 on-demand/always-on。但**核接口抽象（KernelDriver trait + FFI）已经是 FlClash 那套「一套接口多传输形态」的范式**，底座对的，缺的是数据面打磨。

**一句话**：core 是好引擎，缺的是**仪表盘、守卫、便利开关**和**移动数据面的最后一公里**。优先把已有 core 能力「接出来」，投入产出比最高。

---

## 二、不适合 Xboard 的参考功能（明确剔除，避免过度工程）

| 参考功能 | 出处 | 剔除理由 |
|---|---|---|
| 多订阅 Profiles 管理/拖拽排序/并发批量更新 | verge / FlClash | 面板单订阅，token 由登录态下发，无意义 |
| Merge/Script(Monaco)/运行时配置编辑 | verge | 配置由 `patch_mihomo` 强制注入，用户编辑会破坏面板契约 |
| WebDAV 备份/恢复 | verge / FlClash | 状态在面板侧（登录即同步），价值低 |
| 链式代理 Proxy Chain | verge | 实验性，与「一键连接」主线冲突 |
| Web UI 管理面板（yacd/metacubexd） | verge | 把内部控制器暴露给第三方面板，徒增攻击面 |
| 切换内核(Mihomo/Alpha)/升级内核 UI | verge | 内核由我们签名分发（`updater/kernel.rs` 已做 ed25519），不给用户切 |
| 解锁测试(Netflix/流媒体) | verge | 非机场连接主线，后置到可选 P3 |
| 可拖拽自定义仪表盘网格(SuperGrid) | FlClash | 过度，固定卡片布局即可 |
| 二维码扫码导入订阅 | FlClash | 我们是账号登录，不是订阅链接导入 |
| GeoIP/GeoSite 运行时在线更新 UI | FlClash | 规则集由面板订阅决定，不开放 |

---

## 三、工作流批次（桌面本机可编译验证优先）

> 验证口径：**【本机可编译验证】**= 桌面 `cargo build` + `npm run build` / `cargo tauri dev` 即可在 macOS 本机看到效果；**【需真机验证】**= 必须 Android 真机 / Xcode 模拟器或真机 / Windows 物理机。

---

### 批次 0（P1，地基）：控制通道 IPC 化 + 日志流接出 + 自动重连

这是所有可观测性的前置，且修掉端口冲突隐患。

**0.1 控制通道从 TCP 改 Unix socket / 命名管道**
- 借鉴：verge `config/clash.rs::guard_external_controller_ipc` + `tauri-plugin-mihomo` 的 LocalSocket 连接池；FlClash `ipc.rs` 的代际隔离/强制 fd shutdown/socket 文件清理。
- 改：`core/src/kernel/manager.rs`（`controller_addr` / `DEFAULT_CONTROLLER_ADDR` 改为按平台生成 IPC path，启动参数加 `-ext-ctl-unix`/`-ext-ctl-pipe`）、`core/src/kernel/mihomo.rs`（`controller_url`/`http` client 改用 `hyper-util` + `hyperlocal`(unix) / 命名管道连接器，替换 reqwest 的 TCP base）、`core/src/profile/inject.rs`（`patch_mihomo` 不再强制写 `external-controller: 127.0.0.1:port`）。
- 工程要点：保留每会话随机 secret；macOS/Linux 用 `$work_dir/mihomo.sock`，Windows 用 `\\.\pipe\xboard-mihomo-<rand>`；启停内核时**先 shutdown 旧 socket 文件再清理**（防残留监听）；spawn 前收紧 umask(0o007)（Unix）限制 socket 权限。
- 验证：**【本机可编译验证】**（macOS Unix socket 直接可测；Windows 命名管道路径 **【需 Windows 真机验证】**）。

**0.2 实时日志流经 FFI/命令暴露**
- 借鉴：FlClash 事件主动推送（无 id 的 ActionResult → `CoreEventManager`）；verge `use-mihomo-ws-subscription.ts` 的共享订阅+引用计数+自动重连。
- 改：`core/src/ffi/observer.rs`（新增 `trait LogObserver { fn on_log(&self, line: LogLine); }` callback interface）、`core/src/ffi/manager.rs`（新增 `subscribe_logs(observer)` / `unsubscribe_logs`，内部消费 `KernelManager::live_logs()`）、`desktop/src-tauri/src/commands/connection.rs`（新增 `tauri::command` 把 `live_logs()` 流 spawn 成 task → `app.emit("connection://log", LogLine)`，带级别过滤参数）、`desktop/src-tauri/src/lib.rs`（注册新命令 + 在 connect 成功后启动日志 forwarder）。
- 工程要点：core 已有 `log_stream()`/`live_logs()`，**不需要新写抓取逻辑，只补转发与生命周期**；级别过滤走内核 query（`/logs?level=`）；前端 500 条环形缓冲。
- 验证：**【本机可编译验证】**。

**0.3 自动重连（exited 后退避重连）**
- 借鉴：FlClash `CoreAction.restartCore`（shutdown→connect→init→恢复运行态）；verge fallback restart。
- 改：`core/src/kernel/manager.rs`（监督任务判定 `Exited` 后，若非用户主动 disconnect（`expecting_stop=false`），按退避重连原 subscribe_url，最多 N 次，广播 `Reconnecting` 新状态）、`desktop/src/stores/connection.ts`（处理 `Reconnecting` 态 + UI 文案）、`core/src/kernel/launcher.rs`（区分主动停与崩溃已有，复用）。
- 工程要点：复用现有 `retry_with_backoff`（250ms/1s/4s）；重连前 closeAllConnections 思路 → 这里是重启内核所以天然干净；重连耗尽后停在 `Error` 等用户。
- 验证：**【本机可编译验证】**（kill 内核进程模拟）。

**验收标准（批次0）**：① 桌面连接后控制流走 socket，`lsof -i` 看不到内核控制端口监听，两实例并行启动不冲突；② 实时日志面板能滚动显示内核日志且可按级别过滤；③ 手动 `kill` mihomo 进程后客户端自动在 ≤10s 内重连成功并恢复 connected 态。

---

### 批次 1（P1，可观测性主体）：连接监控 + 流量图 + 实时日志面板 + 规则视图

桌面端纯前端 + 薄命令，全部本机可验证，排障价值最高。

**1.1 活动连接监控页**
- 借鉴：verge `connections.tsx`（活动/已关闭双 tab、列管理、单连接关闭、链路详情、搜索 host/IP/process、按速度排序）；FlClash 关键词点击过滤、单条阻断。
- 改：`core/src/kernel/driver.rs`（trait 加 `async fn connections() -> Vec<ConnectionItem>` 与 `async fn close_connection(id)` / `async fn close_all_connections()`）、`core/src/kernel/mihomo.rs`（实现：`GET /connections` 完整解析 TrackerInfo，`DELETE /connections/{id}`、`DELETE /connections`）、`core/src/kernel/manager.rs`（透传）、`desktop/src-tauri/src/commands/connection.rs`（新增三个命令）、**新增** `desktop/src/pages/Connections.vue`、`desktop/src/stores/connection.ts`（1s 轮询活动连接，或复用日志 forwarder 思路）、`desktop/src/router.ts`（加路由）、`desktop/src/components/HomeIconRail.vue`（加导航入口）。
- 工程要点：列表用虚拟滚动（引入 `@tanstack/vue-virtual` 或 `vue-virtual-scroller`）；搜索支持 host/destinationIP/process；点击字段加为过滤关键词（FlClash 范式）；裁掉 verge 的 ColumnManager（过度，固定列即可）。
- 验证：**【本机可编译验证】**。

**1.2 实时流量折线图卡片**
- 借鉴：verge Canvas 流量图（采样/压缩/Web Worker）；FlClash `LineChart` + `RepaintBoundary` 渐变填充。
- 改：**新增** `desktop/src/components/TrafficChart.vue`（Canvas 绘制上下行曲线，环形缓冲最近 60–120 个点）、`desktop/src/pages/Home.vue`（嵌入卡片，复用现有 `traffic` ref 的 1s 轮询，不必新增订阅）、`desktop/src/stores/connection.ts`（traffic 历史序列 ref）。
- 工程要点：直接用现有 `current_traffic` 轮询喂数据，**不需要 Worker**（机场用户挂机量级足够）；裁掉 verge 的多时间范围切换（保留单一最近窗口）。
- 验证：**【本机可编译验证】**。

**1.3 实时日志面板（消费批次0.2）**
- 借鉴：verge `Logs` 页（级别过滤/搜索高亮/升降序/暂停/清空/自动滚底）。
- 改：**新增** `desktop/src/pages/Logs.vue`（订阅 `connection://log` 事件，500 条环形缓冲，级别 all/info/warn/error 过滤，near-bottom 自动滚底，暂停/清空）、`desktop/src/router.ts` + `HomeIconRail.vue`（导航）。
- 工程要点：error 级日志可触发轻量 toast（FlClash 范式，帮用户第一时间察觉节点连不上）。
- 验证：**【本机可编译验证】**。

**1.4 规则视图（轻量）**
- 借鉴：verge `Rules` 页虚拟列表。
- 改：`core/src/kernel/driver.rs` + `mihomo.rs`（加 `async fn rules() -> Vec<RuleItem>`，`GET /rules`）、`desktop/src-tauri/src/commands/connection.rs`（命令）、**新增** `desktop/src/pages/Rules.vue`。
- 工程要点：贴合面板定位**弱化**——只读展示「当前生效规则 + 行号 + 搜索」，不做规则集更新/编辑；价值在让用户确认「当前流量走代理还是直连」。可降到 P2 若工期紧。
- 验证：**【本机可编译验证】**。

**验收标准（批次1）**：① 连接页能看到活动连接（host/规则/代理链/上下行/时长），可关单条和全部，可搜索；② 首页有实时上下行折线图随流量波动；③ 日志页实时滚动、可过滤级别、可暂停清空；④ 规则页能搜索查看当前生效规则。全部 macOS 本机 `cargo tauri dev` 可见。

---

### 批次 2（P1，可靠性守卫）：sysproxy guard + 退出清理强化 + 配置事务化

直击「连上不走代理」「切换断流」客诉，全部 Rust 本机可验证。

**2.1 系统代理守卫（GuardMonitor）**
- 借鉴：verge `core/sysopt.rs::refresh_guard`（默认 30s 周期重断言，`enable_proxy_guard` 开关，PAC/global 写入顺序 `proxy_apply_steps`）；verge `use-system-proxy-state.ts` 的「最后写入生效」防抖。
- 改：`core/src/tunnel/system_proxy.rs`（新增 `GuardMonitor`：tokio interval 30s，比对 `Sysproxy::get_system_proxy()` 与期望 `host:port`，不符则重写）、`core/src/tunnel/mod.rs`（暴露 start/stop guard）、`core/src/kernel/manager.rs`（SystemProxy 模式 connect 成功后启动 guard，disconnect 停 guard）、`desktop/src/stores/connection.ts` + 设置页（`enable_proxy_guard` 开关）。
- 工程要点：guard 只在 SystemProxy 降级模式下生效（TUN 模式不需要）；clear 时保留其它程序写的 bypass（现有 `clear()` 已这么做）；连点开关用 pending/busy ref 只执行最终态。
- 验证：**【本机可编译验证】**（连接后手动用 `networksetup` 改代理，观察 30s 内被夺回）。

**2.2 配置应用事务化 + reload 优先**
- 借鉴：verge `core/manager/config.rs::apply_config`（先 sidecar `-t` 校验 → mihomo API reload → 失败 fallback restart → 再失败 discard 回滚）；切换前 closeAllConnections。
- 改：`core/src/kernel/mihomo.rs`（`reload()` 走 `PUT /configs?force=true` 热重载而非重启；切节点/切模式前调 `close_all_connections()`）、`core/src/kernel/manager.rs`（切 requested_mode / reselect 时优先 reload，端口/控制器变更才 restart）、`core/src/profile/inject.rs`（确认校验路径）。
- 工程要点：用 `AtomicBool` 互斥 + 去抖（300ms，借 verge `CONFIG_UPDATE_DEBOUNCE`）防前端连点并发重载；reload 失败回滚到旧可用配置「要么换成功要么不断网」。
- 验证：**【本机可编译验证】**。

**2.3 退出清理并行+逐项超时**
- 借鉴：verge `feat/window.rs::quit/clean_async`（set_is_exiting → 并行重置代理1.5s/关TUN1s/停内核2-3s/恢复DNS1s，逐项超时绝不卡死）。
- 改：`desktop/src-tauri/src/lib.rs`（`on_window_event` 的 Destroyed / ExitRequested 分支已 block_on disconnect，改为并行带超时的 clean，置全局 `is_exiting`）、`core/src/kernel/manager.rs`（disconnect 内部各步加独立超时）。
- 工程要点：现有已在崩溃时清代理，这里补「关机/内核卡死也能干净退出并恢复系统代理」。
- 验证：**【本机可编译验证】**。

**验收标准（批次2）**：① SystemProxy 模式下被外部程序篡改代理后 ≤30s 自动夺回，开关可关闭 guard；② 切节点/切模式不重启内核（reload）、不残留脏连接、失败不断网；③ 强杀客户端 / 关机时系统代理被恢复，不留「连不上网」后遗症。

---

### 批次 3（P2，桌面便利）：设置页 + 热键 + 开机自启 + 厚托盘 + 节点UX + 自动连接

提升日常体验，对标 verge 桌面便利，本机可验证。

**3.1 集中设置页**
- 借鉴：verge `home_cards` 持久化 + GuardState 乐观更新+失败回滚。
- 改：**新增** `desktop/src/pages/Settings.vue`（分区：通用[语言/主题/启动即连/开机自启]、连接[默认模式/测速URL/控制路径只读/sysproxy guard 开关/bypass]、关于[内核版本/检查更新/打开日志目录]）、`desktop/src-tauri/src/persistence.rs`（扩展持久化字段）、`desktop/src-tauri/src/commands/meta.rs`（settings get/set 命令）、`desktop/src/router.ts` + `HomeIconRail.vue`。
- 工程要点：把现有散落的模式/主题（`stores/theme.ts`）收敛进来；GuardState 范式（先改 UI 再 patch 后端，失败回滚 + toast）。
- 验证：**【本机可编译验证】**。

**3.2 全局热键 + 开机自启 + 启动即连**
- 借鉴：verge hotkey（开关面板/切模式/切系统代理/退出）+ autostart 插件 + silent-start。
- 改：`desktop/src-tauri/Cargo.toml`（加 `tauri-plugin-global-shortcut`、`tauri-plugin-autostart`）、`desktop/src-tauri/src/lib.rs`（注册插件 + 热键绑定连接/断开/切节点/显示窗口）、`desktop/src-tauri/src/capabilities/default.json`（加权限）、`desktop/src/pages/Settings.vue`（热键录入 + 开机自启 + 启动即连开关）、`desktop/src-tauri/src/commands/connection.rs`（启动即连：setup 完成且有有效会话时自动 connect）。
- 工程要点：启动即连要等会话 hydrate（`stores/auth.ts`）+ 订阅有效再触发，无有效订阅则不连（复用 `canConnect`）。
- 验证：**【本机可编译验证】**。

**3.3 厚托盘（状态变色 + 速率 + 模式/节点快切）**
- 借鉴：verge tray（`update_icon` 三态变色 / `update_tooltip` 实时状态 / 节点组嵌套 Submenu+CheckMenuItem / 速率显示 / 关闭所有连接 / 打开日志目录）。
- 改：`desktop/src-tauri/src/lib.rs`（现极简托盘扩成：连接/断开、模式切换 CheckMenuItem、节点快切 Submenu、关闭所有连接、打开日志目录、状态变色图标 common/sysproxy/tun、tooltip 显示节点+模式+版本）、**新增** `desktop/src-tauri/src/tray.rs`（菜单构建/局部刷新 `update_part`）、`desktop/src-tauri/icons/`（三套托盘图标）。
- 工程要点：图标随 connection-state 变色；可选托盘标题显示速率（`enable_tray_speed`）；菜单局部刷新避免全量重建。
- 验证：**【本机可编译验证】**。

**3.4 节点栏 UX 增强 + 选择持久化 + 整组测速**
- 借鉴：verge 节点过滤/排序/搜索（delay<200 语法）、整组一键测速 `delayGroup`、延迟颜色分级 `formatDelayColor`、单例 `DelayManager`（并发限制+抖动+30min缓存）；FlClash tab/list 两视图。
- 改：`desktop/src/components/NodeRail.vue`（搜索/排序[默认/延迟/字母]/颜色分级/整组测速按钮）、`desktop/src-tauri/src/commands/connection.rs`（`latency_test` 已有，加 `delay_group` 批量带并发限制）、`core/src/kernel/manager.rs`（整组测速并发控制）、`desktop/src/stores/nodes.ts`（选中节点持久化到 `persistence.rs`，重启恢复）。
- 工程要点：测速做并发上限（≤10）+ 随机抖动 + 超时 race + 短期缓存，避免打爆节点；选择持久化解决「重启后回到默认节点」。
- 验证：**【本机可编译验证】**。

**验收标准（批次3）**：① 有独立设置页，开关即时反馈且失败回滚；② 热键可连/断/显示窗口，开机自启与启动即连生效；③ 托盘随状态变色、显示节点+模式、可切模式/切节点/关闭所有连接/开日志目录；④ 节点栏可搜索排序、整组测速带颜色分级、重启后保留上次选中节点。

---

### 批次 4（P2，端口冲突与 Windows 落地的本机部分）

**4.1 控制路径冲突彻底消除 + 内核进程事件监听**
- 借鉴：verge `take_child_sidecar`（ArcSwapOption 原子取子进程再 kill 防重复）、CommandEvent Terminated 上报。
- 改：`core/src/kernel/launcher.rs`（DirectLauncher 已有 exit-watcher，补 Terminated 日志上报 + IPC socket 文件清理）、`core/src/kernel/manager.rs`。
- 验证：**【本机可编译验证】**。

**4.2 Windows xboard-svc 打通（命名管道 TUN）**
- 借鉴：verge Service IPC 就绪等待+退避重试（`wait_for_service_ipc` max_retries=20/250ms）、安装失败优雅降级 Sidecar、`repair_service` 强制重装；FlClash helper SHA256 令牌鉴权。
- 改：`svc/src/main.rs`（补完命名管道 server + start/stop/logs）、`core/src/kernel/launcher.rs`（`SvcPipeLauncher` 的就绪等待+退避）、`desktop/src-tauri/src/svc_install.rs`（UAC 安装链路）、`desktop/src/pages/Settings.vue`（安装/卸载/修复入口）。
- 工程要点：start 前校验请求内核二进制 SHA256 == 编译期嵌入令牌（防滥用）；安装失败降级到系统代理（现有降级逻辑已在）。
- 验证：**【需 Windows 真机验证】**（Rust 编译本机可过，运行需 Windows）。

**验收标准（批次4）**：① 反复启停内核无端口/socket 残留；② Windows 上能装 svc 并走 TUN，失败优雅降级系统代理，设置页有修复按钮。

---

### 批次 5（P3，移动端数据面 / 分应用代理，需真机验证）

排在最后，因为必须真机/Xcode/Android 验证。

**5.1 Android 分应用代理（per-app）**
- 借鉴：FlClash Access Control（白/黑名单 `addAllowed/DisallowedApplication`、列已装包带图标/系统标记/联网权限、搜索/排序/过滤系统应用、智能选择国内应用直连、剪贴板导入导出、运行期 `getConnectionOwnerUid` 归因）。
- 改：`android/app/src/main/kotlin/com/xboard/client/vpn/XboardVpnService.kt`（Builder 阶段按用户选择 addAllowed/Disallowed）、**新增** `android/.../vpn/AppListProvider.kt`（枚举已装包）、**新增** Compose 分应用页、`core/src/ffi/manager.rs`（如需把选择透传/持久化）。
- 验证：**【需 Android 真机验证】**。

**5.2 移动端网络切换/唤醒自动恢复 + 数据面打磨**
- 借鉴：FlClash `NetworkObserveModule`（NetworkCallback 跟随底层网络 DNS → `Core.updateDNS` 热更新不重建 tun）、息屏 `suspended`、`onLowMemory→forceGC`、前台服务 specialUse FGS、`detachFd` 裸 fd 生命周期、protect 回环防护、独立 :remote 进程。
- 改：`android/.../vpn/XboardVpnService.kt`（NetworkCallback + 前台服务子类型 vpn + onLowMemory GC + Quick Tile）、`android/app/src/main/AndroidManifest.xml`（FGS specialUse + 权限）、`core/src/ffi/manager.rs`（暴露 `update_dns` / `force_gc` / `suspend`）、`core/src/kernel/`（对应内核调用）；iOS：`ios/PacketTunnel/PacketTunnelProvider.swift`（NWPathMonitor 跟随网络、on-demand/always-on 配置 `ios/XboardClient/ConnectionController.swift`、fd 生命周期与 protect 原则平移）。
- 工程要点：核心原则——**fd 生命周期（detachFd）+ protect 防回环 + 不重建隧道只更新底层网络**；Android 已修 fd 注入 P0，这里补保活与切网。
- 验证：**【需 Android 真机 / iOS 真机验证】**。

**5.3 移动端可观测性接出（复用批次0.2）**
- 借鉴：批次0 的 FFI 日志/连接接口。
- 改：`core/src/ffi/observer.rs` + `manager.rs` 的 `subscribe_logs`/`connections` 接口在 Android Kotlin / iOS Swift 各加一个简版日志+连接面板（FFI 已对齐，移动端补 UI）。
- 验证：**【需真机验证】**。

**验收标准（批次5）**：① Android 可选哪些应用走代理并生效，连接面板能显示发起应用；② WiFi↔蜂窝切换、息屏唤醒后连接自动恢复不丢包，后台不易被杀；③ 移动端能看实时日志与活动连接。

---

## 四、落地优先级一览

| 批次 | 优先级 | 核心交付 | 验证 |
|---|---|---|---|
| 0 | **P1** | 控制通道 IPC 化、日志流接出、自动重连 | 本机（Win管道需真机） |
| 1 | **P1** | 连接监控、流量图、日志面板、规则视图 | 本机 |
| 2 | **P1** | sysproxy guard、配置事务化 reload、退出清理 | 本机 |
| 3 | P2 | 设置页、热键、开机自启、厚托盘、节点UX、启动即连 | 本机 |
| 4 | P2 | 端口冲突根除、Windows svc TUN 打通 | 本机编译 / Win 真机 |
| 5 | P3 | Android 分应用代理、移动切网/保活、移动可观测性 | 真机/Xcode |

**最高 ROI**：批次 0+1+2 全部桌面本机可编译验证，把已有 core 能力「接出来 + 加守卫」，即可让 xboard-client 在「连接可用性 + 可观测性」上达到 verge 同档；批次 5 的移动数据面是与 FlClash 拉平的最后一公里，但必须真机验证、排在最后。

---

相关真实文件锚点（落地起点）：
- core 已具备但未接出：`core/src/kernel/manager.rs:377`（`live_logs`）、`core/src/kernel/mihomo.rs:352`（`/connections` 仅累计）、`core/src/kernel/driver.rs:83`（`KernelDriver` trait，扩 connections/rules/close）、`core/src/ffi/observer.rs`（加 LogObserver）、`core/src/ffi/manager.rs`（加 subscribe_logs）。
- 桌面接线：`desktop/src-tauri/src/commands/connection.rs`、`desktop/src-tauri/src/lib.rs`（极简托盘+事件转发处）、`desktop/src/stores/connection.ts`、`desktop/src/router.ts`、`desktop/src/components/HomeIconRail.vue`。
- 守卫/事务：`core/src/tunnel/system_proxy.rs`（加 GuardMonitor）、`core/src/kernel/mihomo.rs`（reload 走 `PUT /configs`）。
- 平台落地：`svc/src/main.rs`、`core/src/kernel/launcher.rs`（SvcPipeLauncher）、`android/app/src/main/kotlin/com/xboard/client/vpn/XboardVpnService.kt`、`ios/PacketTunnel/PacketTunnelProvider.swift`。