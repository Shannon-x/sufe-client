<script setup lang="ts">
// Live kernel log viewer. Subscribes to the `connection://log` event the
// Rust side forwards from `KernelManager::live_logs()`. Ring-buffered to 500
// lines, with level filter, pause, clear and near-bottom auto-follow.
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  NButton,
  NEmpty,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NSelect,
  NSpace,
  NText,
} from "naive-ui";
import type { LogLine } from "@/types";

const router = useRouter();

const MAX_LINES = 500;
const lines = ref<LogLine[]>([]);
const level = ref<"all" | "info" | "warning" | "error">("all");
const paused = ref(false);
const scroller = ref<HTMLElement | null>(null);

let unlisten: UnlistenFn | null = null;

const levelOptions = [
  { label: "全部", value: "all" },
  { label: "信息", value: "info" },
  { label: "警告", value: "warning" },
  { label: "错误", value: "error" },
];

const filtered = computed(() => {
  if (level.value === "all") return lines.value;
  return lines.value.filter((l) => normLevel(l.level) === level.value);
});

function normLevel(raw: string): string {
  const s = raw.toLowerCase();
  if (s.startsWith("warn")) return "warning";
  if (s.startsWith("err")) return "error";
  return "info";
}

function fmtTime(at: string): string {
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString();
}

function nearBottom(): boolean {
  const el = scroller.value;
  if (!el) return true;
  return el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

async function autoFollow() {
  await nextTick();
  const el = scroller.value;
  if (el) el.scrollTop = el.scrollHeight;
}

onMounted(async () => {
  unlisten = await listen<LogLine>("connection://log", (e) => {
    if (paused.value) return;
    const follow = nearBottom();
    lines.value.push(e.payload);
    if (lines.value.length > MAX_LINES) {
      lines.value.splice(0, lines.value.length - MAX_LINES);
    }
    if (follow) void autoFollow();
  });
});

onUnmounted(() => {
  if (unlisten) unlisten();
});
</script>

<template>
  <NLayout class="logs-shell">
    <NLayoutHeader bordered class="logs-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← 返回
        </NButton>
        <NText strong>实时日志</NText>
        <NText depth="3" class="count">{{ filtered.length }} 行</NText>
      </NSpace>
      <NSpace :size="8" align="center">
        <NSelect
          v-model:value="level"
          size="small"
          :options="levelOptions"
          style="width: 110px"
        />
        <NButton
          size="small"
          :type="paused ? 'primary' : 'default'"
          tertiary
          @click="paused = !paused"
        >
          {{ paused ? "继续" : "暂停" }}
        </NButton>
        <NButton size="small" quaternary @click="lines = []">清空</NButton>
      </NSpace>
    </NLayoutHeader>

    <NLayoutContent class="logs-content">
      <NEmpty
        v-if="filtered.length === 0"
        description="暂无日志 — 连接后内核日志会实时显示在这里"
        class="empty"
      />
      <div v-else ref="scroller" class="log-scroll">
        <div
          v-for="(l, i) in filtered"
          :key="i"
          class="log-line"
          :class="normLevel(l.level)"
        >
          <span class="ts">{{ fmtTime(l.at) }}</span>
          <span class="lvl">{{ normLevel(l.level).slice(0, 4) }}</span>
          <span class="msg">{{ l.message }}</span>
        </div>
      </div>
    </NLayoutContent>
  </NLayout>
</template>

<style scoped>
.logs-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.logs-header {
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
.logs-content {
  padding: 12px 16px;
  height: calc(100vh - 56px);
}
.empty {
  margin-top: 64px;
}
.log-scroll {
  height: 100%;
  overflow-y: auto;
  font-family: "SFMono-Regular", ui-monospace, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.55;
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  padding: 8px 10px;
}
.log-line {
  display: flex;
  gap: 10px;
  white-space: pre-wrap;
  word-break: break-word;
  padding: 1px 0;
}
.log-line .ts {
  color: #7c7790;
  flex: 0 0 auto;
  font-variant-numeric: tabular-nums;
}
.log-line .lvl {
  flex: 0 0 34px;
  text-transform: uppercase;
  color: #8a84a0;
}
.log-line.warning .lvl {
  color: #e0a23c;
}
.log-line.error .lvl {
  color: #e05d5d;
}
.log-line.warning .msg {
  color: #f0c27a;
}
.log-line.error .msg {
  color: #ff9a9a;
}
.log-line .msg {
  flex: 1 1 auto;
  color: #cfc9de;
}
</style>
