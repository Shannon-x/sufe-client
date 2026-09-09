<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NAlert,
  NButton,
  NInput,
  NModal,
  NSpace,
  NSpin,
  useMessage,
} from "naive-ui";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listen } from "@/platform";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import QRCode from "qrcode";
import { api } from "@/api";
import { billingApi, type OrderDetail } from "@/api/billing";
import {
  couponDiscount,
  money,
  paymentFee,
  paymentState,
  periodNames,
} from "@/utils/billing";
import { useAuthStore } from "@/stores/auth";
import { formatError } from "@/utils/error";
import type {
  CheckoutResponse,
  CouponCheckResult,
  PaymentMethod,
  Plan,
} from "@/types";

const props = defineProps<{
  show: boolean;
  plan: Plan | null;
  periodKey: string | null;
  priceCents: number | null;
  existingTradeNo?: string | null;
  displayName?: string | null;
  displayPeriod?: string | null;
}>();
const emit = defineEmits<{ "update:show": [boolean]; done: [] }>();
const router = useRouter();
const { t } = useI18n();
const toast = useMessage();
const auth = useAuthStore();
const stage = ref<"select" | "review" | "pending" | "complete" | "cancelled">(
  "select",
);
const methods = ref<PaymentMethod[]>([]);
const methodId = ref<number | null>(null);
const loading = ref(false);
const busy = ref(false);
const couponCode = ref("");
const coupon = ref<CouponCheckResult | null>(null);
const couponBusy = ref(false);
const couponError = ref("");
const error = ref("");
const detail = ref<OrderDetail | null>(null);
const trade = ref("");
const checkout = ref<CheckoutResponse | null>(null);
const expired = ref(false);
const checking = ref(false);
const lastStatus = ref<number | null>(null);
const qrCanvas = ref<HTMLCanvasElement | null>(null);
const qrError = ref(false);
let generation = 0;
let couponGeneration = 0;
let couponTimer: ReturnType<typeof setTimeout> | undefined;
let pollTimer: ReturnType<typeof setTimeout> | undefined;
let pollStarted = 0;
let unlisten: UnlistenFn | undefined;
const method = computed(() =>
  methods.value.find((m) => m.id === methodId.value),
);
const discount = computed(() =>
  coupon.value
    ? couponDiscount(
        props.priceCents ?? 0,
        coupon.value.type,
        coupon.value.value ?? 0,
      )
    : 0,
);
const estimatedBalance = computed(() =>
  Math.min(
    Math.max(0, props.priceCents ?? 0) - discount.value,
    Math.max(0, auth.userInfo?.balance ?? 0),
  ),
);
const payable = computed(() =>
  detail.value
    ? detail.value.total_amount
    : Math.max(
        0,
        (props.priceCents ?? 0) - discount.value - estimatedBalance.value,
      ),
);
const fee = computed(() =>
  detail.value?.payment_id
    ? Number(detail.value.handling_amount ?? 0)
    : paymentFee(
        payable.value,
        method.value?.handling_fee_fixed ?? 0,
        method.value?.handling_fee_percent ?? 0,
      ),
);
const total = computed(() => payable.value + fee.value);
const name = computed(
  () =>
    detail.value?.plan?.name ||
    props.displayName ||
    props.plan?.name ||
    "套餐订单",
);
const period = computed(
  () =>
    props.displayPeriod ||
    periodNames[detail.value?.period || props.periodKey || ""] ||
    "订阅",
);
const paymentUrl = computed(() =>
  typeof checkout.value?.data === "string" &&
  /^https:\/\//i.test(checkout.value.data)
    ? checkout.value.data
    : "",
);
const lockedMethod = computed(() => detail.value?.payment_id != null);
const canCheckout = computed(
  () => detail.value && (payable.value === 0 || methodId.value !== null),
);

