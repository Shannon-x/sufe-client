<script setup lang="ts">
// Hero card that crowns the left rail. Shows the user the three things that
// matter most at a glance:
//   1. protection state (label + accent color)
//   2. who they are right now on the internet (IP / exit-node / location)
//   3. one big CTA to flip the state
// And a compact TUN ⇆ 系统代理 toggle below, because tunnel mode lives in the
// same conceptual cluster as "connect/disconnect" — keeping it here means the
// node rail underneath can stay a pure node list.
import { computed } from "vue";
import type { TunnelMode } from "@/types";

export type HeroState = "disconnected" | "connecting" | "connected" | "error";

const props = withDefaults(
  defineProps<{
    state: HeroState;
    stateLabel: string;
    primaryLine?: string | null;
    secondaryLine?: string | null;
    ctaLabel: string;
    busy: boolean;
    mode: TunnelMode;
    modeOptions: Array<{ value: TunnelMode; label: string }>;
    // Gating: when false the big CTA is locked. We still render it so the user
    // sees what's there, just with a visual hint that they need a plan first.
    canConnect?: boolean;
    gatedHint?: string;
    // Error-state inline strip — only consumed when `state === "error"`.
    errorMessage?: string;
    retryLabel?: string;
    viewLogsLabel?: string;
  }>(),
  {
    canConnect: true,
    gatedHint: "",
    errorMessage: "",
    retryLabel: "",
    viewLogsLabel: "",
  },
);

const emit = defineEmits<{
  (e: "toggle-connection"): void;
  (e: "change-mode", mode: TunnelMode): void;
  (e: "view-logs"): void;
}>();

// Accent palette per state. Driving everything (label color, button bg, border
// glow) from one source means the card visually "ticks" through red → yellow →
// green as the user lands on a fresh node.
const accent = computed(() => {
  switch (props.state) {
    case "connected":
      return { color: "#4ade80", glow: "rgba(74, 222, 128, 0.32)" };
    case "connecting":
      return { color: "#ffd166", glow: "rgba(255, 209, 102, 0.30)" };
    case "error":
      return { color: "#ff5a7a", glow: "rgba(255, 90, 122, 0.35)" };
    default:
      return { color: "#ff5a7a", glow: "rgba(255, 90, 122, 0.30)" };
  }
});

function onCta() {
  if (props.busy) return;
  if (!props.canConnect) return;
  emit("toggle-connection");
}

function onRetry() {
  if (props.busy) return;
  emit("toggle-connection");
}

function onViewLogs() {
  emit("view-logs");
}

function onModeClick(next: TunnelMode) {
  if (next === props.mode) return;
  emit("change-mode", next);
}

const ctaDisabled = computed(() => props.busy || !props.canConnect);
const isErrorState = computed(() => props.state === "error");
</script>

