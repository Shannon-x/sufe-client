<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { useI18n } from "vue-i18n";
import AppIcon from "@/components/AppIcon.vue";
import { useNodesStore } from "@/stores/nodes";
import { useConnectionStore } from "@/stores/connection";
import { useNodePreferences } from "@/stores/nodePreferences";
import { api } from "@/api";
import { formatError } from "@/utils/error";
const nodes = useNodesStore(),
  conn = useConnectionStore(),
  prefs = useNodePreferences(),
  message = useMessage(),
  { t } = useI18n();
const search = ref(""),
  country = ref("全部地区"),
  sort = ref("default"),
  favoritesOnly = ref(false),
  testing = ref(false),
  selecting = ref("");
const selected = computed(() => conn.effectiveProxy || prefs.selected);
const countries = computed(() => [
  "全部地区",
  ...nodes.countries.map((c) => c.country),
]);
const filtered = computed(() => {
  let list = nodes.sidebarNodes.filter(
    (n) =>
      (!favoritesOnly.value || prefs.favorites.includes(n.name)) &&
      (country.value === "全部地区" ||
        n.geo?.country === country.value ||
        (!n.geo && country.value === "其他")) &&
      `${n.name} ${n.kind} ${n.geo?.country || ""}`
        .toLowerCase()
        .includes(search.value.trim().toLowerCase()),
  );
  if (sort.value === "latency")
    list = [...list].sort(
      (a, b) =>
        (prefs.latency[a.name] || 99999) - (prefs.latency[b.name] || 99999),
    );
  if (sort.value === "name")
    list = [...list].sort((a, b) => a.name.localeCompare(b.name));
  return list;
});
async function select(name: string) {
  if (selecting.value) return;
  selecting.value = name;
  try {
    if (conn.isConnected) {
      const g = conn.proxies.find((p) => p.all.includes(name));
      if (!g) throw new Error("当前代理组中没有该线路，请刷新订阅");
      await conn.selectProxy(g.name, name);
    }
    prefs.select(name);
    message.success(
      conn.isConnected ? "已切换到所选线路" : "已设为下次连接线路",
    );
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    selecting.value = "";
  }
}
async function testAll() {
  if (!conn.isConnected) {
    message.info("连接后即可测试线路延迟");
    return;
  }
  if (testing.value) return;
  testing.value = true;
  const queue = filtered.value.map((n) => n.name);
  await Promise.all(
    Array.from({ length: Math.min(5, queue.length) }, async () => {
      while (queue.length) {
        const name = queue.shift()!;
        try {
          prefs.latency[name] = await api.latencyTest(name);
        } catch {
          prefs.latency[name] = null;
        }
      }
    }),
  );
  testing.value = false;
}
onMounted(() => {
  if (!nodes.raw.length) void nodes.refresh();
});
</script>
<template>
  <div>
    <div class="page-heading">
      <div>
        <h1>每一站，都有好连接</h1>
        <p>全球精选线路，找到适合你的那一条。</p>
      </div>
      <button class="button" :disabled="nodes.loading" @click="nodes.refresh">
        <AppIcon
          name="refresh"
          :size="15"
          :class="{ spin: nodes.loading }"
        />更新订阅
      </button>
    </div>
    <div class="nodes-summary">
      <div class="summary-emblem"><AppIcon name="globe" :size="28" /></div>
      <div>
        <strong>{{ nodes.raw.length }} <small>条可用线路</small></strong>
        <p>
          覆盖 {{ nodes.countries.length }} 个地区 ·
          {{
            selected ? "当前选择：" + selected : "选择线路，下次连接时自动使用"
          }}
        </p>
      </div>
      <button
        class="button soft"
        :disabled="testing || !conn.isConnected"
        @click="testAll"
      >
        <AppIcon
          :name="testing ? 'refresh' : 'signal'"
          :size="16"
          :class="{ spin: testing }"
        />{{ testing ? "正在测速…" : "一键测速" }}
      </button>
    </div>
    <div class="node-toolbar">
      <div class="node-tabs">
        <button
          :class="{ active: !favoritesOnly }"
          @click="favoritesOnly = false"
        >
          全部线路 <span>{{ nodes.raw.length }}</span></button
        ><button
          :class="{ active: favoritesOnly }"
          @click="favoritesOnly = true"
        >
          <AppIcon name="star" :size="14" />我的收藏
          <span>{{ prefs.favorites.length }}</span>
        </button>
      </div>
      <div class="node-filters">
        <label class="search-field"
          ><AppIcon name="search" :size="16" /><input
            v-model="search"
            placeholder="搜索线路或协议"
            aria-label="搜索线路或协议" /></label
        ><select v-model="sort" class="field" aria-label="线路排序">
          <option value="default">默认排序</option>
          <option value="latency">延迟优先</option>
          <option value="name">名称排序</option>
        </select>
      </div>
    </div>
    <div class="country-filters">
      <button
        v-for="c in countries"
        :key="c"
        :class="{ active: country === c }"
        @click="country = c"
      >
        {{ c }}
      </button>
    </div>
    <div v-if="nodes.lastError" class="error-note" role="alert">
      {{ nodes.lastError }}
      <button class="text-button" @click="nodes.refresh">重试</button>
    </div>
    <div v-if="!filtered.length" class="panel empty-state">
      <AppIcon name="globe" :size="42" />
      <h3>
        {{
          nodes.loading
            ? "正在获取专属线路…"
            : search || favoritesOnly
              ? "没有找到匹配的线路"
              : "这里是你的全球连接地图"
        }}
      </h3>
      <p>
        {{
          search || favoritesOnly
            ? "试试其他关键词，或在全部线路中收藏常用节点。"
            : "开通订阅后，线路会自动同步到这里。"
        }}
      </p>
      <RouterLink
        v-if="!nodes.raw.length && !nodes.loading"
        class="button primary"
        to="/plans"
        >查看订阅套餐</RouterLink
      >
    </div>
    <div class="node-grid">
      <article
        v-for="node in filtered"
        :key="node.name"
        class="node-card"
        :class="{ selected: node.name === selected }"
      >
        <button
          class="node-main"
          :disabled="!!selecting"
          @click="select(node.name)"
        >
          <span class="flag">{{ node.geo?.flag || "◎" }}</span
          ><span class="node-text"
            ><strong>{{ node.name }}</strong
            ><small
              >{{ node.geo?.country || "全球线路" }} <i>·</i>
              {{ node.kind.toUpperCase() }}</small
            ></span
          ><span class="select-ring"
            ><AppIcon v-if="node.name === selected" name="check" :size="12"
          /></span>
        </button>
        <div class="node-card-bottom">
          <span
            v-if="prefs.latency[node.name]"
            class="delay"
            :class="{ slow: (prefs.latency[node.name] || 0) > 180 }"
            ><AppIcon name="signal" :size="13" />{{
              prefs.latency[node.name]
            }}
            ms</span
          ><span v-else class="untested">{{
            node.name in prefs.latency ? "暂不可达" : "延迟未测试"
          }}</span
          ><span v-if="node.name === selected" class="selected-tag">已选择</span
          ><button
            class="favorite"
            :class="{ saved: prefs.favorites.includes(node.name) }"
            :aria-label="
              (prefs.favorites.includes(node.name) ? '取消收藏 ' : '收藏 ') +
              node.name
            "
            @click="prefs.toggleFavorite(node.name)"
          >
            <AppIcon name="star" :size="16" />
          </button>
        </div>
      </article>
    </div>
    <p class="nodes-footnote">
      <AppIcon
        name="info"
        :size="13"
      />延迟取决于当前网络环境；连接后可测速，切换线路可能中断正在传输的连接。
    </p>
  </div>
