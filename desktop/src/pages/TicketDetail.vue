<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NAlert,
  NButton,
  NCard,
  NInput,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NPopconfirm,
  NSkeleton,
  NSpace,
  NTag,
  NText,
  useMessage,
} from "naive-ui";
import { api } from "@/api";
import type { TicketDetail } from "@/types";
import { formatError } from "@/utils/error";

const props = defineProps<{ id: number }>();

const { t } = useI18n();
const router = useRouter();
const message = useMessage();

const ticket = ref<TicketDetail | null>(null);
const loading = ref(true);
const replyDraft = ref("");
const replying = ref(false);
const closing = ref(false);

// Maps mirror Tickets.vue. Keep them in lockstep — divergence will surface
// as colour swaps that confuse users navigating list ↔ detail.
const STATUS_META: Record<
  number,
  { labelKey: string; type: "default" | "success" | "warning" }
> = {
  0: { labelKey: "tickets.status.open", type: "warning" },
  1: { labelKey: "tickets.status.closed", type: "default" },
};
const LEVEL_META: Record<
  number,
  { labelKey: string; type: "default" | "info" | "warning" | "error" }
> = {
  0: { labelKey: "tickets.level.low", type: "default" },
  1: { labelKey: "tickets.level.normal", type: "info" },
  2: { labelKey: "tickets.level.high", type: "error" },
};

