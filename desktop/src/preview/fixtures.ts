import type { ConnectionState, Plan } from "@/types";
const GiB = 1024 ** 3;
const names = [
  "香港 · 优选 01",
  "香港 · 优选 02",
  "日本 · 东京 01",
  "日本 · 大阪 02",
  "新加坡 · 狮城 01",
  "美国 · 洛杉矶 01",
  "英国 · 伦敦 01",
  "德国 · 法兰克福 01",
];
let connection: ConnectionState = { kind: "disconnected" };
let selected = names[0];
let tick = 0;
let mode = "system_proxy";
let customRules: unknown[] = [];
const emit = (name: string, detail: unknown) =>
  window.dispatchEvent(new CustomEvent(name, { detail }));
export const plans: Plan[] = [
  {
    id: 1,
    name: "轻享计划",
    month_price: 1500,
    year_price: 15000,
    transfer_enable: 100,
  },
  {
    id: 2,
    name: "畅游计划",
    month_price: 2800,
    year_price: 28000,
    transfer_enable: 300,
  },
  {
    id: 3,
    name: "无界计划",
    month_price: 5800,
    year_price: 58000,
    transfer_enable: 800,
  },
].map((p) => ({
  content:
    "<p>全球精选线路</p><p>不限设备数量 · 全平台支持</p><p>智能分流 · 在线客服</p>",
  group_id: 1,
  type: 1,
  quarter_price: null,
  half_year_price: null,
  two_year_price: null,
  three_year_price: null,
  onetime_price: null,
  reset_price: null,
  reset_traffic_method: 0,
  show: true,
  sell: true,
  renew: true,
  sort: 1,
  created_at: null,
  updated_at: null,
  ...p,
}));
export async function previewInvoke(
  command: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  const read: Record<string, unknown> = {
    hydrate_session: {
      email: "hello@sufe.example",
      is_admin: false,
      subscribe_token: "preview-only",
    },
    check_login: true,
    current_user: {
      email: "hello@sufe.example",
      balance: 5000,
      commission_balance: 1200,
      plan_id: 2,
      expired_at: Math.floor(Date.now() / 1000) + 86400 * 28,
      uuid: null,
      avatar_url: null,
    },
    current_subscribe: {
      plan_id: 2,
      token: "preview-only",
      expired_at: Math.floor(Date.now() / 1000) + 86400 * 28,
      u: 4.5 * GiB,
      d: 58.3 * GiB,
      transfer_enable: 300 * GiB,
      subscribe_url: "",
      reset_day: 18,
    },
    fetch_site_config: {
      tos_url: "",
      is_email_verify: true,
      is_invite_force: false,
      email_whitelist_suffix: [],
      is_captcha: false,
      captcha_type: "",
      app_description: "让连接，自由一点。",
      app_url: "",
      logo: "",
    },
    fetch_client_config: {
      brand_name: "Sufe",
      features: {
        purchase: true,
        recharge: false,
        gift_card: true,
        checkin: false,
        notice: true,
        invite: true,
        tickets: true,
        chatwoot: false,
        custom_rules: true,
      },
      chatwoot_base_url: null,
      chatwoot_inbox_identifier: null,
      config_source: "界面预览",
      transport_mode: "preview",
      api_candidates: 0,
    },
    preview_subscribe_nodes: names.map((name, i) => ({
      name,
      kind: i % 3 === 0 ? "hysteria2" : "trojan",
      server: `preview-${i}.invalid`,
      port: 443,
    })),
    proxies: [
      { name: "选择节点", type: "Selector", now: selected, all: names },
    ],
    fetch_plans: plans,
    fetch_orders: [],
    fetch_tickets: [],
    fetch_notices: [
      {
        id: 1,
        title: "欢迎来到 Sufe，开启自由连接",
        content:
          "选择适合你的套餐，一键连接全球精选线路。遇到问题，随时通过帮助中心联系我们。",
        tags: ["使用指南"],
        img_url: null,
        created_at: Math.floor(Date.now() / 1000),
        updated_at: null,
      },
      {
        id: 2,
        title: "让每一次连接，都简单可靠",
        content:
          "在「节点」中收藏常用线路；在「分流规则」中为工作与生活选择合适的连接方式。",
        tags: ["小贴士"],
        img_url: null,
        created_at: Math.floor(Date.now() / 1000) - 86400,
        updated_at: null,
      },
    ],
    fetch_payment_methods: [
      {
        id: 1,
        name: "支付宝",
        payment: "alipay",
        icon: null,
        handling_fee_fixed: 0,
        handling_fee_percent: 0,
      },
      {
        id: 2,
        name: "微信支付",
        payment: "wechat",
        icon: null,
        handling_fee_fixed: 0,
        handling_fee_percent: 0,
      },
    ],
    connection_state: connection,
    kernel_health: {
      tun_supported: false,
      mihomo_present: true,
      mihomo_path: "界面预览 / 未运行内核",
      helper_present: false,
      helper_path: null,
      work_dir: "界面预览",
    },
    app_version: "0.1.0",
    core_version: "0.1.0",
    proxy_guard_enabled: true,
    kernel_version: {
      version: null,
      raw: "预览模式不运行内核",
      mihomo_path: "",
    },
    helper_status: { supported: false, installed: false, reachable: false },
    rules: [
      { type: "GeoSite", payload: "cn", proxy: "DIRECT" },
      { type: "GeoIP", payload: "CN", proxy: "DIRECT" },
      { type: "Match", payload: "", proxy: "选择节点" },
    ],
    connections: [],
    fetch_custom_rules: customRules,
    tail_kernel_log: "",
    gift_card_history: [],
    fetch_invites: { codes: [], stat: [0, 0, 0, 0, 0] },
  };
  if (command in read) return structuredClone(read[command]);
  if (command === "connect" || command === "reconnect") {
    connection = {
      kind: "connecting",
      stage: "fetching",
      mode: mode as "system_proxy",
    };
    emit("xboard://connection-state", connection);
    await new Promise((r) => setTimeout(r, 500));
    connection = {
      kind: "connected",
      since: new Date().toISOString(),
      mode: mode as "system_proxy",
      mixed_port: 7890,
    };
    emit("xboard://connection-state", connection);
    return connection;
  }
  if (command === "disconnect") {
    connection = { kind: "disconnected" };
    emit("xboard://connection-state", connection);
    return connection;
  }
  if (command === "current_traffic") {
    tick++;
    return {
      up: (Math.sin(tick * 0.6) + 1.2) * 84000,
      down: (Math.sin(tick * 0.32) + 1.6) * 1600000,
      up_total: tick * 84000,
      down_total: tick * 2600000,
    };
  }
  if (command === "latency_test")
    return [28, 36, 61, 72, 49, 168, 192, 184][
      Math.max(0, names.indexOf(String(args.name)))
    ];
  if (command === "select_proxy") {
    selected = String(args.name);
    return;
  }
  if (command === "set_tunnel_mode") {
    mode = String(args.mode);
    return;
  }
  if (command === "set_proxy_guard_enabled") return;
  if (command === "save_custom_rules") {
    customRules = structuredClone(args.rules as unknown[]);
    return;
  }
  if (command === "logout") {
    emit("xboard://session-expired", null);
    return;
  }
  throw {
    kind: "preview_only",
    message: "这是界面预览，此操作需要在客户端连接真实账户后完成。",
  };
}
