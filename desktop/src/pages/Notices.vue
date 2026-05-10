<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NButton,
  NCard,
  NEmpty,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NSkeleton,
  NSpace,
  NTag,
  NText,
  useMessage,
} from "naive-ui";
import { api } from "@/api";
import type { Notice } from "@/types";
import { formatError } from "@/utils/error";

const { t } = useI18n();
const router = useRouter();
const message = useMessage();

const notices = ref<Notice[]>([]);
const loading = ref(true);

async function load() {
  loading.value = true;
  try {
    notices.value = await api.fetchNotices();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}

onMounted(load);

// Backend stores HTML for content. We render it as plain text — strip tags
// and decode common entities to keep the UI safe and readable. Keep the
// transform here (not on the server) so a future "show original HTML"
// affordance can opt in deliberately.
function plainContent(html: string): string {
  if (!html) return "";
  // 1. Insert blank lines between block-level closes so paragraphs survive.
  const withBreaks = html
    .replace(/<\s*br\s*\/?\s*>/gi, "\n")
    .replace(/<\/(p|div|li|h[1-6])>/gi, "\n\n");
  // 2. Drop every tag.
  const stripped = withBreaks.replace(/<[^>]+>/g, "");
  // 3. Decode the handful of HTML entities we'll actually see in panels.
  const decoded = stripped
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
  // 4. Collapse runs of >2 blank lines.
  return decoded.replace(/\n{3,}/g, "\n\n").trim();
}

function fmtDate(unix: number | null | undefined): string {
  if (!unix) return "";
  return new Date(unix * 1000).toLocaleString();
}

const empty = computed(() => !loading.value && notices.value.length === 0);
</script>

<template>
  <NLayout class="notices-shell">
    <NLayoutHeader bordered class="notices-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← {{ t("notices.back") }}
        </NButton>
        <NText strong>{{ t("notices.title") }}</NText>
      </NSpace>
      <NButton size="small" quaternary :loading="loading" @click="load">
        {{ t("notices.refresh") }}
      </NButton>
    </NLayoutHeader>

    <NLayoutContent class="notices-content">
      <div class="list">
        <template v-if="loading && notices.length === 0">
          <NCard v-for="i in 3" :key="i" embedded class="notice-card">
            <NSkeleton text :repeat="4" />
          </NCard>
        </template>

        <NEmpty v-else-if="empty" :description="t('notices.empty')" />

        <NCard
          v-for="n in notices"
          :key="n.id"
          embedded
          class="notice-card"
          :title="n.title || t('notices.untitled')"
        >
          <template #header-extra>
            <NText depth="3" class="meta">{{ fmtDate(n.created_at) }}</NText>
          </template>

          <NSpace v-if="n.tags.length" :size="4" class="tag-row">
            <NTag
              v-for="(tag, idx) in n.tags"
              :key="`${n.id}-${idx}`"
              size="small"
              :bordered="false"
              type="info"
            >
              {{ tag }}
            </NTag>
          </NSpace>

          <img
            v-if="n.img_url"
            :src="n.img_url"
            class="notice-img"
            :alt="n.title"
          />

          <!-- Plain-text only; never v-html — backend content is staff-authored
               but treated as untrusted. -->
          <pre class="notice-body">{{ plainContent(n.content) }}</pre>
        </NCard>
      </div>
    </NLayoutContent>
  </NLayout>
</template>

<style scoped>
.notices-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.notices-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.notices-content {
  padding: 20px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 760px;
  margin: 0 auto;
}
.notice-card {
  border-radius: 10px;
}
.meta {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.tag-row {
  margin-bottom: 8px;
}
.notice-img {
  display: block;
  max-width: 100%;
  border-radius: 6px;
  margin: 4px 0 10px;
}
.notice-body {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 13px;
  line-height: 1.65;
}
</style>
