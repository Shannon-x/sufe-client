<script setup lang="ts">
// Thin grid orchestrator for the home page. Splits screen into 4 regions
// (top bar / left rail / map / right icon rail) and wires each to the
// pre-existing stores + Tauri commands. State machines (auth refresh,
// auto-connect, helper / kernel / log modals, updater) all live here so
// the leaf components stay declarative.
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  NAlert,
  NButton,
  NEmpty,
  NModal,
  NScrollbar,
  NSpace,
  NSpin,
  NTag,
  NText,
  useDialog,
  useMessage,
} from "naive-ui";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { platform } from "@tauri-apps/plugin-os";
import type { DropdownOption } from "naive-ui";

import { useAuthStore } from "@/stores/auth";
import { useConnectionStore } from "@/stores/connection";
import { usePlanStore } from "@/stores/plan";
import { useNodesStore } from "@/stores/nodes";
import { api } from "@/api";
import { formatError } from "@/utils/error";
import { locateNode, type GeoPoint } from "@/utils/geo";
import type {
  ConnectStage,
  HelperStatus,
  KernelHealth,
  KernelVersion,
  NodeGeo,
  ProxyGroup,
  TunnelMode,
} from "@/types";

import WorldMap, { type NodePin } from "@/components/WorldMap.vue";
import HomeTopBar, { type ConnectionPillKind } from "@/components/HomeTopBar.vue";
import StatusHero, { type HeroState } from "@/components/StatusHero.vue";
import NodeRail from "@/components/NodeRail.vue";
import HomeIconRail, { type RailAction } from "@/components/HomeIconRail.vue";

const { t } = useI18n();
const router = useRouter();
const message = useMessage();
const dialog = useDialog();
const auth = useAuthStore();
const connection = useConnectionStore();
const planStore = usePlanStore();
const nodesStore = useNodesStore();

const loading = ref(true);
const connectBusy = ref(false);
const selecting = ref<string | null>(null);
const geoTesting = ref<string | null>(null);
const nodeGeo = ref<Record<string, NodeGeo>>({});
const hostPlatform = ref<string>("");
const health = ref<KernelHealth | null>(null);
const worldMapRef = ref<InstanceType<typeof WorldMap> | null>(null);

// Modal state
const showLogModal = ref(false);
const logText = ref("");
const logLoading = ref(false);
const showHelperModal = ref(false);
const helperStatus = ref<HelperStatus | null>(null);
const helperBusy = ref(false);
const showKernelInfoModal = ref(false);
const kernelVersion = ref<KernelVersion | null>(null);
const kernelInfoLoading = ref(false);
const checkingUpdate = ref(false);

// ────────────────── state derivations ──────────────────
// Reactive "now" — refreshed once a minute so cue card / canConnect re-evaluate
// without forcing a full page reload as a subscription rolls over the
// expiry boundary or crosses the 7-day expiring threshold.
const nowSec = ref(Math.floor(Date.now() / 1000));
let nowTimer: number | null = null;

const SEVEN_DAYS_SEC = 7 * 24 * 60 * 60;

// Tri-state subscription summary derived from auth.subscribe. `null` plan_id
// means the user never bought a plan; a finite `expired_at` in the past means
// it lapsed. Unlimited subs have expired_at === null *and* a plan_id.
const subscriptionStatus = computed<
  "none" | "expired" | "expiring" | "trafficLow" | "healthy"
>(() => {
  const sub = auth.subscribe;
  if (!sub || sub.plan_id == null) return "none";
  const exp = sub.expired_at;
  if (exp != null) {
    if (exp <= nowSec.value) return "expired";
    if (exp - nowSec.value <= SEVEN_DAYS_SEC) return "expiring";
  }
  // Healthy window — flag traffic at >95% usage so the user gets a heads-up
  // before the kernel starts dropping packets.
  const cap = sub.transfer_enable;
  if (cap && cap > 0) {
    const used = (sub.u ?? 0) + (sub.d ?? 0);
    if (used / cap >= 0.95) return "trafficLow";
  }
  return "healthy";
});

// "May the user attempt a connect right now?" — gate the big CTA on a real
// subscription. Expiring/trafficLow are still OK (kernel will still serve).
const canConnect = computed(
  () =>
    subscriptionStatus.value !== "none" &&
    subscriptionStatus.value !== "expired",
);

const cueExpiryLabel = computed(() => {
  const exp = auth.subscribe?.expired_at;
  if (!exp) return "";
  try {
    return new Date(exp * 1000).toLocaleDateString();
  } catch {
    return "";
  }
});

