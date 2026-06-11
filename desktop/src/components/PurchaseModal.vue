<script setup lang="ts">
// Purchase modal — drives a state machine across the
// save_order → checkout_order → check_order pipeline.
//
//   "select"   — pick payment method (+ optional coupon), confirm.
//                Coupon code is debounce-validated against /coupon/check
//                so the user sees the discount before they commit.
//   "pending"  — checkout returned a QR code / redirect URL.
//                A 4s interval polls /order/check_order so the user
//                doesn't have to manually refresh after paying in
//                Alipay / WeChat. The window 'focus' event also pokes
//                a one-shot check so tabbing back from the browser
//                feels instant.
//   "settled"  — backend reported a terminal status. Emit 'done',
//                auto-close shortly after.
//   "failed"   — gateway reported cancelled / refunded mid-pay.
//                User gets a "Retry payment" button.
//
// B-07: if save_order rejects with "你已有未付款订单" we don't just
// surface the raw error — we pop a confirmation dialog asking the user
// whether to resume the existing order (route to /orders) or cancel it
// and retry the new purchase.

import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
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
  NSpin,
  NTag,
  NText,
  useDialog,
  useMessage,
} from "naive-ui";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import QRCode from "qrcode";
import { api } from "@/api";
import type {
  CheckoutResponse,
  CouponCheckResult,
  PaymentMethod,
  Plan,
} from "@/types";
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
const dialog = useDialog();
const router = useRouter();

type Stage = "select" | "pending" | "settled" | "failed";
const stage = ref<Stage>("select");

const methods = ref<PaymentMethod[]>([]);
const methodsLoading = ref(false);
const selectedMethod = ref<number | null>(null);
const couponCode = ref("");
const submitting = ref(false);

const tradeNo = ref<string | null>(null);
const checkout = ref<CheckoutResponse | null>(null);
const lastStatus = ref<number | null>(null);
const cancelling = ref(false);

// QR canvas + draw bookkeeping. The canvas mounts inside a v-if so we use
// a watcher (not onMounted) to draw once the element is in the DOM.
const qrCanvas = ref<HTMLCanvasElement | null>(null);
const qrDrawError = ref(false);

// Coupon preview state.
const couponLoading = ref(false);
const couponResult = ref<CouponCheckResult | null>(null);
const couponError = ref<string | null>(null);
const couponDiscount = ref(0); // in cents — subtracted from base price for display

// Auto-poll bookkeeping. interval id + an `inFlight` guard so we don't
// stack concurrent /check_order calls if the network is slow.
let pollInterval: ReturnType<typeof setInterval> | null = null;
let pollInFlight = false;
let settleCloseTimer: ReturnType<typeof setTimeout> | null = null;
let couponDebounceTimer: ReturnType<typeof setTimeout> | null = null;

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

// Effective price = base − validated coupon discount, floored at 0.
const effectivePriceCents = computed(() => {
  const base = props.priceCents ?? 0;
  return Math.max(0, base - couponDiscount.value);
});
const priceYuan = computed(() => (effectivePriceCents.value / 100).toFixed(2));
const basePriceYuan = computed(() =>
  props.priceCents == null ? "" : (props.priceCents / 100).toFixed(2),
);
const hasDiscount = computed(
  () => couponDiscount.value > 0 && props.priceCents != null,
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
      resetCoupon();
      qrDrawError.value = false;
      void loadMethods();
    } else {
      // Cleanup if the user dismissed via the X.
      stopPolling();
      clearSettleTimer();
      clearCouponDebounce();
    }
  },
);

// Once checkout lands and we're in pending stage with a QR payment, the
// canvas mounts. Watch for the canvas ref to populate, then draw.
watch(
  [qrCanvas, () => stage.value, () => checkout.value],
  () => {
    if (
      stage.value === "pending" &&
      checkout.value?.type === 0 &&
      typeof checkout.value.data === "string" &&
      qrCanvas.value
    ) {
      drawQr(checkout.value.data);
    }
  },
);

