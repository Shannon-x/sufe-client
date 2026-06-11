// Subscribe-derived node directory. Populates immediately after login so the
// sidebar can show real countries + nodes without waiting for the kernel to
// start. The connection store still owns the kernel-side `proxies()` view —
// once mihomo is up, latency from that view feeds back into entries here by
// `name`.

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api } from "@/api";
import type { NodePreview } from "@/types";
import { locateNode, type GeoPoint } from "@/utils/geo";

export interface SidebarNode {
  name: string;
  /** Lowercase protocol family (vless / vmess / trojan / ss / hysteria2 / ...). */
  kind: string;
  server: string;
  port: number;
  /** Heuristic location pulled from name. `null` if nothing matched. */
  geo: GeoPoint | null;
}

export interface CountryGroup {
  country: string;
  flag: string;
  nodes: SidebarNode[];
}

export const useNodesStore = defineStore("nodes", () => {
  const raw = ref<NodePreview[]>([]);
  const loading = ref(false);
  const lastError = ref<string | null>(null);
  const fetchedAt = ref<number | null>(null);

  const sidebarNodes = computed<SidebarNode[]>(() =>
    raw.value.map((n) => ({
      name: n.name,
      kind: n.kind,
      server: n.server,
      port: n.port,
      geo: locateNode(n.name),
    })),
  );

  /// Group by country (Chinese label from GeoPoint). Unknown locations fall
  /// into a "其他" bucket so users can still find them.
  const countries = computed<CountryGroup[]>(() => {
    const byCountry = new Map<string, CountryGroup>();
    for (const node of sidebarNodes.value) {
      const country = node.geo?.country ?? "其他";
      const flag = node.geo?.flag ?? "🌐";
      let entry = byCountry.get(country);
      if (!entry) {
        entry = { country, flag, nodes: [] };
        byCountry.set(country, entry);
      }
      entry.nodes.push(node);
    }
    return [...byCountry.values()].sort((a, b) => {
      if (a.country === "其他") return 1;
      if (b.country === "其他") return -1;
      return b.nodes.length - a.nodes.length;
    });
  });

  /// Distinct lowercase protocol kinds present in the current node set,
  /// each paired with its count. Sort by descending count so the most
  /// common protocol leads the tab row.
  const kinds = computed<Array<{ kind: string; count: number }>>(() => {
    const tally = new Map<string, number>();
    for (const n of raw.value) {
      tally.set(n.kind, (tally.get(n.kind) ?? 0) + 1);
    }
    return [...tally.entries()]
      .map(([kind, count]) => ({ kind, count }))
      .sort((a, b) => b.count - a.count);
  });

  async function refresh(): Promise<void> {
    if (loading.value) return;
    loading.value = true;
    lastError.value = null;
    try {
      raw.value = await api.previewSubscribeNodes();
      fetchedAt.value = Date.now();
    } catch (err) {
      lastError.value = err instanceof Error ? err.message : String(err);
    } finally {
      loading.value = false;
    }
  }

  /// Cheap lookup for the sidebar entry by node name. Used when the
  /// connection store finishes a latency test and wants to overlay it.
  function findByName(name: string): SidebarNode | undefined {
    return sidebarNodes.value.find((n) => n.name === name);
  }

  function reset() {
    raw.value = [];
    fetchedAt.value = null;
    lastError.value = null;
  }

  return {
    raw,
    loading,
    lastError,
    fetchedAt,
    sidebarNodes,
    countries,
    kinds,
    refresh,
    findByName,
    reset,
  };
});
