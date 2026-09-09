# 移动端共享原生接口

本接口由 `core/src/ffi.udl` 生成 Kotlin/Swift；JSON 字段保留服务端 snake_case。新声明已用现有 `uniffi-bindgen` 在本机生成两种绑定；这只验证声明与命名，实际 Rust、Kotlin、Swift 编译结果由交付记录单独列出。

## 构造和配置

生产入口 Kotlin `Client.forDeployment(locale, secure)`、Swift `try Client.forDeployment(locale: "zh-CN", secure: secure)`。旧的 URL constructor 保留兼容测试；产品不展示任意后端地址输入。

桌面与移动统一读取 `core/src/api/runtime_config.rs` 的编译变量 `SUFE_DEPLOYMENT_JSON`，兼容 `SUFE_BACKEND_URL`、`SUFE_STEALTH_PASSWORD`、`SUFE_MIDDLEWARE_TURNSTILE_SITE_KEY`。无覆盖时使用 `https://www.isufe.me`；非空完整 JSON 优先于单独后端地址。更改这些值需要重编 Rust 原生库；只重编 Kotlin/Swift、修改 Info.plist 或安全存储不会更新配置。协议、密码、公钥配置见 [部署说明](client-deployment.md)。所有壳复用同一 Stealth-v1、签名加密 OSS、API 故障切换实现；请求新配置和受开关保护的业务操作最多每五分钟刷新一次 OSS。

`await client.fetchClientConfigJson()` 示例：

```json
{"brand_name":"SUFE","features":{"purchase":true,"recharge":false,"gift_card":true,"checkin":false,"notice":true,"invite":true,"tickets":true,"chatwoot":false,"custom_rules":true},"chatwoot_base_url":null,"chatwoot_inbox_identifier":null,"config_source":"built_in","transport_mode":"https","api_candidates":1}
```

`config_source` 为 `built_in` 或 `encrypted_oss`，传输模式为 `https` 或 `stealth-v1`。未实现的独立充值、签到始终关闭。配置不包含路径加密密码、OSS 密码、API 候选地址或客服 contact source 凭据。`fetchSiteConfig()` 合并中间件 Turnstile 配置；同一个验证码不能被中间件和后端重复消费。Android/iOS 已接入动态验证码控件与登录、注册、找回密码、发送邮件验证码；真实运营站点密钥与设备验证仍需验收，见 [移动验证码说明](mobile-captcha.md)。

## 账户、订单和权益

下表方法均为 async throws / Kotlin suspend，返回 String 的内容是 JSON；失败抛出 FfiError，不能视为成功。礼品卡、邀请、规则、客服要求已登录，且在 Rust 层检查对应功能开关。关闭购买后仍允许支付/查询已有订单。

| Kotlin/Swift 方法名 | 参数 | 返回 |
|---|---|---|
| giftCardCheckJson | code: String | 服务端 `{code_info,reward_preview,can_redeem,reason}` |
| giftCardRedeemJson | code: String | `{message,rewards,invite_rewards,template_name}` |
| giftCardHistoryJson | page: Long / Int64，从 1 开始 | `{data:[...],pagination:{current_page,last_page,per_page,total}}`，每页 30 条 |
| fetchInvitesJson | 无 | `{codes:[...],stat:[注册数,累计佣金,待确认佣金,比例,可用佣金]}` |
| createInvite | code: String?；null 为后端自动生成 | Bool / Boolean；完成后重新拉列表 |
| fetchOrderJson | tradeNo: String | 真实 `/user/order/detail` 对象，包含 total_amount、balance_amount、discount_amount、surplus_amount、handling_amount、payment_id、plan、status 等服务端提供的字段 |
| checkCouponForPeriodJson | code: String, planId: Long / Int64, period: String | `{id,code,type,value}`；type=1 固定金额，type=2 百分比；value 可能为 null |
| supportRequestJson | action: String, conversationId: ULong? / UInt64?, content: String? | 见客服小节 |

金额字段以分为单位，流量以字节为单位。礼品卡奖励含不同类型，必须按后端返回结构展示，不把奖励一律解释为余额；盲盒预览可能重新抽样，最终以兑换结果为准。订单保存后必须展示实际服务端金额再 checkout，失败时保留 tradeNo 重试。支付成功以 checkOrder 返回的真实状态 3/4 为准，2 表示取消；checkout 空响应不是成功。

`currentUser()` 现保留 `banned: Boolean/Bool` 与 `transferEnable: ULong?/UInt64?`。封禁用户应解释无法连接；后端订阅接口仍是最终判断依据。

## 自定义规则

```json
[{"id":"rule-1","kind":"DOMAIN-SUFFIX","value":"example.com","target":"DIRECT","enabled":true}]
```

`customRulesJson()` 读取；`saveCustomRulesJson(json)` 校验后保存。支持 DOMAIN、DOMAIN-SUFFIX、DOMAIN-KEYWORD、IP-CIDR、IP-CIDR6；目标 DIRECT、PROXY、REJECT；最多 200 条，拒绝逗号/换行注入、无效 CIDR 和重复 id。PROXY 在订阅中解析到真实代理组，缺少代理组时明确失败。

规则保存在 Client 的 SecureStore，按固定后端身份和登录邮箱隔离。Android `ConnectionManager.connect()` 自动把这份规则同步至内核配置，保存后重连生效。iOS 无 mihomo ConnectionManager，获取 YAML 后先调用 `await client.applyCustomRulesYaml(subscribeYaml: yaml)`，再执行 `renderSingboxConfig`。关闭规则功能时连接不注入规则。桌面保留既有规则文件，重新开启可恢复；移动的 SecureStore 同样保留原记录。

## 原生客服

桌面和移动共用 `core/src/api/support.rs`。只访问部署配置中的 HTTPS Public API Inbox，不接受 UI 任意 URL、账号管理员 token 或 contact source_id。网络重定向禁用，实际响应体限制 4 MiB；返回值递归去除 source_id/pubsub_token。

- `restore`：返回 `{"conversations":[...]}`；未建过 contact 时返回空数组，不创建联系人。
- `start`：恢复未解决会话或创建会话，返回会话对象 `{id,status,...}`。
- `messages`：conversationId 必填，返回消息数组。
- `send`：conversationId、content 必填，1–4000 字；返回消息对象，不额外包一层 message。

消息以纯文本显示；会话 ID 与消息 ID 应去重。实际 Chatwoot Inbox、消息收发与通知权限仍需运营环境验证。仅完成 REST 桥接不能代表系统关闭应用后能收到推送。
