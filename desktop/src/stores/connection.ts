// Pinia store for the kernel connection lifecycle. Mirrors the state machine
// owned by `xboard_core::KernelManager`: this store does NOT decide state, it
// reflects what the Rust side broadcasts over the `xboard://connection-state`
// event. Calls to connect/disconnect dispatch to Tauri commands and let the
// listener pipe drive UI updates.

import { defineStore } from "pinia";
import { ref, computed, type Ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "@/api";
import { i18n } from "@/i18n";
import type {
  ConnectionState,
  ProxyGroup,
  TrafficStats,
  TunnelMode,
} from "@/types";

const TRAFFIC_POLL_MS = 1000;
// How many 1s samples to keep for the live traffic chart (~2 min window).
const TRAFFIC_HISTORY_LEN = 120;
// Bounded auto-reconnect backoff after a kernel crash (kind === "exited").
const RECONNECT_BACKOFFS_MS = [1000, 3000, 6000];

// Payload of the `connection://kernel-failure` event broadcast by the Rust
// side (agent 2.1). `kind` distinguishes a process that exited on its own
// (likely a config / port-bind failure) from one that's still alive but no
// longer responding on the control plane (likely a hang / deadlock).
interface KernelFailurePayload {
  kind: "exited" | "unresponsive";
  exit_code?: number;
  log_tail?: string;
}

export const useConnectionStore = defineStore("connection", () => {
  const state = ref<ConnectionState>({ kind: "disconnected" });
  const traffic = ref<TrafficStats>({ up: 0, down: 0, up_total: 0, down_total: 0 });
  // Rolling up/down samples for the live traffic chart.
  const trafficHistory = ref<Array<{ up: number; down: number }>>([]);
  const proxies = ref<ProxyGroup[]>([]);
  const mode = ref<TunnelMode>("tun");
  // True while the bounded auto-reconnect loop is running after a crash.
  const reconnecting = ref(false);
  // What the user *asked for* — distinct from the mode KernelManager actually
  // ended up using. When TUN elevation fails (no consent / no helper / no
  // capability), the kernel silently downgrades to system_proxy and the
  // connected state will report `mode: "system_proxy"` while requestedMode
  // stays "tun". The UI uses the gap to surface a one-shot warning.
  const requestedMode = ref<TunnelMode>("tun");
  // Tail of the kernel's stdout/stderr captured by KernelManager at the
  // moment a `connection://kernel-failure` event fired. Surfaced via
  // StatusHero / a log modal so users can self-diagnose port conflicts,
  // config errors, etc. Cleared when the next healthy connect lands.
  const lastKernelLog: Ref<string | null> = ref(null);
  // Why the kernel last transitioned to an error state. Lets the
  // `kernel-healthy` listener decide whether to auto-recover: we only do
  // that for `unresponsive` (control plane briefly stalled but kernel is
  // still running), not for `exited` (process is gone, requires reconnect).
  let lastErrorCause: "exited" | "unresponsive" | null = null;

  let unlisten: UnlistenFn | null = null;
  let unlistenKernelFailure: UnlistenFn | null = null;
  let unlistenKernelHealthy: UnlistenFn | null = null;
  let trafficTimer: number | null = null;
  let reconnectTimer: number | null = null;
  // Set when the user manually connects/disconnects so an in-flight
  // auto-reconnect doesn't fight a deliberate action.
  let reconnectCancelled = false;

  const isConnected = computed(() => state.value.kind === "connected");
  const isBusy = computed(() => state.value.kind === "connecting");
  const currentMode = computed<TunnelMode>(() => {
    const s = state.value;
    if (s.kind === "connected" || s.kind === "connecting" || s.kind === "error") {
      return s.mode;
    }
    return mode.value;
  });
  // True only while connected via a different mode than the user picked —
  // i.e. KernelManager auto-downgraded TUN→system_proxy at connect time.
  const wasDowngraded = computed<boolean>(() => {
    const s = state.value;
    if (s.kind !== "connected") return false;
    return s.mode !== requestedMode.value;
  });
  const primaryGroup = computed<ProxyGroup | null>(() => {
    const switchable = proxies.value.filter((g) => g.all.length > 0);
    return switchable[0] ?? proxies.value[0] ?? null;
  });
  const currentProxy = computed<string | null>(() => primaryGroup.value?.now ?? null);
  const effectiveProxy = computed<string | null>(() => {
    if (!currentProxy.value) return null;
    return resolveProxyLeaf(currentProxy.value, proxies.value);
  });

  /// Read the current state once + start listening. Idempotent — repeated
  /// calls just refresh the snapshot.
  async function hydrate() {
    state.value = await api.connectionState();
    if (!unlisten) {
      unlisten = await listen<ConnectionState>(
        "xboard://connection-state",
        (e) => {
          state.value = e.payload;
          // Drive the traffic poller from state transitions: only poll while
          // connected, stop the moment we leave that state.
          if (e.payload.kind === "connected") {
            // A clean connected snapshot means whatever failure we previously
            // recorded is now stale — drop the cached log/cause so the UI
            // stops showing yesterday's error.
            lastKernelLog.value = null;
            lastErrorCause = null;
            reconnecting.value = false;
            startTrafficPoll();
            void refreshProxies().then(() => restoreSelections());
          } else {
            stopTrafficPoll();
            // Leaving "connected" invalidates the proxy snapshot — the kernel
            // process is being torn down (or has already errored), so any
            // cached selector "now" / latency map will only mislead the user
            // until the next successful connect refreshes them.
            if (proxies.value.length > 0) proxies.value = [];
          }
        },
      );
    }
    if (!unlistenKernelFailure) {
      unlistenKernelFailure = await listen<KernelFailurePayload>(
        "connection://kernel-failure",
        (e) => {
          const payload = e.payload;
          lastErrorCause = payload.kind;
          lastKernelLog.value = payload.log_tail ?? null;
          // Build a human-readable cause for `connect.status.error`. Use the
          // exit code when we have one (typical for `exited`), otherwise a
          // generic label so the UI never renders "Connection error: ".
          const cause =
            payload.kind === "exited"
              ? payload.exit_code !== undefined
                ? `exit ${payload.exit_code}`
                : "kernel exited"
              : "kernel unresponsive";
          const message = i18n.global.t("connect.status.error", {
            message: cause,
          });
          // The `error` ConnectionState variant carries a mode; reuse the
          // current/requested one since the failure event itself doesn't
          // include it.
          const errMode: TunnelMode =
            state.value.kind === "connected" ||
            state.value.kind === "connecting" ||
            state.value.kind === "error"
              ? state.value.mode
              : requestedMode.value;
          state.value = { kind: "error", message, mode: errMode };
          stopTrafficPoll();
          if (proxies.value.length > 0) proxies.value = [];
          // A process that exited on its own (crash) is recoverable by
          // respawning the kernel — kick off a bounded auto-reconnect.
          // `unresponsive` is handled by the kernel-healthy path instead.
          if (payload.kind === "exited") {
            scheduleReconnect(0);
          }
        },
      );
    }
    if (!unlistenKernelHealthy) {
      unlistenKernelHealthy = await listen<Record<string, never>>(
        "connection://kernel-healthy",
        () => {
          // Only auto-recover from a transient `unresponsive` stall — an
          // `exited` kernel needs a fresh connect() to respawn the process,
          // and the connection-state listener will publish that.
          if (
            state.value.kind === "error" &&
            lastErrorCause === "unresponsive"
          ) {
            state.value = {
              kind: "connected",
              // Best-effort: we don't have the original `since` / mixed_port
              // here, so seed them from `now` / 0. The next `connection-state`
              // broadcast (or hydrate) will overwrite this with the real
              // KernelManager snapshot.
              since: new Date().toISOString(),
              mode: state.value.mode,
              mixed_port: 0,
            };
            lastErrorCause = null;
            lastKernelLog.value = null;
            startTrafficPoll();
            void refreshProxies();
          }
        },
      );
    }
    if (state.value.kind === "connected") {
      startTrafficPoll();
      void refreshProxies();
    }
  }

  async function connect() {
    // A deliberate connect supersedes any pending auto-reconnect.
    cancelReconnect();
    // Capture the mode the user is asking for *before* the connect call —
    // KernelManager may downgrade silently and we need the original intent
    // to detect that.
    requestedMode.value = mode.value;
    state.value = await api.connect();
    if (state.value.kind === "connected") {
      await refreshProxies();
    }
  }

  async function disconnect() {
    // A deliberate disconnect cancels any in-flight auto-reconnect.
    cancelReconnect();
    await api.disconnect();
    // Listener will publish `disconnected`; reset traffic eagerly so the UI
    // doesn't briefly flash stale numbers.
    traffic.value = { up: 0, down: 0, up_total: 0, down_total: 0 };
  }

  /// Bounded auto-reconnect after a kernel crash. Retries `api.reconnect()`
  /// with [1s, 3s, 6s] backoff; stops on the first connected result or when
  /// the user manually (dis)connects. The connection-state listener clears
  /// `reconnecting` once a connected snapshot lands.
  function scheduleReconnect(attempt: number) {
    if (attempt >= RECONNECT_BACKOFFS_MS.length) {
      reconnecting.value = false;
      return;
    }
    reconnectCancelled = false;
    reconnecting.value = true;
    reconnectTimer = window.setTimeout(async () => {
      if (reconnectCancelled) return;
      try {
        const st = await api.reconnect();
        state.value = st;
        if (st.kind === "connected") {
          reconnecting.value = false;
          await refreshProxies();
          return;
        }
      } catch {
        // fall through to the next attempt
      }
      if (!reconnectCancelled) scheduleReconnect(attempt + 1);
    }, RECONNECT_BACKOFFS_MS[attempt]);
  }

  function cancelReconnect() {
    reconnectCancelled = true;
    if (reconnectTimer !== null) {
      window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    reconnecting.value = false;
  }

  async function setMode(next: TunnelMode) {
    mode.value = next;
    requestedMode.value = next;
    await api.setTunnelMode(next);
    // If the user switches mode while connected, transparently reconnect so
    // the new mode takes effect — KernelManager only consumes requested_mode
    // on the next `connect()`.
    if (state.value.kind === "connected") {
      await disconnect();
      await connect();
    }
  }

  async function refreshProxies() {
    try {
      proxies.value = await api.proxies();
    } catch {
      // Kernel might briefly be unreachable just after spawn; the next poll
      // tick will retry.
      proxies.value = [];
    }
  }

  async function selectProxy(group: string, name: string) {
    await api.selectProxy(group, name);
    saveSelection(group, name);
    await refreshProxies();
  }

  // Persist the user's per-group node choice so a reconnect / app restart
  // restores it instead of snapping back to the group default — a top
  // annoyance vs. mature clients.
  function saveSelection(group: string, name: string) {
    try {
      localStorage.setItem(`xboard.sel.${group}`, name);
    } catch {
      // localStorage may be unavailable in some webviews; non-fatal.
    }
  }

  async function restoreSelections() {
    let changed = false;
    for (const g of proxies.value) {
      let saved: string | null = null;
      try {
        saved = localStorage.getItem(`xboard.sel.${g.name}`);
      } catch {
        saved = null;
      }
      if (saved && saved !== g.now && g.all.includes(saved)) {
        try {
          await api.selectProxy(g.name, saved);
          changed = true;
        } catch {
          // node may be temporarily unavailable; skip
        }
      }
    }
    if (changed) await refreshProxies();
  }

  function startTrafficPoll() {
    if (trafficTimer !== null) return;
    const tick = async () => {
      try {
        traffic.value = await api.currentTraffic();
        // Feed the live chart, capping the ring buffer.
        trafficHistory.value.push({
          up: traffic.value.up,
          down: traffic.value.down,
        });
        if (trafficHistory.value.length > TRAFFIC_HISTORY_LEN) {
          trafficHistory.value.splice(
            0,
            trafficHistory.value.length - TRAFFIC_HISTORY_LEN,
          );
        }
      } catch {
        // ignore — likely a transient kernel/control issue
      }
    };
    void tick();
    trafficTimer = window.setInterval(tick, TRAFFIC_POLL_MS);
  }

  function stopTrafficPoll() {
    if (trafficTimer !== null) {
      window.clearInterval(trafficTimer);
      trafficTimer = null;
    }
    trafficHistory.value = [];
  }

  async function dispose() {
    cancelReconnect();
    stopTrafficPoll();
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    if (unlistenKernelFailure) {
      unlistenKernelFailure();
      unlistenKernelFailure = null;
    }
    if (unlistenKernelHealthy) {
      unlistenKernelHealthy();
      unlistenKernelHealthy = null;
    }
  }

  return {
    state,
    traffic,
    trafficHistory,
    reconnecting,
    proxies,
    mode,
    requestedMode,
    lastKernelLog,
    primaryGroup,
    isConnected,
    isBusy,
    currentMode,
    wasDowngraded,
    currentProxy,
    effectiveProxy,
    hydrate,
    connect,
    disconnect,
    setMode,
    refreshProxies,
    selectProxy,
    dispose,
  };
});

function resolveProxyLeaf(name: string, groups: ProxyGroup[]): string {
  const seen = new Set<string>();
  let current = name;
  while (!seen.has(current)) {
    seen.add(current);
    const group = groups.find((g) => g.name === current);
    if (!group?.now) return current;
    current = group.now;
  }
  return current;
}