function stopPolling() {
  if (pollTimer) clearTimeout(pollTimer);
  pollTimer = undefined;
}
function close() {
  generation++;
  stopPolling();
  emit("update:show", false);
}
function toOrders() {
  close();
  void router.push({ name: "orders" });
}
async function readDetail() {
  if (!trade.value) return;
  const token = generation;
  const order = await billingApi.detail(trade.value);
  if (token !== generation) return;
  detail.value = order;
  lastStatus.value = order.status;
  if (order.payment_id != null) methodId.value = order.payment_id;
  const state = paymentState(order.status);
  if (state === "complete") stage.value = "complete";
  else if (state === "cancelled") stage.value = "cancelled";
  else if (state === "activating") {
    stage.value = "pending";
    startPolling();
  } else stage.value = "review";
}
watch(
  () => props.show,
  async (visible) => {
    const token = ++generation;
    stopPolling();
    if (!visible) return;
    error.value = "";
    couponCode.value = "";
    coupon.value = null;
    couponError.value = "";
    detail.value = null;
    checkout.value = null;
    methodId.value = null;
    lastStatus.value = null;
    stage.value = "select";
    expired.value = false;
    trade.value = props.existingTradeNo || "";
    loading.value = true;
    const result = await Promise.allSettled([
      api.fetchPaymentMethods(),
      trade.value ? readDetail() : Promise.resolve(),
    ]);
    if (token !== generation) return;
    if (result[0].status === "fulfilled") {
      methods.value = result[0].value;
      if (methodId.value == null) methodId.value = methods.value[0]?.id ?? null;
    } else {
      methods.value = [];
      error.value = formatError(result[0].reason, t);
    }
    if (result[1].status === "rejected")
      error.value = formatError(result[1].reason, t);
    loading.value = false;
  },
  { immediate: true },
);

watch([couponCode, () => props.plan?.id, () => props.periodKey], () => {
  const token = ++couponGeneration;
  if (couponTimer) clearTimeout(couponTimer);
  coupon.value = null;
  couponError.value = "";
  couponBusy.value = false;
  const code = couponCode.value.trim();
  if (!code || !props.plan || !props.periodKey || trade.value) return;
  couponBusy.value = true;
  const plan = props.plan.id;
  const periodKey = props.periodKey;
  couponTimer = setTimeout(async () => {
    try {
      const response = await billingApi.checkCoupon(code, plan, periodKey);
      if (token === couponGeneration) coupon.value = response;
    } catch (e) {
      if (token === couponGeneration) couponError.value = formatError(e, t);
    } finally {
      if (token === couponGeneration) couponBusy.value = false;
    }
  }, 450);
});

async function createOrder() {
  if (
    busy.value ||
    couponBusy.value ||
    (!trade.value &&
      (!props.plan ||
        !props.periodKey ||
        (couponCode.value.trim() && !coupon.value)))
  )
    return;
  busy.value = true;
  error.value = "";
  const token = generation;
  try {
    // Keep the allocated trade number even when the following detail fetch
    // fails. Retrying must never allocate or cancel a second order.
    if (!trade.value)
      trade.value = await api.saveOrder({
        planId: props.plan!.id,
        period: props.periodKey!,
        couponCode: couponCode.value.trim() || null,
      });
    if (token !== generation) return;
    await readDetail();
    void auth.refreshUser().catch(() => {});
  } catch (e) {
    error.value = formatError(e, t);
  } finally {
    if (token === generation) busy.value = false;
  }
}
async function pay() {
  if (!canCheckout.value || busy.value || !trade.value) return;
  busy.value = true;
  error.value = "";
  const token = generation;
  try {
    checkout.value = await api.checkoutOrder(
      trade.value,
      payable.value === 0 ? 0 : methodId.value!,
    );
    if (token !== generation) return;
    // Even a free checkout may still be activating. Never report active
    // service until the backend confirms completion.
    stage.value = "pending";
    startPolling();
    if (checkout.value.type === 1 || checkout.value.type === -2) {
      if (paymentUrl.value) await openPayment();
      else
        error.value =
          "该支付方式需要额外的支付表单，暂时无法在客户端完成。请前往订单中心，或联系客户支持。";
    }
  } catch (e) {
    if (token === generation) error.value = formatError(e, t);
  } finally {
    if (token === generation) busy.value = false;
  }
}
async function openPayment() {
  if (!paymentUrl.value) return;
  try {
    await billingApi.openPayment(paymentUrl.value, trade.value);
  } catch {
    try {
      await shellOpen(paymentUrl.value);
    } catch (e) {
      error.value = formatError(e, t);
    }
  }
}
function startPolling() {
  stopPolling();
  expired.value = false;
  pollStarted = Date.now();
  void checkStatus();
}
async function checkStatus() {
  if (
    checking.value ||
    !trade.value ||
    !props.show ||
    stage.value !== "pending"
  )
    return;
  stopPolling();
  checking.value = true;
  const token = generation;
  try {
    const status = await api.checkOrder(trade.value);
    if (token !== generation || !props.show) return;
    lastStatus.value = status;
    error.value = "";
    if (paymentState(status) === "complete") {
      stage.value = "complete";
      emit("done");
      return;
    }
    if (status === 2) {
      stage.value = "cancelled";
      return;
    }
  } catch (e) {
    if (token === generation) error.value = formatError(e, t);
  } finally {
    checking.value = false;
    if (token === generation && stage.value === "pending" && props.show) {
      if (Date.now() - pollStarted >= 10 * 60 * 1000) expired.value = true;
      else
        pollTimer = setTimeout(
          () => void checkStatus(),
          Date.now() - pollStarted > 60000 ? 8000 : 4000,
        );
    }
  }
}
function focusCheck() {
  if (props.show && stage.value === "pending") void checkStatus();
}
window.addEventListener("focus", focusCheck);
void listen<string>("payment://returned", (event) => {
  if (event.payload === trade.value) focusCheck();
})
  .then((off) => {
    unlisten = off;
  })
  .catch(() => {});
