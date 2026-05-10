<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  NAlert,
  NLayout,
  NLayoutHeader,
  NLayoutContent,
  NCard,
  NSpace,
  NButton,
  NDropdown,
  NModal,
  NStatistic,
  NTag,
  NProgress,
  NText,
  NDivider,
  NEmpty,
  NSkeleton,
  NSelect,
  NSwitch,
  NCollapseTransition,
  NScrollbar,
  NSpin,
  useDialog,
  useMessage,
} from "naive-ui";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { platform } from "@tauri-apps/plugin-os";
import type { DropdownOption } from "naive-ui";
import { useAuthStore } from "@/stores/auth";
import { useThemeStore } from "@/stores/theme";
import { useConnectionStore } from "@/stores/connection";
import { usePlanStore } from "@/stores/plan";
import { formatError } from "@/utils/error";
import { api } from "@/api";
import type {
  ConnectStage,
  HelperStatus,
  KernelHealth,
  KernelVersion,
  ProxyGroup,
  TunnelMode,
} from "@/types";

const { t } = useI18n();
const router = useRouter();
const auth = useAuthStore();
const theme = useThemeStore();
const connection = useConnectionStore();
const planStore = usePlanStore();
const message = useMessage();
const dialog = useDialog();

const loading = ref(true);
const connectBusy = ref(false);
const showNodes = ref(false);
const refreshingGroup = ref<string | null>(null);
const selecting = ref<string | null>(null);
// Per-node latency cache. Refreshed lazily on group expand or via the
// "test all" button. -1 sentinel means "tested but timed out".
const latency = reactive<Record<string, number>>({});

const health = ref<KernelHealth | null>(null);
const showLogModal = ref(false);
const logText = ref("");
const logLoading = ref(false);

// Critical: bundled mihomo missing — almost certainly a corrupted install.
const sidecarMissing = computed(
  () => health.value !== null && !health.value.mihomo_present,
);
// Soft hint. Meaning depends on platform:
//   - macOS: helper LaunchDaemon hasn't been installed yet (first connect
//     will trigger an admin auth prompt)
//   - Linux: bundled mihomo lacks cap_net_admin (AppImage / dev build) —
//     TUN will fall back to system-proxy until the user installs deb/rpm
const helperMissing = computed(() => health.value?.helper_present === false);
const hostPlatform = ref<string>("");
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
const inErrorState = computed(() => connection.state.kind === "error");

const modeOptions = computed(() => [
  { label: t("connect.mode.tun"), value: "tun" satisfies TunnelMode },
  { label: t("connect.mode.system_proxy"), value: "system_proxy" satisfies TunnelMode },
]);

const statusLabel = computed(() => {
  const s = connection.state;
  switch (s.kind) {
    case "disconnected":
      return t("connect.status.disconnected");
    case "connected":
      return t("connect.status.connected");
    case "error":
      return t("connect.status.error", { message: s.message });
    case "connecting":
      return t(`connect.status.connecting.${s.stage satisfies ConnectStage}`);
  }
});

const connectionPillType = computed<"success" | "warning" | "error" | "default">(() => {
  const k = connection.state.kind;
  if (k === "connected") return "success";
  if (k === "connecting") return "warning";
  if (k === "error") return "error";
  return "default";
});

const switchValue = computed({
  get: () => connection.isConnected,
  set: () => {
    /* clicks dispatch to toggleConnection() instead */
  },
});

// Selectable groups only — Direct/Reject/etc. show nothing useful for the user.
const selectableGroups = computed<ProxyGroup[]>(() =>
  connection.proxies.filter(
    (g) => g.type === "Selector" || g.type === "URLTest" || g.type === "Fallback",
  ),
);

async function toggleConnection(value: boolean) {
  if (connectBusy.value) return;
  connectBusy.value = true;
  try {
    if (value) {
      await connection.connect();
    } else {
      await connection.disconnect();
    }
  } catch (e) {
    // KernelManager auto-downgrades TUN→system_proxy on the consent /
    // service-missing / not-permitted paths, so those LauncherError variants
    // never surface here. Anything that reaches this catch is a real failure
    // (kernel start timeout, IPC error, …) — show it raw.
    message.error(formatError(e, t));
  } finally {
    connectBusy.value = false;
  }
}

