// Pinia store for the kernel connection lifecycle. Mirrors the state machine
// owned by `xboard_core::KernelManager`: this store does NOT decide state, it
// reflects what the Rust side broadcasts over the `xboard://connection-state`
// event. Calls to connect/disconnect dispatch to Tauri commands and let the
// listener pipe drive UI updates.

import { defineStore } from "pinia";
import { ref, computed, type Ref } from "vue";
import { listen } from "@/platform";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api } from "@/api";
import { i18n } from "@/i18n";
import { useAuthStore } from "@/stores/auth";
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
  const mode = ref<TunnelMode>(localStorage.getItem('sufe.tunnelMode') === 'system_proxy' ? 'system_proxy' : 'tun');
  // True while the bounded auto-reconnect loop is running after a crash.
  const reconnecting = ref(false);
  // Preserve the user's selected mode through failed authorization and reconnects.
  const requestedMode = ref<TunnelMode>(mode.value);
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
  let active = false;
  let generation = 0;
  let operation = 0;
  let stateRevision = 0;
  let proxyRequest = 0;
  let trafficGeneration = 0;
  let disconnecting = false;
  let hydration: Promise<void> | null = null;
  let cleanup: Promise<void> | null = null;
  const mutations = new Set<Promise<unknown>>();
  const auth = useAuthStore();
  const live = (version: number) => active && version === generation && !!auth.session;
  function track<T>(request: Promise<T>): Promise<T> {
    mutations.add(request);
    void request.finally(() => mutations.delete(request)).catch(() => undefined);
    return request;
  }

  const isConnected = computed(() => state.value.kind === "connected");
  const isBusy = computed(() => state.value.kind === "connecting");
  const currentMode = computed<TunnelMode>(() => {
    const s = state.value;
    if (s.kind === "connected" || s.kind === "connecting" || s.kind === "error") {
      return s.mode;
    }
    return mode.value;
  });
  // Detect a mode change from another UI surface while connected.
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

  function applyState(next: ConnectionState) {
    if (disconnecting && (next.kind === 'connected' || next.kind === 'connecting')) return;
    stateRevision++;
    state.value = next;
    if (next.kind === 'connected') {
      lastErrorCause = null;
      lastKernelLog.value = null;
      reconnecting.value = false;
      startTrafficPoll();
      void refreshProxies().then(() => restoreSelections());
    } else {
      stopTrafficPoll();
      proxyRequest++;
      proxies.value = [];
    }
  }

  function hydrate(): Promise<void> {
    if (hydration) return hydration;
    const task = initialize();
    hydration = task;
    void task.finally(() => { if (hydration === task) hydration = null; }).catch(() => undefined);
    return task;
  }

  async function initialize() {
    const version = generation;
    if (cleanup) await cleanup;
    if (!auth.session || version !== generation) return;
    active = true;
    // A disposed initialization must unregister even listeners whose async
    // registration finished after logout.
    if (!unlisten) {
      const off = await listen<ConnectionState>('xboard://connection-state', e => {
        if (live(version)) applyState(e.payload);
      });
      if (!live(version)) { off(); return; }
      unlisten = off;
    }
    if (!unlistenKernelFailure) {
      const off = await listen<KernelFailurePayload>('connection://kernel-failure', e => {
        if (!live(version) || disconnecting) return;
        const payload = e.payload;
        lastErrorCause = payload.kind;
        lastKernelLog.value = payload.log_tail ?? null;
        const cause = payload.kind === 'exited'
          ? payload.exit_code !== undefined ? `exit ${payload.exit_code}` : 'kernel exited'
          : 'kernel unresponsive';
        applyState({ kind: 'error', message: i18n.global.t('connect.status.error', { message: cause }), mode: currentMode.value });
        if (payload.kind === 'exited') scheduleReconnect(0);
      });
      if (!live(version)) { off(); return; }
      unlistenKernelFailure = off;
    }
    if (!unlistenKernelHealthy) {
      const off = await listen('connection://kernel-healthy', async () => {
        if (!live(version) || lastErrorCause !== 'unresponsive' || disconnecting) return;
        const revision = stateRevision;
        try {
          const snapshot = await api.connectionState();
          if (live(version) && revision === stateRevision) applyState(snapshot);
        } catch { /* Wait for an authoritative state instead of inventing port/since. */ }
      });
      if (!live(version)) { off(); return; }
      unlistenKernelHealthy = off;
    }
    const revision = stateRevision;
    const snapshot = await api.connectionState();
    if (!live(version) || revision !== stateRevision) return;
    applyState(snapshot);
    if (snapshot.kind === 'disconnected') {
      await api.setTunnelMode(mode.value);
      if (!live(version)) return;
      const savedGuard = localStorage.getItem('sufe.proxyGuard');
      if (savedGuard !== null) await api.setProxyGuardEnabled(savedGuard !== '0');
    }
  }

  async function connect() {
    const owner = auth.session, lifecycle = generation;
    if (cleanup) await cleanup;
    if (!auth.session) throw new Error('请先登录后再连接。');
    if (owner !== auth.session || lifecycle !== generation) return;
    if (!active) await hydrate();
    if (owner !== auth.session || lifecycle !== generation || !active || isBusy.value || isConnected.value) return;
    cancelReconnect();
    disconnecting = false;
    const version = generation, request = ++operation;
    requestedMode.value = mode.value;
    applyState({ kind: 'connecting', stage: 'fetching', mode: mode.value });
    try {
      const snapshot = await track(api.connect());
      if (live(version) && request === operation) applyState(snapshot);
    } catch (error) {
      if (live(version) && request === operation) {
        applyState({ kind: 'error', mode: mode.value, message: error instanceof Error ? error.message : String(error) });
      }
      throw error;
    }
  }

  async function disconnect() {
    cancelReconnect();
    disconnecting = true;
    const version = generation, request = ++operation;
    // A connect may still be fetching a subscription before it reaches the
    // kernel mutex. Disconnect after that command settles so it cannot win.
    const outstanding = [...mutations];
    if (outstanding.length) await Promise.allSettled(outstanding);
    try {
      const snapshot = await track(api.disconnect());
      if (live(version) && request === operation) applyState(snapshot);
    } finally {
      if (version === generation && request === operation) disconnecting = false;
    }
  }

  function scheduleReconnect(attempt: number) {
    if (!active || !auth.session || attempt >= RECONNECT_BACKOFFS_MS.length) {
      reconnecting.value = false;
      return;
    }
    if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
    reconnectCancelled = false;
    reconnecting.value = true;
    const version = generation, request = operation;
    reconnectTimer = window.setTimeout(async () => {
      reconnectTimer = null;
      if (!live(version) || reconnectCancelled || request !== operation) return;
      try {
        const snapshot = await track(api.reconnect());
        if (!live(version) || reconnectCancelled || request !== operation) return;
        applyState(snapshot);
        if (snapshot.kind === 'connected') return;
      } catch { /* Retry only while this lifecycle and operation still own the request. */ }
      if (live(version) && !reconnectCancelled && request === operation) scheduleReconnect(attempt + 1);
    }, RECONNECT_BACKOFFS_MS[attempt]);
  }

  function cancelReconnect() {
    reconnectCancelled = true;
    if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
    reconnectTimer = null;
    reconnecting.value = false;
  }

  async function setMode(next: TunnelMode) {
    if (!auth.session) return;
    const version = generation;
    if (cleanup) await cleanup;
    if (version !== generation || !auth.session) return;
    await api.setTunnelMode(next);
    if (version !== generation || !auth.session) return;
    mode.value = requestedMode.value = next;
    localStorage.setItem('sufe.tunnelMode', next);
    if (isConnected.value) {
      await disconnect();
      if (version === generation && auth.session) await connect();
    }
  }

  async function refreshProxies() {
    if (!active || !auth.session || !isConnected.value) return;
    const version = generation, request = ++proxyRequest;
    try {
      const snapshot = await api.proxies();
      if (live(version) && isConnected.value && request === proxyRequest) proxies.value = snapshot;
    } catch {
      if (live(version) && request === proxyRequest) proxies.value = [];
    }
  }

  async function selectProxy(group: string, name: string) {
    if (!active || !auth.session || !isConnected.value) return;
    const version = generation;
    await api.selectProxy(group, name);
    if (!live(version) || !isConnected.value) return;
    try { localStorage.setItem(`xboard.sel.${group}`, name); } catch { /* Optional persistence. */ }
    await refreshProxies();
  }

  async function restoreSelections() {
    const version = generation;
    let changed = false;
    for (const group of proxies.value) {
      if (!live(version) || !isConnected.value || disconnecting) return;
      let saved: string | null = null;
      try { saved = localStorage.getItem(`xboard.sel.${group.name}`); } catch { /* Optional persistence. */ }
      if (saved && saved !== group.now && group.all.includes(saved)) {
        try { await api.selectProxy(group.name, saved); changed = true; } catch { /* Node may be unavailable. */ }
      }
    }
    if (changed && live(version) && isConnected.value) await refreshProxies();
  }

  function startTrafficPoll() {
    if (trafficTimer !== null) return;
    const version = generation, poll = ++trafficGeneration;
    const tick = async () => {
      try {
        const snapshot = await api.currentTraffic();
        if (!live(version) || poll !== trafficGeneration || !isConnected.value) return;
        traffic.value = snapshot;
        trafficHistory.value.push({ up: snapshot.up, down: snapshot.down });
        if (trafficHistory.value.length > TRAFFIC_HISTORY_LEN) trafficHistory.value.shift();
      } catch { /* A temporary control-plane failure must not erase session state. */ }
      if (live(version) && poll === trafficGeneration && isConnected.value)
        trafficTimer = window.setTimeout(tick, TRAFFIC_POLL_MS);
    };
    // Schedule rather than overlap requests when the control plane is slow.
    trafficTimer = window.setTimeout(tick, 0);
  }

  function stopTrafficPoll() {
    trafficGeneration++;
    if (trafficTimer !== null) window.clearTimeout(trafficTimer);
    trafficTimer = null;
    trafficHistory.value = [];
    traffic.value = { up: 0, down: 0, up_total: 0, down_total: 0 };
  }

  async function dispose() {
    active = false;
    generation++;
    operation++;
    proxyRequest++;
    hydration = null;
    cancelReconnect();
    stopTrafficPoll();
    unlisten?.(); unlisten = null;
    unlistenKernelFailure?.(); unlistenKernelFailure = null;
    unlistenKernelHealthy?.(); unlistenKernelHealthy = null;
    state.value = { kind: 'disconnected' };
    proxies.value = [];
    lastKernelLog.value = null;
    lastErrorCause = null;
    disconnecting = false;
  }

  function endSession(): Promise<void> {
    if (cleanup) return cleanup;
    // Dispose synchronously before awaiting IPC; late traffic/events cannot
    // repopulate the logged-out UI. New sessions wait for kernel cleanup.
    void dispose();
    const outstanding = [...mutations];
    const task = (async () => {
      await Promise.allSettled(outstanding);
      await api.disconnect();
    })();
    cleanup = task;
    void task.finally(() => { if (cleanup === task) cleanup = null; }).catch(() => undefined);
    return task;
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
    endSession,
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
