<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";

export interface NodePin {
  id: string;
  lat: number;
  lon: number;
  label: string;
  country?: string;
  ip?: string;
  active: boolean;
  count: number;
}

export interface OriginPoint {
  lat: number;
  lon: number;
  label?: string;
}

const props = withDefaults(
  defineProps<{
    pins: NodePin[];
    origin?: OriginPoint | null;
    styleUrl?: string;
    center?: [number, number];
    zoom?: number;
  }>(),
  {
    origin: null,
    // OpenFreeMap dark — free, no API key, global CDN. If this is unreachable
    // we fall back to a minimal inline style so the rest of the UI still works.
    styleUrl: "https://tiles.openfreemap.org/styles/dark",
    center: () => [10, 25],
    zoom: 1.4,
  },
);

const emit = defineEmits<{ (e: "pin-click", id: string): void }>();

const container = ref<HTMLDivElement | null>(null);
let map: maplibregl.Map | null = null;
let mapLoaded = false;
const markers: globalThis.Map<string, maplibregl.Marker> = new globalThis.Map();
let originMarker: maplibregl.Marker | null = null;

// Minimal fallback style — flat dark background with only a country-outline
// overlay would need an extra geojson source, so for offline / unreachable
// CDN we just render a flat color and let the markers carry the story.
const FALLBACK_STYLE: maplibregl.StyleSpecification = {
  version: 8,
  sources: {},
  layers: [
    {
      id: "bg",
      type: "background",
      paint: { "background-color": "#0b0820" },
    },
  ],
};

// OpenFreeMap dark uses OpenMapTiles' boundary schema, which renders both
// country borders (admin_level=2) and every subnational division below.
// Province / state / county lines clutter a node map without adding any
// signal, so we hide them after the style loads. Country lines stay.
function hideSubnationalBorders() {
  if (!map) return;
  const style = map.getStyle();
  if (!style?.layers) return;
  for (const layer of style.layers as Array<Record<string, unknown>>) {
    const id = String(layer.id ?? "");
    const sourceLayer = String((layer as { "source-layer"?: string })["source-layer"] ?? "");
    const isAdmin =
      sourceLayer === "boundary" || /admin|boundary/i.test(id);
    if (!isAdmin) continue;

    // Layer-name heuristic — covers OpenFreeMap's own naming (admin_sub*) and
    // most OpenMapTiles-derived styles (state, province, subnational, etc.).
    if (/sub|state|province|district|county|region|admin_level_[3-9]/i.test(id)) {
      try {
        map.setLayoutProperty(id, "visibility", "none");
      } catch {
        /* layer may have been removed by style swap */
      }
      continue;
    }

    // Filter heuristic — some styles keep all borders in one layer and
    // discriminate by `admin_level`. If the layer's filter mentions
    // admin_level 3 or higher, treat it as subnational.
    const filterStr = JSON.stringify((layer as { filter?: unknown }).filter ?? null);
    if (/admin_level[^\d]*([3-9]|1\d)/.test(filterStr)) {
      try {
        map.setLayoutProperty(id, "visibility", "none");
      } catch {
        /* noop */
      }
    }
  }
}

function ensureArcSource() {
  if (!map || !mapLoaded) return;
  if (map.getSource("xboard-arcs")) return;
  map.addSource("xboard-arcs", {
    type: "geojson",
    data: { type: "FeatureCollection", features: [] },
  });
  map.addLayer({
    id: "xboard-arc-glow",
    type: "line",
    source: "xboard-arcs",
    paint: {
      "line-color": "#9b73ff",
      "line-width": 5,
      "line-opacity": 0.22,
      "line-blur": 4,
    },
  });
  map.addLayer({
    id: "xboard-arc-core",
    type: "line",
    source: "xboard-arcs",
    paint: {
      "line-color": "#c4adff",
      "line-width": 1.4,
      "line-opacity": 0.9,
    },
  });
}

