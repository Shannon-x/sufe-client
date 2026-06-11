<script setup lang="ts">
// Live active-connections monitor — the verge/FlClash "what is my traffic
// actually doing" view. Polls `/connections` once a second while connected,
// supports search + close-one / close-all. Pairs with the live traffic chart.
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NButton,
  NCard,
  NEmpty,
  NInput,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NPopconfirm,
  NSpace,
  NTag,
  NText,
  useMessage,
} from "naive-ui";
import { api } from "@/api";
import { useConnectionStore } from "@/stores/connection";
import TrafficChart from "@/components/TrafficChart.vue";
import type { ConnectionItem } from "@/types";
import { formatError } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const { t } = useI18n();
const conn = useConnectionStore();

const items = ref<ConnectionItem[]>([]);
const search = ref("");
let timer: number | null = null;

const filtered = computed(() => {
  const q = search.value.trim().toLowerCase();
  const rows = [...items.value].sort((a, b) => b.download + b.upload - (a.download + a.upload));
  if (!q) return rows;
  return rows.filter((c) =>
    [c.host, c.destination_ip, c.process, c.rule, c.chains.join(" ")]
      .join(" ")
      .toLowerCase()
      .includes(q),
  );
});

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function ageOf(start: string): string {
  const ts = Date.parse(start);
  if (Number.isNaN(ts)) return "";
  const secs = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  return `${Math.floor(secs / 3600)}h`;
}

async function refresh() {
  if (!conn.isConnected) {
    items.value = [];
    return;
  }
  try {
    items.value = await api.connections();
  } catch {
    // transient — the next tick retries
  }
}

async function closeOne(id: string) {
  try {
    await api.closeConnection(id);
    items.value = items.value.filter((c) => c.id !== id);
  } catch (e) {
    message.error(formatError(e, t));
  }
}

async function closeAll() {
  try {
    await api.closeAllConnections();
    items.value = [];
    message.success("已关闭全部连接");
  } catch (e) {
    message.error(formatError(e, t));
  }
}

onMounted(() => {
  void refresh();
  timer = window.setInterval(refresh, 1000);
});
onUnmounted(() => {
  if (timer !== null) window.clearInterval(timer);
});
</script>

<template>
  <NLayout class="conn-shell">
    <NLayoutHeader bordered class="conn-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← 返回
        </NButton>
        <NText strong>连接监控</NText>
        <NTag size="small" :bordered="false" type="info">
          {{ filtered.length }} 条
        </NTag>
      </NSpace>
      <NSpace :size="8">
        <NInput
          v-model:value="search"
          size="small"
          clearable
          placeholder="搜索 host / IP / 进程 / 规则"
          style="width: 240px"
        />
        <NPopconfirm @positive-click="closeAll">
          <template #trigger>
            <NButton size="small" type="warning" tertiary :disabled="!items.length">
              关闭全部
            </NButton>
          </template>
          确定关闭全部活动连接？
        </NPopconfirm>
      </NSpace>
    </NLayoutHeader>

    <NLayoutContent class="conn-content">
      <NCard embedded class="chart-card" v-if="conn.isConnected">
        <TrafficChart :history="conn.trafficHistory" />
      </NCard>

      <NEmpty
        v-if="!conn.isConnected"
        description="未连接 — 连接后即可查看活动连接"
        class="empty"
      />
      <NEmpty
        v-else-if="filtered.length === 0"
        description="暂无活动连接"
        class="empty"
      />

      <div v-else class="rows">
        <div class="row head">
          <span class="c-host">目标</span>
          <span class="c-chain">代理链 / 规则</span>
          <span class="c-proc">进程</span>
          <span class="c-num">↑/↓</span>
          <span class="c-age">时长</span>
          <span class="c-act" />
        </div>
        <div v-for="c in filtered" :key="c.id" class="row">
          <span class="c-host" :title="`${c.destination_ip}:${c.destination_port}`">
            <NTag size="tiny" :bordered="false">{{ c.network || "tcp" }}</NTag>
            {{ c.host || c.destination_ip }}
          </span>
          <span class="c-chain">
            <span class="chain">{{ c.chains.join(" › ") || "—" }}</span>
            <span class="rule">{{ c.rule }}{{ c.rule_payload ? `(${c.rule_payload})` : "" }}</span>
          </span>
          <span class="c-proc" :title="c.process">{{ c.process || "—" }}</span>
          <span class="c-num">{{ fmtBytes(c.upload) }} / {{ fmtBytes(c.download) }}</span>
          <span class="c-age">{{ ageOf(c.start) }}</span>
          <span class="c-act">
            <NButton size="tiny" quaternary type="error" @click="closeOne(c.id)">
              ✕
            </NButton>
          </span>
        </div>
      </div>
    </NLayoutContent>
  </NLayout>
</template>

<style scoped>
.conn-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.conn-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.conn-content {
  padding: 16px 20px;
}
.chart-card {
  border-radius: 10px;
  margin-bottom: 12px;
}
.empty {
  margin-top: 64px;
}
.rows {
  font-size: 12.5px;
  font-variant-numeric: tabular-nums;
}
.row {
  display: grid;
  grid-template-columns: 2fr 2fr 1.2fr 1.4fr 0.6fr 0.4fr;
  gap: 10px;
  align-items: center;
  padding: 7px 8px;
  border-radius: 6px;
}
.row:hover {
  background: rgba(155, 115, 255, 0.08);
}
.row.head {
  color: var(--n-text-color-3, #9a93ad);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.c-host,
.c-proc {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.c-chain {
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: hidden;
}
.c-chain .chain {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.c-chain .rule {
  font-size: 11px;
  color: var(--n-text-color-3, #9a93ad);
}
.c-num,
.c-age {
  white-space: nowrap;
}
.c-act {
  text-align: right;
}
</style>
