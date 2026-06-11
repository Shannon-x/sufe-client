import { invoke } from "@tauri-apps/api/core";
import type {
  CheckoutResponse,
  ConnectionItem,
  ConnectionState,
  CouponCheckResult,
  RuleItem,
  HelperStatus,
  KernelHealth,
  KernelVersion,
  LoginSummary,
  NodeGeo,
  NodePreview,
  Notice,
  Order,
  PaymentMethod,
  Plan,
  ProxyGroup,
  SiteConfig,
  SubscribeInfo,
  Ticket,
  TicketDetail,
  TrafficStats,
  TunnelMode,
  UserInfo,
} from "./types";

export const api = {
  appVersion: () => invoke<string>("app_version"),
  coreVersion: () => invoke<string>("core_version"),

  // `captchaType` is the provider tag from guest/comm/config.captcha_type
  // ("turnstile" | "recaptcha" | "recaptcha-v3"); the Rust side routes
  // `captchaToken` into the exact field the panel's CaptchaService reads
  // (turnstile_token / recaptcha_v3_token / recaptcha_data).
  login: (args: {
    email: string;
    password: string;
    captchaType?: string;
    captchaToken?: string;
  }) => invoke<LoginSummary>("login", args),

  register: (args: {
    email: string;
    password: string;
    emailCode: string;
    inviteCode?: string;
    captchaType?: string;
    captchaToken?: string;
  }) => invoke<LoginSummary>("register", args),

  // sendEmailVerify is captcha-gated on the backend (register / forget).
  // Pass the provider tag + resolved token so the panel validates it.
  sendEmailVerify: (email: string, captchaType?: string, captchaToken?: string) =>
    invoke<void>("send_email_verify", { email, captchaType, captchaToken }),

  forgetPassword: (args: {
    email: string;
    password: string;
    emailCode: string;
    captchaType?: string;
    captchaToken?: string;
  }) => invoke<void>("forget_password", args),

  // Returns null when no snapshot exists or the backend pointer changed.
  hydrateSession: () => invoke<LoginSummary | null>("hydrate_session"),

  // false ⇒ token rejected and session was wiped (a `xboard://session-expired`
  // event has already been emitted). true ⇒ either the token is still valid
  // or we couldn't reach the backend but were last validated <24h ago.
  checkLogin: () => invoke<boolean>("check_login"),

  fetchSiteConfig: () => invoke<SiteConfig>("fetch_site_config"),

  currentUser: () => invoke<UserInfo>("current_user"),
  currentSubscribe: () => invoke<SubscribeInfo>("current_subscribe"),

  logout: () => invoke<void>("logout"),

  // Kernel / connection
  connect: () => invoke<ConnectionState>("connect"),
  disconnect: () => invoke<ConnectionState>("disconnect"),
  connectionState: () => invoke<ConnectionState>("connection_state"),
  setTunnelMode: (mode: TunnelMode) =>
    invoke<void>("set_tunnel_mode", { mode }),
  proxies: () => invoke<ProxyGroup[]>("proxies"),
  selectProxy: (group: string, name: string) =>
    invoke<void>("select_proxy", { group, name }),
  latencyTest: (name: string) => invoke<number>("latency_test", { name }),
  nodeGeoTest: (group: string, name: string) =>
    invoke<NodeGeo>("node_geo_test", { group, name }),
  resolveNodeGeoBatch: () =>
    invoke<Record<string, NodeGeo>>("resolve_node_geo_batch"),
  currentTraffic: () => invoke<TrafficStats>("current_traffic"),
  // Observability (Connections / Rules pages).
  connections: () => invoke<ConnectionItem[]>("connections"),
  closeConnection: (id: string) => invoke<void>("close_connection", { id }),
  closeAllConnections: () => invoke<void>("close_all_connections"),
  rules: () => invoke<RuleItem[]>("rules"),
  // Reliability.
  reconnect: () => invoke<ConnectionState>("reconnect"),
  setProxyGuardEnabled: (enabled: boolean) =>
    invoke<void>("set_proxy_guard_enabled", { enabled }),
  proxyGuardEnabled: () => invoke<boolean>("proxy_guard_enabled"),
  /// Pulls + parses the Clash YAML directly from the user's subscribe URL,
  /// returning `{ name, kind, server, port }` for every entry. No kernel
  /// touched — used to populate the country/node sidebar before the user
  /// has ever pressed "connect".
  previewSubscribeNodes: () =>
    invoke<NodePreview[]>("preview_subscribe_nodes"),

  // Read-only diagnostics — neither call spawns mihomo.
  kernelHealth: () => invoke<KernelHealth>("kernel_health"),
  kernelVersion: () => invoke<KernelVersion>("kernel_version"),
  tailKernelLog: (maxBytes?: number) =>
    invoke<string>("tail_kernel_log", { maxBytes }),

  // macOS LaunchDaemon management. `helperStatus` is read-only; the
  // install/uninstall calls each pop a single admin auth dialog and
  // resolve once the daemon is loaded / removed.
  helperStatus: () => invoke<HelperStatus>("helper_status"),
  helperInstall: () => invoke<void>("helper_install"),
  helperUninstall: () => invoke<void>("helper_uninstall"),

  // User center — read-only surfaces.
  fetchNotices: () => invoke<Notice[]>("fetch_notices"),
  fetchPlans: () => invoke<Plan[]>("fetch_plans"),
  fetchOrders: () => invoke<Order[]>("fetch_orders"),

  // Purchase flow. `saveOrder` returns a `trade_no`; we then `checkoutOrder`
  // with the user's chosen `PaymentMethod.id`. The CheckoutResponse `type`
  // tells the UI what to do next (open URL, show QR, balance settled, etc).
  // `checkOrder` is polled by the UI while the user pays externally.
  fetchPaymentMethods: () =>
    invoke<PaymentMethod[]>("fetch_payment_methods"),
  saveOrder: (args: {
    planId: number;
    period: string;
    couponCode?: string | null;
  }) =>
    invoke<string>("save_order", {
      planId: args.planId,
      period: args.period,
      couponCode: args.couponCode ?? null,
    }),
  checkoutOrder: (tradeNo: string, method: number) =>
    invoke<CheckoutResponse>("checkout_order", { tradeNo, method }),
  // Returns the raw `status` integer (0 pending, 1 activating, 3 completed…).
  checkOrder: (tradeNo: string) => invoke<number>("check_order", { tradeNo }),
  // Validates a coupon against a plan. Throws on invalid/expired/wrong-plan
  // codes — the UI should catch and surface the message inline rather than
  // toasting. `value` semantics depend on `type`: 1 = cents off, 2 = percent.
  checkCoupon: (code: string, planId: number) =>
    invoke<CouponCheckResult>("check_coupon", { code, planId }),
  cancelOrder: (tradeNo: string) => invoke<void>("cancel_order", { tradeNo }),

  // Tickets — read for free, reply / close are gated on `status === 0`.
  fetchTickets: () => invoke<Ticket[]>("fetch_tickets"),
  fetchTicket: (id: number) => invoke<TicketDetail>("fetch_ticket", { id }),
  replyTicket: (id: number, message: string) =>
    invoke<void>("reply_ticket", { id, message }),
  closeTicket: (id: number) => invoke<void>("close_ticket", { id }),
  // Returns the new ticket id when the backend reveals it; some forks only
  // emit `true` and we then surface `null` to the caller.
  saveTicket: (args: { subject: string; level: number; message: string }) =>
    invoke<number | null>("save_ticket", args),
};