const cueCard = computed<
  | { tone: "warning" | "error" | "info"; title: string; body: string; cta: string }
  | null
>(() => {
  switch (subscriptionStatus.value) {
    case "none":
      return {
        tone: "warning",
        title: t("home.cue.noneTitle"),
        body: t("home.cue.noneBody"),
        cta: t("home.cue.cta.browse"),
      };
    case "expired":
      return {
        tone: "error",
        title: t("home.cue.expiredTitle"),
        body: t("home.cue.expiredBody"),
        cta: t("home.cue.cta.renew"),
      };
    case "expiring":
      return {
        tone: "warning",
        title: t("home.cue.expiringTitle"),
        body: t("home.cue.expiringBody", { date: cueExpiryLabel.value }),
        cta: t("home.cue.cta.renew"),
      };
    case "trafficLow":
      return {
        tone: "info",
        title: t("home.cue.trafficLowTitle"),
        body: t("home.cue.trafficLowBody"),
        cta: t("home.cue.cta.upgrade"),
      };
    default:
      return null;
  }
});

function onCueCta() {
  router.push({ name: "plans" });
}

const heroState = computed<HeroState>(() => {
  const k = connection.state.kind;
  if (k === "connected") return "connected";
  if (k === "connecting") return "connecting";
  if (k === "error") return "error";
  return "disconnected";
});

const connectionErrorMessage = computed(() => {
  const s = connection.state;
  return s.kind === "error" ? s.message : "";
});

const pillKind = computed<ConnectionPillKind>(() => heroState.value);

const statusLabel = computed(() => {
  const s = connection.state;
  switch (s.kind) {
    case "disconnected":
      return "未受保护";
    case "connected":
      return "已保护";
    case "error":
      return t("connect.status.error", { message: s.message });
    case "connecting":
      return t(`connect.status.connecting.${s.stage satisfies ConnectStage}`);
  }
});

const stateLabel = computed(() => {
  switch (heroState.value) {
    case "connected": return "已保护";
    case "connecting": return "正在连接";
    case "error": return "连接失败";
    default: return "未受保护";
  }
});

const modeOptions = computed<Array<{ label: string; value: TunnelMode }>>(() => [
  { label: t("connect.mode.tun"), value: "tun" },
  { label: t("connect.mode.system_proxy"), value: "system_proxy" },
]);

const selectableGroups = computed<ProxyGroup[]>(() =>
  connection.proxies.filter(
    (g) => g.type === "Selector" || g.type === "URLTest" || g.type === "Fallback",
  ),
);

const sidecarMissing = computed(() => health.value !== null && !health.value.mihomo_present);
const helperMissing = computed(() => health.value?.helper_present === false);
const helperMissingTitle = computed(() => {
  if (hostPlatform.value === "linux") return t("connect.health.helperMissingLinux");
  if (hostPlatform.value === "windows") return t("connect.health.helperMissingWindows");
  return t("connect.health.helperMissing");
});
const helperMissingBody = computed(() => {
  if (hostPlatform.value === "linux") return t("connect.health.helperMissingBodyLinux");
  if (hostPlatform.value === "windows") return t("connect.health.helperMissingBodyWindows");
  return t("connect.health.helperMissingBody");
});

// ────────────────── geo / hero copy ──────────────────
function geoToPoint(geo: NodeGeo): GeoPoint {
  return {
    label: [geo.city, geo.country].filter(Boolean).join(", ") || geo.ip,
    country: geo.country || geo.ip,
    flag: "●",
    lat: geo.lat,
    lon: geo.lon,
  };
}
function locationForNode(name: string): GeoPoint | null {
  return nodeGeo.value[name] ? geoToPoint(nodeGeo.value[name]) : locateNode(name);
}

const activeNodeName = computed(() => connection.effectiveProxy ?? connection.currentProxy);
const activeGeoInfo = computed<NodeGeo | null>(() =>
  activeNodeName.value ? (nodeGeo.value[activeNodeName.value] ?? null) : null,
);
const heroPrimary = computed(() => {
  if (heroState.value === "connected") {
    return activeNodeName.value ?? t("connect.nodes.fastestServer");
  }
  // Disconnected / connecting: prefer the exit-node IP if we have it, fall
  // back to the chosen node name, then "—". Gives the card *something* to
  // show even when nothing is hooked up yet.
  return activeGeoInfo.value?.ip ?? activeNodeName.value ?? "—";
});
const heroSecondary = computed(() => {
  const loc = activeNodeName.value ? locationForNode(activeNodeName.value) : null;
  if (heroState.value === "connected" && loc) {
    return `${loc.country} · ${loc.label}`;
  }
  if (heroState.value === "connecting") return "正在建立加密隧道…";
  return "当前网络未加密，建议立即连接";
});
const heroCta = computed(() => {
  if (heroState.value === "connected") return "断开连接";
  if (heroState.value === "connecting") return "正在连接";
  return "快速连接";
});

