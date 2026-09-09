<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { NButton, NEmpty, NPopconfirm, NSkeleton, useMessage } from "naive-ui";
import { api } from "@/api";
import type { Order } from "@/types";
import { formatError } from "@/utils/error";
import { money, periodNames } from "@/utils/billing";
import PurchaseModal from "@/components/PurchaseModal.vue";
import AppIcon from "@/components/AppIcon.vue";
import { useAuthStore } from "@/stores/auth";
const { t } = useI18n();
const router = useRouter();
const toast = useMessage();
const auth = useAuthStore();
const orders = ref<Order[]>([]);
const loading = ref(true);
const error = ref("");
const cancelling = ref<number | null>(null);
const filter = ref("all");
const showPurchase = ref(false);
const selected = ref<Order | null>(null);
const planNames = ref<Record<number, string>>({});
const statuses: Record<number, string> = {
  0: "待付款",
  1: "正在开通",
  2: "已取消",
  3: "已完成",
  4: "已折抵",
};
const pending = computed(() => orders.value.filter((o) => o.status === 0));
const completed = computed(() =>
  orders.value.filter((o) => o.status === 3 || o.status === 4),
);
const shown = computed(() =>
  filter.value === "all"
    ? orders.value
    : filter.value === "pending"
      ? pending.value
      : completed.value,
);
let fetching = false;
let active = true;
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
async function load() {
  if (fetching) return;
  fetching = true;
  loading.value = true;
  error.value = "";
  if (refreshTimer) clearTimeout(refreshTimer);
  try {
    const result = await api.fetchOrders();
    if (active) orders.value = result;
  } catch (e) {
    if (active) error.value = formatError(e, t);
  } finally {
    fetching = false;
    loading.value = false;
    if (active && orders.value.some((o) => o.status === 1))
      refreshTimer = setTimeout(() => void load(), 15000);
  }
}
function resume(order: Order) {
  selected.value = order;
  showPurchase.value = true;
}
async function cancel(order: Order) {
  cancelling.value = order.id;
  try {
    await api.cancelOrder(order.trade_no);
    toast.success("订单已取消，余额将由服务端退回");
    await load();
    void auth.refreshUser().catch(() => {});
  } catch (e) {
    toast.error(formatError(e, t));
  } finally {
    cancelling.value = null;
  }
}
function done() {
  void load();
  void Promise.allSettled([auth.refreshUser(), auth.refreshSubscribe()]);
}
function name(order: Order) {
  return planNames.value[order.plan_id ?? 0] || "订阅套餐";
}
function date(value: number | null | undefined) {
  return value
    ? new Date(value * 1000).toLocaleString("zh-CN", {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "—";
}
onMounted(() => {
  void load();
  window.addEventListener("focus", load);
  void api
    .fetchPlans()
    .then((rows) => {
      planNames.value = Object.fromEntries(rows.map((p) => [p.id, p.name]));
    })
    .catch(() => {});
});
onBeforeUnmount(() => {
  active = false;
  window.removeEventListener("focus", load);
  if (refreshTimer) clearTimeout(refreshTimer);
});
</script>
<template>
  <section class="orders-page">
    <div class="orders-intro">
      <div>
        <span class="eyebrow">YOUR SUBSCRIPTIONS, ORGANIZED</span>
        <h2>每一笔订阅，都清晰可见。</h2>
        <p>查看购买记录，继续付款，随时掌握订单进度。</p>
      </div>
      <NButton type="primary" round @click="router.push({ name: 'plans' })"
        >选购套餐 <span style="margin-left: 14px">→</span></NButton
      >
    </div>
    <div class="order-stats">
      <div>
        <span class="stat-icon lavender"
          ><AppIcon name="bag" :size="20"
        /></span>
        <section>
          <small>全部订单</small><strong>{{ orders.length }}<em>笔</em></strong>
        </section>
      </div>
      <div>
        <span class="stat-icon peach"><AppIcon name="card" :size="20" /></span>
        <section>
          <small>待支付</small><strong>{{ pending.length }}<em>笔</em></strong>
        </section>
      </div>
      <div>
        <span class="stat-icon mint"><AppIcon name="check" :size="20" /></span>
        <section>
          <small>已完成 / 已折抵</small
          ><strong>{{ completed.length }}<em>笔</em></strong>
        </section>
      </div>
    </div>
    <div class="orders-card">
      <div class="orders-toolbar">
        <div class="tabs">
          <button
            type="button"
            :class="{ active: filter === 'all' }"
            @click="filter = 'all'"
          >
            全部订单</button
          ><button
            type="button"
            :class="{ active: filter === 'pending' }"
            @click="filter = 'pending'"
          >
            待付款
            <span v-if="pending.length">{{ pending.length }}</span></button
          ><button
            type="button"
            :class="{ active: filter === 'complete' }"
            @click="filter = 'complete'"
          >
            已完成
          </button>
        </div>
        <NButton text :loading="loading" @click="load"
          ><AppIcon name="refresh" :size="15"
        /></NButton>
      </div>
      <div v-if="loading && !orders.length" class="empty">
        <NSkeleton text :repeat="6" />
      </div>
      <div v-else-if="error && !orders.length" class="empty">
        <NEmpty :description="error"
          ><template #extra
            ><NButton @click="load">重试</NButton></template
          ></NEmpty
        >
      </div>
      <div v-else-if="!shown.length" class="empty">
        <div class="empty-bag"><AppIcon name="bag" :size="31" /></div>
        <h3>
          {{ filter === "pending" ? "没有待付款订单" : "还没有相关订单" }}
        </h3>
        <p>
          {{
            filter === "pending"
              ? "一切准备就绪，享受你的连接。"
              : "挑选一份适合自己的套餐，开启顺畅连接。"
          }}
        </p>
        <NButton
          v-if="filter === 'all'"
          type="primary"
          secondary
          @click="router.push({ name: 'plans' })"
          >查看套餐</NButton
        >
      </div>
      <template v-else
        ><div class="order-table-head">
          <span>套餐与订单</span><span>金额</span><span>状态</span
          ><span>操作</span>
        </div>
        <article v-for="order in shown" :key="order.id" class="order-row">
          <div class="order-info">
            <span class="order-icon"><AppIcon name="bag" :size="18" /></span>
            <div>
              <strong
                >{{ name(order) }}
                <small>{{
                  periodNames[order.period || ""] || order.period
                }}</small></strong
              ><code>{{ order.trade_no }}</code
              ><time>{{ date(order.created_at) }}</time>
            </div>
          </div>
          <div class="order-amount">
            <strong>¥{{ money(order.total_amount) }}</strong
            ><small v-if="order.balance_amount"
              >余额抵扣 ¥{{ money(order.balance_amount) }}</small
            >
          </div>
          <div>
            <span class="status" :class="'status-' + order.status"
              ><i />{{ statuses[order.status] || "未知状态" }}</span
            >
          </div>
          <div class="order-actions">
            <template v-if="order.status === 0"
              ><NButton
                size="small"
                type="primary"
                secondary
                :disabled="cancelling === order.id"
                @click="resume(order)"
                >继续支付</NButton
              ><NPopconfirm
                :positive-text="'确认取消'"
                :negative-text="'保留订单'"
                @positive-click="cancel(order)"
                ><template #trigger
                  ><NButton text size="tiny" :loading="cancelling === order.id"
                    >取消订单</NButton
                  ></template
                >确认取消此订单？未支付订单将关闭，已抵扣余额按服务端规则退回。</NPopconfirm
              ></template
            ><NButton
              v-else-if="order.status === 1"
              size="small"
              text
              @click="load"
              >检查进度</NButton
            ><span v-else class="dash">—</span>
          </div>
        </article></template
      >
    </div>
    <p class="order-footnote">
      <AppIcon
        name="check"
        :size="13"
      />支付后无需重复下单。返回客户端会自动更新订单状态；支付渠道手续费在付款核对页单独列出。
    </p>
    <PurchaseModal
      v-model:show="showPurchase"
      :plan="null"
      :period-key="null"
      :price-cents="selected?.total_amount ?? null"
      :existing-trade-no="selected?.trade_no"
      :display-name="selected ? name(selected) : null"
      :display-period="periodNames[selected?.period || '']"
      @done="done"
    />
  </section>
</template>
<style scoped>
.orders-page {
  color: #242539;
}
.orders-intro {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  margin: 5px 0 28px;
}
.eyebrow {
  font-size: 10px;
  letter-spacing: 1.8px;
  font-weight: 600;
  color: #aaa7b9;
}
.orders-intro h2 {
  font-size: 27px;
  letter-spacing: -0.6px;
  font-weight: 650;
  margin: 11px 0 10px;
}
.orders-intro p {
  font-size: 13px;
  color: #9390a4;
  margin: 0;
}
.order-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 19px;
  margin-bottom: 24px;
}
.order-stats > div {
  display: flex;
  align-items: center;
  gap: 15px;
  background: #fff;
  border: 1px solid #eeebf4;
  padding: 21px 25px;
  border-radius: 20px;
}
.stat-icon {
  height: 44px;
  width: 44px;
  border-radius: 14px;
  display: grid;
  place-items: center;
  flex-shrink: 0;
}
.lavender {
  background: #eeebfc;
  color: #aa98d8;
}
.peach {
  background: #fff3e8;
  color: #dbb289;
}
.mint {
  background: #eaf6f0;
  color: #8ac0ae;
}
.order-stats small {
  display: block;
  font-size: 10px;
  color: #aaa0b6;
  margin-bottom: 5px;
}
.order-stats strong {
  font-size: 23px;
  line-height: 1.2;
  font-weight: 600;
}
.order-stats em {
  font-size: 10px;
  font-style: normal;
  color: #bfb4c7;
  font-weight: 400;
  margin-left: 7px;
}
.orders-card {
  background: #fff;
  border: 1px solid #eeebf4;
  border-radius: 24px;
  overflow: hidden;
}
.orders-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 1px solid #f2eef7;
  padding: 20px 26px 0;
}
.orders-toolbar :deep(.n-button) {
  margin-bottom: 19px;
  color: #b2a6bd;
}
.tabs {
  display: flex;
  gap: 25px;
}
.tabs button {
  position: relative;
  border: 0;
  background: transparent;
  padding: 0 0 21px;
  color: #b4a9be;
  font-size: 12px;
  cursor: pointer;
}
.tabs button.active {
  color: #8c76d3;
  font-weight: 600;
}
.tabs button.active:after {
  position: absolute;
  content: "";
  bottom: 0;
  left: 0;
  right: 0;
  height: 2px;
  background: #9380dc;
  border-radius: 5px;
}
.tabs span {
  display: inline-block;
  background: #f3eaf7;
  color: #b08cbf;
  font-size: 9px;
  padding: 0 5px;
  border-radius: 4px;
  margin-left: 5px;
}
.order-table-head,
.order-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 150px 115px 115px;
  gap: 15px;
  align-items: center;
  padding: 16px 27px;
}
.order-table-head {
  font-size: 10px;
  color: #b8adc3;
  background: #fdfbfe;
}
.order-row {
  padding-top: 23px;
  padding-bottom: 23px;
  border-top: 1px solid #f6f2f9;
}
.order-info {
  display: flex;
  align-items: center;
  gap: 13px;
  min-width: 0;
}
.order-icon {
  height: 39px;
  width: 39px;
  border-radius: 12px;
  display: grid;
  place-items: center;
  background: #f3eefb;
  color: #b3a0cf;
  flex-shrink: 0;
}
.order-info > div {
  min-width: 0;
}
.order-info strong {
  font-size: 12px;
  font-weight: 550;
  display: block;
}
.order-info strong small {
  font-size: 10px;
  color: #b5a6c1;
  font-weight: 400;
  margin-left: 4px;
}
.order-info code {
  font-size: 9px;
  color: #b4a7c1;
  display: block;
  margin: 5px 0;
  overflow-wrap: anywhere;
}
.order-info time {
  font-size: 9px;
  color: #c7bbd1;
}
.order-amount strong {
  display: block;
  font-size: 14px;
  font-weight: 550;
}
.order-amount small {
  display: block;
  font-size: 9px;
  color: #b3a2c2;
  margin-top: 6px;
}
.status {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  background: #f7f4fa;
  padding: 5px 8px;
  border-radius: 6px;
  color: #b3a7bf;
  font-size: 10px;
  white-space: nowrap;
}
.status i {
  height: 4px;
  width: 4px;
  border-radius: 50%;
  background: currentColor;
}
.status-0 {
  color: #c7a373;
  background: #fff7ea;
}
.status-1 {
  color: #a08dd1;
  background: #f4efff;
}
.status-3,
.status-4 {
  color: #84b2a0;
  background: #edf7f2;
}
.order-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 9px;
}
.order-actions :deep(.n-button) {
  font-size: 10px;
  border-radius: 7px;
}
.order-actions :deep(.n-button--text-type) {
  color: #b7a8c5;
}
.dash {
  color: #d3cadb;
}
.order-footnote {
  font-size: 10px;
  line-height: 1.7;
  display: flex;
  align-items: center;
  gap: 7px;
  color: #b5a8c2;
  margin: 21px 7px;
}
.empty {
  padding: 58px 25px;
  text-align: center;
}
.empty-bag {
  width: 70px;
  height: 70px;
  border-radius: 23px;
  background: #f4eefb;
  display: grid;
  place-items: center;
  color: #c3b0dc;
  margin: 0 auto 22px;
}
.empty h3 {
  font-size: 17px;
  font-weight: 500;
}
.empty p {
  font-size: 12px;
  color: #b4a6c1;
  margin-bottom: 23px;
}
@media (max-width: 900px) {
  .order-table-head,
  .order-row {
    grid-template-columns: minmax(0, 1fr) 100px 85px;
    gap: 10px;
    padding-left: 20px;
    padding-right: 20px;
  }
  .order-table-head > span:nth-child(3) {
    display: none;
  }
  .order-row > div:nth-child(3) {
    grid-column: 2;
    grid-row: auto;
  }
  .order-row .order-actions {
    grid-column: 3;
    grid-row: 1 / span 2;
  }
  .order-row .order-info {
    grid-row: 1 / span 2;
  }
  .order-stats > div {
    padding: 18px 16px;
    gap: 10px;
  }
  .order-stats {
    gap: 12px;
  }
}
@media (max-width: 650px) {
  .orders-intro h2 {
    font-size: 23px;
  }
  .order-stats > div {
    padding: 17px 11px;
  }
  .stat-icon {
    display: none;
  }
  .order-stats small {
    font-size: 9px;
  }
  .order-icon {
    display: none;
  }
}
</style>
