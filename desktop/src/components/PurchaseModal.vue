<script setup lang="ts">
// Purchase modal — drives a small state machine across the
// save_order → checkout_order → check_order pipeline.
//
//   "select"   — pick payment method (+ optional coupon), confirm
//   "pending"  — checkout returned a redirect / QR / gateway response;
//                user pays externally and we expose a manual refresh
//                that calls check_order
//   "settled"  — backend reported a terminal status (completed /
//                cancelled / activating). The user closes the modal.
//
// We deliberately don't auto-poll: paying via Alipay/WeChat in another
// app can take minutes, and a 5-minute background timer would feel like
// spam. The "Refresh" button is the explicit handshake.

import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  NAlert,
  NButton,
  NEmpty,
  NForm,
  NFormItem,
  NInput,
  NModal,
  NPopconfirm,
  NRadio,
  NRadioGroup,
  NSkeleton,
  NSpace,
  NTag,
  NText,
  useMessage,
} from "naive-ui";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { api } from "@/api";
import type { CheckoutResponse, PaymentMethod, Plan } from "@/types";
import { formatError } from "@/utils/error";

const props = defineProps<{
  show: boolean;
  // "New" path (from Plans.vue): plan + periodKey are both required to call
  //   /order/save; the modal will then continue into /order/checkout.
  // "Resume" path (from Orders.vue): existingTradeNo is set and we skip
  //   straight to /order/checkout. plan/periodKey can be null in this case.
  plan: Plan | null;
  periodKey: string | null;
  // Cents — purely for header display.
  priceCents: number | null;
  existingTradeNo?: string | null;
  // Optional header override; used in Resume mode where we surface the
  // trade_no instead of a plan name.
  displayName?: string | null;
  // Optional human-readable period label (used by Resume mode where we
  // already have it formatted from the Orders row, no key→label lookup).
  displayPeriod?: string | null;
}>();

const emit = defineEmits<{
  "update:show": [boolean];
  // Fired when an order reaches a terminal state so the caller (Plans.vue)
  // can refresh the user's plan / orders pages.
  done: [];
}>();

const { t } = useI18n();
const message = useMessage();

type Stage = "select" | "pending" | "settled";
const stage = ref<Stage>("select");

const methods = ref<PaymentMethod[]>([]);
const methodsLoading = ref(false);
const selectedMethod = ref<number | null>(null);
const couponCode = ref("");
const submitting = ref(false);

const tradeNo = ref<string | null>(null);
const checkout = ref<CheckoutResponse | null>(null);
const lastStatus = ref<number | null>(null);
const checking = ref(false);
const cancelling = ref(false);

const periodLabel = computed(() => {
  if (props.displayPeriod) return props.displayPeriod;
  if (!props.periodKey) return "—";
  // Reuse the same map as Plans.vue.
  const map: Record<string, string> = {
    month_price: t("plans.period.month"),
    quarter_price: t("plans.period.quarter"),
    half_year_price: t("plans.period.halfYear"),
    year_price: t("plans.period.year"),
    two_year_price: t("plans.period.twoYear"),
    three_year_price: t("plans.period.threeYear"),
    onetime_price: t("plans.period.onetime"),
  };
  return map[props.periodKey] ?? props.periodKey;
});

const headerTitle = computed(() => {
  if (props.displayName) return props.displayName;
  if (props.plan) return props.plan.name || `#${props.plan.id}`;
  return "—";
});

// Resume mode: we already have a trade_no from a previous /order/save call,
// so the user only needs to pick a payment method.
const isResume = computed(() => !!props.existingTradeNo);

const priceYuan = computed(() =>
  props.priceCents == null ? "" : (props.priceCents / 100).toFixed(2),
);

watch(
  () => props.show,
  (visible) => {
    if (visible) {
      stage.value = "select";
      tradeNo.value = null;
      checkout.value = null;
      lastStatus.value = null;
      couponCode.value = "";
      selectedMethod.value = null;
      void loadMethods();
    }
  },
);

async function loadMethods() {
  methodsLoading.value = true;
  try {
    methods.value = await api.fetchPaymentMethods();
    if (methods.value.length === 1) {
      selectedMethod.value = methods.value[0].id;
    }
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    methodsLoading.value = false;
  }
}

function feeLabel(m: PaymentMethod): string | null {
  if (m.handling_fee_fixed && m.handling_fee_fixed > 0) {
    return t("purchase.feeFixed", {
      yuan: (m.handling_fee_fixed / 100).toFixed(2),
    });
  }
  if (m.handling_fee_percent && m.handling_fee_percent > 0) {
    return t("purchase.feePercent", { percent: m.handling_fee_percent });
  }
  return null;
}