// ────────────────── map pins ──────────────────
const mapPins = computed<NodePin[]>(() => {
  const byLocation = new globalThis.Map<string, NodePin>();
  const activeNode = connection.effectiveProxy;
  const activeSelector = connection.currentProxy;
  const names = [
    ...nodesStore.sidebarNodes.map((n) => n.name),
    ...selectableGroups.value.flatMap((g) => g.all),
  ];
  if (activeSelector) names.unshift(activeSelector);
  if (activeNode) names.unshift(activeNode);

  const seen = new Set<string>();
  for (const node of names) {
    if (seen.has(node)) continue;
    seen.add(node);
    const loc = locationForNode(node);
    if (!loc) continue;
    const key = `${loc.lat.toFixed(3)},${loc.lon.toFixed(3)}`;
    const isActive = node === activeNode || node === activeSelector;
    const current = byLocation.get(key);
    if (current) {
      current.count += 1;
      current.active = current.active || isActive;
      continue;
    }
    const geo = nodeGeo.value[node];
    byLocation.set(key, {
      id: key,
      lat: loc.lat,
      lon: loc.lon,
      label: loc.label,
      country: loc.country,
      ip: geo?.ip,
      count: 1,
      active: isActive,
    });
  }
  return [...byLocation.values()]
    .sort((a, b) => Number(b.active) - Number(a.active) || b.count - a.count)
    .slice(0, 40);
});

function findNodeAtLocation(lat: number, lon: number): { group: string; node: string } | null {
  const key = `${lat.toFixed(3)},${lon.toFixed(3)}`;
  for (const g of selectableGroups.value) {
    for (const node of g.all) {
      const loc = locationForNode(node);
      if (!loc) continue;
      if (`${loc.lat.toFixed(3)},${loc.lon.toFixed(3)}` === key) {
        return { group: g.name, node };
      }
    }
  }
  return null;
}

function onMapPinClick(id: string) {
  const pin = mapPins.value.find((p) => p.id === id);
  if (!pin) return;
  worldMapRef.value?.flyTo(pin.lat, pin.lon, 3.2);
  if (pin.count === 1) {
    const target = findNodeAtLocation(pin.lat, pin.lon);
    if (target) void selectProxy(target.group, target.node);
  }
}

// ────────────────── connection actions ──────────────────
async function toggleConnection() {
  if (connectBusy.value) return;
  connectBusy.value = true;
  try {
    if (connection.isConnected) await connection.disconnect();
    else await connection.connect();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    connectBusy.value = false;
  }
}

async function selectProxy(group: string, name: string) {
  if (selecting.value) return;
  selecting.value = `${group}::${name}`;
  try {
    await connection.selectProxy(group, name);
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    selecting.value = null;
  }
}

async function onSelectFromRail(nodeName: string) {
  // Kernel idle → spin up + leave it on its default selector. The user can
  // re-pick once we land.
  if (!connection.isConnected && !connection.isBusy) {
    await toggleConnection();
    return;
  }
  const owning = selectableGroups.value.find((g) => g.all.includes(nodeName));
  if (!owning) {
    await connection.refreshProxies();
    const retry = selectableGroups.value.find((g) => g.all.includes(nodeName));
    if (!retry) {
      message.warning(t("connect.nodes.notInGroup", { name: nodeName }));
      return;
    }
    await selectProxy(retry.name, nodeName);
    return;
  }
  await selectProxy(owning.name, nodeName);
}

async function onModeChange(next: TunnelMode) {
  const reconnecting = connection.isConnected && next !== connection.currentMode;
  try {
    if (reconnecting) message.info(t("connect.modeReconnect"));
    await connection.setMode(next);
  } catch (e) {
    message.error(formatError(e, t));
  }
}

// One-shot toast when KernelManager silently downgraded TUN → system_proxy.
watch(
  () => connection.wasDowngraded,
  (now, prev) => { if (now && !prev) message.warning(t("connect.downgraded")); },
);

