// Mirror of Rust shapes returned by tauri::commands. Rust uses snake_case by
// default and we keep that shape on the wire — it's the lingua franca of the
// rest of the codebase.

export interface LoginSummary {
  email: string;
  is_admin: boolean;
  subscribe_token: string;
}

export interface UserInfo {
  email: string;
  balance: number;
  commission_balance: number;
  plan_id: number | null;
  expired_at: number | null;
  uuid: string | null;
  avatar_url: string | null;
}

export interface SubscribeInfo {
  plan_id: number | null;
  token: string;
  expired_at: number | null;
  u: number;
  d: number;
  transfer_enable: number;
  subscribe_url: string;
  reset_day: number | null;
}

export interface CommandError {
  kind: string;
  message: string;
  status?: number;
}

export function isCommandError(e: unknown): e is CommandError {
  return (
    typeof e === "object" &&
    e !== null &&
    "kind" in e &&
    "message" in e &&
    typeof (e as { kind: unknown }).kind === "string"
  );
}

// --- Public site config (`/api/v1/guest/comm/config`) ---

// Captcha provider strings reported by the backend. UI should treat any
// other value as "captcha required but provider unsupported" and refuse
// to silently skip — see `CaptchaWidget.vue`.
export type CaptchaType =
  | ""
  | "recaptcha"
  | "recaptcha-v3"
  | "turnstile"
  | (string & {});

export interface SiteConfig {
  tos_url: string;
  is_email_verify: boolean;
  is_invite_force: boolean;
  email_whitelist_suffix: string[];
  is_captcha: boolean;
  captcha_type: CaptchaType;
  recaptcha_site_key: string;
  recaptcha_v3_site_key: string;
  // `null` ⇒ use reCAPTCHA-recommended default. The Rust core widens this
  // to `Option<f32>` because admins can store non-numeric junk in the
  // backing `admin_setting` row, which is unparseable.
  recaptcha_v3_score_threshold: number | null;
  turnstile_site_key: string;
  is_recaptcha: boolean;
  app_description: string;
  app_url: string;
  logo: string;
}

export interface CheckLoginResp {
  is_login: boolean;
  is_admin: boolean;
}

// --- Kernel / connection ---

export type TunnelMode = "tun" | "system_proxy";

export type ConnectStage =
  | "fetching"
  | "elevating"
  | "writing"
  | "spawning"
  | "applying_route"
  | "fallback_proxy";

export type ConnectionState =
  | { kind: "disconnected" }
  | { kind: "connecting"; stage: ConnectStage; mode: TunnelMode }
  | { kind: "connected"; since: string; mode: TunnelMode; mixed_port: number }
  | { kind: "error"; message: string; mode: TunnelMode };

export interface TrafficStats {
  up: number;
  down: number;
  up_total: number;
  down_total: number;
}

// Mirrors `xboard_core::kernel::ConnectionItem` — one active connection from
// the kernel's `/connections`, flattened for the Connections monitor table.
export interface ConnectionItem {
  id: string;
  upload: number;
  download: number;
  start: string;
  chains: string[];
  rule: string;
  rule_payload: string;
  network: string;
  conn_type: string;
  host: string;
  source_ip: string;
  destination_ip: string;
  destination_port: string;
  process: string;
}

// Mirrors `xboard_core::kernel::RuleItem`.
export interface RuleItem {
  type: string;
  payload: string;
  proxy: string;
}

// Mirrors `xboard_core::kernel::LogLine`, streamed over `connection://log`.
export interface LogLine {
  level: string;
  message: string;
  at: string;
}

// Mirrors `commands::nodes::NodePreview` — the subscribe-only view used by
// the sidebar before any kernel is running.
export interface NodePreview {
  name: string;
  kind: string;
  server: string;
  port: number;
}

// Mirrors `xboard_core::kernel::ProxyGroup`. The backend renames `kind` to
// `type` on the wire (serde rename) — keep that shape here.
export interface ProxyGroup {
  name: string;
  type: string;
  now?: string | null;
  all: string[];
}

export interface NodeGeo {
  node: string;
  ip: string;
  country: string;
  city: string;
  lat: number;
  lon: number;
  isp?: string | null;
  source: string;
}

// --- Kernel diagnostics ---

// Pre-flight check returned by `kernel_health`. `helper_*` semantics depend
// on the host:
//   - macOS: helper_present = LaunchDaemon installed
//   - Linux: helper_present = mihomo has cap_net_admin via deb/rpm setcap;
//            false on AppImage (no postinst hook ran)
//   - Windows: both null until xboard-svc lands (M2)
// The Vue layer reads `helper_present === false` as "no privileged path
// available" and picks a platform-appropriate hint body accordingly.
export interface KernelHealth {
  mihomo_present: boolean;
  mihomo_path: string;
  helper_present: boolean | null;
  helper_path: string | null;
  work_dir: string;
}

