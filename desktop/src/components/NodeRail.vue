<script setup lang="ts">
// Scrollable node directory that lives under the hero card. Contains the
// 国家/收藏 tabs, search box, protocol-filter chips, a pinned "最快服务器"
// row, and the expandable country list.
//
// This component is intentionally dumb: it owns local filter UI state but
// reads node data from the store and forwards row clicks to the parent.
// The parent is responsible for translating a node-name into a kernel
// `selectProxy(group, node)` call (or a connect if the kernel is idle).
import { computed, ref } from "vue";
import { NScrollbar } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useNodesStore } from "@/stores/nodes";

const props = defineProps<{
  // Currently active leaf node name — drives the row highlight.
  activeNodeName?: string | null;
  // Disables row clicks while the kernel is busy switching nodes.
  switching: boolean;
  // Disables the "fastest" row + general connect path while a connect/disconnect
  // is in flight.
  connectBusy: boolean;
  isConnected: boolean;
}>();

const emit = defineEmits<{
  (e: "select-node", name: string): void;
  (e: "fastest"): void;
  (e: "refresh"): void;
}>();

const { t } = useI18n();
const nodesStore = useNodesStore();

type Tab = "country" | "favorite";
const tab = ref<Tab>("country");
const search = ref("");
const kindFilter = ref<string>("all");
const expanded = ref<Set<string>>(new Set());

function toggleExpand(country: string) {
  const next = new Set(expanded.value);
  if (next.has(country)) next.delete(country);
  else next.add(country);
  expanded.value = next;
}

// Country list filtered by the active protocol chip + the search box. We
// fall back to `nodesStore.countries` (which already does the grouping) —
// just trim each country's node list and drop empties.
const filteredCountries = computed(() => {
  const query = search.value.trim().toLowerCase();
  const kind = kindFilter.value;
  return nodesStore.countries
    .map((c) => {
      const matched = c.nodes.filter((n) => {
        if (kind !== "all" && n.kind !== kind) return false;
        if (!query) return true;
        return (
          n.name.toLowerCase().includes(query) ||
          c.country.toLowerCase().includes(query)
        );
      });
      return { ...c, nodes: matched };
    })
    .filter((c) => c.nodes.length > 0);
});

const filteredTotalCount = computed(() =>
  filteredCountries.value.reduce((sum, c) => sum + c.nodes.length, 0),
);

function onSelect(name: string) {
  if (props.switching) return;
  emit("select-node", name);
}

function onFastest() {
  if (props.connectBusy) return;
  emit("fastest");
}
</script>

<template>
  <div class="node-rail">
    <div class="tabs">
      <button
        type="button"
        :class="{ active: tab === 'country' }"
        @click="tab = 'country'"
      >
        {{ t("connect.nodes.tabCountry") }}
      </button>
      <button
        type="button"
        :class="{ active: tab === 'favorite' }"
        @click="tab = 'favorite'"
      >
        {{ t("connect.nodes.tabFavorite") }}
      </button>
    </div>

    <label class="search">
      <span class="icon" aria-hidden="true">⌕</span>
      <input
        v-model="search"
        type="text"
        :placeholder="t('connect.nodes.searchPlaceholder')"
      />
    </label>

    <div class="kinds">
      <button
        type="button"
        :class="{ active: kindFilter === 'all' }"
        @click="kindFilter = 'all'"
      >
        {{ t("connect.nodes.kindAll") }}
        <em>{{ nodesStore.raw.length }}</em>
      </button>
      <button
        v-for="k in nodesStore.kinds"
        :key="k.kind"
        type="button"
        :class="{ active: kindFilter === k.kind }"
        @click="kindFilter = k.kind"
      >
        {{ k.kind.toUpperCase() }}
        <em>{{ k.count }}</em>
      </button>
    </div>

    <div class="section-head">
      <span>{{ t("connect.nodes.tabCountry") }}（{{ filteredCountries.length }}）</span>
      <button
        type="button"
        class="refresh"
        :disabled="nodesStore.loading"
        :title="t('home.refresh')"
        @click="emit('refresh')"
      >↻</button>
    </div>

    <button
      type="button"
      class="row fastest"
      :disabled="connectBusy"
      @click="onFastest"
    >
      <span class="flag">⚡</span>
      <span class="label">{{ t("connect.nodes.fastestServer") }}</span>
      <span class="count">{{ filteredTotalCount }}</span>
      <span class="chev">›</span>
    </button>

    <div class="list">
      <NScrollbar style="height: 100%">
        <div
          v-if="nodesStore.loading && filteredCountries.length === 0"
          class="empty"
        >
          {{ t("connect.nodes.loading") }}
        </div>
        <div
          v-else-if="!nodesStore.loading && filteredCountries.length === 0"
          class="empty"
        >
          {{ nodesStore.raw.length === 0
            ? t("connect.nodes.needsConnect")
            : t("connect.nodes.noMatch") }}
        </div>

        <div
          v-for="c in filteredCountries"
          :key="c.country"
          class="country"
        >
          <button
            type="button"
            class="country-head"
            @click="toggleExpand(c.country)"
          >
            <span class="flag">{{ c.flag }}</span>
            <span class="label">{{ c.country }}</span>
            <span class="count">{{ c.nodes.length }}</span>
            <span class="chev">{{ expanded.has(c.country) ? "˅" : "›" }}</span>
          </button>

          <div v-if="expanded.has(c.country)" class="country-nodes">
            <button
              v-for="node in c.nodes"
              :key="node.name"
              type="button"
              class="row node"
              :class="{ selected: node.name === activeNodeName }"
              :disabled="switching"
              @click="onSelect(node.name)"
            >
              <span class="kind-pill">{{ node.kind.toUpperCase() }}</span>
              <span class="label">{{ node.name }}</span>
              <span v-if="node.name === activeNodeName" class="caret-active">●</span>
            </button>
          </div>
        </div>
      </NScrollbar>
    </div>
  </div>
