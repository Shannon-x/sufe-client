<script setup lang="ts">
// Read-only routing-rules viewer. Lets the user confirm "is this domain
// going through the proxy or direct" — the only rules affordance that fits a
// panel client (we never edit rules; the subscription owns them).
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  NButton,
  NEmpty,
  NInput,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NSpace,
  NTag,
  NText,
} from "naive-ui";
import { api } from "@/api";
import { useConnectionStore } from "@/stores/connection";
import type { RuleItem } from "@/types";

const router = useRouter();
const conn = useConnectionStore();

const rules = ref<RuleItem[]>([]);
const search = ref("");
const loading = ref(false);

const filtered = computed(() => {
  const q = search.value.trim().toLowerCase();
  if (!q) return rules.value;
  return rules.value.filter((r) =>
    `${r.type} ${r.payload} ${r.proxy}`.toLowerCase().includes(q),
  );
});

function proxyTagType(proxy: string): "success" | "warning" | "error" | "default" {
  const p = proxy.toUpperCase();
  if (p === "DIRECT") return "success";
  if (p === "REJECT" || p === "BLOCK") return "error";
  return "default";
}

async function load() {
  if (!conn.isConnected) {
    rules.value = [];
    return;
  }
  loading.value = true;
  try {
    rules.value = await api.rules();
  } catch {
    rules.value = [];
  } finally {
    loading.value = false;
  }
}

onMounted(load);
</script>

<template>
  <NLayout class="rules-shell">
    <NLayoutHeader bordered class="rules-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← 返回
        </NButton>
        <NText strong>分流规则</NText>
        <NText depth="3" class="count">{{ filtered.length }} 条</NText>
      </NSpace>
      <NSpace :size="8">
        <NInput
          v-model:value="search"
          size="small"
          clearable
          placeholder="搜索规则 / 域名 / 出口"
          style="width: 240px"
        />
        <NButton size="small" quaternary :loading="loading" @click="load">
          刷新
        </NButton>
      </NSpace>
    </NLayoutHeader>

    <NLayoutContent class="rules-content">
      <NEmpty
        v-if="!conn.isConnected"
        description="未连接 — 连接后即可查看当前生效规则"
        class="empty"
      />
      <NEmpty
        v-else-if="filtered.length === 0"
        description="暂无规则"
        class="empty"
      />
      <div v-else class="rows">
        <div
          v-for="(r, i) in filtered"
          :key="i"
          class="row"
        >
          <span class="idx">{{ i + 1 }}</span>
          <NTag size="small" :bordered="false" class="kind">{{ r.type }}</NTag>
          <span class="payload" :title="r.payload">{{ r.payload || "—" }}</span>
          <NTag
            size="small"
            :bordered="false"
            :type="proxyTagType(r.proxy)"
            class="proxy"
          >
            {{ r.proxy }}
          </NTag>
        </div>
      </div>
    </NLayoutContent>
  </NLayout>
</template>

<style scoped>
.rules-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.rules-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.count {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.rules-content {
  padding: 16px 20px;
}
.empty {
  margin-top: 64px;
}
.rows {
  font-size: 12.5px;
  max-width: 880px;
  margin: 0 auto;
}
.row {
  display: grid;
  grid-template-columns: 44px 130px 1fr 120px;
  gap: 10px;
  align-items: center;
  padding: 6px 8px;
  border-radius: 6px;
}
.row:hover {
  background: rgba(155, 115, 255, 0.08);
}
.idx {
  color: var(--n-text-color-3, #9a93ad);
  font-variant-numeric: tabular-nums;
  text-align: right;
}
.payload {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.proxy {
  justify-self: start;
}
</style>
