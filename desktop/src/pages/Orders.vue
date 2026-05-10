<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NButton,
  NDataTable,
  NEmpty,
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
import type { DataTableColumns } from "naive-ui";
import { api } from "@/api";
import type { Order } from "@/types";
import { formatError } from "@/utils/error";
import PurchaseModal from "@/components/PurchaseModal.vue";
import { useAuthStore } from "@/stores/auth";

const { t } = useI18n();
const router = useRouter();
const message = useMessage();
const auth = useAuthStore();

const orders = ref<Order[]>([]);
const loading = ref(true);
const cancellingId = ref<number | null>(null);

// Resume-payment modal state. We pass `existingTradeNo` so PurchaseModal
// skips /order/save and goes straight to method-pick + /order/checkout.
const showPurchase = ref(false);
const resumeTradeNo = ref<string | null>(null);
const resumeDisplayName = ref<string | null>(null);
const resumePeriodLabel = ref<string | null>(null);
const resumePriceCents = ref<number | null>(null);

async function load() {
  loading.value = true;
  try {
    orders.value = await api.fetchOrders();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}

onMounted(load);

function openResume(order: Order) {
  resumeTradeNo.value = order.trade_no;
  resumeDisplayName.value = order.trade_no || `#${order.id}`;
  // Period on Order is the same `*_price` key the user picked at /save.
  // We map it back to the friendly label so the modal header reads naturally.
  resumePeriodLabel.value = order.period
    ? (PERIOD_LABEL_MAP[order.period] ?? order.period)
    : null;
  resumePriceCents.value = order.total_amount;
  showPurchase.value = true;
}

async function cancelOrder(order: Order) {
  cancellingId.value = order.id;
  try {
    await api.cancelOrder(order.trade_no);
    message.success(t("purchase.cancelled"));
    await load();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    cancellingId.value = null;
  }
}

// After a resume payment settles we both refresh the list (so the row
// flips to its new status) and refresh the user's plan / subscribe in
// the auth store so Home.vue picks up the change.
async function onPurchaseDone() {
  void load();
  void auth.refreshUser();
  void auth.refreshSubscribe();
}

const PERIOD_LABEL_MAP: Record<string, string> = {
  // Functions read at runtime so locale switches reflow naturally.
  get month_price() { return t("plans.period.month"); },
  get quarter_price() { return t("plans.period.quarter"); },
  get half_year_price() { return t("plans.period.halfYear"); },
  get year_price() { return t("plans.period.year"); },
  get two_year_price() { return t("plans.period.twoYear"); },
  get three_year_price() { return t("plans.period.threeYear"); },
  get onetime_price() { return t("plans.period.onetime"); },
  get reset_price() { return t("plans.resetPrice"); },
};

function yuan(cents: number): string {
  return (cents / 100).toFixed(2);
}

function fmtDate(unix: number | null | undefined): string {
  if (!unix) return "—";
  return new Date(unix * 1000).toLocaleString();
}

// Status semantics from `core/src/api/order.rs`. Map to (label-key, NTag type)
// so colour coding is consistent with the v2board admin panel users may have
// seen elsewhere.
const STATUS_META: Record<
  number,
  { labelKey: string; type: "default" | "info" | "success" | "warning" | "error" }
> = {
  0: { labelKey: "orders.status.pending", type: "warning" },
  1: { labelKey: "orders.status.activating", type: "info" },
  2: { labelKey: "orders.status.cancelled", type: "default" },
  3: { labelKey: "orders.status.completed", type: "success" },
  4: { labelKey: "orders.status.discounted", type: "info" },
};

const KIND_LABEL_KEY: Record<number, string> = {
  1: "orders.kind.new",
  2: "orders.kind.renew",
  3: "orders.kind.upgrade",
  4: "orders.kind.reset",
};

const columns: DataTableColumns<Order> = [
  {
    title: () => t("orders.col.tradeNo"),
    key: "trade_no",
    minWidth: 160,
    render: (row) =>
      h(NText, { code: true, depth: 2 }, () => row.trade_no || `#${row.id}`),
  },
  {
    title: () => t("orders.col.kind"),
    key: "kind",
    width: 90,
    render: (row) => {
      if (row.type == null) return "—";
      const key = KIND_LABEL_KEY[row.type];
      return key ? t(key) : `#${row.type}`;
    },
  },
  {
    title: () => t("orders.col.period"),
    key: "period",
    width: 110,
    render: (row) => row.period ?? "—",
  },
  {
    title: () => t("orders.col.amount"),
    key: "total_amount",
    width: 120,
    align: "right",
    render: (row) =>
      h(
        "span",
        { class: "amount-cell" },
        `¥ ${yuan(row.total_amount)}`,
      ),
  },
  {
    title: () => t("orders.col.status"),
    key: "status",
    width: 110,
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
    title: () => t("orders.col.createdAt"),
    key: "created_at",
    minWidth: 160,
    render: (row) => fmtDate(row.created_at),
  },
  {
    title: () => t("orders.col.actions"),
    key: "actions",
    width: 180,
    render: (row) => {
      // Only pending-payment rows are actionable on the user side. Once
      // the order is activating/completed/cancelled the panel admin owns
      // the next step.
      if (row.status !== 0) return "—";
      return h(NSpace, { size: 6, wrap: false }, () => [
        h(
          NButton,
          {
            size: "tiny",
            type: "primary",
            ghost: true,
            disabled: cancellingId.value === row.id,
            onClick: () => openResume(row),
          },
          () => t("orders.action.resume"),
        ),
        h(
          NPopconfirm,
          {
            positiveText: t("purchase.cancelYes"),
            negativeText: t("purchase.cancelNo"),
            onPositiveClick: () => cancelOrder(row),
          },
          {
            trigger: () =>
              h(
                NButton,
                {
                  size: "tiny",
                  ghost: true,
                  type: "warning",
                  loading: cancellingId.value === row.id,
                },
                () => t("orders.action.cancel"),
              ),
            default: () => t("purchase.cancelConfirm"),
          },
        ),
      ]);
    },
  },
];

const empty = computed(() => !loading.value && orders.value.length === 0);
</script>

<template>
  <NLayout class="orders-shell">
    <NLayoutHeader bordered class="orders-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← {{ t("orders.back") }}
        </NButton>
        <NText strong>{{ t("orders.title") }}</NText>
      </NSpace>
      <NButton size="small" quaternary :loading="loading" @click="load">
        {{ t("orders.refresh") }}
      </NButton>
    </NLayoutHeader>

    <NLayoutContent class="orders-content">
      <div class="container">
        <template v-if="loading && orders.length === 0">
          <NSkeleton text :repeat="6" />
        </template>

        <NEmpty v-else-if="empty" :description="t('orders.empty')" />

        <NDataTable
          v-else
          :columns="columns"
          :data="orders"
          :row-key="(row: Order) => row.id"
          :bordered="false"
          size="small"
          striped
        />
      </div>
    </NLayoutContent>

    <PurchaseModal
      v-model:show="showPurchase"
      :plan="null"
      :period-key="null"
      :price-cents="resumePriceCents"
      :existing-trade-no="resumeTradeNo"
      :display-name="resumeDisplayName"
      :display-period="resumePeriodLabel"
      @done="onPurchaseDone"
    />
  </NLayout>
</template>

<style scoped>
.orders-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.orders-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.orders-content {
  padding: 20px;
}
.container {
  max-width: 960px;
  margin: 0 auto;
}
.amount-cell {
  font-variant-numeric: tabular-nums;
  font-weight: 500;
}
</style>
