<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from "vue";
import { useRouter, useRoute } from "vue-router";
import { listen } from "@/platform";
import type { UnlistenFn } from "@tauri-apps/api/event";
import AppShell from "@/components/AppShell.vue";
import AuthLayout from "@/components/AuthLayout.vue";
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
import { useNodesStore } from "@/stores/nodes";
import { usePlanStore } from "@/stores/plan";
import { useFeaturesStore } from "@/stores/features";
import { isRouteEnabled } from "@/router";
import type { TunnelMode } from "@/types";

const theme = useThemeStore();
const auth = useAuthStore();
const site = useSiteStore();
const conn = useConnectionStore();
const router = useRouter();
const route = useRoute();
watch(() => auth.session, (next, prev) => {
  if (!next && prev) {
    useNodesStore().reset();
    usePlanStore().reset();
    void conn.endSession().catch(() => undefined);
    void router.replace('/login');
  }
}, { flush: 'sync' });
const features = useFeaturesStore();
watch(() => [features.config, route.name], () => {
  if (auth.session && route.meta.requiresAuth && !isRouteEnabled(route.name))
    void router.replace({ name: 'home' });
});

const naiveTheme = computed(() => (theme.dark ? darkTheme : null));
const themeOverrides = computed(() => ({
  common: { primaryColor: theme.dark ? '#a295ff' : '#7564e8', primaryColorHover: '#9382ef', primaryColorPressed: '#6452d9', borderRadius: '10px', fontFamily: 'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei", sans-serif', bodyColor: theme.dark ? '#171823' : '#f6f7fb', cardColor: theme.dark ? '#20212f' : '#ffffff', textColorBase: theme.dark ? '#eeeef6' : '#26273d' },
  Card: { borderRadius: '18px' }, Button: { borderRadiusMedium: '9px', heightMedium: '38px' }, Input: { heightLarge: '46px' }
}));

// Tray menu → store/router. Keeping the action logic in the store (not the
// Rust tray handler) means auth/kernel-readiness checks live in one place.
const trayUnlisten: UnlistenFn[] = [];
let disposed = false;
function keepTrayListener(off: UnlistenFn) {
  if (disposed) off();
  else trayUnlisten.push(off);
}

async function setupTrayListeners() {
  keepTrayListener(
    await listen("tray://connect", () => {
      void conn.connect().catch(() => undefined);
    }),
  );
  keepTrayListener(
    await listen("tray://disconnect", () => {
      void conn.disconnect().catch(() => undefined);
    }),
  );
  keepTrayListener(
    await listen<string>("tray://set-mode", (e) => {
      const next = e.payload === "system_proxy" ? "system_proxy" : "tun";
      void conn.setMode(next as TunnelMode).catch(() => undefined);
    }),
  );
  keepTrayListener(
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
  void setupTrayListeners().catch(() => undefined);
});

onUnmounted(() => {
  disposed = true;
  trayUnlisten.forEach((fn) => fn());
  trayUnlisten.length = 0;
});
</script>

<template>
  <NConfigProvider :theme="naiveTheme" :theme-overrides="themeOverrides" :locale="zhCN" :date-locale="dateZhCN">
    <NLoadingBarProvider>
      <NDialogProvider>
        <NMessageProvider>
          <NSpin :show="auth.bootstrapping" class="bootstrap-veil">
            <AppShell v-if="route.meta.requiresAuth && auth.session" />
            <AuthLayout v-else><RouterView /></AuthLayout>
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
