<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NButton,
  NDataTable,
  NEmpty,
  NForm,
  NFormItem,
  NInput,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NModal,
  NSelect,
  NSkeleton,
  NSpace,
  NTag,
  NText,
  useMessage,
} from "naive-ui";
import type { DataTableColumns, SelectOption } from "naive-ui";
import { api } from "@/api";
import type { Ticket } from "@/types";
import { formatError } from "@/utils/error";

const { t } = useI18n();
const router = useRouter();
const message = useMessage();

const tickets = ref<Ticket[]>([]);
const loading = ref(true);

// Composer state — kept minimal: reset on close so re-open is a clean form.
const composerOpen = ref(false);
const submitting = ref(false);
const draft = ref<{ subject: string; level: number; message: string }>({
  subject: "",
  level: 1, // default to "Normal" — most users pick this and it matches admin defaults.
  message: "",
});

const levelOptions = computed<SelectOption[]>(() => [
  { label: t("tickets.level.low"), value: 0 },
  { label: t("tickets.level.normal"), value: 1 },
  { label: t("tickets.level.high"), value: 2 },
]);

function resetDraft() {
  draft.value = { subject: "", level: 1, message: "" };
}

function openComposer() {
  resetDraft();
  composerOpen.value = true;
}

async function load() {
  loading.value = true;
  try {
    tickets.value = await api.fetchTickets();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}

async function submitDraft() {
  const subject = draft.value.subject.trim();
  const body = draft.value.message.trim();
  if (!subject || !body) {
    message.warning(t("tickets.composer.fillAll"));
    return;
  }
  submitting.value = true;
  try {
    const newId = await api.saveTicket({
      subject,
      level: draft.value.level,
      message: body,
    });
    composerOpen.value = false;
    message.success(t("tickets.composer.created"));
    if (newId !== null && newId !== undefined) {
      router.push({ name: "ticket-detail", params: { id: newId } });
    } else {
      // Fork didn't reveal the new id — fall back to a list refresh so the
      // user sees their submission show up at the top.
      await load();
    }
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    submitting.value = false;
  }
}

onMounted(load);

function fmtDate(unix: number | null | undefined): string {
  if (!unix) return "—";
  return new Date(unix * 1000).toLocaleString();
}

// Maps to (label key, NTag type). status: 0=open / 1=closed.
const STATUS_META: Record<
  number,
  { labelKey: string; type: "default" | "success" | "warning" }
> = {
  0: { labelKey: "tickets.status.open", type: "warning" },
  1: { labelKey: "tickets.status.closed", type: "default" },
};

// level 0/1/2 → low/normal/high. Backend uses these as a hint for staff
// triage; we surface the same colour code so the user can spot urgency
// at a glance.
const LEVEL_META: Record<
  number,
  { labelKey: string; type: "default" | "info" | "warning" | "error" }
> = {
  0: { labelKey: "tickets.level.low", type: "default" },
  1: { labelKey: "tickets.level.normal", type: "info" },
  2: { labelKey: "tickets.level.high", type: "error" },
};

const columns: DataTableColumns<Ticket> = [
  {
    title: () => t("tickets.col.subject"),
    key: "subject",
    minWidth: 200,
    render: (row) =>
      h(
        NText,
        { strong: true },
        () => row.subject || `#${row.id}`,
      ),
  },
  {
    title: () => t("tickets.col.level"),
    key: "level",
    width: 90,
    render: (row) => {
      const meta = LEVEL_META[row.level];
      return h(
        NTag,
        { size: "small", bordered: false, type: meta?.type ?? "default" },
        () => (meta ? t(meta.labelKey) : `#${row.level}`),
      );
    },
  },
  {
    title: () => t("tickets.col.status"),
    key: "status",
    width: 90,
    render: (row) => {
      const meta = STATUS_META[row.status];
      return h(
        NTag,
        { size: "small", bordered: false, type: meta?.type ?? "default" },
        () => (meta ? t(meta.labelKey) : `#${row.status}`),
      );
    },
  },
  {
    title: () => t("tickets.col.updatedAt"),
    key: "updated_at",
    minWidth: 160,
    render: (row) => fmtDate(row.updated_at ?? row.created_at),
  },
];

function rowProps(row: Ticket) {
  return {
    style: "cursor: pointer;",
    onClick: () =>
      router.push({ name: "ticket-detail", params: { id: row.id } }),
  };
}

const empty = computed(() => !loading.value && tickets.value.length === 0);
</script>

<template>
  <NLayout class="tickets-shell">
    <NLayoutHeader bordered class="tickets-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← {{ t("tickets.back") }}
        </NButton>
        <NText strong>{{ t("tickets.title") }}</NText>
      </NSpace>
      <NSpace :size="6">
        <NButton size="small" type="primary" @click="openComposer">
          + {{ t("tickets.composer.new") }}
        </NButton>
        <NButton size="small" quaternary :loading="loading" @click="load">
          {{ t("tickets.refresh") }}
        </NButton>
      </NSpace>
    </NLayoutHeader>

    <NLayoutContent class="tickets-content">
      <div class="container">
        <template v-if="loading && tickets.length === 0">
          <NSkeleton text :repeat="6" />
        </template>

        <NEmpty v-else-if="empty" :description="t('tickets.empty')">
          <template #extra>
            <NButton size="small" type="primary" @click="openComposer">
              + {{ t("tickets.composer.new") }}
            </NButton>
          </template>
        </NEmpty>

        <NDataTable
          v-else
          :columns="columns"
          :data="tickets"
          :row-key="(row: Ticket) => row.id"
          :row-props="rowProps"
          :bordered="false"
          size="small"
          striped
        />
      </div>
    </NLayoutContent>

    <NModal
      v-model:show="composerOpen"
      preset="card"
      :title="t('tickets.composer.title')"
      style="max-width: 520px"
      :mask-closable="!submitting"
      :closable="!submitting"
    >
      <NForm label-placement="top" :show-require-mark="false">
        <NFormItem :label="t('tickets.composer.subjectLabel')">
          <NInput
            v-model:value="draft.subject"
            :placeholder="t('tickets.composer.subjectPlaceholder')"
            :disabled="submitting"
            maxlength="80"
            show-count
          />
        </NFormItem>
        <NFormItem :label="t('tickets.composer.levelLabel')">
          <NSelect
            v-model:value="draft.level"
            :options="levelOptions"
            :disabled="submitting"
          />
        </NFormItem>
        <NFormItem :label="t('tickets.composer.messageLabel')">
          <NInput
            v-model:value="draft.message"
            type="textarea"
            :placeholder="t('tickets.composer.messagePlaceholder')"
            :autosize="{ minRows: 4, maxRows: 10 }"
            :disabled="submitting"
            maxlength="2000"
            show-count
          />
        </NFormItem>
      </NForm>
      <template #footer>
        <NSpace justify="end">
          <NButton :disabled="submitting" @click="composerOpen = false">
            {{ t("tickets.composer.cancel") }}
          </NButton>
          <NButton
            type="primary"
            :loading="submitting"
            :disabled="!draft.subject.trim() || !draft.message.trim()"
            @click="submitDraft"
          >
            {{ t("tickets.composer.submit") }}
          </NButton>
        </NSpace>
      </template>
    </NModal>
  </NLayout>
</template>

<style scoped>
.tickets-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.tickets-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.tickets-content {
  padding: 20px;
}
.container {
  max-width: 960px;
  margin: 0 auto;
}
</style>