function arcFeature(o: OriginPoint, p: NodePin): GeoJSON.Feature<GeoJSON.LineString> {
  // Visually-pleasing arc — not a true great circle, but lifts the midpoint
  // proportional to longitudinal distance so trans-pacific lines bow further
  // than intra-Asia ones. Avoids 180° meridian wrap by clamping deltaLon.
  const steps = 64;
  let deltaLon = p.lon - o.lon;
  if (deltaLon > 180) deltaLon -= 360;
  if (deltaLon < -180) deltaLon += 360;
  const deltaLat = p.lat - o.lat;
  const dist = Math.sqrt(deltaLon * deltaLon + deltaLat * deltaLat);
  const lift = Math.min(15, dist * 0.18);
  const coords: [number, number][] = [];
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    const lon = o.lon + deltaLon * t;
    const lat = o.lat + deltaLat * t + Math.sin(t * Math.PI) * lift;
    coords.push([lon, lat]);
  }
  return {
    type: "Feature",
    geometry: { type: "LineString", coordinates: coords },
    properties: {},
  };
}

function renderPins() {
  if (!map || !mapLoaded) return;

  const incoming = new Set(props.pins.map((p) => p.id));
  for (const [id, m] of markers) {
    if (!incoming.has(id)) {
      m.remove();
      markers.delete(id);
    }
  }

  for (const pin of props.pins) {
    let marker = markers.get(pin.id);
    if (!marker) {
      const el = document.createElement("div");
      el.className = "xb-pin";
      el.innerHTML =
        '<span class="xb-pin-dot"></span>' +
        '<span class="xb-pin-label"></span>';
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        emit("pin-click", pin.id);
      });
      marker = new maplibregl.Marker({ element: el, anchor: "center" })
        .setLngLat([pin.lon, pin.lat])
        .addTo(map);
      markers.set(pin.id, marker);
    } else {
      marker.setLngLat([pin.lon, pin.lat]);
    }
    const el = marker.getElement();
    el.dataset.active = pin.active ? "1" : "0";
    el.dataset.count = String(pin.count);
    const labelEl = el.querySelector(".xb-pin-label");
    if (labelEl) {
      labelEl.textContent = pin.country || pin.label;
    }
  }

  if (originMarker) {
    originMarker.remove();
    originMarker = null;
  }
  if (props.origin) {
    const el = document.createElement("div");
    el.className = "xb-origin";
    el.innerHTML =
      '<span class="xb-origin-core"></span>' +
      '<span class="xb-origin-pulse"></span>' +
      `<span class="xb-origin-label">${props.origin.label ?? "You"}</span>`;
    originMarker = new maplibregl.Marker({ element: el, anchor: "center" })
      .setLngLat([props.origin.lon, props.origin.lat])
      .addTo(map);
  }

  const arcs = props.origin
    ? props.pins.filter((p) => p.active).map((p) => arcFeature(props.origin!, p))
    : [];
  const src = map.getSource("xboard-arcs") as maplibregl.GeoJSONSource | undefined;
  if (src) src.setData({ type: "FeatureCollection", features: arcs });
}

onMounted(() => {
  if (!container.value) return;
  map = new maplibregl.Map({
    container: container.value,
    style: props.styleUrl,
    center: props.center,
    zoom: props.zoom,
    minZoom: 0.8,
    maxZoom: 7,
    attributionControl: false,
    dragRotate: false,
    pitchWithRotate: false,
    renderWorldCopies: true,
  });
  map.touchZoomRotate.disableRotation();
  map.on("load", () => {
    mapLoaded = true;
    hideSubnationalBorders();
    ensureArcSource();
    renderPins();
  });
  // `style.load` fires for every setStyle call, including the offline-fallback
  // swap below — re-apply the hide so the rule survives style changes.
  map.on("styledata", () => {
    if (mapLoaded) hideSubnationalBorders();
  });
  // If the OpenFreeMap CDN is unreachable (offline / GFW timeout), swap to
  // the minimal flat dark style so the markers still render rather than
  // leaving a blank container.
  map.on("error", (e: maplibregl.MapLibreEvent & { error?: { status?: number } }) => {
    const err = (e as unknown as { error?: { message?: string; status?: number } }).error;
    if (!mapLoaded && err && (err.status === 0 || err.status === undefined)) {
      try {
        map?.setStyle(FALLBACK_STYLE);
      } catch {
        /* noop */
      }
    }
  });
});