async function submit() {
  if (selectedMethod.value == null) {
    message.warning(t("purchase.error.pickPayment"));
    return;
  }
  submitting.value = true;
  try {
    let trade: string;
    if (props.existingTradeNo) {
      // Resume path — order already saved; coupon application is fixed
      // at /save time so we ignore the field even if the user typed in it.
      trade = props.existingTradeNo;
    } else {
      if (!props.plan || !props.periodKey) return;
      const trimmed = couponCode.value.trim();
      const created = await api.saveOrder({
        planId: props.plan.id,
        period: props.periodKey,
        couponCode: trimmed || null,
      });
      if (!created) {
        message.error(t("purchase.error.noTradeNo"));
        return;
      }
      trade = created;
    }
    tradeNo.value = trade;

    const resp = await api.checkoutOrder(trade, selectedMethod.value);
    checkout.value = resp;

    // type === -1 means the backend settled from balance — we're done.
    if (resp.type === -1) {
      stage.value = "settled";
      lastStatus.value = 3; // surface as "completed"
      emit("done");
      return;
    }

    // type === 1 → `data` is a redirect URL we should open externally.
    if (resp.type === 1 && typeof resp.data === "string") {
      try {
        await shellOpen(resp.data);
      } catch (e) {
        message.error(
          t("purchase.error.openUrl", { message: formatError(e, t) }),
        );
      }
    }

    // type === -2 → gateway form. If `data` is a URL, open it; otherwise
    // we'll just show the raw payload for staff debugging.
    if (
      resp.type === -2 &&
      typeof resp.data === "string" &&
      /^https?:\/\//i.test(resp.data)
    ) {
      try {
        await shellOpen(resp.data);
      } catch {
        /* no-op: the user can copy the link from the pending screen */
      }
    }

    stage.value = "pending";
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    submitting.value = false;
  }
}

async function refreshStatus() {
  if (!tradeNo.value) return;
  checking.value = true;
  try {
    const status = await api.checkOrder(tradeNo.value);
    lastStatus.value = status;
    // 1 = activating, 3 = completed, 4 = discounted/credited — all terminal
    // for the user-side modal. 2 = cancelled is also terminal.
    if (status === 1 || status === 2 || status === 3 || status === 4) {
      stage.value = "settled";
      emit("done");
    }
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    checking.value = false;
  }
}

async function cancelOrder() {
  if (!tradeNo.value) return;
  cancelling.value = true;
  try {
    await api.cancelOrder(tradeNo.value);
    lastStatus.value = 2;
    stage.value = "settled";
    message.success(t("purchase.cancelled"));
    emit("done");
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    cancelling.value = false;
  }
}

async function copyData() {
  if (typeof checkout.value?.data !== "string") return;
  try {
    await navigator.clipboard.writeText(checkout.value.data);
    message.success(t("purchase.qrCopied"));
  } catch {
    /* ignore — user can manually select & copy */
  }
}

const statusLine = computed(() => {
  if (lastStatus.value == null) return "";
  switch (lastStatus.value) {
    case 0:
      return t("purchase.statusPending");
    case 1:
      return t("purchase.statusActivating");
    case 2:
      return t("purchase.statusCancelled");
    case 3:
      return t("purchase.statusCompleted");
    case 4:
      return t("purchase.statusCompleted");
    default:
      return t("purchase.statusUnknown", { code: lastStatus.value });
  }
});

function close() {
  emit("update:show", false);
}

// Coerce `data` to a printable string for the pending screen. Object/null
// payloads are stringified so staff can still read them in dev tools or
// when filing a support ticket.
const checkoutDataText = computed(() => {
  const d = checkout.value?.data;
  if (d == null) return "";
  if (typeof d === "string") return d;
  try {
    return JSON.stringify(d, null, 2);
  } catch {
    return String(d);
  }
});

