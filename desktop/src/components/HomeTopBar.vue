<script setup lang="ts">
// 56px chrome at the top of the home page. Three clusters:
//   - left  : brand mark + product name
//   - center: live state pill (color reflects connection kind) + IP / location
//   - right : avatar dropdown (email + account-menu entries)
//
// All copy comes from i18n; the only literals are the dot / chevron glyphs
// and the "未受保护 / 正在连接 / 已保护" status labels — those mirror what the
// rest of the redesign uses and Chinese is the only shipped locale.
import { computed } from "vue";
import { NDropdown, NButton } from "naive-ui";
import type { DropdownOption } from "naive-ui";

export type ConnectionPillKind = "disconnected" | "connecting" | "connected" | "error";

const props = defineProps<{
  appTitle: string;
  pillKind: ConnectionPillKind;
  statusLabel: string;
  ipLabel?: string | null;
  locationLabel?: string | null;
  email?: string | null;
  menuOptions: DropdownOption[];
}>();

const emit = defineEmits<{ (e: "menu-select", key: string): void }>();

function onSelect(key: string | number) {
  emit("menu-select", String(key));
}


const pillTone = computed(() => {
  switch (props.pillKind) {
    case "connected":
      return { color: "#4ade80", bg: "rgba(74, 222, 128, 0.14)" };
    case "connecting":
      return { color: "#ffd166", bg: "rgba(255, 209, 102, 0.14)" };
    case "error":
      return { color: "#ff5a7a", bg: "rgba(255, 90, 122, 0.16)" };
    default:
      return { color: "#ff5a7a", bg: "rgba(255, 90, 122, 0.14)" };
  }
});

const trailingMeta = computed(() => {
  const parts: string[] = [];
  if (props.ipLabel) parts.push(`IP · ${props.ipLabel}`);
  if (props.locationLabel) parts.push(props.locationLabel);
  return parts.join("  /  ");
});
</script>

<template>
  <header class="top-bar">
    <div class="cluster left">
      <span class="brand-mark" aria-hidden="true" />
      <span class="brand-text">{{ appTitle }}</span>
    </div>

    <div class="cluster center">
      <span
        class="status-pill"
        :style="{ color: pillTone.color, background: pillTone.bg }"
      >
        <span
          class="dot"
          :class="{ pulse: pillKind === 'connecting' }"
          :style="{ background: pillTone.color }"
        />
        {{ statusLabel }}
      </span>
      <span v-if="trailingMeta" class="meta">{{ trailingMeta }}</span>
    </div>

    <div class="cluster right">
      <NDropdown
        trigger="click"
        :options="menuOptions"
        :show-arrow="true"
        placement="bottom-end"
        @select="onSelect"
      >
        <NButton size="small" quaternary class="account-btn">
          <span class="avatar" aria-hidden="true">{{ (email?.[0] ?? '?').toUpperCase() }}</span>
          <span class="email">{{ email ?? "" }}</span>
          <span class="caret">▾</span>
        </NButton>
      </NDropdown>
    </div>
  </header>
</template>

<style scoped>
.top-bar {
  display: grid;
  grid-template-columns: minmax(220px, 1fr) auto minmax(220px, 1fr);
  align-items: center;
  height: 56px;
  padding: 0 20px;
  background: #1a1626;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.cluster {
  display: inline-flex;
  align-items: center;
  gap: 12px;
}
.cluster.left { justify-self: start; }
.cluster.center { justify-self: center; }
.cluster.right { justify-self: end; }

.brand-mark {
  width: 26px;
  height: 26px;
  border-radius: 8px;
  background: linear-gradient(135deg, #8a5cf6, #00c48c);
  box-shadow: 0 4px 12px rgba(138, 92, 246, 0.35);
}
.brand-text {
  color: #f8f7ff;
  font-size: 15px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 12px;
  border-radius: 13px;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.02em;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  box-shadow: 0 0 6px currentColor;
}
.dot.pulse { animation: pillpulse 1.4s ease-in-out infinite; }
@keyframes pillpulse {
  0%, 100% { opacity: 0.55; transform: scale(0.85); }
  50% { opacity: 1; transform: scale(1.15); }
}

.meta {
  color: #a9a3b8;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.account-btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: #ede9fa;
}
.avatar {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: linear-gradient(135deg, #6e49ff, #c4adff);
  color: #fff;
  font-size: 11px;
  font-weight: 800;
}
.email {
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.caret {
  color: #8f879f;
  font-size: 10px;
}
</style>