// Silent per-node geo lookup once a connection lands on a fresh node.
watch(
  () => [connection.isConnected, connection.primaryGroup?.name, connection.currentProxy] as const,
  ([connected, group, node]) => {
    if (!connected || !group || !node || nodeGeo.value[node] || geoTesting.value) return;
    void testNodeGeo(group, node);
  },
);

let didBatchGeo = false;
async function refreshGeoBatch() {
  try {
    const map = await api.resolveNodeGeoBatch();
    nodeGeo.value = { ...nodeGeo.value, ...map };
    if (Object.keys(map).length > 0) await connection.refreshProxies();
  } catch (e) {
    console.warn("resolveNodeGeoBatch failed", e);
  }
}
watch(
  () => selectableGroups.value.length,
  (n) => {
    if (n > 0 && !didBatchGeo) {
      didBatchGeo = true;
      void refreshGeoBatch();
    }
  },
);

async function testNodeGeo(group: string, name: string) {
  if (!connection.isConnected || geoTesting.value) return;
  geoTesting.value = `${group}::${name}`;
  try {
    const geo = await api.nodeGeoTest(group, name);
    nodeGeo.value = { ...nodeGeo.value, [name]: geo };
    await connection.refreshProxies();
  } catch {
    // best-effort
  } finally {
    geoTesting.value = null;
  }
}

// ────────────────── top-bar account menu ──────────────────
const accountMenu = computed<DropdownOption[]>(() => [
  { label: t("home.menu.plans"), key: "plans" },
  { label: t("home.menu.orders"), key: "orders" },
  { label: t("home.menu.tickets"), key: "tickets" },
  { label: t("home.menu.notices"), key: "notices" },
  { type: "divider", key: "d1" },
  { label: `${t("home.copy")} ${t("home.subscribe")}`, key: "copy_subscribe" },
  { label: t("home.menu.helper"), key: "helper" },
  { label: t("home.menu.kernelInfo"), key: "kernel_info" },
  { label: t("home.menu.checkUpdate"), key: "check_update" },
  { type: "divider", key: "d2" },
  { label: t("home.logout"), key: "logout", props: { style: "color: var(--n-error-color, #d03050)" } },
]);

async function copySubscribeUrl() {
  const url = auth.subscribe?.subscribe_url;
  if (!url) {
    message.warning(t("home.notSubscribed"));
    return;
  }
  try {
    await navigator.clipboard.writeText(url);
    message.success(t("home.copied"));
  } catch (e) {
    message.error(formatError(e, t));
  }
}

function onAccountSelect(key: string) {
  switch (key) {
    case "plans": router.push({ name: "plans" }); break;
    case "orders": router.push({ name: "orders" }); break;
    case "tickets": router.push({ name: "tickets" }); break;
    case "notices": router.push({ name: "notices" }); break;
    case "copy_subscribe": void copySubscribeUrl(); break;
    case "helper": void openHelperPanel(); break;
    case "kernel_info": void openKernelInfoPanel(); break;
    case "check_update": void onCheckUpdate(); break;
    case "logout": void onLogout(); break;
  }
}

function onRailAction(key: RailAction) {
  switch (key) {
    case "refresh": void refresh(); void nodesStore.refresh(); break;
    case "connections": router.push({ name: "connections" }); break;
    case "logs": router.push({ name: "logs" }); break;
    case "rules": router.push({ name: "rules" }); break;
    case "notices": router.push({ name: "notices" }); break;
    case "plans": router.push({ name: "plans" }); break;
    case "tickets": router.push({ name: "tickets" }); break;
    case "helper": void openHelperPanel(); break;
    case "settings": router.push({ name: "settings" }); break;
  }
}

// ────────────────── modals / async actions ──────────────────
async function openLogModal() {
  showLogModal.value = true;
  await refreshLog();
}
async function refreshLog() {
  logLoading.value = true;
  try { logText.value = await api.tailKernelLog(); }
  catch (e) { logText.value = formatError(e, t); }
  finally { logLoading.value = false; }
}

