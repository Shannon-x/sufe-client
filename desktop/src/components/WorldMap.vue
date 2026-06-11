<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { feature, mesh } from "topojson-client";
import type { Topology, GeometryCollection, GeometryObject } from "topojson-specification";
import land50m from "world-atlas/land-50m.json";
import countries50m from "world-atlas/countries-50m.json";

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
    center?: [number, number];
    zoom?: number;
  }>(),
  {
    origin: null,
    center: () => [10, 20],
    zoom: 1.4,
  },
);

const emit = defineEmits<{ (e: "pin-click", id: string): void }>();

const container = ref<HTMLDivElement | null>(null);
let map: maplibregl.Map | null = null;
let mapLoaded = false;
const markers: globalThis.Map<string, maplibregl.Marker> = new globalThis.Map();
let originMarker: maplibregl.Marker | null = null;

// Natural Earth 1:50m, shipped as TopoJSON. We use two layers:
//   * land-50m as the fill, normalized below so antimeridian-crossing rings
//     are split before MapLibre triangulates them.
//   * countries-50m only for *interior* borders via topojson `mesh(...)` with
//     the `a !== b` filter (arcs shared by two different country geometries
//     only). Exterior arcs — which is where antimeridian-wrap artifacts came
//     from — are excluded.
const landTopo = land50m as unknown as Topology<{ land: GeometryObject }>;
const countriesTopo = countries50m as unknown as Topology<{
  countries: GeometryCollection<{ name: string }>;
}>;

// Antimeridian post-processing
// ---------------------------------------------------------------------------
// world-atlas ships a handful of land rings that jump directly between
// +180 and -180 (Russia/Chukotka, Fiji, Antarctica). MapLibre treats that
// edge as a normal in-world segment and triangulates a huge rectangle across
// the map. Split those rings into ordinary polygons that close along the
// antimeridian before handing them to the fill layer.

type LonLat = [number, number];

function samePoint(a: LonLat, b: LonLat): boolean {
  return Math.abs(a[0] - b[0]) < 1e-9 && Math.abs(a[1] - b[1]) < 1e-9;
}

function closeRing(ring: LonLat[]): LonLat[] {
  if (ring.length === 0) return ring;
  return samePoint(ring[0], ring[ring.length - 1]) ? ring : [...ring, ring[0]];
}

function splitRingAtAntimeridian(ring: LonLat[]): LonLat[][] {
  if (ring.length < 4) return [closeRing(ring)];

  const segments: LonLat[][] = [];
  let current: LonLat[] = [ring[0]];

  for (let i = 1; i < ring.length; i++) {
    const prev = ring[i - 1];
    const cur = ring[i];
    const dlon = cur[0] - prev[0];

    if (Math.abs(dlon) <= 180) {
      current.push(cur);
      continue;
    }

    const fromEdge = prev[0] < 0 ? -180 : 180;
    const toEdge = fromEdge === 180 ? -180 : 180;
    const seamLat = (prev[1] + cur[1]) / 2;
    current.push([fromEdge, seamLat]);
    segments.push(current);
    current = [[toEdge, seamLat], cur];
  }

  segments.push(current);

  if (segments.length === 1) return [closeRing(segments[0])];

  if (samePoint(ring[0], ring[ring.length - 1])) {
    const first = segments.shift();
    const last = segments.pop();
    if (first && last) {
      segments.unshift([...last, ...first.slice(1)]);
    }
  }

  return segments
    .map(closeRing)
    .filter((part) => part.length >= 4);
}

function ringCentroid(ring: LonLat[]): LonLat {
  const points = samePoint(ring[0], ring[ring.length - 1]) ? ring.slice(0, -1) : ring;
  const sum = points.reduce(
    (acc, point) => {
      acc[0] += point[0];
      acc[1] += point[1];
      return acc;
    },
    [0, 0] as LonLat,
  );
  return [sum[0] / points.length, sum[1] / points.length];
}

function ringMaxLat(ring: LonLat[]): number {
  return ring.reduce((max, point) => Math.max(max, point[1]), -90);
}

function ringContainsPoint(ring: LonLat[], point: LonLat): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const xi = ring[i][0];
    const yi = ring[i][1];
    const xj = ring[j][0];
    const yj = ring[j][1];
    const crosses = yi > point[1] !== yj > point[1];
    if (crosses) {
      const xAtY = ((xj - xi) * (point[1] - yi)) / (yj - yi) + xi;
      if (point[0] < xAtY) inside = !inside;
    }
  }
  return inside;
}

function splitPolygonAtAntimeridian(rings: LonLat[][]): LonLat[][][] {
  if (rings.length === 0) return [];

  const outerParts = splitRingAtAntimeridian(rings[0]);
  const holes = rings.slice(1);
  return outerParts.map((outer) => [
    outer,
    ...holes.filter((hole) => ringContainsPoint(outer, ringCentroid(hole))),
  ]);
}