<template>
  <section
    class="status-hero"
    :style="{
      borderColor: accent.glow,
      boxShadow: `0 8px 28px ${accent.glow}`,
    }"
  >
    <div class="state-row">
      <span class="state-dot" :style="{ background: accent.color, boxShadow: `0 0 10px ${accent.color}` }" />
      <span class="state-label" :style="{ color: accent.color }">{{ stateLabel }}</span>
    </div>

    <p class="primary-line">{{ primaryLine || "—" }}</p>
    <p v-if="isErrorState && errorMessage" class="secondary-line secondary-line--error">
      {{ errorMessage }}
      <span class="hero-inline-actions">
        <button type="button" class="hero-link-btn" :disabled="busy" @click="onRetry">
          {{ retryLabel }}
        </button>
        <span class="hero-link-sep" aria-hidden="true">·</span>
        <button type="button" class="hero-link-btn" @click="onViewLogs">
          {{ viewLogsLabel }}
        </button>
      </span>
    </p>
    <p v-else-if="secondaryLine" class="secondary-line">{{ secondaryLine }}</p>

    <button
      type="button"
      class="cta"
      :class="{ 'cta--gated': !canConnect }"
      :disabled="ctaDisabled"
      :title="!canConnect ? gatedHint : ''"
      :aria-disabled="ctaDisabled || undefined"
      :style="{
        background: canConnect
          ? `linear-gradient(135deg, ${accent.color}, ${accent.color}cc)`
          : 'linear-gradient(135deg, #4a4358, #38314a)',
        boxShadow: canConnect ? `0 12px 24px ${accent.glow}` : 'none',
      }"
      @click="onCta"
    >
      {{ ctaLabel }}
    </button>
    <p v-if="!canConnect && gatedHint" class="cta-hint">{{ gatedHint }}</p>

    <div class="mode-toggle" role="tablist">
      <button
        v-for="opt in modeOptions"
        :key="opt.value"
        type="button"
        role="tab"
        :aria-selected="mode === opt.value"
        :class="{ active: mode === opt.value }"
        @click="onModeClick(opt.value)"
      >
        {{ opt.label }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.status-hero {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 18px 18px 16px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 14px;
  background:
    linear-gradient(180deg, rgba(155, 115, 255, 0.08), rgba(155, 115, 255, 0)) ,
    rgba(24, 20, 36, 0.85);
  transition: border-color 220ms ease, box-shadow 220ms ease;
}

.state-row {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.state-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
}
.state-label {
  font-size: 18px;
  font-weight: 800;
  letter-spacing: 0.01em;
}

.primary-line {
  margin: 4px 0 0;
  color: #f1ecff;
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.secondary-line {
  margin: 0;
  color: #a9a3b8;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cta {
  width: 100%;
  height: 44px;
  margin-top: 12px;
  padding: 0;
  border: 0;
  border-radius: 12px;
  color: #14111a;
  font: inherit;
  font-size: 14px;
  font-weight: 800;
  letter-spacing: 0.02em;
  cursor: pointer;
  transition: filter 180ms ease, transform 120ms ease;
}
.cta:hover:not(:disabled) { filter: brightness(1.08); }
.cta:active:not(:disabled) { transform: translateY(1px); }
.cta:disabled {
  cursor: progress;
  opacity: 0.7;
}
/* Gated style overrides the generic disabled look: dimmer, but with a
   "not-allowed" cursor so the user understands the click won't do anything
   until they pick a plan. */
.cta--gated:disabled {
  cursor: not-allowed;
  color: #d8d2ea;
  opacity: 0.85;
}
.cta-hint {
  margin: 6px 0 0;
  color: #a9a3b8;
  font-size: 11.5px;
  line-height: 1.4;
  text-align: center;
}

.secondary-line--error {
  color: #ffb4c0;
  white-space: normal;
}
.hero-inline-actions {
  display: inline-flex;
  gap: 6px;
  margin-left: 8px;
  align-items: center;
}
.hero-link-btn {
  padding: 0;
  border: 0;
  background: transparent;
  color: #ff9bb0;
  font: inherit;
  font-size: 12px;
  font-weight: 700;
  text-decoration: underline;
  cursor: pointer;
}
.hero-link-btn:hover:not(:disabled) { color: #ffd1da; }
.hero-link-btn:disabled { cursor: progress; opacity: 0.6; }
.hero-link-sep {
  color: #a9a3b8;
  font-size: 12px;
}

.mode-toggle {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 4px;
  margin-top: 10px;
  padding: 3px;
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.05);
}
.mode-toggle button {
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: 7px;
  color: #a9a3b8;
  background: transparent;
  font: inherit;
  font-size: 11.5px;
  font-weight: 700;
  cursor: pointer;
  transition: background 160ms ease, color 160ms ease;
}
.mode-toggle button:hover { color: #fff; }
.mode-toggle button.active {
  color: #fff;
  background: rgba(139, 92, 246, 0.32);
}
</style>