</template>
<style scoped>
.nodes-summary {
  display: flex;
  align-items: center;
  gap: 18px;
  padding: 25px 28px;
  border: 1px solid var(--border);
  background: var(--surface);
  border-radius: 18px;
  margin-bottom: 28px;
}
.summary-emblem {
  width: 58px;
  height: 58px;
  border-radius: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--lavender);
  color: var(--primary);
}
.nodes-summary strong {
  font-size: 26px;
  font-weight: 600;
}
.nodes-summary small {
  font-size: 13px;
  font-weight: 400;
  margin-left: 5px;
}
.nodes-summary p {
  font-size: 11px;
  color: var(--muted);
  margin: 3px 0 0;
}
.nodes-summary > .button {
  margin-left: auto;
  font-size: 11px;
}
.node-toolbar {
  display: flex;
  justify-content: space-between;
  gap: 15px;
  align-items: center;
  border-bottom: 1px solid var(--border);
  margin-bottom: 20px;
}
.node-tabs {
  display: flex;
  gap: 24px;
  align-self: stretch;
}
.node-tabs > button {
  border: 0;
  background: none;
  font-size: 12px;
  display: flex;
  gap: 7px;
  align-items: center;
  padding: 17px 0;
  color: var(--muted);
  border-bottom: 2px solid transparent;
}
.node-tabs > button.active {
  color: var(--primary);
  border-bottom-color: var(--primary);
  font-weight: 550;
}
.node-tabs span {
  font-size: 9px;
  background: var(--border);
  padding: 0 5px;
  border-radius: 4px;
}
.node-filters {
  display: flex;
  gap: 10px;
  padding-bottom: 10px;
}
.search-field {
  display: flex;
  align-items: center;
  gap: 7px;
  background: var(--surface);
  padding: 8px 11px;
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--muted);
}
.search-field input {
  border: 0;
  outline: none;
  background: none;
  color: var(--text);
  width: 130px;
  font-size: 11px;
}
.node-filters select {
  width: 95px;
  font-size: 11px;
  padding: 8px;
}
.country-filters {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin: 0 0 23px;
}
.country-filters button {
  font-size: 11px;
  padding: 6px 12px;
  border-radius: 7px;
  border: 1px solid transparent;
  background: none;
  color: var(--muted);
}
.country-filters button.active {
  border-color: #dcd6f6;
  background: var(--lavender);
  color: var(--primary);
}
.node-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 16px;
}
.node-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  overflow: hidden;
  transition:
    border-color 0.2s,
    box-shadow 0.2s;
}
.node-card.selected {
  border-color: #b7a6ee;
  box-shadow: 0 0 0 2px #a593e60b;
}
.node-main {
  background: none;
  width: 100%;
  padding: 20px 17px 17px;
  display: flex;
  align-items: center;
  gap: 10px;
  border: 0;
  text-align: left;
}
.flag {
  font-size: 23px;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  background: var(--surface-soft);
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  flex-shrink: 0;
}
.node-text {
  min-width: 0;
  flex: 1;
}
.node-text strong {
  font-size: 12px;
  font-weight: 550;
  display: block;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.node-text small {
  font-size: 9px;
  color: var(--muted);
  display: block;
  margin-top: 5px;
}
.node-text i {
  font-style: normal;
  margin: 0 4px;
}
.select-ring {
  width: 17px;
  height: 17px;
  border: 1px solid var(--border);
  border-radius: 50%;
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
}
.selected .select-ring {
  background: var(--primary);
  border-color: var(--primary);
}
.node-card-bottom {
  display: flex;
  gap: 7px;
  align-items: center;
  border-top: 1px solid var(--border);
  padding: 10px 17px;
  font-size: 9px;
}
.delay {
  color: var(--green);
  display: flex;
  gap: 5px;
  align-items: center;
}
.delay.slow {
  color: #ca9a54;
}
.untested {
  color: var(--muted);
}
.favorite {
  padding: 0;
  border: 0;
  background: none;
  margin-left: auto;
  color: #c3c3cf;
  display: flex;
}
.favorite.saved {
  color: #d6a54d;
}
.favorite.saved :deep(svg) {
  fill: #d6a54d33;
}
.selected-tag {
  font-size: 8px;
  color: var(--primary);
  margin-left: auto;
}
.nodes-footnote {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 10px;
  color: var(--muted);
  margin-top: 23px;
}
.error-note {
  margin-bottom: 20px;
}
@media (max-width: 1120px) {
  .node-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .node-tabs {
    gap: 14px;
  }
  .node-filters input {
    width: 105px;
  }
  .node-toolbar {
    flex-wrap: wrap;
  }
}
@media (max-width: 560px) {
  .nodes-summary {
    padding: 20px;
    gap: 12px;
    flex-wrap: wrap;
  }
  .nodes-summary > .button {
    margin-left: 70px;
  }
  .summary-emblem {
    width: 48px;
    height: 48px;
  }
  .nodes-summary p {
    max-width: 225px;
    font-size: 10px;
  }
  .nodes-summary strong {
    font-size: 24px;
  }
  .node-grid {
    grid-template-columns: 1fr;
    gap: 12px;
  }
  .node-toolbar {
    display: block;
  }
  .node-filters {
    margin-top: 12px;
  }
  .search-field {
    flex: 1;
  }
  .node-filters input {
    width: 100%;
  }
  .country-filters {
    gap: 4px;
  }
  .node-main {
    padding: 18px;
  }
  .nodes-footnote {
    align-items: flex-start;
    line-height: 1.8;
  }
  .nodes-footnote svg {
    flex-shrink: 0;
    margin-top: 3px;
  }
}
</style>