async function loadHelperStatus() {
  try { helperStatus.value = await api.helperStatus(); }
  catch (e) { message.error(formatError(e, t)); }
}
async function openHelperPanel() {
  showHelperModal.value = true;
  await loadHelperStatus();
}
async function onHelperInstall() {
  if (helperBusy.value) return;
  helperBusy.value = true;
  try {
    await api.helperInstall();
    message.success(t("helper.installed"));
    await loadHelperStatus();
    void refreshHealth();
  } catch (e) { message.error(formatError(e, t)); }
  finally { helperBusy.value = false; }
}
async function onHelperUninstall() {
  if (helperBusy.value) return;
  dialog.warning({
    title: t("helper.confirmUninstallTitle"),
    content: t("helper.confirmUninstallBody"),
    positiveText: t("helper.uninstallNow"),
    negativeText: t("helper.cancel"),
    onPositiveClick: async () => {
      helperBusy.value = true;
      try {
        if (connection.isConnected) await connection.disconnect();
        await api.helperUninstall();
        message.success(t("helper.uninstalled"));
        await loadHelperStatus();
        void refreshHealth();
      } catch (e) { message.error(formatError(e, t)); }
      finally { helperBusy.value = false; }
    },
  });
}

async function openKernelInfoPanel() {
  showKernelInfoModal.value = true;
  if (kernelVersion.value !== null) return;
  kernelInfoLoading.value = true;
  try { kernelVersion.value = await api.kernelVersion(); }
  catch (e) { message.error(formatError(e, t)); }
  finally { kernelInfoLoading.value = false; }
}

async function onCheckUpdate() {
  if (checkingUpdate.value) return;
  checkingUpdate.value = true;
  try {
    const update = await check();
    if (!update) { message.success(t("updater.upToDate")); return; }
    dialog.warning({
      title: t("updater.availableTitle", { version: update.version }),
      content: update.body || t("updater.availableBody"),
      positiveText: t("updater.installNow"),
      negativeText: t("updater.later"),
      onPositiveClick: async () => {
        try { await update.downloadAndInstall(); await relaunch(); }
        catch (e) { message.error(formatError(e, t)); }
      },
    });
  } catch (e) { message.error(formatError(e, t)); }
  finally { checkingUpdate.value = false; }
}

async function refreshHealth() {
  try { health.value = await api.kernelHealth(); }
  catch (e) { console.warn("kernel_health failed", e); }
}

async function refresh() {
  loading.value = true;
  try {
    const results = await Promise.allSettled([
      auth.refreshUser(),
      auth.refreshSubscribe(),
      connection.hydrate(),
      planStore.ensure(),
    ]);
    const legs = ["auth", "subscribe", "connection", "plan"] as const;
    for (let i = 0; i < results.length; i++) {
      const r = results[i];
      if (r.status !== "rejected") continue;
      const reason = r.reason;
      const leg = legs[i];
      if (leg === "auth") {
        // Only force a logout when the auth-refresh leg explicitly rejects with
        // an Unauthorized error. Network blips / other errors should surface as
        // a toast, not log the user out.
        if (
          typeof reason === "object" &&
          reason !== null &&
          "kind" in reason &&
          (reason as { kind: string }).kind === "unauthorized"
        ) {
          message.error(formatError(reason, t));
          await auth.logout();
          router.push({ name: "login" });
          return;
        }
        message.error(formatError(reason, t));
      } else {
        // One leg's failure shouldn't mask the others — just log.
        console.warn(`refresh leg "${leg}" failed`, reason);
      }
    }
  } finally {
    loading.value = false;
  }
}

async function onLogout() {
  if (connection.isConnected) {
    try { await connection.disconnect(); } catch { /* best-effort */ }
  }
  await auth.logout();
  router.push({ name: "login" });
}

// ────────────────── lifecycle ──────────────────
const AUTO_CONNECT_AFTER_LOGIN_KEY = "xboard.autoConnectAfterLogin";
const TRAY_HINT_KEY = "xboard.trayHintShown";
const HEALTH_POLL_MS = 30_000;
let unlistenTrayHint: UnlistenFn | null = null;
let healthPollId: number | null = null;

const AUTO_CONNECT_ON_START_KEY = "xboard.autoConnectOnStart";

async function autoConnectAfterLogin() {
  if (sessionStorage.getItem(AUTO_CONNECT_AFTER_LOGIN_KEY) !== "1") return;
  sessionStorage.removeItem(AUTO_CONNECT_AFTER_LOGIN_KEY);
  if (connection.isConnected || connection.isBusy) return;
  // Gate: never silently auto-connect a user who has no plan / an expired
  // plan. The hero CTA is disabled in that case and the cue card explains why,
  // so kicking off a doomed connect attempt would just produce a confusing
  // error toast. Manual click only — same policy as the gated button.
  if (!canConnect.value) return;
  connectBusy.value = true;
  try {
    message.info(t("connect.autoConnecting"));
    await connection.connect();
    message.success(t("connect.autoConnected"));
  } catch (e) { message.error(formatError(e, t)); }
  finally { connectBusy.value = false; }
}