const checkoutDataIsUrl = computed(
  () => typeof checkout.value?.data === "string" && /^https?:\/\//i.test(checkout.value.data),
);
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    style="max-width: 480px"
    :title="t('purchase.title')"
    :mask-closable="stage !== 'pending'"
    :close-on-esc="stage !== 'pending'"
    :show-close="true"
    :on-close="close"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <div class="head">
      <div class="head-row">
        <NText depth="3">{{ t("purchase.plan") }}</NText>
        <NText strong>{{ headerTitle }}</NText>
      </div>
      <div class="head-row">
        <NText depth="3">{{ t("purchase.period") }}</NText>
        <NText>{{ periodLabel }}</NText>
      </div>
      <div class="head-row">
        <NText depth="3">{{ t("purchase.price") }}</NText>
        <NText strong class="price">¥ {{ priceYuan }}</NText>
      </div>
    </div>

    <template v-if="stage === 'select'">
      <NForm label-placement="top" size="small" class="form">
        <NFormItem v-if="!isResume" :label="t('purchase.couponLabel')">
          <NInput
            v-model:value="couponCode"
            :placeholder="t('purchase.couponPlaceholder')"
            :disabled="submitting"
            clearable
          />
        </NFormItem>

        <NFormItem :label="t('purchase.paymentMethod')">
          <div v-if="methodsLoading" class="methods-loading">
            <NSkeleton text :repeat="2" />
          </div>
          <NEmpty
            v-else-if="methods.length === 0"
            :description="t('purchase.paymentMethodEmpty')"
            size="small"
          />
          <NRadioGroup
            v-else
            v-model:value="selectedMethod"
            class="methods"
          >
            <NSpace vertical :size="8">
              <NRadio
                v-for="m in methods"
                :key="m.id"
                :value="m.id"
                class="method-row"
              >
                <span class="method-name">{{ m.name || m.payment }}</span>
                <NTag
                  v-if="feeLabel(m)"
                  size="small"
                  :bordered="false"
                  type="warning"
                >
                  {{ feeLabel(m) }}
                </NTag>
              </NRadio>
            </NSpace>
          </NRadioGroup>
        </NFormItem>
      </NForm>
    </template>

    <template v-else-if="stage === 'pending'">
      <NAlert :title="t('purchase.pendingTitle')" type="info" :show-icon="true">
        <template v-if="checkout?.type === 1">
          {{ t("purchase.redirectOpened") }}
        </template>
        <template v-else-if="checkout?.type === 0">
          <p>{{ t("purchase.qrShow") }}</p>
          <pre class="qr-data">{{ checkoutDataText }}</pre>
        </template>
        <template v-else>
          <pre class="qr-data">{{ checkoutDataText }}</pre>
        </template>
      </NAlert>

      <NText v-if="statusLine" depth="3" class="status-line">
        {{ statusLine }}
      </NText>
    </template>

    <template v-else>
      <NAlert
        :title="statusLine"
        :type="lastStatus === 2 ? 'warning' : 'success'"
        :show-icon="true"
      >
        <template v-if="checkout?.type === -1">
          {{ t("purchase.balancePaid") }}
        </template>
      </NAlert>
    </template>

    <template #footer>
      <NSpace justify="end" :size="8">
        <template v-if="stage === 'select'">
          <NButton :disabled="submitting" @click="close">
            {{ t("purchase.cancel") }}
          </NButton>
          <NButton
            type="primary"
            :loading="submitting"
            :disabled="methods.length === 0 || selectedMethod == null"
            @click="submit"
          >
            {{ submitting ? t("purchase.submitting") : t("purchase.submit") }}
          </NButton>
        </template>

        <template v-else-if="stage === 'pending'">
          <NButton
            v-if="checkout?.type === 0 && typeof checkout?.data === 'string'"
            @click="copyData"
          >
            {{ t("purchase.qrCopy") }}
          </NButton>
          <NButton
            v-if="checkout?.type === -2 && checkoutDataIsUrl"
            @click="copyData"
          >
            {{ t("purchase.qrCopy") }}
          </NButton>
          <NPopconfirm
            :positive-text="t('purchase.cancelYes')"
            :negative-text="t('purchase.cancelNo')"
            @positive-click="cancelOrder"
          >
            <template #trigger>
              <NButton :loading="cancelling" type="warning" ghost>
                {{ t("purchase.cancelOrder") }}
              </NButton>
            </template>
            {{ t("purchase.cancelConfirm") }}
          </NPopconfirm>
          <NButton
            type="primary"
            :loading="checking"
            @click="refreshStatus"
          >
            {{ checking ? t("purchase.statusChecking") : t("purchase.statusRefresh") }}
          </NButton>
        </template>

        <template v-else>
          <NButton type="primary" @click="close">
            {{ t("purchase.close") }}
          </NButton>
        </template>
      </NSpace>
    </template>
  </NModal>
</template>

<style scoped>
.head {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 0 14px;
  border-bottom: 1px solid var(--n-border-color);
  margin-bottom: 14px;
}
.head-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  font-size: 13px;
}
.price {
  font-variant-numeric: tabular-nums;
  font-size: 16px;
}
.form {
  margin-top: 4px;
}
.methods {
  width: 100%;
}
.method-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 0;
}
.method-name {
  margin-right: 8px;
}
.methods-loading {
  width: 100%;
  padding: 8px 0;
}
.qr-data {
  margin: 8px 0 0;
  padding: 8px;
  background: var(--n-action-color, rgba(128, 128, 128, 0.08));
  border-radius: 6px;
  white-space: pre-wrap;
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  max-height: 180px;
  overflow: auto;
}
.status-line {
  display: block;
  margin-top: 12px;
  font-size: 13px;
}
</style>
