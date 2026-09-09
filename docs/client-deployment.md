# SUFE 专属后端、加密引导与动态功能

客户端绑定同一个 SUFE 部署，用户无需填写服务器地址。默认保留项目原有的 `https://imitate.cnqq.de`；打包前必须确认这是你的实际服务。功能协议已经根据旁边的 `Xboard-sh-1` 和 `sufe-middleware-rs` 源码核对。本地契约测试不代表生产账号、支付渠道或网络已经验收。

## 构建配置

简单直连部署可设置构建环境变量 `SUFE_BACKEND_URL`。完整部署把下列 JSON 保存在本机受限配置文件（不要提交密码），将文件原文设置为 **构建时** 的 `SUFE_DEPLOYMENT_JSON`；Cargo 重新构建后即嵌入应用：

```json
{
  "deployment": {
    "project_id": "sufe",
    "brand_name": "SUFE",
    "api_endpoints": ["https://api.example.com", "https://api-backup.example.com"],
    "features": {
      "purchase": true,
      "recharge": false,
      "gift_card": true,
      "checkin": false,
      "notice": true,
      "invite": true,
      "tickets": true,
      "chatwoot": false,
      "custom_rules": true
    },
    "chatwoot_base_url": null,
    "chatwoot_inbox_identifier": null
  },
  "stealth_password": null,
  "middleware_turnstile_site_key": null,
  "oss": null
}
```

PowerShell：`$env:SUFE_DEPLOYMENT_JSON = Get-Content -Raw -LiteralPath 'C:\private\sufe-deployment.json'`，然后正常打包。API 地址必须是 HTTPS 站点根地址，不可含路径、查询、账户或密码。调试构建仅额外接受 `localhost`/`127.0.0.1`/`::1` 的 HTTP。所有备用 API 必须属于同一用户数据库、同一 Sanctum 会话体系与同一运营方。

## 与真实中间件兼容

`stealth_password` 填中间件的 `SEC_PASSWORD`。算法完全对应 `sufe-middleware-rs/src/crypto.rs` 和 `pathname.rs`：HKDF-SHA256（salt=`xb-stealth-v1`，info=`xb-stealth-keystream/v1`）派生 64 字节；前半 AES-256-GCM，后半 HMAC-SHA256；API 路径变为 `/v2/<前16字节HMAC的hex>`。Query、Body 使用不同随机 nonce，响应必须带独立 `X-Request-Id` 并通过 GCM 校验。密钥或路由不匹配会明确失败，绝不自动转为明文。

若中间件开启独立 Turnstile 验证，配置 `middleware_turnstile_site_key`，客户端会显示该挑战并向中间件发送 `X-Captcha-Provider`/`X-Captcha-Token`。同时关闭 Xboard 对同一个挑战的重复验证；Turnstile token 不能重复消费。中间件腾讯天御目前没有客户端挑战控件，因此该组合暂不支持。普通后端 Turnstile/reCAPTCHA 沿用现有字段，无需配置中间件挑战。

现有中间件白名单只列出了礼品卡 `redeem`，启用礼品卡查询/历史时需要在中间件部署环境补充：

```text
EXTRA_OBFUSCATED_PATHS=/api/v1/user/gift-card/check,/api/v1/user/gift-card/history,/api/v1/user/gift-card/detail,/api/v1/user/gift-card/types
```

该能力中间件已有实现，无需修改 Laravel。订阅下载使用 Xboard 返回的 HTTPS `subscribe_url`，不会伪造订阅路径映射；它与登录 API 传输配置分开处理。

## 多 OSS，每个 OSS 多 API

将完整配置里的 `oss` 替换为：

```json
{
  "urls": ["https://oss-a.example.com/client.json", "https://oss-b.example.com/client.json"],
  "password": "单独的长随机配置解密口令",
  "public_key": "32字节Ed25519公钥的标准Base64"
}
```

最多 8 个 OSS 并行读取，每个对象最多 256 KiB、含 1–16 个 API。每份对象必须先通过内嵌 Ed25519 公钥验证，再用单独的 HKDF/AES-GCM 密钥解密。客户端检查 `project_id`、签发/过期时间和本次运行中的版本回退，选择可用对象中签发时间最新者。每份对象可列出不同的备用 API，但必须服务于同一部署。远程对象不能更改加密模式或中间件口令。配置每 5 分钟检查一次；OSS 失败保留当前可信配置，冷启动失败使用内嵌配置。本版本未持久化远程对象或版本水位，重启后的最低版本以内嵌配置为准。

Python 发布工具：

```text
python -m pip install cryptography
python scripts/pack-client-config.py keygen C:/private/sufe-bootstrap-key.pem
python scripts/pack-client-config.py pack C:/private/deployment-document.json C:/private/sufe-bootstrap-key.pem C:/private/client.json --valid-days 7
```

`keygen` 仅打印公钥；将其填入内嵌 `oss.public_key`。私钥必须留在受限运维目录或 CI Secret，不能打包到客户端。`pack` 从环境变量 `SUFE_BOOTSTRAP_PASSWORD` 读取与 `oss.password` 相同的口令。`deployment-document.json` 只包含上例 `deployment` 对象；工具自动写入 Unix 秒时间戳。生成后将密文上传到你配置的 OSS URL 即生效。每份文档最长 90 天有效，运营方需在过期前重新签发。

内嵌解密口令和中间件共享口令可以从客户端二进制提取；它们用于传输混淆和配置静态加密，不能代替服务端鉴权。防止第三方篡改 OSS 配置依靠独立 Ed25519 私钥。HTTPS 始终必需。

GET 查询在连接故障或 502/503/504 时切换备用 API。下单、支付、兑换等写请求只在未建立连接时尝试备用地址；收到错误/超时后不自动重放，避免重复写入。请求拒绝重定向，以免凭据或密文被重新发送到非预期主机。`resetSecurity`、旧 GET `invite/save` 也按写请求保护。

## 功能与真实接口

| 功能 | Xboard-sh-1 实际接口 | 状态 |
|---|---|---|
| 购买、优惠券、订单 | `/user/order/*`, `/user/coupon/check` | 支持，按真实周期验券 |
| 礼品卡 | POST `/user/gift-card/check`, `/redeem`；GET `/history` | 支持预览、兑换和记录 |
| 邀请 | GET `/user/invite/fetch`；POST `/user/invite/save` | 支持查看、创建随机/自定义邀请码 |
| 公告、工单 | `/user/notice/fetch`, `/user/ticket/*` | 支持，受功能开关控制 |
| 独立余额充值 | 未发现路由 | 强制关闭；不把套餐购买冒充充值 |
| 签到 | 未发现路由 | 强制关闭 |
| Chatwoot | 非 Xboard 路由；独立 Chatwoot Public API | 配置后启用，默认关闭 |

Chatwoot 需要 `chatwoot_base_url` 与 **API Inbox 的 identifier**，不是网站 widget 的 website token，也不能放管理员 access token。先在自己的 Chatwoot 配好对应 inbox，再设 `features.chatwoot=true`。验证码、实际付款及客服生产通知仍需运营方真实环境验证。

Tauri：`fetch_client_config` 返回公开品牌/开关/客服地址/传输模式，绝不返回 API 列表、密码或私钥。礼品卡命令 `gift_card_check`、`gift_card_redeem`、`gift_card_history`；邀请命令 `fetch_invites`、`create_invite`。返回值保留后端奖励结构、分页和错误，前端不能将请求失败当兑换成功。