// Re-validate coupon when the user changes plan or payment method.
watch(
  () => [props.plan?.id, selectedMethod.value, props.periodKey] as const,
  () => {
    if (couponCode.value.trim().length >= 3) {
      scheduleCouponCheck();
    } else {
      resetCoupon();
    }
  },
);

watch(couponCode, () => {
  scheduleCouponCheck();
});

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

// ─── Coupon ────────────────────────────────────────────────────────────

function clearCouponDebounce() {
  if (couponDebounceTimer) {
    clearTimeout(couponDebounceTimer);
    couponDebounceTimer = null;
  }
}

function resetCoupon() {
  clearCouponDebounce();
  couponLoading.value = false;
  couponResult.value = null;
  couponError.value = null;
  couponDiscount.value = 0;
}

function scheduleCouponCheck() {
  clearCouponDebounce();
  const code = couponCode.value.trim();
  if (code.length < 3) {
    // Too short — reset preview but don't show "invalid".
    couponLoading.value = false;
    couponResult.value = null;
    couponError.value = null;
    couponDiscount.value = 0;
    return;
  }
  // Resume mode: coupon was locked in at save time; don't re-validate.
  if (isResume.value) return;
  if (!props.plan) return;

  couponDebounceTimer = setTimeout(() => {
    void runCouponCheck(code);
  }, 500);
}

async function runCouponCheck(code: string) {
  if (!props.plan) return;
  const planId = props.plan.id;
  couponLoading.value = true;
  couponError.value = null;
  try {
    const result = await api.checkCoupon(code, planId);
    // Bail if the user edited the input while the request was in flight.
    if (code !== couponCode.value.trim()) return;
    couponResult.value = result;
    couponDiscount.value = computeDiscountCents(result);
  } catch (e) {
    if (code !== couponCode.value.trim()) return;
    couponResult.value = null;
    couponDiscount.value = 0;
    couponError.value = t("purchase.coupon.invalid");
    // Don't toast — `formatError(e, t)` is intentionally suppressed.
    // The inline red text is enough; toasting on every keystroke is noisy.
    void e;
  } finally {
    if (code === couponCode.value.trim()) {
      couponLoading.value = false;
    }
  }
}

function computeDiscountCents(r: CouponCheckResult): number {
  const base = props.priceCents ?? 0;
  if (r.value == null) return 0;
  if (r.type === 1) {
    // Fixed amount off in cents.
    return Math.max(0, Math.min(base, Math.round(r.value)));
  }
  if (r.type === 2) {
    // Percent off.
    const pct = Math.max(0, Math.min(100, r.value));
    return Math.round((base * pct) / 100);
  }
  return 0;
}

const couponHint = computed<{ text: string; tone: "ok" | "err" } | null>(() => {
  if (couponLoading.value) {
    return { text: t("purchase.coupon.checking"), tone: "ok" };
  }
  if (couponError.value) {
    return { text: couponError.value, tone: "err" };
  }
  const r = couponResult.value;
  if (!r) return null;
  const discountYuan = (couponDiscount.value / 100).toFixed(2);
  if (r.type === 2 && r.value != null) {
    return {
      text: t("purchase.coupon.discountPercent", {
        percent: r.value,
        amount: discountYuan,
      }),
      tone: "ok",
    };
  }
  return {
    text: t("purchase.coupon.discount", { amount: discountYuan }),
    tone: "ok",
  };
});

// ─── QR rendering ──────────────────────────────────────────────────────

async function drawQr(payload: string) {
  if (!qrCanvas.value) return;
  qrDrawError.value = false;
  try {
    await QRCode.toCanvas(qrCanvas.value, payload, {
      width: 220,
      margin: 2,
      color: { dark: "#f8f7ff", light: "#1a1430" },
    });
  } catch {
    // qrcode lib refuses payloads longer than ~2.9 KB; in practice the
    // panel only ever sends Alipay / WeChat URIs which are well under
    // that, but fail gracefully if it ever happens.
    qrDrawError.value = true;
  }
}