// One-shot toast when the kernel quietly downgraded the user's TUN request
// to system_proxy. The watch guards on a false→true transition so toggling
// the switch off and back on doesn't re-toast unless the downgrade reoccurs.
watch(
  () => connection.wasDowngraded,
  (now, prev) => {
    if (now && !prev) {
      message.warning(t("connect.downgraded"));
    }
  },
);

// Retry from the error pill. Reuses toggleConnection so the same busy guard
// + error handling apply — keeps the path the user takes from "switch off,
// switch on" identical to "tap retry".
async function retryConnect() {
  await toggleConnection(true);
}

async function onModeChange(next: TunnelMode) {
  // setMode auto-reconnects when currently connected (so the new mode actually
  // takes effect). Surface that so the user understands why the connection
  // pill briefly drops to "connecting".
  const reconnecting = connection.isConnected && next !== connection.currentMode;
  try {
    if (reconnecting) {
      message.info(t("connect.modeReconnect"));
    }
    await connection.setMode(next);
  } catch (e) {
    message.error(formatError(e, t));
  }
}

async function refreshHealth() {
  try {
    health.value = await api.kernelHealth();
  } catch (e) {
    // Banner is purely advisory — silently swallow.
    console.warn("kernel_health failed", e);
  }
}

async function openLogModal() {
  showLogModal.value = true;
  await refreshLog();
}

async function refreshLog() {
  logLoading.value = true;
  try {
    logText.value = await api.tailKernelLog();
  } catch (e) {
    logText.value = formatError(e, t);
  } finally {
    logLoading.value = false;
  }
}

async function toggleNodes() {
  showNodes.value = !showNodes.value;
  if (showNodes.value && connection.proxies.length === 0 && connection.isConnected) {
    await connection.refreshProxies();
  }
}

