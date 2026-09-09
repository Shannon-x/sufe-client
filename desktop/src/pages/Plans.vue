<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { NButton, NEmpty, NSkeleton, useMessage } from "naive-ui";
import { api } from "@/api";
import type { Plan } from "@/types";
import { formatError } from "@/utils/error";
import { money, periodNames } from "@/utils/billing";
import PurchaseModal from "@/components/PurchaseModal.vue";
import { useAuthStore } from "@/stores/auth";
import { useFeaturesStore } from "@/stores/features";
const { t } = useI18n();
const router = useRouter();
const toast = useMessage();
const auth = useAuthStore();
const featuresConfig = useFeaturesStore();
const plans = ref<Plan[]>([]);
const loading = ref(true);
const failed = ref(false);
const showPurchase = ref(false);
const purchasePlan = ref<Plan | null>(null);
const purchasePeriodKey = ref<string | null>(null);
const purchasePriceCents = ref<number | null>(null);
const selected = ref<Record<number, string>>({});
const periods = [
  "month_price",
  "quarter_price",
  "half_year_price",
  "year_price",
  "two_year_price",
  "three_year_price",
  "onetime_price",
];
const visible = computed(() =>
  plans.value
    .filter((p) => p.show || p.id === auth.userInfo?.plan_id)
    .sort((a, b) => (a.sort ?? 0) - (b.sort ?? 0)),
);
function prices(p: Plan) {
  return periods.flatMap((key) => {
    const value = p[key as keyof Plan];
    return typeof value === "number" && value >= 0
      ? [{ key, cents: value, label: periodNames[key] }]
      : [];
  });
}
function activePrice(p: Plan) {
  const rows = prices(p);
  return rows.find((row) => row.key === selected.value[p.id]) || rows[0];
}
function canBuy(p: Plan) {
  return (
    featuresConfig.enabled("purchase") &&
    (p.id === auth.userInfo?.plan_id ? p.renew : p.sell)
  );
}
function buy(p: Plan, reset = false) {
  const price = activePrice(p);
  if (!reset && !price) return;
  purchasePlan.value = p;
  purchasePeriodKey.value = reset ? "reset_price" : price!.key;
  purchasePriceCents.value = reset ? p.reset_price : price!.cents;
  showPurchase.value = true;
}
async function load() {
  loading.value = true;
  failed.value = false;
  try {
    plans.value = await api.fetchPlans();
  } catch (e) {
    failed.value = true;
    toast.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}
function onDone() {
  void Promise.allSettled([auth.refreshUser(), auth.refreshSubscribe()]);
}
function features(content: string): { text: string; support: boolean }[] {
  if (!content) return [];
  try {
    const value: unknown = JSON.parse(content);
    if (Array.isArray(value))
      return value
        .filter((v) => v && typeof v.feature === "string")
        .map((v) => ({ text: v.feature, support: v.support !== false }));
  } catch {}
  const plain = content
    .replace(/<\s*br\s*\/?\s*>|<\/(?:p|div|li|h[1-6])>/gi, "\n")
    .replace(/<[^>]*>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"');
  return plain
    .split("\n")
    .map((text) => text.trim())
    .filter(Boolean)
    .map((text) => ({ text, support: true }));
}
onMounted(load);
</script>
<template>
  <section class="plans-page">
    <div class="catalog-intro">
      <div>
        <span class="eyebrow">FIND YOUR CONNECTION</span>
        <h2>让每一次连接，都恰到好处。</h2>
        <p>选择适合你的套餐。清晰的价格，自由的使用节奏。</p>
      </div>
      <NButton secondary round @click="router.push({ name: 'orders' })"
        >我的订单 ↗</NButton
      >
    </div>
    <div class="catalog-bar">
      <div class="small-label">
        <span />所有订阅套餐 <b>{{ visible.length }}</b>
      </div>
      <NButton text :loading="loading" @click="load">刷新套餐 ↻</NButton>
    </div>
    <div v-if="loading && !plans.length" class="plan-grid">
      <div v-for="i in 3" :key="i" class="plan-card">
        <NSkeleton height="28px" width="55%" /><NSkeleton
          text
          :repeat="8"
          style="margin-top: 30px"
        />
      </div>
    </div>
    <div v-else-if="!visible.length" class="empty-card">
      <NEmpty
        :description="
          failed ? '套餐暂时加载失败，请检查网络后重试' : '暂时没有上架的套餐'
        "
        ><template #extra
          ><NButton @click="load">重新加载</NButton></template
        ></NEmpty
      >
    </div>
    <div v-else class="plan-grid">
      <article
        v-for="(plan, index) in visible"
        :key="plan.id"
        class="plan-card"
        :class="{ current: plan.id === auth.userInfo?.plan_id }"
      >
        <div class="plan-top">
          <div class="plan-symbol" :class="'symbol-' + (index % 3)">
            <svg width="28" height="28" viewBox="0 0 24 24" fill="none">
              <path
                v-if="index % 3 === 0"
                d="M12 3 4 8v8l8 5 8-5V8l-8-5Zm0 0v18M4 8l8 5 8-5"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linejoin="round"
              />
              <path
                v-else-if="index % 3 === 1"
                d="m13 2-9 12h7l-1 8 10-13h-7l0-7Z"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linejoin="round"
              />
              <path
                v-else
                d="m12 3 3 6 7 1-5 5 1 7-6-3-6 3 1-7-5-5 7-1 3-6Z"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linejoin="round"
              />
            </svg>
          </div>
          <span v-if="plan.id === auth.userInfo?.plan_id" class="plan-badge"
            >当前套餐</span
          ><span v-else-if="!plan.sell" class="unavailable-badge">暂停售卖</span
          ><span v-else class="plan-index">{{
            String(index + 1).padStart(2, "0")
          }}</span>
        </div>
        <h3>{{ plan.name }}</h3>
        <p class="plan-description">
          {{ plan.transfer_enable }} GB
          {{
            activePrice(plan)?.key === "onetime_price" ? "总流量" : "套餐流量"
          }}
        </p>
        <div class="plan-price">
          <template v-if="activePrice(plan)"
            ><span>¥</span
            ><strong>{{ money(activePrice(plan)!.cents).split(".")[0] }}</strong
            ><small>.{{ money(activePrice(plan)!.cents).split(".")[1] }}</small
            ><em>/ {{ activePrice(plan)!.label }}</em></template
          ><span v-else class="no-price">暂无定价</span>
        </div>
        <div class="period-selector">
          <button
            v-for="row in prices(plan)"
            :key="row.key"
            type="button"
            :class="{ active: activePrice(plan)?.key === row.key }"
            @click="selected[plan.id] = row.key"
          >
            {{ row.label }}
          </button>
        </div>
        <div class="plan-divider" />
        <ul class="plan-features">
          <li>
            <span class="check">✓</span
            ><span>{{ plan.transfer_enable }} GB 高速流量</span>
          </li>
          <li
            v-for="(feature, i) in features(plan.content)"
            :key="i"
            :class="{ unsupported: !feature.support }"
          >
            <span class="check">{{ feature.support ? "✓" : "−" }}</span
            ><span>{{ feature.text }}</span>
          </li>
          <li v-if="!features(plan.content).length">
            <span class="check">✓</span><span>统一账户，多端使用</span>
          </li>
        </ul>
        <NButton
          class="buy-button"
          type="primary"
          size="large"
          :disabled="!canBuy(plan) || !activePrice(plan)"
          @click="buy(plan)"
          >{{
            !canBuy(plan)
              ? plan.id === auth.userInfo?.plan_id
                ? "此套餐暂不支持续费"
                : "暂不可购买"
              : plan.id === auth.userInfo?.plan_id
                ? "续费当前套餐"
                : "选择此套餐"
          }}
          <span class="button-arrow">→</span></NButton
        ><button
          v-if="plan.id === auth.userInfo?.plan_id && plan.reset_price != null"
          type="button"
          class="reset-link"
          @click="buy(plan, true)"
        >
          流量用完了？¥{{ money(plan.reset_price) }} 重置流量
        </button>
      </article>
    </div>
    <div class="purchase-note">
      <div class="note-icon">i</div>
      <div>
        <strong>购买前，了解这些小细节</strong>
        <p>
          账户余额和优惠码会自动抵扣，升级套餐的折抵金额以订单核对页为准。付款完成后，订阅会自动更新。
        </p>
      </div>
      <button type="button" @click="router.push({ name: 'support' })">
        需要帮助？ →
      </button>
    </div>
    <PurchaseModal
      v-model:show="showPurchase"
      :plan="purchasePlan"
      :period-key="purchasePeriodKey"
      :price-cents="purchasePriceCents"
      @done="onDone"
    />
  </section>
</template>
<style scoped>
.plans-page {
  color: #242539;
}
.catalog-intro {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  margin: 5px 0 33px;
}
.eyebrow {
  font-size: 10px;
  letter-spacing: 1.8px;
  font-weight: 600;
  color: #aaa7b9;
}
.catalog-intro h2 {
  font-size: 27px;
  letter-spacing: -0.6px;
  font-weight: 650;
  margin: 11px 0 10px;
}
.catalog-intro p {
  font-size: 13px;
  color: #9390a4;
  margin: 0;
}
.catalog-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 19px;
}
.small-label {
  display: flex;
  align-items: center;
  gap: 9px;
  font-size: 13px;
  font-weight: 600;
}
.small-label > span {
  width: 6px;
  height: 6px;
  background: #7165e9;
  border-radius: 50%;
}
.small-label b {
  font-size: 11px;
  color: #a19bae;
  background: #edeaf6;
  padding: 1px 7px;
  border-radius: 6px;
}
.catalog-bar :deep(.n-button) {
  font-size: 11px;
  color: #9690a5;
}
.plan-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 19px;
}
.plan-card {
  background: #fff;
  border: 1px solid #eeecf4;
  border-radius: 24px;
  padding: 27px 25px 22px;
  display: flex;
  flex-direction: column;
  position: relative;
  transition:
    transform 0.2s,
    box-shadow 0.2s;
}
.plan-card:hover {
  transform: translateY(-3px);
  box-shadow: 0 12px 34px #34305408;
}
.plan-card.current {
  border-color: #b5acf4;
  box-shadow: 0 7px 25px #7165e910;
}
.plan-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.plan-symbol {
  height: 49px;
  width: 49px;
  border-radius: 15px;
  background: #eeeafa;
  color: #9985d8;
  display: grid;
  place-items: center;
}
.symbol-1 {
  background: #eeeaff;
  color: #8474e7;
}
.symbol-2 {
  background: #fff1e5;
  color: #c49b6f;
}
.plan-index {
  font-size: 12px;
  color: #d8d4e2;
  letter-spacing: 1px;
}
.plan-badge,
.unavailable-badge {
  background: #f0edff;
  color: #8071d9;
  padding: 5px 9px;
  border-radius: 7px;
  font-size: 10px;
}
.unavailable-badge {
  color: #a7a0b1;
  background: #f6f4f8;
}
.plan-card h3 {
  font-size: 21px;
  font-weight: 650;
  margin: 22px 0 5px;
}
.plan-description {
  margin: 0;
  color: #aaa4b5;
  font-size: 12px;
}
.plan-price {
  display: flex;
  align-items: baseline;
  gap: 2px;
  margin: 24px 0 21px;
  white-space: nowrap;
}
.plan-price > span {
  font-size: 18px;
  color: #898197;
  margin-right: 4px;
}
.plan-price strong {
  font-size: 40px;
  font-weight: 650;
  letter-spacing: -1.4px;
}
.plan-price small {
  font-size: 22px;
  font-weight: 550;
}
.plan-price em {
  font-style: normal;
  font-size: 11px;
  color: #aaa3b7;
  margin-left: 5px;
}
.period-selector {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
  min-height: 28px;
}
.period-selector button {
  border: 0;
  border-radius: 7px;
  color: #a59eaf;
  background: #f7f6fa;
  padding: 6px 8px;
  font-size: 10px;
  cursor: pointer;
}
.period-selector button.active {
  color: #8270de;
  background: #eee9fd;
  font-weight: 600;
}
.plan-divider {
  height: 1px;
  background: #f1eef6;
  margin: 23px 0 21px;
}
.plan-features {
  list-style: none;
  padding: 0;
  margin: 0 0 26px;
  display: grid;
  gap: 14px;
  flex: 1;
  align-content: start;
}
.plan-features li {
  display: flex;
  align-items: start;
  gap: 9px;
  font-size: 11px;
  line-height: 1.8;
  color: #8b839a;
}
.check {
  color: #9283d3;
  font-size: 10px;
  flex-shrink: 0;
}
.unsupported {
  opacity: 0.45;
  text-decoration: line-through;
}
.buy-button {
  width: 100%;
  border-radius: 11px !important;
  font-size: 12px !important;
  height: 42px !important;
}
.button-arrow {
  margin-left: 12px;
}
.reset-link {
  border: 0;
  background: transparent;
  color: #9e91c4;
  font-size: 10px;
  margin-top: 13px;
  cursor: pointer;
}
.purchase-note {
  margin-top: 26px;
  border: 1px solid #ebe7f3;
  border-radius: 17px;
  padding: 20px 23px;
  display: flex;
  align-items: center;
  gap: 14px;
  background: #f3f1f9;
}
.note-icon {
  height: 30px;
  width: 30px;
  display: grid;
  place-items: center;
  border: 1px solid #dcd5ee;
  border-radius: 10px;
  color: #a296c2;
  flex-shrink: 0;
}
.purchase-note strong {
  font-size: 12px;
  font-weight: 550;
  color: #857a9e;
}
.purchase-note p {
  font-size: 11px;
  color: #aca1ba;
  line-height: 1.7;
  margin: 5px 0 0;
}
.purchase-note button {
  border: 0;
  white-space: nowrap;
  background: transparent;
  color: #9b8db8;
  font-size: 11px;
  margin-left: auto;
  cursor: pointer;
}
.empty-card {
  background: white;
  padding: 70px 20px;
  border-radius: 24px;
}
@media (max-width: 1150px) {
  .plan-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
@media (max-width: 730px) {
  .plan-grid {
    grid-template-columns: 1fr;
  }
  .catalog-intro {
    align-items: start;
  }
  .catalog-intro h2 {
    font-size: 23px;
  }
  .purchase-note {
    flex-wrap: wrap;
  }
  .purchase-note button {
    margin-left: 44px;
  }
}
</style>