// "Auto-connect on launch" preference (Settings toggle) — covers the common
// "returning user reopens the app" case the after-login hook doesn't, while
// still respecting the same `canConnect` gate so we never auto-connect an
// expired account into a doomed attempt.
async function maybeAutoConnectOnStart() {
  if (localStorage.getItem(AUTO_CONNECT_ON_START_KEY) !== "1") return;
  if (connection.isConnected || connection.isBusy) return;
  if (!canConnect.value) return;
  connectBusy.value = true;
  try {
    await connection.connect();
  } catch {
    // Silent: launch-time auto-connect failures shouldn't nag with a toast.
  } finally {
    connectBusy.value = false;
  }
}

onMounted(async () => {
  await refresh();
  void nodesStore.refresh();
  await autoConnectAfterLogin();
  await maybeAutoConnectOnStart();
  void refreshHealth();
  // Keep the sidecar/helper presence banners honest if the user installs the
  // helper or the mihomo binary appears mid-session — re-poll every 30s.
  healthPollId = window.setInterval(() => void refreshHealth(), HEALTH_POLL_MS);
  // Re-evaluate cue-card / canConnect once a minute so the expiring → expired
  // boundary doesn't require a manual refresh to take effect.
  nowTimer = window.setInterval(() => {
    nowSec.value = Math.floor(Date.now() / 1000);
  }, 60_000);
  try { hostPlatform.value = await platform(); } catch { hostPlatform.value = ""; }
  unlistenTrayHint = await listen("xboard://hidden-to-tray", () => {
    if (localStorage.getItem(TRAY_HINT_KEY) === "1") return;
    message.info(t("connect.hiddenToTray"), { duration: 5000 });
    localStorage.setItem(TRAY_HINT_KEY, "1");
  });
});

onBeforeUnmount(() => {
  void connection.dispose();
  if (healthPollId !== null) { clearInterval(healthPollId); healthPollId = null; }
  if (nowTimer !== null) { clearInterval(nowTimer); nowTimer = null; }
  if (unlistenTrayHint) { unlistenTrayHint(); unlistenTrayHint = null; }
});
</script>