function cleanLandGeoJSON(input: GeoJSON.GeoJSON): GeoJSON.FeatureCollection {
  const sourceFeatures =
    input.type === "FeatureCollection" ? input.features :
    input.type === "Feature" ? [input] :
    [{ type: "Feature" as const, properties: {}, geometry: input as GeoJSON.Geometry }];

  const features: GeoJSON.Feature<GeoJSON.Polygon>[] = [];
  for (const sourceFeature of sourceFeatures) {
    const geom = sourceFeature.geometry;
    if (!geom) continue;
    const properties = sourceFeature.properties ?? {};

    if (geom.type === "Polygon") {
      if (geom.coordinates[0] && ringMaxLat(geom.coordinates[0] as LonLat[]) < -58) continue;
      for (const polygon of splitPolygonAtAntimeridian(geom.coordinates as LonLat[][])) {
        features.push({ type: "Feature", properties, geometry: { type: "Polygon", coordinates: polygon } });
      }
    }

    if (geom.type === "MultiPolygon") {
      for (const polygon of geom.coordinates as LonLat[][][]) {
        if (polygon[0] && ringMaxLat(polygon[0]) < -58) continue;
        for (const splitPolygon of splitPolygonAtAntimeridian(polygon)) {
          features.push({ type: "Feature", properties, geometry: { type: "Polygon", coordinates: splitPolygon } });
        }
      }
    }
  }

  return { type: "FeatureCollection", features };
}

const LAND_FC = cleanLandGeoJSON(
  feature(landTopo, landTopo.objects.land) as unknown as GeoJSON.GeoJSON,
);

const RAW_BORDERS = mesh(countriesTopo, countriesTopo.objects.countries, (a, b) => a !== b) as unknown as GeoJSON.MultiLineString;
const INTERIOR_BORDERS = RAW_BORDERS;

const STYLE: maplibregl.StyleSpecification = {
  version: 8,
  sources: {
    land: { type: "geojson", data: LAND_FC as GeoJSON.GeoJSON },
    borders: { type: "geojson", data: INTERIOR_BORDERS as GeoJSON.GeoJSON },
  },
  layers: [
    {
      id: "bg",
      type: "background",
      paint: { "background-color": "#0d0a1f" },
    },
    {
      id: "land-fill",
      type: "fill",
      source: "land",
      paint: {
        "fill-color": "#231a3d",
        "fill-opacity": 1,
        "fill-antialias": true,
      },
    },
    {
      id: "borders-line",
      type: "line",
      source: "borders",
      paint: {
        "line-color": "rgba(210, 210, 235, 0.4)",
        "line-width": 0.5,
      },
    },
  ],
};

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
    const labelEl = el.querySelector(".xb-pin-label") as HTMLSpanElement | null;
    if (labelEl) {
      labelEl.textContent = pin.count > 1 ? `${pin.label} ×${pin.count}` : pin.label;
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
  if (src) {
    src.setData({ type: "FeatureCollection", features: arcs });
  }
}

onMounted(() => {
  if (!container.value) return;
  const rect = container.value.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return;
  try {
    map = new maplibregl.Map({
      container: container.value,
      style: STYLE,
      center: props.center,
      zoom: props.zoom,
      minZoom: 0.3,
      maxZoom: 7,
      attributionControl: false,
      dragRotate: false,
      pitchWithRotate: false,
      renderWorldCopies: false,
    });
  } catch (err) {
    console.error("[WorldMap] map construct failed", err);
    return;
  }
  map.on("error", (e) => {
    console.error("[WorldMap] runtime error", e?.error ?? e);
  });
  map.touchZoomRotate.disableRotation();
  map.on("load", () => {
    mapLoaded = true;
    try {
      map?.fitBounds(
        [
          [-170, -55],
          [180, 75],
        ],
        { padding: 24, duration: 0, animate: false, maxZoom: 3 },
      );
    } catch (err) {
      console.warn("[WorldMap] fitBounds failed", err);
    }
    // Render pins AFTER fitBounds so markers see the final projection.
    map?.once("idle", () => {
      ensureArcSource();
      renderPins();
    });
  });
});

watch(() => props.pins, renderPins, { deep: true });
watch(() => props.origin, renderPins, { deep: true });

onBeforeUnmount(() => {
  for (const m of markers.values()) m.remove();
  markers.clear();
  originMarker?.remove();
  originMarker = null;
  map?.remove();
  map = null;
  mapLoaded = false;
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
  <div class="world-map-wrap">
    <div ref="container" class="world-map" />
  </div>
</template>

<style scoped>
.world-map-wrap {
  width: 100%;
  height: 100%;
  position: relative;
}
.world-map {
  position: absolute;
  inset: 0;
}
</style>

<!-- Global: maplibre injects markers / controls into a subtree that does not
     pick up scoped attribute selectors, so the pin styles live in a non-scoped
     block. The .world-map ancestor scopes them tightly enough in practice. -->
<style>
.world-map .xb-pin {
  cursor: pointer;
  pointer-events: auto;
}
/* maplibre needs the marker element to stay position:absolute (it sets that
   inline + via its .maplibregl-marker class). Don't override `position` here. */
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
    transform: scale(1.6);
    opacity: 0;
  }
}
</style>