// `kernel_version` — best-effort parse of `mihomo -v`. `version` is null
// when the output schema changed; UI should fall back to displaying `raw`.
export interface KernelVersion {
  version: string | null;
  raw: string;
  mihomo_path: string;
}

// `helper_status` — macOS-only helper diagnostic. On Linux/Windows
// `supported` is false and the UI hides the management panel entirely.
export interface HelperStatus {
  supported: boolean;
  installed: boolean;
  reachable: boolean;
  helper_path: string | null;
  plist_path: string | null;
}

// --- Notices ---

// Mirror of `xboard_core::api::Notice`. `content` is staff-authored HTML —
// callers MUST render it as plain text (no v-html) for the same reason
// `app_description` is plain-text only on the login page.
export interface Notice {
  id: number;
  title: string;
  content: string;
  img_url: string | null;
  tags: string[];
  created_at: number | null;
  updated_at: number | null;
}

// --- Plans + orders ---

// Mirror of `xboard_core::api::Plan`. Money fields are **cents**. A null
// `*_price` means the panel admin disabled that billing cadence — UI should
// hide the row, not render "免费" / "free".
export interface Plan {
  id: number;
  name: string;
  content: string;
  group_id: number | null;
  type: number | null;
  transfer_enable: number;

  month_price: number | null;
  quarter_price: number | null;
  half_year_price: number | null;
  year_price: number | null;
  two_year_price: number | null;
  three_year_price: number | null;
  onetime_price: number | null;

  reset_price: number | null;
  reset_traffic_method: number | null;

  show: boolean;
  sell: boolean;
  renew: boolean;

  sort: number | null;
  created_at: number | null;
  updated_at: number | null;
}

// Mirror of `xboard_core::api::PaymentMethod`. The client always passes
// `id` to `/checkout` — `payment` (driver name) is informational.
export interface PaymentMethod {
  id: number;
  name: string;
  payment: string;
  icon: string | null;
  handling_fee_fixed: number | null;
  handling_fee_percent: number | null;
}

// Mirror of `xboard_core::api::CheckoutResponse`.
//   type === -1 → balance settled the order; nothing else to do
//   type ===  1 → redirect URL — open `data` (string) in browser
//   type ===  0 → QR code — `data` is image URL or QR content string
//   type === -2 → gateway-specific (e.g. Stripe form). Best-effort: open
//                 `data` if it's a URL, else show raw payload.
export interface CheckoutResponse {
  type: number;
  data: unknown;
}

// Mirror of `xboard_core::api::CouponCheckResult`. Returned by
// `api.checkCoupon(code, planId)` after a successful panel validation —
// invalid / expired / wrong-plan codes throw `CommandError` instead.
//   type === 1 → fixed amount off; `value` is cents.
//   type === 2 → percent off;       `value` is an integer in 0..=100.
export interface CouponCheckResult {
  id: number;
  code: string;
  type: number;
  value: number | null;
}

// Mirror of `xboard_core::api::Order`. `status` semantics:
//   0 = pending payment, 1 = activating, 2 = cancelled, 3 = completed,
//   4 = discounted / credited.
// `type` semantics: 1 = new, 2 = renew, 3 = upgrade, 4 = traffic reset.
export interface Order {
  id: number;
  trade_no: string;
  plan_id: number | null;
  period: string | null;
  type: number | null;
  status: number;
  commission_status: number | null;

  total_amount: number;
  balance_amount: number | null;
  discount_amount: number | null;
  surplus_amount: number | null;
  refund_amount: number | null;

  created_at: number | null;
  updated_at: number | null;
}

// --- Tickets ---

// Mirror of `xboard_core::api::Ticket` (list item) and `TicketDetail` /
// `TicketMessage` (single thread).
//   level        — 0 low, 1 normal, 2 high
//   status       — 0 open, 1 closed
//   reply_status — opaque-ish flag indicating who is on the hook to reply
//                  next; semantics drift across panel forks so use it only
//                  for sort hints, not for strict "you must reply" gating.
export interface Ticket {
  id: number;
  level: number;
  reply_status: number;
  status: number;
  subject: string;
  last_reply_user_id: number | null;
  created_at: number | null;
  updated_at: number | null;
}

export interface TicketMessage {
  id: number;
  ticket_id: number;
  user_id: number | null;
  message: string;
  is_me: boolean;
  created_at: number | null;
}

export interface TicketDetail {
  id: number;
  level: number;
  reply_status: number;
  status: number;
  subject: string;
  message: TicketMessage[];
  created_at: number | null;
  updated_at: number | null;
}
