<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  darkTheme,
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  NLoadingBarProvider,
  NSpin,
  zhCN,
  dateZhCN,
} from "naive-ui";
import { useThemeStore } from "@/stores/theme";
import { useAuthStore } from "@/stores/auth";
import { useSiteStore } from "@/stores/site";
import { useConnectionStore } from "@/stores/connection";
import type { TunnelMode } from "@/types";

const theme = useThemeStore();
const auth = useAuthStore();
const site = useSiteStore();
const conn = useConnectionStore();
const router = useRouter();

const naiveTheme = computed(() => (theme.dark ? darkTheme : null));

// Tray menu → store/router. Keeping the action logic in the store (not the
// Rust tray handler) means auth/kernel-readiness checks live in one place.
const trayUnlisten: UnlistenFn[] = [];

async function setupTrayListeners() {
  trayUnlisten.push(
    await listen("tray://connect", () => {
      void conn.connect().catch(() => undefined);
    }),
  );
  trayUnlisten.push(
    await listen("tray://disconnect", () => {
      void conn.disconnect().catch(() => undefined);
    }),
  );
  trayUnlisten.push(
    await listen<string>("tray://set-mode", (e) => {
      const next = e.payload === "system_proxy" ? "system_proxy" : "tun";
      void conn.setMode(next as TunnelMode).catch(() => undefined);
    }),
  );
  trayUnlisten.push(
    await listen("tray://open-logs", () => {
      void router.push({ name: "logs" }).catch(() => undefined);
    }),
  );
}

// Hydrate session + warm site config in parallel. Bootstrap rejection is
// already swallowed inside the store; we don't surface anything here.
onMounted(() => {
  auth.bootstrap();
  site.ensure().catch(() => undefined);
  void setupTrayListeners();
});

onUnmounted(() => {
  trayUnlisten.forEach((fn) => fn());
  trayUnlisten.length = 0;
});
</script>

<template>
  <NConfigProvider :theme="naiveTheme" :locale="zhCN" :date-locale="dateZhCN">
    <NLoadingBarProvider>
      <NDialogProvider>
        <NMessageProvider>
          <NSpin :show="auth.bootstrapping" class="bootstrap-veil">
            <RouterView />
          </NSpin>
        </NMessageProvider>
      </NDialogProvider>
    </NLoadingBarProvider>
  </NConfigProvider>
</template>

<style scoped>
.bootstrap-veil {
  /* min-height so NSpin's overlay still covers the viewport on short
     pages (e.g. Login); Home.vue self-locks to 100vh via its own
     .home-shell rule, and other routes that legitimately need to scroll
     are now free to grow past the viewport. */
  min-height: 100vh;
}
.bootstrap-veil :deep(.n-spin-content) {
  min-height: 100vh;
}
</style>