// ─── Submit / checkout ────────────────────────────────────────────────

async function submit() {
  if (selectedMethod.value == null) {
    message.warning(t("purchase.error.pickPayment"));
    return;
  }
  submitting.value = true;
  try {
    await doCheckout();
  } finally {
    submitting.value = false;
  }
}

async function doCheckout() {
  let trade: string;
  try {
    if (props.existingTradeNo) {
      // Resume path — order already saved.
      trade = props.existingTradeNo;
    } else {
      if (!props.plan || !props.periodKey) return;
      const trimmed = couponCode.value.trim();
      // Only pass the coupon code if it validated cleanly; passing a broken
      // code would just have the panel reject /save with a different error.
      const couponToSend =
        trimmed && couponResult.value && !couponError.value ? trimmed : null;
      trade = await saveOrderWithPendingHandling(
        props.plan.id,
        props.periodKey,
        couponToSend,
      );
      if (!trade) return;
    }
  } catch (e) {
    // SilentAbort = user picked "Resume payment" in the pending dialog
    // (we already routed to /orders) or dismissed it. Don't toast.
    if (!(e instanceof SilentAbort)) {
      message.error(formatError(e, t));
    }
    return;
  }
  tradeNo.value = trade;

  let resp: CheckoutResponse;
  try {
    resp = await api.checkoutOrder(trade, selectedMethod.value!);
  } catch (e) {
    message.error(formatError(e, t));
    return;
  }
  checkout.value = resp;

  // type === -1 → backend settled (e.g. from balance). Auto-close shortly.
  if (resp.type === -1) {
    stage.value = "settled";
    lastStatus.value = 3;
    emit("done");
    scheduleAutoClose(1000);
    return;
  }

  // type === 1 → redirect URL. Open it for the user, but stay in pending
  // stage so the poller can pick up the payment when they come back.
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
  // we surface the raw payload for the user to copy.
  if (
    resp.type === -2 &&
    typeof resp.data === "string" &&
    /^https?:\/\//i.test(resp.data)
  ) {
    try {
      await shellOpen(resp.data);
    } catch {
      /* no-op — the user can copy the link from the pending screen */
    }
  }

  stage.value = "pending";
  startPolling();
}

// B-07: detect the panel's "you already have an unpaid order" error and
// offer Resume / Cancel + retry. Returns the trade_no on success, or
// rejects (so the caller surfaces the error normally).
async function saveOrderWithPendingHandling(
  planId: number,
  period: string,
  coupon: string | null,
): Promise<string> {
  try {
    const trade = await api.saveOrder({
      planId,
      period,
      couponCode: coupon,
    });
    if (!trade) throw new Error(t("purchase.error.noTradeNo"));
    return trade;
  } catch (e) {
    if (!isPendingOrderError(e)) throw e;
    // Resolve once the user picks a path — either resume (which closes
    // this modal and routes to /orders) or cancel-and-retry (which loops
    // back into saveOrder with the same params).
    return new Promise<string>((resolve, reject) => {
      const d = dialog.warning({
        title: t("purchase.pending.title"),
        content: t("purchase.pending.body"),
        positiveText: t("purchase.pending.resume"),
        negativeText: t("purchase.pending.cancel"),
        onPositiveClick: () => {
          // Close this modal and route to /orders so the user can resume.
          close();
          void router.push({ name: "orders" });
          // Don't resolve — the caller's outer try/catch swallows the
          // rejection silently because we already redirected.
          reject(new SilentAbort());
        },
        onNegativeClick: async () => {
          d.loading = true;
          try {
            await cancelPendingAndRetry(planId, period, coupon, resolve, reject);
          } finally {
            d.loading = false;
          }
        },
        onClose: () => reject(new SilentAbort()),
        onMaskClick: () => reject(new SilentAbort()),
      });
    });
  }
}

