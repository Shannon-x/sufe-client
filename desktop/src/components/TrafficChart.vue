<script setup lang="ts">
// Lightweight SVG sparkline of up/down throughput. Fed by
// `connectionStore.trafficHistory` (one sample/sec). No canvas / Worker —
// the airport use-case never has enough points to need them.
import { computed } from "vue";

const props = defineProps<{
  history: Array<{ up: number; down: number }>;
  height?: number;
}>();

const VIEW_W = 600;
const viewH = computed(() => props.height ?? 110);

function fmtRate(bytes: number): string {
  if (bytes < 1024) return `${bytes} B/s`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB/s`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB/s`;
}

// Headroom so a flat-out line doesn't touch the top edge.
const maxVal = computed(() => {
  let m = 1;
  for (const s of props.history) {
    if (s.up > m) m = s.up;
    if (s.down > m) m = s.down;
  }
  return m * 1.15;
});

function pointsFor(key: "up" | "down"): string {
  const h = props.history;
  const n = h.length;
  if (n === 0) return "";
  const max = maxVal.value;
  const hh = viewH.value;
  return h
    .map((s, i) => {
      const x = n === 1 ? VIEW_W : (i / (n - 1)) * VIEW_W;
      const y = hh - (s[key] / max) * (hh - 6) - 3;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

const upPoints = computed(() => pointsFor("up"));
const downPoints = computed(() => pointsFor("down"));
const latest = computed(
  () => props.history[props.history.length - 1] ?? { up: 0, down: 0 },
);
</script>

<template>
  <div class="traffic-chart">
    <div class="legend">
      <span class="item">
        <i class="dot down" />下载 {{ fmtRate(latest.down) }}
      </span>
      <span class="item">
        <i class="dot up" />上传 {{ fmtRate(latest.up) }}
      </span>
    </div>
    <svg
      class="canvas"
      :viewBox="`0 0 ${VIEW_W} ${viewH}`"
      preserveAspectRatio="none"
    >
      <polyline
        v-if="downPoints"
        :points="downPoints"
        fill="none"
        stroke="#5b9bff"
        stroke-width="2"
        vector-effect="non-scaling-stroke"
      />
      <polyline
        v-if="upPoints"
        :points="upPoints"
        fill="none"
        stroke="#52c08a"
        stroke-width="2"
        vector-effect="non-scaling-stroke"
      />
    </svg>
  </div>
</template>

<style scoped>
.traffic-chart {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.legend {
  display: flex;
  gap: 16px;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--n-text-color-3, #c9c2dd);
}
.item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.dot.up {
  background: #52c08a;
}
.dot.down {
  background: #5b9bff;
}
.canvas {
  width: 100%;
  height: v-bind('viewH + "px"');
  display: block;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.03);
}
</style>