async function selectNode(group: string, name: string) {
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

async function testGroupLatency(group: ProxyGroup) {
  if (refreshingGroup.value) return;
  refreshingGroup.value = group.name;
  // Run probes concurrently but cap to 8 in flight so we don't slam the kernel.
  const queue = [...group.all];
  const inflight: Promise<void>[] = [];
  const runOne = async (name: string) => {
    try {
      const ms = await api.latencyTest(name);
      latency[name] = ms;
    } catch {
      latency[name] = -1;
    }
  };
  while (queue.length > 0 || inflight.length > 0) {
    while (inflight.length < 8 && queue.length > 0) {
      const name = queue.shift()!;
      const p = runOne(name).finally(() => {
        const idx = inflight.indexOf(p);
        if (idx >= 0) inflight.splice(idx, 1);
      });
      inflight.push(p);
    }
    if (inflight.length > 0) await Promise.race(inflight);
  }
  refreshingGroup.value = null;
}

function latencyText(name: string): string {
  const v = latency[name];
  if (v === undefined) return "—";
  if (v < 0 || v >= 5000) return t("connect.nodes.timeout");
  return `${v} ms`;
}

function latencyTone(name: string): "success" | "warning" | "error" | "default" {
  const v = latency[name];
  if (v === undefined) return "default";
  if (v < 0) return "error";
  if (v < 200) return "success";
  if (v < 600) return "warning";
  return "error";
}

function fmtSpeed(bytesPerSec: number): string {
  if (!bytesPerSec) return "0 B/s";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bytesPerSec;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

async function refresh() {
  loading.value = true;
  try {
    await Promise.all([
      auth.refreshUser(),
      auth.refreshSubscribe(),
      connection.hydrate(),
      // Best-effort plan catalog warm-up so the cards can show plan names
      // instead of bare numeric ids. Failures are non-fatal — the UI falls
      // back to `#<id>` automatically.
      planStore.ensure().catch(() => {}),
    ]);
  } catch (e) {
    message.error(formatError(e, t));
    if (
      typeof e === "object" &&
      e !== null &&
      "kind" in e &&
      (e as { kind: string }).kind === "unauthorized"
    ) {
      await auth.logout();
      router.push({ name: "login" });
    }
  } finally {
    loading.value = false;
  }
}

async function onLogout() {
  if (connection.isConnected) {
    try {
      await connection.disconnect();
    } catch {
      // best-effort
    }
  }
  await auth.logout();
  router.push({ name: "login" });
}

// Header account menu — keeps the inline buttons short while adding room for
// future user-center entries (tickets, invitations, …) without re-cluttering
// the bar.
const accountMenu = computed<DropdownOption[]>(() => [
  { label: t("home.menu.plans"), key: "plans" },
  { label: t("home.menu.orders"), key: "orders" },
  { label: t("home.menu.tickets"), key: "tickets" },
  { label: t("home.menu.notices"), key: "notices" },
  { type: "divider", key: "d1" },
  { label: t("home.menu.helper"), key: "helper" },
  { label: t("home.menu.kernelInfo"), key: "kernel_info" },
  { label: t("home.menu.checkUpdate"), key: "check_update" },
  { type: "divider", key: "d2" },
  { label: t("home.logout"), key: "logout", props: { style: "color: var(--n-error-color, #d03050)" } },
]);

function onAccountSelect(key: string) {
  switch (key) {
    case "plans":
      router.push({ name: "plans" });
      break;
    case "orders":
      router.push({ name: "orders" });
      break;
    case "tickets":
      router.push({ name: "tickets" });
      break;
    case "notices":
      router.push({ name: "notices" });
      break;
    case "helper":
      void openHelperPanel();
      break;
    case "kernel_info":
      void openKernelInfoPanel();
      break;
    case "check_update":
      void onCheckUpdate();
      break;
    case "logout":
      void onLogout();
      break;
  }
}

// Manual update entry. tauri-plugin-updater fetches `latest.json` from the
// configured endpoint, verifies the bundle's ed25519 signature, and only
// then surfaces an `Update` object — so by the time we render the dialog
// the artifact has already been authenticated. We keep the install path
// behind an explicit user confirm because `relaunch()` quits the app.
const checkingUpdate = ref(false);
async function onCheckUpdate() {
  if (checkingUpdate.value) return;
  checkingUpdate.value = true;
  try {
    const update = await check();
    if (!update) {
      message.success(t("updater.upToDate"));
      return;
    }
    dialog.warning({
      title: t("updater.availableTitle", { version: update.version }),
      content: update.body || t("updater.availableBody"),
      positiveText: t("updater.installNow"),
      negativeText: t("updater.later"),
      onPositiveClick: async () => {
        try {
          await update.downloadAndInstall();
          await relaunch();
        } catch (e) {
          message.error(formatError(e, t));
        }
      },
    });
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    checkingUpdate.value = false;
  }
}

// Helper-service management (macOS only). The backend hides the panel by
// reporting `supported: false` on Linux/Windows; the menu entry is still
// surfaced uniformly so users on those platforms see why it's empty.
const showHelperModal = ref(false);
const helperStatus = ref<HelperStatus | null>(null);
const helperBusy = ref(false);
async function loadHelperStatus() {
  try {
    helperStatus.value = await api.helperStatus();
  } catch (e) {
    message.error(formatError(e, t));
  }
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
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    helperBusy.value = false;
  }
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
        if (connection.isConnected) {
          await connection.disconnect();
        }
        await api.helperUninstall();
        message.success(t("helper.uninstalled"));
        await loadHelperStatus();
        void refreshHealth();
      } catch (e) {
        message.error(formatError(e, t));
      } finally {
        helperBusy.value = false;
      }
    },
  });
}

// Kernel info modal — shows the bundled mihomo version + path. Auto-update
// of the kernel rides with the app updater, so we deliberately do not
// expose a "check for kernel update" button here.
const showKernelInfoModal = ref(false);
const kernelVersion = ref<KernelVersion | null>(null);
const kernelInfoLoading = ref(false);
async function openKernelInfoPanel() {
  showKernelInfoModal.value = true;
  if (kernelVersion.value !== null) return;
  kernelInfoLoading.value = true;
  try {
    kernelVersion.value = await api.kernelVersion();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    kernelInfoLoading.value = false;
  }
}