watch(() => props.pins, renderPins, { deep: true });
watch(() => props.origin, renderPins, { deep: true });

onBeforeUnmount(() => {
  for (const m of markers.values()) m.remove();
  markers.clear();
  originMarker?.remove();
  map?.remove();
  map = null;
});

defineExpose({
  flyTo(lat: number, lon: number, zoom = 3.2) {
    map?.flyTo({ center: [lon, lat], zoom, duration: 1400 });
  },
  resetView() {
    map?.flyTo({ center: props.center, zoom: props.zoom, duration: 1000 });
  },
});
</script>

<template>
  <div ref="container" class="world-map" />
</template>

<style scoped>
.world-map {
  width: 100%;
  height: 100%;
  position: relative;
}
</style>

<!-- Global: maplibre injects markers / controls into a subtree that does not
     pick up scoped attribute selectors, so the pin styles live in a non-scoped
     block. The .world-map ancestor scopes them tightly enough in practice. -->
<style>
.world-map .xb-pin {
  position: relative;
  cursor: pointer;
  pointer-events: auto;
}
.world-map .xb-pin .xb-pin-dot {
  display: block;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: radial-gradient(circle, #c4adff 0%, #6f4af5 70%);
  box-shadow:
    0 0 0 3px rgba(155, 115, 255, 0.16),
    0 0 12px rgba(155, 115, 255, 0.55);
  transition: transform 0.2s ease;
}
.world-map .xb-pin[data-active="1"] .xb-pin-dot {
  width: 14px;
  height: 14px;
  background: radial-gradient(circle, #ffffff 0%, #c4adff 55%, #7d52ff 100%);
  box-shadow:
    0 0 0 5px rgba(196, 173, 255, 0.3),
    0 0 22px rgba(196, 173, 255, 0.95);
  animation: xb-pulse 1.8s ease-out infinite;
}
.world-map .xb-pin .xb-pin-label {
  display: none;
  position: absolute;
  left: 50%;
  bottom: calc(100% + 6px);
  transform: translateX(-50%);
  padding: 4px 8px;
  background: rgba(15, 9, 28, 0.92);
  border: 1px solid rgba(155, 115, 255, 0.35);
  color: #eee5ff;
  font-size: 11px;
  border-radius: 4px;
  white-space: nowrap;
  pointer-events: none;
}
.world-map .xb-pin:hover .xb-pin-label,
.world-map .xb-pin[data-active="1"] .xb-pin-label {
  display: block;
}

.world-map .xb-origin {
  position: relative;
  pointer-events: none;
}
.world-map .xb-origin .xb-origin-core {
  display: block;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #ff5a7a;
  box-shadow:
    0 0 0 3px rgba(255, 90, 122, 0.22),
    0 0 12px rgba(255, 90, 122, 0.55);
}
.world-map .xb-origin .xb-origin-pulse {
  position: absolute;
  inset: -8px;
  border-radius: 50%;
  border: 1px solid rgba(255, 90, 122, 0.55);
  animation: xb-pulse 1.8s ease-out infinite;
}
.world-map .xb-origin .xb-origin-label {
  position: absolute;
  left: calc(100% + 6px);
  top: -2px;
  color: #ff8aa3;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
}

@keyframes xb-pulse {
  0% {
    transform: scale(0.85);
    opacity: 1;
  }
  100% {
    transform: scale(2.2);
    opacity: 0;
  }
}

.world-map .maplibregl-ctrl-bottom-right,
.world-map .maplibregl-ctrl-bottom-left {
  display: none;
}
</style>