<template>
  <div class="home-shell">
    <HomeTopBar
      :app-title="t('app.title')"
      :pill-kind="pillKind"
      :status-label="statusLabel"
      :ip-label="activeGeoInfo?.ip ?? null"
      :location-label="activeGeoInfo ? [activeGeoInfo.country, activeGeoInfo.city].filter(Boolean).join(' / ') : null"
      :email="auth.session?.email ?? null"
      :menu-options="accountMenu"
      @menu-select="onAccountSelect"
    />

    <div class="workspace">
      <aside class="left-rail">
        <NAlert
          v-if="sidecarMissing"
          type="error"
          :show-icon="true"
          :title="t('connect.health.binaryMissing')"
          class="health-alert"
        >
          {{ t("connect.health.binaryMissingBody", { path: health?.mihomo_path ?? "" }) }}
        </NAlert>
        <NAlert
          v-else-if="helperMissing"
          :type="hostPlatform === 'linux' ? 'warning' : 'info'"
          :show-icon="true"
          :title="helperMissingTitle"
          class="health-alert"
        >
          {{ helperMissingBody }}
        </NAlert>

        <StatusHero
          :state="heroState"
          :state-label="stateLabel"
          :primary-line="heroPrimary"
          :secondary-line="heroSecondary"
          :cta-label="heroCta"
          :busy="connection.isBusy || connectBusy"
          :can-connect="canConnect"
          :gated-hint="t('home.cue.noneTitle')"
          :error-message="connectionErrorMessage"
          :retry-label="t('connect.retry')"
          :view-logs-label="t('connect.viewLogs')"
          :mode="connection.currentMode"
          :mode-options="modeOptions"
          @toggle-connection="toggleConnection"
          @change-mode="onModeChange"
          @view-logs="openLogModal"
        />

        <NodeRail
          :active-node-name="activeNodeName"
          :switching="!!selecting"
          :connect-busy="connectBusy || connection.isBusy"
          :is-connected="connection.isConnected"
          @select-node="onSelectFromRail"
          @fastest="toggleConnection"
          @refresh="nodesStore.refresh()"
        />
      </aside>

      <main class="map-stage">
        <div
          v-if="cueCard"
          class="home-cue-card"
          :class="`home-cue-card--${cueCard.tone}`"
          role="status"
        >
          <div class="home-cue-card__text">
            <p class="home-cue-card__title">{{ cueCard.title }}</p>
            <p class="home-cue-card__body">{{ cueCard.body }}</p>
          </div>
          <button
            type="button"
            class="home-cue-card__cta"
            @click="onCueCta"
          >
            {{ cueCard.cta }}
          </button>
        </div>

        <WorldMap
          ref="worldMapRef"
          class="map-canvas"
          :pins="mapPins"
          @pin-click="onMapPinClick"
        />
      </main>

      <HomeIconRail @action="onRailAction" />
    </div>

    <!-- Log modal -->
    <NModal
      v-model:show="showLogModal"
      preset="card"
      :title="t('connect.logModal.title')"
      style="max-width: 760px"
      :bordered="false"
      size="huge"
    >
      <template #header-extra>
        <NButton size="small" :loading="logLoading" @click="refreshLog">
          {{ t("connect.logModal.refresh") }}
        </NButton>
      </template>
      <NScrollbar style="max-height: 60vh">
        <pre v-if="logText" class="log-pre">{{ logText }}</pre>
        <NEmpty v-else :description="t('connect.logModal.empty')" />
      </NScrollbar>
      <NText v-if="health" depth="3" class="log-path">
        {{ health.work_dir }}/mihomo.log
      </NText>
    </NModal>

    <!-- Helper modal -->
    <NModal
      v-model:show="showHelperModal"
      preset="card"
      :title="t('helper.title')"
      style="max-width: 560px"
      :bordered="false"
      size="huge"
    >
      <template #header-extra>
        <NButton size="small" @click="loadHelperStatus">{{ t("home.refresh") }}</NButton>
      </template>
      <NSpace v-if="!helperStatus" justify="center" style="padding: 24px 0">
        <NSpin />
      </NSpace>
      <div v-else-if="!helperStatus.supported">
        <NAlert type="info" :show-icon="false">{{ t("helper.unsupported") }}</NAlert>
      </div>
      <div v-else class="helper-panel">
        <NSpace align="center" :size="8">
          <NTag :type="helperStatus.installed ? 'success' : 'warning'" :bordered="false">
            {{ helperStatus.installed ? t("helper.tag.installed") : t("helper.tag.notInstalled") }}
          </NTag>
          <NTag
            v-if="helperStatus.installed"
            :type="helperStatus.reachable ? 'success' : 'error'"
            :bordered="false"
          >
            {{ helperStatus.reachable ? t("helper.tag.reachable") : t("helper.tag.unreachable") }}
          </NTag>
        </NSpace>
        <NText depth="3" class="helper-tip">
          {{ helperStatus.installed ? t("helper.bodyInstalled") : t("helper.bodyMissing") }}
        </NText>
        <div v-if="helperStatus.helper_path" class="helper-paths">
          <NText depth="3">{{ helperStatus.helper_path }}</NText>
          <NText v-if="helperStatus.plist_path" depth="3">{{ helperStatus.plist_path }}</NText>
        </div>
        <NSpace>
          <NButton type="primary" :loading="helperBusy" @click="onHelperInstall">
            {{ helperStatus.installed ? t("helper.reinstall") : t("helper.installNow") }}
          </NButton>
          <NButton v-if="helperStatus.installed" :loading="helperBusy" @click="onHelperUninstall">
            {{ t("helper.uninstall") }}
          </NButton>
        </NSpace>
      </div>
    </NModal>

    <!-- Kernel info modal -->
    <NModal
      v-model:show="showKernelInfoModal"
      preset="card"
      :title="t('kernelInfo.title')"
      style="max-width: 560px"
      :bordered="false"
      size="huge"
    >
      <NSpace v-if="kernelInfoLoading" justify="center" style="padding: 24px 0">
        <NSpin />
      </NSpace>
      <div v-else-if="kernelVersion" class="kernel-info">
        <NSpace align="center" :size="8">
          <NTag type="success" :bordered="false">
            {{ kernelVersion.version || t("kernelInfo.unknownVersion") }}
          </NTag>
        </NSpace>
        <NText depth="3" class="helper-tip">{{ t("kernelInfo.bundledHint") }}</NText>
        <pre class="log-pre">{{ kernelVersion.raw }}</pre>
        <NText depth="3" class="log-path">{{ kernelVersion.mihomo_path }}</NText>
      </div>
      <NEmpty v-else :description="t('kernelInfo.empty')" />
    </NModal>
  </div>