class SilentAbort extends Error {
  constructor() {
    super("aborted");
    this.name = "SilentAbort";
  }
}

async function cancelPendingAndRetry(
  planId: number,
  period: string,
  coupon: string | null,
  resolve: (trade: string) => void,
  reject: (err: unknown) => void,
) {
  try {
    // Find the user's pending order and cancel it.
    const orders = await api.fetchOrders();
    const pending = orders.find((o) => o.status === 0);
    if (pending) {
      await api.cancelOrder(pending.trade_no);
    }
    // Retry save_order — if it fails again we surface the new error
    // verbatim (no recursive dialog loop).
    const trade = await api.saveOrder({
      planId,
      period,
      couponCode: coupon,
    });
    if (!trade) {
      reject(new Error(t("purchase.error.noTradeNo")));
      return;
    }
    resolve(trade);
  } catch (e) {
    message.error(formatError(e, t));
    reject(new SilentAbort());
  }
}

function isPendingOrderError(e: unknown): boolean {
  // The panel returns either the English ("You have an unpaid or pending
  // order…") or the Chinese ("您有未付款或开通中的订单…") variant
  // depending on the user's locale. Match on stable substrings.
  const msg =
    (typeof e === "object" && e && "message" in e
      ? String((e as { message: unknown }).message)
      : String(e)) || "";
  return (
    msg.includes("未付款") ||
    msg.includes("未支付") ||
    msg.includes("pending order") ||
    msg.includes("unpaid or pending")
  );
}

// ─── Polling ──────────────────────────────────────────────────────────

function startPolling() {
  stopPolling();
  if (!tradeNo.value) return;
  // Kick once immediately so the user doesn't wait a full 4 s for the
  // first heartbeat (relevant when payment cleared before this point).
  void pollOnce();
  pollInterval = setInterval(() => {
    void pollOnce();
  }, 4000);
  // Window focus → check immediately. Users routinely tab back from the
  // browser the instant they finish paying.
  window.addEventListener("focus", onWindowFocus);
}

function stopPolling() {
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }
  window.removeEventListener("focus", onWindowFocus);
  pollInFlight = false;
}

function onWindowFocus() {
  if (stage.value === "pending") void pollOnce();
}

async function pollOnce() {
  if (!tradeNo.value || pollInFlight) return;
  if (stage.value !== "pending") return;
  pollInFlight = true;
  try {
    const status = await api.checkOrder(tradeNo.value);
    lastStatus.value = status;
    // Backend status semantics:
    //   0 pending, 1 activating, 2 cancelled, 3 completed, 4 credited.
    // 1/3/4 are happy-path terminals; 2 is the user-cancelled / gateway-
    // refunded path.
    if (status === 1 || status === 3 || status === 4) {
      stopPolling();
      stage.value = "settled";
      emit("done");
      message.success(t("purchase.purchaseComplete"));
      scheduleAutoClose(1500);
    } else if (status === 2) {
      stopPolling();
      stage.value = "failed";
    }
  } catch {
    // Swallow — transient poll errors should not toast every 4 s. The
    // user can still hit the manual refresh / retry buttons.
  } finally {
    pollInFlight = false;
  }
}

function scheduleAutoClose(ms: number) {
  clearSettleTimer();
  settleCloseTimer = setTimeout(() => {
    close();
  }, ms);
}

function clearSettleTimer() {
  if (settleCloseTimer) {
    clearTimeout(settleCloseTimer);
    settleCloseTimer = null;
  }
}

// ─── Other actions ────────────────────────────────────────────────────

async function refreshStatus() {
  // Manual refresh from the pending UI — simply triggers a poll right now.
  await pollOnce();
}

async function cancelOrder() {
  if (!tradeNo.value) return;
  cancelling.value = true;
  try {
    await api.cancelOrder(tradeNo.value);
    lastStatus.value = 2;
    stopPolling();
    stage.value = "settled";
    message.success(t("purchase.cancelled"));
    emit("done");
    scheduleAutoClose(1500);
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    cancelling.value = false;
  }
}

