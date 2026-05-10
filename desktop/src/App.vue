<script setup lang="ts">
import { computed, onMounted } from "vue";
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

const theme = useThemeStore();
const auth = useAuthStore();
const site = useSiteStore();

const naiveTheme = computed(() => (theme.dark ? darkTheme : null));

// Hydrate session + warm site config in parallel. Bootstrap rejection is
// already swallowed inside the store; we don't surface anything here.
onMounted(() => {
  auth.bootstrap();
  site.ensure().catch(() => undefined);
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
  min-height: 100vh;
}
</style>
