import { invoke } from "@tauri-apps/api/core";
import type {
  CheckoutResponse,
  ConnectionState,
  HelperStatus,
  KernelHealth,
  KernelVersion,
  LoginSummary,
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

  login: (args: {
    email: string;
    password: string;
    turnstile?: string;
    recaptcha?: string;
  }) => invoke<LoginSummary>("login", args),

  register: (args: {
    email: string;
    password: string;
    emailCode: string;
    inviteCode?: string;
    turnstile?: string;
    recaptcha?: string;
  }) => invoke<LoginSummary>("register", args),

  sendEmailVerify: (email: string) =>
    invoke<void>("send_email_verify", { email }),

  forgetPassword: (args: {
    email: string;
    password: string;
    emailCode: string;
    turnstile?: string;
    recaptcha?: string;
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
  currentTraffic: () => invoke<TrafficStats>("current_traffic"),

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
