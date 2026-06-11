<script setup lang="ts">
// Centralised settings — consolidates the mode / theme / language toggles
// that were previously scattered, plus the new reliability switches
// (sysproxy guard, launch-time auto-connect). Each toggle writes through
// immediately; failures revert the UI.
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NButton,
  NCard,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NRadioButton,
  NRadioGroup,
  NSpace,
  NSwitch,
  NText,
  useMessage,
} from "naive-ui";
import { api } from "@/api";
import { useThemeStore } from "@/stores/theme";
import { useConnectionStore } from "@/stores/connection";
import { i18n } from "@/i18n";
import type { TunnelMode } from "@/types";
import { formatError } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const { t } = useI18n();
const theme = useThemeStore();
const conn = useConnectionStore();

const AUTO_CONNECT_ON_START_KEY = "xboard.autoConnectOnStart";

type Locale = "zh-CN" | "en-US";
const locale = ref<Locale>(i18n.global.locale.value);
const guardEnabled = ref(true);
const autoConnectOnStart = ref(
  localStorage.getItem(AUTO_CONNECT_ON_START_KEY) === "1",
);
const appVer = ref("");
const coreVer = ref("");

async function load() {
  try {
    guardEnabled.value = await api.proxyGuardEnabled();
  } catch {
    /* keep default */
  }
  try {
    appVer.value = await api.appVersion();
    coreVer.value = await api.coreVersion();
  } catch {
    /* ignore */
  }
}

async function onModeChange(next: TunnelMode) {
  try {
    await conn.setMode(next);
  } catch (e) {
    message.error(formatError(e, t));
  }
}

function onLocaleChange(next: string) {
  const loc: Locale = next === "en-US" ? "en-US" : "zh-CN";
  i18n.global.locale.value = loc;
  locale.value = loc;
  try {
    localStorage.setItem("xboard.locale", loc);
  } catch {
    /* non-fatal */
  }
}

async function onGuardChange(next: boolean) {
  const prev = !next;
  try {
    await api.setProxyGuardEnabled(next);
  } catch (e) {
    guardEnabled.value = prev; // revert on failure
    message.error(formatError(e, t));
  }
}

function onAutoConnectChange(next: boolean) {
  try {
    if (next) localStorage.setItem(AUTO_CONNECT_ON_START_KEY, "1");
    else localStorage.removeItem(AUTO_CONNECT_ON_START_KEY);
  } catch {
    autoConnectOnStart.value = !next;
  }
}

onMounted(load);
</script>

<template>
  <NLayout class="set-shell">
    <NLayoutHeader bordered class="set-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← 返回
        </NButton>
        <NText strong>设置</NText>
      </NSpace>
    </NLayoutHeader>

    <NLayoutContent class="set-content">
      <div class="cards">
        <NCard title="通用" embedded class="set-card">
          <div class="row">
            <span class="label">主题</span>
            <NRadioGroup
              :value="theme.dark ? 'dark' : 'light'"
              size="small"
              @update:value="(v: string) => (theme.dark = v === 'dark')"
            >
              <NRadioButton value="light">浅色</NRadioButton>
              <NRadioButton value="dark">深色</NRadioButton>
            </NRadioGroup>
          </div>
          <div class="row">
            <span class="label">语言</span>
            <NRadioGroup
              :value="locale"
              size="small"
              @update:value="onLocaleChange"
            >
              <NRadioButton value="zh-CN">简体中文</NRadioButton>
              <NRadioButton value="en-US">English</NRadioButton>
            </NRadioGroup>
          </div>
          <div class="row">
            <div class="label-col">
              <span class="label">启动时自动连接</span>
              <NText depth="3" class="hint">
                打开应用且套餐有效时自动发起连接
              </NText>
            </div>
            <NSwitch
              v-model:value="autoConnectOnStart"
              @update:value="onAutoConnectChange"
            />
          </div>
        </NCard>

        <NCard title="连接" embedded class="set-card">
          <div class="row">
            <div class="label-col">
              <span class="label">默认连接模式</span>
              <NText depth="3" class="hint">
                TUN 全局接管；系统代理为降级方案
              </NText>
            </div>
            <NRadioGroup
              :value="conn.mode"
              size="small"
              @update:value="onModeChange"
            >
              <NRadioButton value="tun">TUN</NRadioButton>
              <NRadioButton value="system_proxy">系统代理</NRadioButton>
            </NRadioGroup>
          </div>
          <div class="row">
            <div class="label-col">
              <span class="label">系统代理守卫</span>
              <NText depth="3" class="hint">
                被其它程序篡改后自动夺回（仅系统代理模式）
              </NText>
            </div>
            <NSwitch
              v-model:value="guardEnabled"
              @update:value="onGuardChange"
            />
          </div>
        </NCard>

        <NCard title="关于" embedded class="set-card">
          <div class="row">
            <span class="label">客户端版本</span>
            <NText depth="3">{{ appVer || "—" }}</NText>
          </div>
          <div class="row">
            <span class="label">核心版本</span>
            <NText depth="3">{{ coreVer || "—" }}</NText>
          </div>
        </NCard>
      </div>
    </NLayoutContent>
  </NLayout>
</template>

<style scoped>
.set-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.set-header {
  display: flex;
  align-items: center;
  padding: 10px 20px;
}
.set-content {
  padding: 20px;
}
.cards {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 640px;
  margin: 0 auto;
}
.set-card {
  border-radius: 10px;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 8px 0;
}
.row + .row {
  border-top: 1px solid rgba(255, 255, 255, 0.05);
}
.label-col {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.label {
  font-size: 13px;
}
.hint {
  font-size: 11.5px;
}
</style>
