<script setup lang="ts">
// 84px-wide vertical menu column on the right edge. Each cell shows an
// always-visible Lucide-style stroke icon plus a Chinese label so users
// never need to hover to discover what an action does (Proton VPN-style).
import { computed } from "vue";
import { useI18n } from "vue-i18n";

export type RailAction =
  | "refresh"
  | "connections"
  | "logs"
  | "rules"
  | "notices"
  | "plans"
  | "tickets"
  | "helper"
  | "settings";

const props = defineProps<{
  active?: RailAction | null;
}>();

const emit = defineEmits<{
  (e: "action", key: RailAction): void;
}>();

const { t } = useI18n();

const items = computed<Array<{ key: RailAction; label: string }>>(() => [
  { key: "refresh", label: t("home.refresh") },
  { key: "connections", label: "连接" },
  { key: "logs", label: t("connect.viewLogs") },
  { key: "rules", label: "规则" },
  { key: "notices", label: t("home.menu.notices") },
  { key: "plans", label: t("home.menu.plans") },
  { key: "tickets", label: t("home.menu.tickets") },
  { key: "helper", label: t("home.menu.helper") },
  { key: "settings", label: "设置" },
]);

function onClick(key: RailAction) {
  emit("action", key);
}
</script>

<template>
  <aside class="icon-rail">
    <button
      v-for="item in items"
      :key="item.key"
      type="button"
      class="cell"
      :class="{ 'is-active': props.active === item.key }"
      @click="onClick(item.key)"
    >
      <span class="icon" aria-hidden="true">
        <!-- refresh-cw -->
        <svg
          v-if="item.key === 'refresh'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M21 12a9 9 0 0 1-15.36 6.36L3 16" />
          <path d="M3 12a9 9 0 0 1 15.36-6.36L21 8" />
          <polyline points="21 3 21 8 16 8" />
          <polyline points="3 21 3 16 8 16" />
        </svg>
        <!-- file-text -->
        <svg
          v-else-if="item.key === 'logs'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
          <polyline points="14 2 14 8 20 8" />
          <line x1="8" y1="13" x2="16" y2="13" />
          <line x1="8" y1="17" x2="16" y2="17" />
          <line x1="8" y1="9" x2="10" y2="9" />
        </svg>
        <!-- activity (connections) -->
        <svg
          v-else-if="item.key === 'connections'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
        </svg>
        <!-- filter (rules) -->
        <svg
          v-else-if="item.key === 'rules'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
        </svg>
        <!-- bell -->
        <svg
          v-else-if="item.key === 'notices'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" />
          <path d="M13.73 21a2 2 0 0 1-3.46 0" />
        </svg>
        <!-- gem -->
        <svg
          v-else-if="item.key === 'plans'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polygon points="6 3 18 3 22 9 12 22 2 9 6 3" />
          <path d="M11 3 8 9l4 13 4-13-3-6" />
          <line x1="2" y1="9" x2="22" y2="9" />
        </svg>
        <!-- message-square -->
        <svg
          v-else-if="item.key === 'tickets'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
        </svg>
        <!-- shield (helper) -->
        <svg
          v-else-if="item.key === 'helper'"
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
          <polyline points="9 12 11 14 15 10" />
        </svg>
        <!-- settings -->
        <svg
          v-else
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </span>
      <span class="label">{{ item.label }}</span>
    </button>
  </aside>
</template>

<style scoped>
.icon-rail {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 12px 6px;
  background: rgba(18, 15, 25, 0.6);
  border-left: 1px solid rgba(255, 255, 255, 0.06);
}

.cell {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  width: 72px;
  min-height: 68px;
  padding: 8px 4px;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: #c9c2dd;
  cursor: pointer;
  transition: background 160ms ease, color 160ms ease;
  -webkit-app-region: no-drag;
}
.cell:hover,
.cell:focus-visible {
  color: #fff;
  background: rgba(155, 115, 255, 0.18);
  outline: none;
}
.cell.is-active {
  color: #fff;
  background: rgba(155, 115, 255, 0.22);
}
.cell.is-active::before {
  content: "";
  position: absolute;
  left: -6px;
  top: 12px;
  bottom: 12px;
  width: 3px;
  border-radius: 2px;
  background: #9b73ff;
}

.icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
}

.label {
  font-size: 11px;
  line-height: 1.1;
  letter-spacing: 0.02em;
  text-align: center;
  white-space: nowrap;
}
</style>
