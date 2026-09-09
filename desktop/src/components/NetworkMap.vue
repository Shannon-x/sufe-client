<script setup lang="ts">
import { computed } from "vue";
import { feature } from "topojson-client";
import world from "world-atlas/countries-110m.json";
import type { Topology, GeometryCollection } from "topojson-specification";
import { useNodesStore } from "@/stores/nodes";
import { locateNode } from "@/utils/geo";
const props = defineProps<{ selected?: string | null; connected?: boolean }>();
const nodes = useNodesStore();
type MapFeature = {
  geometry: { type: string; coordinates: number[][][] | number[][][][] };
};
const countries = (
  feature(
    world as unknown as Topology,
    world.objects.countries as GeometryCollection,
  ) as unknown as { features: MapFeature[] }
).features;
const project = (lon: number, lat: number) => [
  (lon + 180) * 2,
  (80 - lat) * 2.6,
];
const outlines = countries.flatMap((f) => {
  const polygons =
    f.geometry.type === "Polygon"
      ? [f.geometry.coordinates as number[][][]]
      : (f.geometry.coordinates as number[][][][]);
  return polygons.map((poly) =>
    poly
      .map(
        (ring) =>
          ring
            .map(
              (p, i) =>
                `${i ? "L" : "M"}${project(p[0], p[1])
                  .map((v) => v.toFixed(1))
                  .join(",")}`,
            )
            .join(" ") + "Z",
      )
      .join(" "),
  );
});
const dots = computed(() => {
  const unique = new Map<string, { label: string; x: number; y: number }>();
  for (const n of nodes.sidebarNodes) {
    if (n.geo) {
      const [x, y] = project(n.geo.lon, n.geo.lat);
      unique.set(n.geo.label, { label: n.geo.country, x, y });
    }
  }
  return [...unique.values()];
});
const selectedPoint = computed(() => {
  const g = locateNode(props.selected || "");
  return g ? { ...g, point: project(g.lon, g.lat) } : null;
});
</script>
<template>
  <div class="network-map">
    <svg
      viewBox="0 0 720 360"
      role="img"
      aria-label="根据订阅节点名称标示的大致地区分布"
    >
      <defs>
        <pattern
          id="map-dots"
          width="5"
          height="5"
          patternUnits="userSpaceOnUse"
        >
          <circle cx="2" cy="2" r="1" fill="currentColor" />
        </pattern>
      </defs>
      <g class="map-land">
        <path
          v-for="(outline, i) in outlines"
          :key="i"
          :d="outline"
          fill="url(#map-dots)"
        />
      </g>
      <g v-for="dot in dots" :key="dot.label">
        <circle :cx="dot.x" :cy="dot.y" r="7" class="map-halo" />
        <circle :cx="dot.x" :cy="dot.y" r="2.8" class="map-dot" />
        <title>{{ dot.label }} · 名称推测位置</title>
      </g>
      <g v-if="selectedPoint">
        <circle
          :cx="selectedPoint.point[0]"
          :cy="selectedPoint.point[1]"
          r="19"
          class="selected-halo"
          :class="{ pulse: connected }"
        />
        <circle
          :cx="selectedPoint.point[0]"
          :cy="selectedPoint.point[1]"
          r="6"
          class="selected-dot"
        />
        <rect
          :x="Math.min(625, selectedPoint.point[0] + 13)"
          :y="selectedPoint.point[1] - 14"
          width="80"
          height="28"
          rx="8"
          class="map-label"
        />
        <text
          :x="Math.min(625, selectedPoint.point[0] + 13) + 40"
          :y="selectedPoint.point[1] + 4"
          text-anchor="middle"
        >
          {{ selectedPoint.country }}
        </text>
      </g></svg
    ><span class="map-caption">节点分布示意 · 根据线路名称识别</span>
  </div>
</template>
<style scoped>
.network-map {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 210px;
  display: flex;
  align-items: center;
  overflow: hidden;
}
.network-map svg {
  width: 100%;
  height: auto;
  overflow: visible;
}
.map-land {
  color: #cdc5ec;
  opacity: 0.65;
}
.map-halo {
  fill: #8c74df14;
}
.map-dot {
  fill: #a396d7;
}
.selected-halo {
  fill: #9c80ec22;
}
.selected-dot {
  fill: #8c69dc;
  stroke: white;
  stroke-width: 2;
}
.map-label {
  fill: var(--surface);
  filter: drop-shadow(0 4px 6px #5d487218);
}
text {
  font-size: 11px;
  fill: var(--primary);
}
.map-caption {
  position: absolute;
  bottom: 0;
  right: 0;
  font-size: 8px;
  color: var(--muted);
  opacity: 0.7;
}
.pulse {
  animation: mapPulse 2.5s ease-in-out infinite;
  transform-box: fill-box;
  transform-origin: center;
}
@keyframes mapPulse {
  50% {
    transform: scale(1.4);
    opacity: 0.4;
  }
}
</style>