async function onCopySubscribe() {
  if (!auth.subscribe) return;
  await navigator.clipboard.writeText(auth.subscribe.subscribe_url);
  message.success(t("home.copied"));
}

// Backend emits this the first time it intercepts the main window's close
// (closing now hides to tray). The localStorage flag keeps the toast a
// one-shot — repeating it every close would just be noise.
const TRAY_HINT_KEY = "xboard.trayHintShown";
let unlistenTrayHint: UnlistenFn | null = null;

onMounted(async () => {
  void refresh();
  void refreshHealth();
  // Best-effort: pick the helper-missing copy variant. Failure here just
  // keeps the default (mac) wording, which is fine — the alert only shows
  // when helper_present is false anyway, which is platform-correlated.
  try {
    hostPlatform.value = await platform();
  } catch {
    hostPlatform.value = "";
  }
  unlistenTrayHint = await listen("xboard://hidden-to-tray", () => {
    if (localStorage.getItem(TRAY_HINT_KEY) === "1") return;
    message.info(t("connect.hiddenToTray"), { duration: 5000 });
    localStorage.setItem(TRAY_HINT_KEY, "1");
  });
});
onBeforeUnmount(() => {
  void connection.dispose();
  if (unlistenTrayHint) {
    unlistenTrayHint();
    unlistenTrayHint = null;
  }
});

const yuan = (cents: number) => (cents / 100).toFixed(2);
const trafficUsed = computed(() => {
  const s = auth.subscribe;
  if (!s) return 0;
  return s.u + s.d;
});
const trafficPct = computed(() => {
  const s = auth.subscribe;
  if (!s || !s.transfer_enable) return 0;
  return Math.min(100, (trafficUsed.value / s.transfer_enable) * 100);
});

function fmtBytes(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(2)} ${units[i]}`;
}

function fmtExpiry(ts: number | null | undefined): string {
  if (!ts) return t("home.expiryNever");
  return new Date(ts * 1000).toLocaleString();
}

// Friendly plan name for the user info card. Falls back to `#<id>` (or
// "—" when the user has no plan yet) so a slow / failed plan-catalog
// fetch never leaves the card looking broken.
const planLabel = computed(() => {
  const id = auth.userInfo?.plan_id ?? null;
  if (id == null) return "—";
  return planStore.nameFor(id) ?? `#${id}`;
});

// Subscription state hint — drives a small CTA panel that links the user
// to the right page (Plans for new/expiring, Plans for upgrade when
// traffic-bound). Intentionally tier-based instead of a bunch of booleans
// so the template only has to switch on one value.
type SubState = "none" | "expired" | "expiringSoon" | "trafficLow" | "ok";
const SEVEN_DAYS_S = 7 * 24 * 60 * 60;
const subState = computed<SubState>(() => {
  const s = auth.subscribe;
  if (!s) return "none";
  const nowS = Math.floor(Date.now() / 1000);
  if (s.expired_at !== null && s.expired_at !== undefined) {
    if (s.expired_at <= nowS) return "expired";
    if (s.expired_at - nowS <= SEVEN_DAYS_S) return "expiringSoon";
  }
  if (s.transfer_enable > 0 && trafficUsed.value / s.transfer_enable >= 0.95) {
    return "trafficLow";
  }
  return "ok";
});

const subStateMeta = computed<{
  type: "default" | "info" | "success" | "warning" | "error";
  title: string;
  body: string;
  ctaLabel: string;
} | null>(() => {
  switch (subState.value) {
    case "none":
      return {
        type: "info",
        title: t("home.cue.noneTitle"),
        body: t("home.cue.noneBody"),
        ctaLabel: t("home.cue.cta.browse"),
      };
    case "expired":
      return {
        type: "error",
        title: t("home.cue.expiredTitle"),
        body: t("home.cue.expiredBody"),
        ctaLabel: t("home.cue.cta.renew"),
      };
    case "expiringSoon":
      return {
        type: "warning",
        title: t("home.cue.expiringTitle"),
        body: t("home.cue.expiringBody", {
          date: fmtExpiry(auth.subscribe?.expired_at),
        }),
        ctaLabel: t("home.cue.cta.renew"),
      };
    case "trafficLow":
      return {
        type: "warning",
        title: t("home.cue.trafficLowTitle"),
        body: t("home.cue.trafficLowBody"),
        ctaLabel: t("home.cue.cta.upgrade"),
      };
    default:
      return null;
  }
});