async function load() {
  loading.value = true;
  try {
    ticket.value = await api.fetchTicket(props.id);
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}

onMounted(load);
watch(() => props.id, load);

const isOpen = computed(() => ticket.value?.status === 0);

const messageContainer = ref<HTMLDivElement | null>(null);
async function scrollToLatest() {
  await nextTick();
  const el = messageContainer.value;
  if (el) el.scrollTop = el.scrollHeight;
}
watch(
  () => ticket.value?.message.length,
  (n) => {
    if (n) void scrollToLatest();
  },
);

async function onReply() {
  const trimmed = replyDraft.value.trim();
  if (!trimmed) {
    message.warning(t("tickets.reply.empty"));
    return;
  }
  replying.value = true;
  try {
    await api.replyTicket(props.id, trimmed);
    replyDraft.value = "";
    await load();
    message.success(t("tickets.reply.sent"));
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    replying.value = false;
  }
}

async function onClose() {
  closing.value = true;
  try {
    await api.closeTicket(props.id);
    await load();
    message.success(t("tickets.closed"));
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    closing.value = false;
  }
}

function fmtDate(unix: number | null | undefined): string {
  if (!unix) return "";
  return new Date(unix * 1000).toLocaleString();
}

// Same XSS-safe HTML→text scrub used in Notices/Plans. Staff replies
// occasionally contain panel-rendered HTML; user replies are plain text
// but won't break the transform.
function plain(html: string): string {
  if (!html) return "";
  const withBreaks = html
    .replace(/<\s*br\s*\/?\s*>/gi, "\n")
    .replace(/<\/(p|div|li|h[1-6])>/gi, "\n\n");
  const stripped = withBreaks.replace(/<[^>]+>/g, "");
  const decoded = stripped
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
  return decoded.replace(/\n{3,}/g, "\n\n").trim();
}
</script>

<template>
  <NLayout class="td-shell">
    <NLayoutHeader bordered class="td-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'tickets' })">
          ← {{ t("tickets.back") }}
        </NButton>
        <NText strong>{{ t("tickets.detailTitle") }}</NText>
      </NSpace>
      <NButton size="small" quaternary :loading="loading" @click="load">
        {{ t("tickets.refresh") }}
      </NButton>
    </NLayoutHeader>

    <NLayoutContent class="td-content">
      <div class="container">
        <NSkeleton v-if="loading && !ticket" text :repeat="6" />

        <template v-else-if="ticket">
          <NCard embedded class="meta-card">
            <NSpace vertical :size="8">
              <NText strong class="subject">
                {{ ticket.subject || `#${ticket.id}` }}
              </NText>
              <NSpace :size="6">
                <NTag
                  size="small"
                  :bordered="false"
                  :type="STATUS_META[ticket.status]?.type ?? 'default'"
                >
                  {{
                    STATUS_META[ticket.status]
                      ? t(STATUS_META[ticket.status].labelKey)
                      : `#${ticket.status}`
                  }}
                </NTag>
                <NTag
                  size="small"
                  :bordered="false"
                  :type="LEVEL_META[ticket.level]?.type ?? 'default'"
                >
                  {{
                    LEVEL_META[ticket.level]
                      ? t(LEVEL_META[ticket.level].labelKey)
                      : `#${ticket.level}`
                  }}
                </NTag>
                <NText depth="3" class="meta-date">
                  {{ t("tickets.createdAt") }}: {{ fmtDate(ticket.created_at) }}
                </NText>
              </NSpace>
            </NSpace>
          </NCard>

          <div ref="messageContainer" class="thread">
            <div
              v-for="m in ticket.message"
              :key="m.id"
              class="bubble-row"
              :class="{ mine: m.is_me }"
            >
              <div class="bubble" :class="{ mine: m.is_me }">
                <pre class="bubble-body">{{ plain(m.message) }}</pre>
                <div class="bubble-meta">{{ fmtDate(m.created_at) }}</div>
              </div>
            </div>
            <NText
              v-if="ticket.message.length === 0"
              depth="3"
              class="no-msg"
            >
              {{ t("tickets.noMessages") }}
            </NText>
          </div>

          <NCard v-if="isOpen" embedded class="reply-card">
            <NInput
              v-model:value="replyDraft"
              type="textarea"
              :placeholder="t('tickets.reply.placeholder')"
              :autosize="{ minRows: 3, maxRows: 8 }"
              :disabled="replying"
              maxlength="2000"
              show-count
            />
            <NSpace justify="space-between" class="reply-actions">
              <NPopconfirm
                :positive-text="t('tickets.confirm.yes')"
                :negative-text="t('tickets.confirm.no')"
                @positive-click="onClose"
              >
                <template #trigger>
                  <NButton size="small" type="error" ghost :loading="closing">
                    {{ t("tickets.close") }}
                  </NButton>
                </template>
                {{ t("tickets.confirm.close") }}
              </NPopconfirm>
              <NButton
                type="primary"
                :loading="replying"
                :disabled="!replyDraft.trim()"
                @click="onReply"
              >
                {{ t("tickets.reply.send") }}
              </NButton>
            </NSpace>
          </NCard>

          <NAlert
            v-else
            type="default"
            :show-icon="false"
            class="closed-alert"
          >
            {{ t("tickets.closedHint") }}
          </NAlert>
        </template>
      </div>
    </NLayoutContent>
  </NLayout>
</template>

<style scoped>
.td-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.td-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.td-content {
  padding: 20px;
}
.container {
  max-width: 760px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.meta-card {
  border-radius: 10px;
}
.subject {
  font-size: 16px;
}
.meta-date {
  font-size: 12px;
  margin-left: 4px;
}
.thread {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 4px;
  max-height: 60vh;
  overflow-y: auto;
}
.no-msg {
  text-align: center;
  padding: 24px;
  font-size: 13px;
}
.bubble-row {
  display: flex;
  justify-content: flex-start;
}
.bubble-row.mine {
  justify-content: flex-end;
}
.bubble {
  max-width: 78%;
  background: var(--n-action-color, rgba(128, 128, 128, 0.08));
  border-radius: 10px;
  padding: 8px 12px;
  font-size: 13px;
  line-height: 1.55;
}
.bubble.mine {
  background: var(--n-color-target, rgba(24, 160, 88, 0.12));
}
.bubble-body {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
}
.bubble-meta {
  margin-top: 4px;
  font-size: 11px;
  opacity: 0.55;
  font-variant-numeric: tabular-nums;
}
.reply-card {
  border-radius: 10px;
}
.reply-actions {
  margin-top: 10px;
}
.closed-alert {
  border-radius: 10px;
}
</style>