async function copyPayment() {
  const text =
    typeof checkout.value?.data === "string" ? checkout.value.data : "";
  if (!text) return;
  try {
    await writeText(text);
    toast.success("支付信息已复制");
  } catch (e) {
    error.value = formatError(e, t);
  }
}
watch([qrCanvas, checkout], async () => {
  if (
    !qrCanvas.value ||
    checkout.value?.type !== 0 ||
    typeof checkout.value.data !== "string"
  )
    return;
  try {
    await QRCode.toCanvas(qrCanvas.value, checkout.value.data, {
      width: 232,
      margin: 2,
      color: { dark: "#242539", light: "#ffffff" },
    });
  } catch {
    qrError.value = true;
  }
});
onBeforeUnmount(() => {
  generation++;
  couponGeneration++;
  stopPolling();
  if (couponTimer) clearTimeout(couponTimer);
  unlisten?.();
  window.removeEventListener("focus", focusCheck);
});
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    title="确认订阅"
    class="purchase-modal"
    style="width: min(520px, calc(100vw - 32px)); border-radius: 24px"
    :mask-closable="!busy"
    :closable="!busy"
    @update:show="close"
  >
    <div class="steps">
      <span :class="{ active: stage === 'select' }">01 选择订阅</span><i /><span
        :class="{ active: stage === 'review' }"
        >02 核对订单</span
      ><i /><span
        :class="{ active: stage === 'pending' || stage === 'complete' }"
        >03 完成支付</span
      >
    </div>
    <div class="summary">
      <div>
        <span class="eyebrow">YOUR SUBSCRIPTION</span>
        <h2>{{ name }}</h2>
        <span class="muted">{{ period }}</span>
      </div>
      <div class="summary-price">
        <small>¥</small>{{ money(total)
        }}<span>{{ detail ? "应付金额" : "预计应付" }}</span>
      </div>
    </div>
    <NSpin v-if="loading" class="loading" />
    <template v-else-if="stage === 'select' || stage === 'review'">
      <div v-if="!trade" class="coupon-field">
        <label>优惠码 <span>可选</span></label
        ><NInput
          v-model:value="couponCode"
          placeholder="输入优惠码，自动验证"
          :disabled="busy"
          clearable
        /><small v-if="couponBusy" class="muted">正在验证此套餐与周期…</small
        ><small v-else-if="couponError" class="error-text">{{
          couponError
        }}</small
        ><small v-else-if="coupon" class="success-text"
          >已优惠 ¥{{ money(discount) }}</small
        >
      </div>
      <div class="breakdown">
        <div>
          <span>套餐金额</span
          ><span
            >¥{{
              money(
                detail
                  ? detail.total_amount +
                      (detail.discount_amount ?? 0) +
                      (detail.balance_amount ?? 0) +
                      (detail.surplus_amount ?? 0)
                  : (priceCents ?? 0),
              )
            }}</span
          >
        </div>
        <div v-if="detail?.discount_amount || discount">
          <span>优惠券优惠</span
          ><span class="success-text"
            >− ¥{{ money(detail?.discount_amount ?? discount) }}</span
          >
        </div>
        <div v-if="detail?.balance_amount || (!detail && estimatedBalance)">
          <span>余额抵扣</span
          ><span class="success-text"
            >− ¥{{ money(detail?.balance_amount ?? estimatedBalance) }}</span
          >
        </div>
        <div v-if="detail?.surplus_amount">
          <span>旧套餐折抵</span
          ><span class="success-text"
            >− ¥{{ money(detail.surplus_amount) }}</span
          >
        </div>
        <div v-if="fee">
          <span>支付手续费</span><span>¥{{ money(fee) }}</span>
        </div>
      </div>
      <template v-if="payable > 0"
        ><label class="field-label"
          >支付方式 <span v-if="lockedMethod">已绑定此订单</span></label
        >
        <div class="payment-methods">
          <button
            v-for="m in methods"
            :key="m.id"
            type="button"
            :disabled="busy || (lockedMethod && m.id !== detail?.payment_id)"
            :class="{ selected: methodId === m.id }"
            @click="methodId = m.id"
          >
            <span class="radio-dot" /><strong>{{ m.name || m.payment }}</strong
            ><small v-if="m.handling_fee_fixed || m.handling_fee_percent"
              >含手续费 ¥{{
                money(
                  paymentFee(
                    payable,
                    m.handling_fee_fixed ?? 0,
                    m.handling_fee_percent ?? 0,
                  ),
                )
              }}</small
            >
          </button>
        </div>
        <p v-if="!methods.length" class="muted">
          当前暂无可用的支付渠道。你仍可创建订单核对余额及折抵，或联系客户支持。
        </p>
        <p
          v-if="
            lockedMethod && !methods.some((m) => m.id === detail?.payment_id)
          "
          class="error-text"
        >
          此订单绑定的支付方式已停用，请联系客户支持。
        </p></template
      >
      <NAlert v-else type="success" :show-icon="false"
        >此订单预计可通过优惠或余额结清，无需选择支付渠道。</NAlert
      >
      <p class="footnote">
        {{
          detail
            ? "金额已由服务器核对。关闭窗口会保留订单，可在订单中心继续支付。"
            : "创建订单后将自动使用账户余额，最终折抵金额将在下一步核对。"
        }}
      </p>
    </template>
    <template v-else-if="stage === 'pending'">
      <div class="pending">
        <template v-if="checkout?.type === 0"
          ><canvas ref="qrCanvas" />
          <p>使用对应支付应用扫描二维码</p>
          <p v-if="qrError" class="error-text">
            二维码生成失败，请复制支付信息。
          </p></template
        ><template v-else
          ><div class="payment-orbit"><span>↗</span></div>
          <h3>
            {{ lastStatus === 1 ? "付款已确认，正在开通" : "等待支付完成" }}
          </h3>
          <p>
            {{
              lastStatus === 1
                ? "服务器正在处理你的订阅，请稍候。"
                : "请在支付窗口完成付款。返回客户端后会自动更新。"
            }}
          </p></template
        >
        <p v-if="expired" class="timeout-note">
          暂未确认最终结果，已暂停自动检查。订单仍然保留，请勿重复付款。
        </p>
        <div v-else class="sync-note"><span />正在同步订单状态</div>
        <code>{{ trade }}</code>
      </div>
    </template>
    <div v-else-if="stage === 'complete'" class="result">
      <div class="success-mark">✓</div>
      <h2>订阅已就绪</h2>
      <p>付款已确认，套餐信息已更新。现在可以开始连接。</p>
    </div>
    <div v-else class="result">
      <h2>订单已取消</h2>
      <p>此订单无法继续付款，可以返回套餐页面重新选择。</p>
    </div>
    <NAlert v-if="error" type="warning" :show-icon="false" class="payment-error"
      >{{ error
      }}<NButton v-if="stage === 'select'" text type="primary" @click="toOrders"
        >前往订单中心查看已有订单 →</NButton
      ></NAlert
    >
    <template #footer
      ><NSpace justify="end" :size="10"
        ><NButton :disabled="busy" @click="close">{{
          stage === "pending" ? "稍后继续" : "关闭"
        }}</NButton
        ><NButton
          v-if="stage === 'select'"
          type="primary"
          :loading="busy"
          :disabled="loading || couponBusy || (!!couponCode.trim() && !coupon)"
          @click="createOrder"
          >{{ trade ? "重新核对订单" : "创建订单并核对" }}</NButton
        ><NButton
          v-if="stage === 'review'"
          type="primary"
          :loading="busy"
          :disabled="
            !canCheckout ||
            (payable > 0 && !methods.some((m) => m.id === methodId))
          "
          @click="pay"
          >{{ total === 0 ? "立即开通" : `确认支付 ¥${money(total)}` }}</NButton
        ><template v-if="stage === 'pending'"
          ><NButton
            v-if="checkout && typeof checkout.data === 'string'"
            @click="copyPayment"
            >复制支付信息</NButton
          ><NButton v-if="paymentUrl" @click="openPayment">打开支付页</NButton
          ><NButton type="primary" :loading="checking" @click="startPolling"
            >我已付款，检查状态</NButton
          ></template
        ><NButton
          v-if="stage === 'complete'"
          type="primary"
          @click="
            close();
            router.push({ name: 'home' });
          "
          >开始连接</NButton
        ></NSpace
      ></template
    >
  </NModal>
