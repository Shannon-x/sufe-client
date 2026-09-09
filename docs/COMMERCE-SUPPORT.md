# 购买与客户支持

## 已实现的购买流程

客户端先创建订单，再读取 `/user/order/detail` 核对服务端的优惠、余额、旧套餐折抵与支付方式绑定，最后由用户确认付款。所有金额使用整数分；手续费计算与 Xboard-sh-1 的 OrderController 一致。已创建但未付款的订单始终保留在订单中心，客户端不会因再次购买自动取消它。

- 优惠码检查同时发送 `code`、`plan_id`、`period`，防止周期限定券预览通过但下单失败。
- 服务端应付为零时，checkout 传 `method: 0`，不需要面板启用任何支付渠道。
- 恢复订单读取 `payment_id` 与 `handling_amount`，锁定首次 checkout 所用的支付渠道及手续费。
- HTTPS 跳转支付在独立 Tauri 支付窗口中打开，该窗口不拥有主窗口的 IPC 权限。返回面板订单页只触发状态查询，绝不据跳转 URL 宣告支付成功。
- 原生窗口无法打开时降级到系统浏览器；返回客户端触发立即检查。二维码支付使用本地二维码绘制。
- 订单状态 1 表示已付款、正在开通；只有状态 3 / 4 进入完成状态。自动检查最多 10 分钟，暂停后可手动检查或稍后从订单中心恢复。
- 某些网关返回 `type: -2` 的专有支付表单且没有 HTTPS URL；客户端会解释当前限制并保留订单，不执行任意 HTML。

## Chatwoot 原生界面

开启发布配置中的 `features.chatwoot`，并配置 `chatwoot_base_url`（HTTPS）与 `chatwoot_inbox_identifier`（API channel inbox identifier）。Website widget token 不能替代 API 收件箱标识。发布配置载入与 OSS 加密机制见部署配置文档。

Chatwoot 公共 Client API 的来源：

- [创建联系人](https://developers.chatwoot.com/api-reference/contacts-api/create-a-contact)
- [创建会话](https://developers.chatwoot.com/api-reference/conversations-api/create-a-conversation)
- [读取消息](https://developers.chatwoot.com/api-reference/messages-api/list-all-messages)
- [发送消息](https://developers.chatwoot.com/api-reference/messages-api/create-a-message)

客户端不会携带 Chatwoot 管理员或 Account API token。Rust 仅能请求发布配置指定的 HTTPS 收件箱，并为当前登录用户建立联系人。联系人 `source_id` 是访问该联系人的凭据，保存在系统密钥链；缓存键包括 Xboard 后端、Chatwoot 地址、收件箱和用户邮箱。它不返回 JavaScript，也不写入 localStorage；密钥链不可用时仅保留进程内缓存。localStorage 只保存已读消息编号，不保存聊天正文。

原生页面支持历史会话、纯文本消息、客服附件链接、应用内未读数和新消息提示。消息在应用运行期间定时同步：前台约 8 秒、后台约 20 秒，网络失败逐步延长至 60 秒。系统通知仅在用户已授予系统通知权限时发送，通知不包含聊天正文。关闭应用后不提供推送唤醒；当前未配置移动系统推送。

没有配置在线客服时显示真实说明及工单入口，不展示虚构在线坐席、聊天记录或承诺的响应时间。当前没有生产 Chatwoot 地址，因此本地验证不等于生产客服端到端验收。

## 自动验证

- `node --test desktop/tests/billing.test.ts`：金额舍入、零元手续费、优惠上界与开通状态。
- 在 desktop 下启动 `npm run dev -- --host 127.0.0.1`，再运行 `npx playwright test tests/commerce.spec.ts --workers=1`：预览禁止真实下单、零元支付、订单恢复绑定、周期优惠券、客服未配置空态。

Playwright 使用 HTTP 路由替换开发服务器的 platform 模块，测试桥仅存在于测试浏览器中，生产源码没有支付成功模拟开关。