async function retry() {
  // From the 'failed' stage — re-attempt /checkout on the same trade_no.
  // If the original order was already cancelled by the gateway, the user
  // should start over from /plans; surface the panel's error in that case.
  stage.value = "select";
  lastStatus.value = null;
  checkout.value = null;
  // Keep the existing tradeNo so submit() will reuse it as a resume.
  // Force the resume path by passing tradeNo through props is non-trivial,
  // so we just re-run the checkout half directly.
  if (tradeNo.value && selectedMethod.value != null) {
    submitting.value = true;
    try {
      await doCheckoutResume();
    } finally {
      submitting.value = false;
    }
  }
}

async function doCheckoutResume() {
  if (!tradeNo.value || selectedMethod.value == null) return;
  let resp: CheckoutResponse;
  try {
    resp = await api.checkoutOrder(tradeNo.value, selectedMethod.value);
  } catch (e) {
    message.error(formatError(e, t));
    return;
  }
  checkout.value = resp;
  if (resp.type === -1) {
    stage.value = "settled";
    lastStatus.value = 3;
    emit("done");
    scheduleAutoClose(1000);
    return;
  }
  if (resp.type === 1 && typeof resp.data === "string") {
    try {
      await shellOpen(resp.data);
    } catch (e) {
      message.error(
        t("purchase.error.openUrl", { message: formatError(e, t) }),
      );
    }
  }
  stage.value = "pending";
  startPolling();
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

async function openUrl() {
  if (typeof checkout.value?.data !== "string") return;
  try {
    await shellOpen(checkout.value.data);
  } catch (e) {
    message.error(t("purchase.error.openUrl", { message: formatError(e, t) }));
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
  stopPolling();
  clearSettleTimer();
  clearCouponDebounce();
  emit("update:show", false);
}

onBeforeUnmount(() => {
  stopPolling();
  clearSettleTimer();
  clearCouponDebounce();
});

// Coerce `data` to a printable string for the pending screen.
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
  () =>
    typeof checkout.value?.data === "string" &&
    /^https?:\/\//i.test(checkout.value.data),
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
        <span class="price-stack">
          <NText
            v-if="hasDiscount"
            depth="3"
            class="price-strike"
          >¥ {{ basePriceYuan }}</NText>
          <NText strong class="price">¥ {{ priceYuan }}</NText>
        </span>
      </div>
    </div>

    <template v-if="stage === 'select'">
      <NForm label-placement="top" size="small" class="form">
        <NFormItem v-if="!isResume" :label="t('purchase.couponLabel')">
          <div class="coupon-wrap">
            <NInput
              v-model:value="couponCode"
              :placeholder="t('purchase.couponPlaceholder')"
              :disabled="submitting"
              clearable
            />
            <div
              v-if="couponHint"
              class="coupon-hint"
              :class="couponHint.tone === 'err' ? 'is-err' : 'is-ok'"
            >
              <NSpin v-if="couponLoading" size="small" />
              <span>{{ couponHint.text }}</span>
            </div>
          </div>
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
      <!-- QR-payment driver (Alipay F2F / WeChat F2F) — render a canvas. -->
      <template v-if="checkout?.type === 0 && typeof checkout?.data === 'string'">
        <div class="qr-block">
          <NText strong class="qr-title">{{ t("purchase.qr.title") }}</NText>
          <canvas ref="qrCanvas" class="qr-canvas" />
          <NText depth="3" class="qr-hint">{{ t("purchase.qr.hint") }}</NText>
          <NAlert
            v-if="qrDrawError"
            type="warning"
            :show-icon="false"
            class="qr-fallback"
          >
            {{ t("purchase.qr.fallback") }}
          </NAlert>
          <NText depth="3" class="poll-line">{{ t("purchase.poll.waiting") }}</NText>
        </div>
      </template>

      <!-- Redirect URL — already opened in the browser. -->
      <template v-else-if="checkout?.type === 1">
        <NAlert :title="t('purchase.pendingTitle')" type="info" :show-icon="true">
          {{ t("purchase.redirectOpened") }}
        </NAlert>
        <NText depth="3" class="poll-line">{{ t("purchase.poll.waiting") }}</NText>
      </template>

      <!-- Gateway-specific (Stripe form etc.) — surface the raw payload. -->
      <template v-else>
        <NAlert :title="t('purchase.pendingTitle')" type="info" :show-icon="true">
          <pre class="qr-data">{{ checkoutDataText }}</pre>
        </NAlert>
        <NText depth="3" class="poll-line">{{ t("purchase.poll.waiting") }}</NText>
      </template>

      <NText v-if="statusLine" depth="3" class="status-line">
        {{ statusLine }}
      </NText>
    </template>

    <template v-else-if="stage === 'failed'">
      <NAlert
        :title="t('purchase.poll.failed')"
        type="warning"
        :show-icon="true"
      >
        {{ statusLine || t("purchase.statusCancelled") }}
      </NAlert>
    </template>

    <template v-else>
      <NAlert
        :title="lastStatus === 2 ? statusLine : t('purchase.poll.success')"
        :type="lastStatus === 2 ? 'warning' : 'success'"
        :show-icon="true"
      >
        <template v-if="checkout?.type === -1">
          {{ t("purchase.balancePaid") }}
        </template>
        <template v-else-if="lastStatus !== 2">
          {{ t("purchase.statusCompleted") }}
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
            :disabled="
              methods.length === 0 ||
              selectedMethod == null ||
              couponLoading
            "
            @click="submit"
          >
            {{ submitting ? t("purchase.submitting") : t("purchase.submit") }}
          </NButton>
        </template>

        <template v-else-if="stage === 'pending'">
          <!-- QR mode — let user copy the raw payment string. -->
          <NButton
            v-if="checkout?.type === 0 && typeof checkout?.data === 'string'"
            @click="copyData"
          >
            {{ t("purchase.qrCopy") }}
          </NButton>
          <!-- Redirect / gateway-with-URL — offer copy + re-open. -->
          <NButton
            v-if="(checkout?.type === 1 || checkout?.type === -2) && checkoutDataIsUrl"
            @click="copyData"
          >
            {{ t("purchase.copyLink") }}
          </NButton>
          <NButton
            v-if="checkout?.type === 1 && checkoutDataIsUrl"
            @click="openUrl"
          >
            {{ t("purchase.reopenLink") }}
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
          <NButton type="primary" @click="refreshStatus">
            {{ t("purchase.statusRefresh") }}
          </NButton>
        </template>

        <template v-else-if="stage === 'failed'">
          <NButton @click="close">
            {{ t("purchase.close") }}
          </NButton>
          <NButton type="primary" :loading="submitting" @click="retry">
            {{ t("purchase.poll.retry") }}
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
.price-stack {
  display: inline-flex;
  align-items: baseline;
  gap: 8px;
}
.price-strike {
  text-decoration: line-through;
  font-size: 12px;
  opacity: 0.6;
}
.price {
  font-variant-numeric: tabular-nums;
  font-size: 16px;
}
.form {
  margin-top: 4px;
}
.coupon-wrap {
  width: 100%;
}
.coupon-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  font-size: 12px;
}
.coupon-hint.is-ok {
  color: #2ecc71;
}
.coupon-hint.is-err {
  color: #e74c3c;
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
.qr-block {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 8px 0 4px;
}
.qr-title {
  font-size: 14px;
}
.qr-canvas {
  width: 220px;
  height: 220px;
  border-radius: 10px;
  background: #1a1430;
}
.qr-hint {
  font-size: 12px;
  text-align: center;
}
.qr-fallback {
  width: 100%;
}
.poll-line {
  display: block;
  margin-top: 10px;
  font-size: 12px;
  text-align: center;
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