function onCueAction() {
  router.push({ name: "plans" });
}

const trafficBarStatus = computed<"default" | "warning" | "error">(() => {
  if (subState.value === "trafficLow") return "error";
  if (trafficPct.value >= 80) return "warning";
  return "default";
});
</script>

<template>
  <NLayout class="home-shell">
    <NLayoutHeader bordered class="home-header">
      <div class="brand">
        <span class="brand-mark" />
        <NText strong class="brand-text">{{ t("app.title") }}</NText>
        <NTag :type="connectionPillType" size="small" round class="brand-pill">
          <span class="status-dot" :class="{ pulse: connection.isBusy }" />
          {{ statusLabel }}
        </NTag>
      </div>
      <NSpace :size="8" align="center">
        <NButton size="small" quaternary @click="theme.toggle">
          {{ theme.dark ? "☀︎" : "☾" }}
        </NButton>
        <NButton size="small" quaternary :loading="loading" @click="refresh">
          {{ t("home.refresh") }}
        </NButton>
        <NDropdown
          trigger="click"
          :options="accountMenu"
          :show-arrow="true"
          placement="bottom-end"
          @select="onAccountSelect"
        >
          <NButton size="small" quaternary class="account-btn">
            <span class="header-email">{{ auth.session?.email ?? "" }}</span>
            <span class="caret">▾</span>
          </NButton>
        </NDropdown>
      </NSpace>
    </NLayoutHeader>

    <NLayoutContent class="home-content">
      <div class="grid">
        <NAlert
          v-if="sidecarMissing"
          type="error"
          :show-icon="true"
          :title="t('connect.health.binaryMissing')"
          class="health-alert"
        >
          {{
            t("connect.health.binaryMissingBody", {
              path: health?.mihomo_path ?? "",
            })
          }}
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

        <NCard
          class="connect-card"
          :class="{ 'is-connected': connection.isConnected }"
          :title="t('connect.title')"
          embedded
        >
          <NSpace vertical :size="14">
            <NSpace align="center" justify="space-between" style="width: 100%">
              <NSpace align="center" :size="10">
                <NText depth="3" style="font-size: 12px">
                  {{ t("connect.modeLabel") }}
                </NText>
                <NSelect
                  :value="connection.currentMode"
                  :options="modeOptions"
                  :consistent-menu-width="false"
                  size="small"
                  style="width: 168px"
                  @update:value="onModeChange"
                />
              </NSpace>
              <NSpace align="center" :size="8">
                <NText depth="3" class="status-line">{{ statusLabel }}</NText>
                <NButton
                  v-if="inErrorState"
                  size="tiny"
                  type="primary"
                  :loading="connectBusy"
                  @click="retryConnect"
                >
                  {{ t("connect.retry") }}
                </NButton>
                <NButton
                  v-if="inErrorState"
                  size="tiny"
                  type="error"
                  ghost
                  @click="openLogModal"
                >
                  {{ t("connect.viewLogs") }}
                </NButton>
              </NSpace>
            </NSpace>

            <div class="connect-toggle">
              <NSwitch
                size="large"
                :value="switchValue"
                :loading="connection.isBusy || connectBusy"
                @update:value="toggleConnection"
              >
                <template #checked>{{ t("connect.button.connected") }}</template>
                <template #unchecked>{{ t("connect.button.disconnected") }}</template>
              </NSwitch>
            </div>

            <NSpace align="center" justify="space-between" style="width: 100%">
              <div class="current-node">
                <NText depth="3" style="font-size: 12px; display: block">
                  {{ t("connect.currentNode") }}
                </NText>
                <NText strong>{{ connection.currentProxy ?? "—" }}</NText>
              </div>
              <NButton
                size="small"
                :type="showNodes ? 'primary' : 'default'"
                :ghost="!showNodes"
                :disabled="!connection.isConnected"
                @click="toggleNodes"
              >
                {{ t("connect.button.nodes") }}
                <span style="margin-left: 4px">{{ showNodes ? "▴" : "▾" }}</span>
              </NButton>
            </NSpace>

            <div v-if="connection.isConnected" class="traffic-line">
              <span class="traffic-arrow">↑</span>
              <span class="traffic-value">{{ fmtSpeed(connection.traffic.up) }}</span>
              <span class="traffic-arrow">↓</span>
              <span class="traffic-value">{{ fmtSpeed(connection.traffic.down) }}</span>
            </div>
          </NSpace>

          <NCollapseTransition :show="showNodes">
            <div class="nodes-panel">
              <NDivider style="margin: 14px 0" />
              <div v-if="!connection.isConnected">
                <NEmpty :description="t('connect.nodes.needsConnect')" />
              </div>
              <div v-else-if="selectableGroups.length === 0">
                <NSpin v-if="connection.proxies.length === 0" size="small">
                  <NText depth="3">{{ t("connect.nodes.loading") }}</NText>
                </NSpin>
                <NEmpty v-else :description="t('connect.nodes.empty')" />
              </div>
              <NScrollbar v-else style="max-height: 360px">
                <div class="groups">
                  <div v-for="g in selectableGroups" :key="g.name" class="group">
                    <div class="group-head">
                      <div>
                        <NText strong>{{ g.name }}</NText>
                        <NTag size="tiny" :bordered="false" style="margin-left: 8px">
                          {{ g.type }}
                        </NTag>
                      </div>
                      <NButton
                        size="tiny"
                        quaternary
                        :loading="refreshingGroup === g.name"
                        @click="testGroupLatency(g)"
                      >
                        {{ t("connect.nodes.testAll") }}
                      </NButton>
                    </div>
                    <div class="members">
                      <button
                        v-for="member in g.all"
                        :key="member"
                        type="button"
                        class="member"
                        :class="{
                          'is-current': member === g.now,
                          'is-selecting': selecting === `${g.name}::${member}`,
                        }"
                        :disabled="!!selecting"
                        @click="selectNode(g.name, member)"
                      >
                        <span class="member-name">{{ member }}</span>
                        <span
                          class="member-latency"
                          :class="`tone-${latencyTone(member)}`"
                        >
                          {{ latencyText(member) }}
                        </span>
                      </button>
                    </div>
                  </div>
                </div>
              </NScrollbar>
            </div>
          </NCollapseTransition>
        </NCard>

        <NCard :title="t('home.welcome', { email: auth.session?.email ?? '' })" embedded>
          <template v-if="loading && !auth.userInfo">
            <NSkeleton text :repeat="3" />
          </template>
          <template v-else-if="auth.userInfo">
            <NSpace size="large" wrap>
              <NStatistic :label="t('home.balance')">
                <span class="stat-num">¥ {{ yuan(auth.userInfo.balance) }}</span>
              </NStatistic>
              <NStatistic :label="t('home.commission')">
                <span class="stat-num">¥ {{ yuan(auth.userInfo.commission_balance) }}</span>
              </NStatistic>
              <NStatistic :label="t('home.plan')">
                <NTag :type="auth.userInfo.plan_id ? 'success' : 'default'" size="medium">
                  {{ planLabel }}
                </NTag>
              </NStatistic>
              <NStatistic :label="t('home.expiry')">
                <span class="stat-date">{{ fmtExpiry(auth.userInfo.expired_at) }}</span>
              </NStatistic>
            </NSpace>
          </template>
          <NEmpty v-else />
        </NCard>

        <NCard :title="t('home.traffic')" embedded>
          <NAlert
            v-if="subStateMeta"
            :type="subStateMeta.type"
            :title="subStateMeta.title"
            :show-icon="true"
            class="cue-alert"
          >
            <div class="cue-body">
              <span>{{ subStateMeta.body }}</span>
              <NButton size="small" type="primary" ghost @click="onCueAction">
                {{ subStateMeta.ctaLabel }}
              </NButton>
            </div>
          </NAlert>

          <template v-if="loading && !auth.subscribe">
            <NSkeleton text :repeat="2" />
          </template>
          <template v-else-if="auth.subscribe">
            <NSpace vertical :size="12">
              <NProgress
                type="line"
                :percentage="trafficPct"
                :status="trafficBarStatus"
                :show-indicator="true"
                :indicator-text-color="theme.dark ? '#eee' : '#333'"
                :height="10"
                :border-radius="6"
              />
              <NSpace :size="20" wrap>
                <NText>
                  {{ t("home.used") }}:
                  <strong>{{ fmtBytes(trafficUsed) }}</strong>
                </NText>
                <NText>
                  {{ t("home.total") }}:
                  <strong>{{ fmtBytes(auth.subscribe.transfer_enable) }}</strong>
                </NText>
                <NText>
                  {{ t("home.expiry") }}:
                  <strong>{{ fmtExpiry(auth.subscribe.expired_at) }}</strong>
                </NText>
              </NSpace>
              <NDivider style="margin: 4px 0" />
              <div class="subscribe-row">
                <NText depth="3" style="font-size: 12px; display: block; margin-bottom: 4px">
                  {{ t("home.subscribe") }}
                </NText>
                <div class="subscribe-line">
                  <code class="subscribe-url">{{ auth.subscribe.subscribe_url }}</code>
                  <NButton size="small" @click="onCopySubscribe">
                    {{ t("home.copy") }}
                  </NButton>
                </div>
              </div>
            </NSpace>
          </template>
          <NEmpty v-else :description="t('home.notSubscribed')" />
        </NCard>
      </div>
    </NLayoutContent>

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

    <NModal
      v-model:show="showHelperModal"
      preset="card"
      :title="t('helper.title')"
      style="max-width: 560px"
      :bordered="false"
      size="huge"
    >
      <template #header-extra>
        <NButton size="small" @click="loadHelperStatus">
          {{ t("home.refresh") }}
        </NButton>
      </template>
      <NSpace v-if="!helperStatus" justify="center" style="padding: 24px 0">
        <NSpin />
      </NSpace>
      <div v-else-if="!helperStatus.supported">
        <NAlert type="info" :show-icon="false">
          {{ t("helper.unsupported") }}
        </NAlert>
      </div>
      <div v-else class="helper-panel">
        <NSpace align="center" :size="8">
          <NTag
            :type="helperStatus.installed ? 'success' : 'warning'"
            :bordered="false"
          >
            {{
              helperStatus.installed
                ? t("helper.tag.installed")
                : t("helper.tag.notInstalled")
            }}
          </NTag>
          <NTag
            v-if="helperStatus.installed"
            :type="helperStatus.reachable ? 'success' : 'error'"
            :bordered="false"
          >
            {{
              helperStatus.reachable
                ? t("helper.tag.reachable")
                : t("helper.tag.unreachable")
            }}
          </NTag>
        </NSpace>
        <NText depth="3" class="helper-tip">
          {{
            helperStatus.installed
              ? t("helper.bodyInstalled")
              : t("helper.bodyMissing")
          }}
        </NText>
        <div v-if="helperStatus.helper_path" class="helper-paths">
          <NText depth="3">{{ helperStatus.helper_path }}</NText>
          <NText v-if="helperStatus.plist_path" depth="3">
            {{ helperStatus.plist_path }}
          </NText>
        </div>
        <NSpace>
          <NButton
            type="primary"
            :loading="helperBusy"
            @click="onHelperInstall"
          >
            {{
              helperStatus.installed
                ? t("helper.reinstall")
                : t("helper.installNow")
            }}
          </NButton>
          <NButton
            v-if="helperStatus.installed"
            :loading="helperBusy"
            @click="onHelperUninstall"
          >
            {{ t("helper.uninstall") }}
          </NButton>
        </NSpace>
      </div>
    </NModal>

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
        <NText depth="3" class="helper-tip">
          {{ t("kernelInfo.bundledHint") }}
        </NText>
        <pre class="log-pre">{{ kernelVersion.raw }}</pre>
        <NText depth="3" class="log-path">
          {{ kernelVersion.mihomo_path }}
        </NText>
      </div>
      <NEmpty v-else :description="t('kernelInfo.empty')" />
    </NModal>
  </NLayout>