</template>

<style scoped>
.steps {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 11px;
  color: #aaa9bb;
  margin: 2px 0 25px;
}
.steps i {
  height: 1px;
  flex: 1;
  background: #eeedf5;
}
.steps .active {
  color: #7165e9;
  font-weight: 700;
}
.summary {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: #f6f5fe;
  border-radius: 18px;
  padding: 24px;
  margin-bottom: 24px;
}
.eyebrow {
  color: #aaa2db;
  font-size: 9px;
  letter-spacing: 1.5px;
}
.summary h2 {
  font-size: 21px;
  margin: 6px 0;
  color: #242539;
}
.summary-price {
  font-size: 30px;
  color: #7165e9;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.summary-price small {
  font-size: 16px;
  margin-right: 3px;
}
.summary-price > span {
  display: block;
  font-size: 11px;
  text-align: right;
  color: #9895b4;
  font-weight: 400;
}
.muted,
.footnote {
  color: #9391a6;
  font-size: 12px;
}
.coupon-field label,
.field-label {
  display: block;
  margin-bottom: 9px;
  font-size: 13px;
  font-weight: 600;
  color: #45445a;
}
.coupon-field label span,
.field-label span {
  font-size: 11px;
  color: #aaa8b9;
  margin-left: 5px;
  font-weight: 400;
}
.coupon-field small {
  display: block;
  margin-top: 7px;
}
.breakdown {
  display: grid;
  gap: 9px;
  margin: 22px 0;
  font-size: 12px;
  color: #777487;
}
.breakdown > div {
  display: flex;
  justify-content: space-between;
  gap: 15px;
}
.success-text {
  color: #35a888;
}
.error-text {
  color: #d8736c;
}
.payment-methods {
  display: grid;
  gap: 8px;
}
.payment-methods button {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 1px solid #e9e7f2;
  background: #fff;
  border-radius: 12px;
  padding: 13px 14px;
  color: #49465c;
  cursor: pointer;
  text-align: left;
}
.payment-methods button.selected {
  border-color: #7165e9;
  background: #f8f7ff;
}
.payment-methods button:disabled {
  opacity: 0.55;
  cursor: default;
}
.payment-methods button strong {
  font-weight: 500;
  flex: 1;
}
.payment-methods button small {
  color: #9c96af;
  font-size: 10px;
}
.radio-dot {
  height: 13px;
  width: 13px;
  border: 1px solid #d3cfe3;
  border-radius: 50%;
}
.selected .radio-dot {
  border: 4px solid #7165e9;
  height: 7px;
  width: 7px;
}
.footnote {
  font-size: 11px;
  line-height: 1.8;
  margin: 17px 0 0;
}
.pending,
.result {
  text-align: center;
  padding: 15px 0;
}
.pending canvas {
  border: 1px solid #eeedf5;
  border-radius: 18px;
  width: 232px;
  height: 232px;
}
.pending p,
.result p {
  color: #8d899d;
  font-size: 12px;
  line-height: 1.8;
}
.pending code {
  display: block;
  font-size: 10px;
  color: #aaa6ba;
  margin: 15px 0;
}
.payment-orbit,
.success-mark {
  height: 78px;
  width: 78px;
  border-radius: 24px;
  margin: 0 auto 20px;
  background: #eeeafd;
  color: #7165e9;
  display: grid;
  place-items: center;
  font-size: 32px;
}
.success-mark {
  background: #e8f7ef;
  color: #3aaa83;
}
.sync-note {
  display: inline-flex;
  gap: 7px;
  align-items: center;
  color: #9e96b8;
  font-size: 11px;
}
.sync-note span {
  width: 6px;
  height: 6px;
  background: #7165e9;
  border-radius: 50%;
  animation: pulse 2s infinite;
}
.timeout-note {
  background: #fff6e9;
  padding: 12px;
  border-radius: 12px;
  color: #b98e44 !important;
}
.payment-error {
  margin-top: 15px;
}
.payment-error :deep(.n-button) {
  display: block;
  margin-top: 8px;
}
.loading {
  display: block;
  margin: 30px auto;
}
@keyframes pulse {
  50% {
    opacity: 0.3;
  }
}
</style>
