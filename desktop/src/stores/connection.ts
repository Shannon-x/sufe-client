// Pinia store for the kernel connection lifecycle. Mirrors the state machine
// owned by `xboard_core::KernelManager`: this store does NOT decide state, it
// reflects what the Rust side broadcasts over the `xboard://connection-state`
// event. Calls to connect/disconnect dispatch to Tauri commands and let the
// listener pipe drive UI updates.

import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "@/api";
import type {
  ConnectionState,
  ProxyGroup,
  TrafficStats,
  TunnelMode,
} from "@/types";

const TRAFFIC_POLL_MS = 1000;

export const useConnectionStore = defineStore("connection", () => {
  const state = ref<ConnectionState>({ kind: "disconnected" });
  const traffic = ref<TrafficStats>({ up: 0, down: 0, up_total: 0, down_total: 0 });
  const proxies = ref<ProxyGroup[]>([]);
  const mode = ref<TunnelMode>("tun");
  // What the user *asked for* — distinct from the mode KernelManager actually
  // ended up using. When TUN elevation fails (no consent / no helper / no
  // capability), the kernel silently downgrades to system_proxy and the
  // connected state will report `mode: "system_proxy"` while requestedMode
  // stays "tun". The UI uses the gap to surface a one-shot warning.
  const requestedMode = ref<TunnelMode>("tun");

  let unlisten: UnlistenFn | null = null;
  let trafficTimer: number | null = null;

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
  const currentProxy = computed<string | null>(() => {
    // Best-effort: pick the first selector group's `now`. Refine once UI
    // distinguishes "primary" group (Xboard subs almost always use PROXY).
    const primary =
      proxies.value.find((g) => g.name === "PROXY") ??
      proxies.value.find((g) => g.type === "Selector") ??
      proxies.value[0];
    return primary?.now ?? null;
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
            startTrafficPoll();
            void refreshProxies();
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
    if (state.value.kind === "connected") {
      startTrafficPoll();
      void refreshProxies();
    }
  }

  async function connect() {
    // Capture the mode the user is asking for *before* the connect call —
    // KernelManager may downgrade silently and we need the original intent
    // to detect that.
    requestedMode.value = mode.value;
    state.value = await api.connect();
  }

  async function disconnect() {
    await api.disconnect();
    // Listener will publish `disconnected`; reset traffic eagerly so the UI
    // doesn't briefly flash stale numbers.
    traffic.value = { up: 0, down: 0, up_total: 0, down_total: 0 };
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
    await refreshProxies();
  }

  function startTrafficPoll() {
    if (trafficTimer !== null) return;
    const tick = async () => {
      try {
        traffic.value = await api.currentTraffic();
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
  }

  async function dispose() {
    stopTrafficPoll();
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  }

  return {
    state,
    traffic,
    proxies,
    mode,
    requestedMode,
    isConnected,
    isBusy,
    currentMode,
    wasDowngraded,
    currentProxy,
    hydrate,
    connect,
    disconnect,
    setMode,
    refreshProxies,
    selectProxy,
    dispose,
  };
});