</template>

<style scoped>
.home-shell {
  min-height: 100vh;
  background: var(--n-color);
}

.home-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
}

.brand-mark {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: linear-gradient(135deg, #18a058 0%, #36ad6a 60%, #69d490 100%);
  box-shadow: 0 0 0 1px rgba(24, 160, 88, 0.25);
}

.brand-text {
  font-size: 16px;
  letter-spacing: 0.06em;
}

.brand-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding-left: 8px;
}

.status-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.85;
}
.status-dot.pulse {
  animation: dot-pulse 1.4s ease-in-out infinite;
}
@keyframes dot-pulse {
  0%, 100% { opacity: 0.4; transform: scale(0.85); }
  50%      { opacity: 1;   transform: scale(1.15); }
}

.header-email {
  font-size: 12px;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: middle;
}

.account-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.caret {
  font-size: 10px;
  opacity: 0.7;
}

.home-content {
  padding: 20px;
}

.grid {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 980px;
  margin: 0 auto;
}

.connect-card {
  transition: box-shadow 0.25s;
}
.connect-card.is-connected {
  box-shadow: 0 0 0 1px rgba(24, 160, 88, 0.45),
              0 4px 24px rgba(24, 160, 88, 0.18);
}

.status-line {
  font-size: 12px;
}