</template>

<style scoped>
.home-shell {
  /* CSS Grid: 56px top bar across the top, then workspace below. The shell
     itself owns the viewport — html/body/#app are already overflow:hidden in
     global.css, so we never need to fight a page-level scrollbar. */
  display: grid;
  grid-template-rows: 56px 1fr;
  height: 100vh;
  width: 100%;
  overflow: hidden;
  color: #f8f7ff;
  background: #15121f;
}

.workspace {
  display: grid;
  grid-template-columns: 340px 1fr 84px;
  min-height: 0;
  height: 100%;
  background:
    radial-gradient(circle at 48% 20%, rgba(98, 70, 180, 0.20), transparent 32%),
    linear-gradient(135deg, #171320 0%, #221c30 50%, #14111a 100%);
}

.left-rail {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: 0;
  padding: 16px 14px;
  background:
    linear-gradient(180deg, rgba(155, 115, 255, 0.06), rgba(155, 115, 255, 0)),
    rgba(18, 15, 25, 0.92);
  border-right: 1px solid rgba(255, 255, 255, 0.06);
  overflow: hidden;
}

.health-alert {
  margin: 0;
}

.map-stage {
  position: relative;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  background: #0d0a1f;
}
.map-canvas {
  position: absolute;
  inset: 0;
}

/* Cue card overlays the top of the map. It's the first thing the user sees
   when their plan needs attention — title + body + single CTA, no chrome. */
.home-cue-card {
  position: absolute;
  top: 16px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 5;
  display: flex;
  align-items: center;
  gap: 16px;
  max-width: min(720px, calc(100% - 32px));
  padding: 12px 16px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 12px;
  background: rgba(24, 20, 36, 0.92);
  backdrop-filter: blur(6px);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.32);
  color: #f1ecff;
}
.home-cue-card--warning {
  border-color: rgba(255, 209, 102, 0.45);
  box-shadow: 0 12px 32px rgba(255, 209, 102, 0.18);
}
.home-cue-card--error {
  border-color: rgba(255, 90, 122, 0.5);
  box-shadow: 0 12px 32px rgba(255, 90, 122, 0.22);
}
.home-cue-card--info {
  border-color: rgba(139, 92, 246, 0.42);
  box-shadow: 0 12px 32px rgba(139, 92, 246, 0.2);
}
.home-cue-card__text {
  flex: 1 1 auto;
  min-width: 0;
}
.home-cue-card__title {
  margin: 0;
  font-size: 13.5px;
  font-weight: 700;
  letter-spacing: 0.01em;
}
.home-cue-card__body {
  margin: 4px 0 0;
  color: #b8b2c7;
  font-size: 12px;
  line-height: 1.5;
}
.home-cue-card__cta {
  flex: 0 0 auto;
  height: 32px;
  padding: 0 14px;
  border: 0;
  border-radius: 8px;
  background: linear-gradient(135deg, #8b5cf6, #6d28d9);
  color: #fff;
  font: inherit;
  font-size: 12.5px;
  font-weight: 700;
  letter-spacing: 0.02em;
  cursor: pointer;
  transition: filter 160ms ease, transform 120ms ease;
}
.home-cue-card__cta:hover { filter: brightness(1.08); }
.home-cue-card__cta:active { transform: translateY(1px); }

/* Modal-internal helpers (kept inline so the templated content stays compact) */
.log-pre {
  margin: 0;
  padding: 12px;
  background: rgba(127, 127, 127, 0.08);
  border-radius: 6px;
  font-family: ui-monospace, "SFMono-Regular", Menlo, monospace;
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}
.log-path {
  display: block;
  margin-top: 10px;
  font-size: 11px;
  font-family: ui-monospace, "SFMono-Regular", Menlo, monospace;
  opacity: 0.65;
}
.helper-panel,
.kernel-info,
.helper-paths {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.helper-tip {
  font-size: 12px;
  line-height: 1.55;
}
.helper-paths {
  gap: 4px;
  font-family: ui-monospace, "SFMono-Regular", Menlo, monospace;
  font-size: 11px;
}

/* Make sure the maplibre attribution & naive scroll containers don't paint
   a stray light band against our dark canvas. */
.home-shell :deep(.n-scrollbar),
.home-shell :deep(.n-scrollbar-container),
.home-shell :deep(.n-scrollbar-content) {
  background-color: transparent !important;
}
</style>