</template>

<style scoped>
.node-rail {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  gap: 10px;
}

.tabs {
  display: grid;
  grid-template-columns: 1fr 1fr;
  height: 28px;
  padding: 2px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.04);
}
.tabs button {
  height: 100%;
  border: 0;
  border-radius: 7px;
  color: #a9a3b8;
  background: transparent;
  font: inherit;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
  transition: background 160ms ease, color 160ms ease;
}
.tabs button:hover { color: #fff; }
.tabs button.active {
  color: #fff;
  background: rgba(139, 92, 246, 0.28);
}

.search {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 10px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.03);
}
.search .icon {
  color: #6b6680;
  font-size: 13px;
}
.search input {
  flex: 1;
  min-width: 0;
  height: 100%;
  padding: 0;
  border: 0;
  outline: none;
  color: #ede9fa;
  background: transparent;
  font: inherit;
  font-size: 12.5px;
}
.search input::placeholder { color: #6b6680; }

.kinds {
  display: flex;
  flex-wrap: nowrap;
  gap: 6px;
  height: 32px;
  overflow-x: auto;
  scrollbar-width: none;
}
.kinds::-webkit-scrollbar { display: none; }
.kinds button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  height: 24px;
  padding: 0 10px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 12px;
  color: #bdb7cb;
  background: rgba(255, 255, 255, 0.03);
  font: inherit;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.04em;
  cursor: pointer;
}
.kinds button em {
  color: #8f879f;
  font-style: normal;
  font-variant-numeric: tabular-nums;
}
.kinds button:hover { color: #fff; }
.kinds button.active {
  color: #fff;
  border-color: rgba(139, 92, 246, 0.5);
  background: rgba(139, 92, 246, 0.28);
}
.kinds button.active em { color: rgba(255, 255, 255, 0.85); }

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 4px;
  padding: 0 2px;
  color: #8f879f;
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.refresh {
  width: 22px;
  height: 22px;
  border: 0;
  border-radius: 6px;
  color: #c9c2dd;
  background: rgba(255, 255, 255, 0.04);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.refresh:hover:not(:disabled) {
  color: #fff;
  background: rgba(139, 92, 246, 0.22);
}
.refresh:disabled { cursor: not-allowed; opacity: 0.5; }

.row {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-height: 36px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  color: #e9e4f5;
  background: transparent;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 140ms ease;
}
.row:hover { background: rgba(255, 255, 255, 0.05); }
.row .label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.row .count {
  min-width: 24px;
  padding: 1px 6px;
  border-radius: 9px;
  color: #a9a3b8;
  background: rgba(255, 255, 255, 0.06);
  font-size: 11px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  text-align: center;
}
.row .chev {
  width: 12px;
  color: #6b6680;
  font-size: 13px;
  text-align: center;
}
.row .flag {
  font-size: 16px;
  line-height: 1;
}

.fastest {
  margin-top: 2px;
  background: rgba(139, 92, 246, 0.10);
}
.fastest:hover { background: rgba(139, 92, 246, 0.22); }
.fastest .flag { color: #c4adff; }

.list {
  flex: 1;
  min-height: 0;
  margin-top: 2px;
}

.empty {
  padding: 18px 6px;
  color: #6b6680;
  font-size: 12px;
  text-align: center;
}

.country { margin-bottom: 2px; }
.country-head {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 34px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  color: #e9e4f5;
  background: transparent;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.country-head:hover { background: rgba(255, 255, 255, 0.04); }
.country-head .flag { font-size: 16px; line-height: 1; }
.country-head .label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  font-weight: 600;
}
.country-head .count {
  min-width: 24px;
  padding: 1px 6px;
  border-radius: 9px;
  color: #a9a3b8;
  background: rgba(255, 255, 255, 0.06);
  font-size: 11px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  text-align: center;
}
.country-head .chev {
  width: 12px;
  color: #6b6680;
  font-size: 13px;
  text-align: center;
}

.country-nodes {
  margin: 2px 0 6px 18px;
  padding-left: 6px;
  border-left: 1px solid rgba(255, 255, 255, 0.06);
}
.country-nodes .row.node {
  grid-template-columns: 48px minmax(0, 1fr) auto;
  min-height: 32px;
  font-size: 12px;
}
.row.node.selected {
  background: rgba(139, 92, 246, 0.18);
}
.row.node.selected .label { color: #fff; }
.caret-active {
  color: #c4adff;
  font-size: 10px;
}

.kind-pill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 18px;
  padding: 0 8px;
  border-radius: 9px;
  color: #c9c2dd;
  background: rgba(139, 92, 246, 0.18);
  font-size: 10px;
  font-weight: 800;
  letter-spacing: 0.04em;
}
</style>