.connect-toggle {
  display: flex;
  justify-content: center;
  padding: 6px 0;
}

.current-node {
  min-width: 0;
  flex: 1;
}

.traffic-line {
  display: flex;
  gap: 14px;
  align-items: center;
  font-variant-numeric: tabular-nums;
  font-size: 12px;
}
.traffic-arrow {
  opacity: 0.5;
}
.traffic-value {
  font-weight: 600;
}

.nodes-panel {
  /* container for the collapsing div, padding lives on inner blocks */
}

.groups {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.group-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.members {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 6px;
}

.member {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--n-border-color, rgba(255, 255, 255, 0.09));
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s, transform 0.05s;
}
.member:hover:not(:disabled) {
  border-color: rgba(24, 160, 88, 0.55);
  background: rgba(24, 160, 88, 0.06);
}
.member:active:not(:disabled) {
  transform: translateY(1px);
}
.member:disabled {
  opacity: 0.5;
  cursor: progress;
}
.member.is-current {
  border-color: #18a058;
  background: rgba(24, 160, 88, 0.12);
}
.member.is-selecting {
  opacity: 0.6;
}

.member-name {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 13px;
}

.member-latency {
  font-variant-numeric: tabular-nums;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  flex-shrink: 0;
}
.tone-success { color: #18a058; }
.tone-warning { color: #f0a020; }
.tone-error   { color: #d03050; }
.tone-default { color: rgba(150, 150, 150, 0.7); }

.stat-num {
  font-variant-numeric: tabular-nums;
  font-size: 20px;
  font-weight: 600;
}

.stat-date {
  font-variant-numeric: tabular-nums;
  font-size: 14px;
}

.subscribe-line {
  display: flex;
  align-items: center;
  gap: 8px;
}
.subscribe-url {
  flex: 1;
  min-width: 0;
  padding: 6px 8px;
  border-radius: 4px;
  background: rgba(127, 127, 127, 0.08);
  font-family: ui-monospace, "SFMono-Regular", Menlo, monospace;
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.health-alert {
  margin-bottom: 4px;
}

.cue-alert {
  margin-bottom: 12px;
}
.cue-body {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

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
.kernel-info {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.helper-tip {
  font-size: 12px;
  line-height: 1.55;
}

.helper-paths {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-family: ui-monospace, "SFMono-Regular", Menlo, monospace;
  font-size: 11px;
}
</style>
