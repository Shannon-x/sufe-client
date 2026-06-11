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
import type { Plan } from "@/types";
import { formatError } from "@/utils/error";
import PurchaseModal from "@/components/PurchaseModal.vue";
import { useAuthStore } from "@/stores/auth";

const { t } = useI18n();
const router = useRouter();
const message = useMessage();
const auth = useAuthStore();

const plans = ref<Plan[]>([]);
const loading = ref(true);

// Purchase modal state — null plan means hidden.
const showPurchase = ref(false);
const purchasePlan = ref<Plan | null>(null);
const purchasePeriodKey = ref<string | null>(null);
const purchasePriceCents = ref<number | null>(null);

function openPurchase(plan: Plan, key: keyof Plan, cents: number) {
  purchasePlan.value = plan;
  purchasePeriodKey.value = String(key);
  purchasePriceCents.value = cents;
  showPurchase.value = true;
}

// After a successful order we refresh the user's subscription so the
// home page shows the new plan immediately. We don't await — failures
// are non-fatal and Home.vue will retry on its own refresh button.
async function onPurchaseDone() {
  void auth.refreshUser();
  void auth.refreshSubscribe();
}

async function load() {
  loading.value = true;
  try {
    plans.value = await api.fetchPlans();
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}

onMounted(load);

// Hide plans the panel admin marked invisible. `sell=false` is kept so users
// can still see plans they're already on (e.g. for renew price reference)
// but rendered in a disabled style.
const visible = computed(() => plans.value.filter((p) => p.show));
const empty = computed(() => !loading.value && visible.value.length === 0);

interface PriceRow {
  key: keyof Plan;
  labelKey: string;
}
const PERIODS: PriceRow[] = [
  { key: "month_price", labelKey: "plans.period.month" },
  { key: "quarter_price", labelKey: "plans.period.quarter" },
  { key: "half_year_price", labelKey: "plans.period.halfYear" },
  { key: "year_price", labelKey: "plans.period.year" },
  { key: "two_year_price", labelKey: "plans.period.twoYear" },
  { key: "three_year_price", labelKey: "plans.period.threeYear" },
  { key: "onetime_price", labelKey: "plans.period.onetime" },
];

function pricesFor(
  p: Plan,
): { key: keyof Plan; label: string; cents: number }[] {
  return PERIODS.flatMap(({ key, labelKey }) => {
    const cents = p[key] as number | null;
    if (cents === null || cents === undefined) return [];
    return [{ key, label: t(labelKey), cents }];
  });
}

function yuan(cents: number): string {
  return (cents / 100).toFixed(2);
}

// Backend stores HTML / markdown for plan content. Render plain text for the
// same XSS-defence reason as Notices.vue — admins paste arbitrary HTML.
function plainContent(html: string): string {
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

// Xboard panel admins increasingly paste a JSON array
// `[{"feature":"…","support":true}, …]` into the plan body to get a
// checklist-style render. Detect that and parse it; otherwise fall back
// to the HTML→text path above.
interface PlanFeature {
  feature: string;
  support: boolean;
}

function featuresOf(content: string | null | undefined): PlanFeature[] {
  if (!content) return [];
  const trimmed = content.trim();
  if (!trimmed.startsWith("[")) return [];
  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (!Array.isArray(parsed)) return [];
    const rows: PlanFeature[] = [];
    for (const item of parsed) {
      if (!item || typeof item !== "object") continue;
      const obj = item as Record<string, unknown>;
      if (typeof obj.feature !== "string") continue;
      rows.push({
        feature: obj.feature,
        support: obj.support !== false, // default true when omitted
      });
    }
    return rows;
  } catch {
    return [];
  }
}
</script>

<template>
  <NLayout class="plans-shell">
    <NLayoutHeader bordered class="plans-header">
      <NSpace align="center" :size="10">
        <NButton size="small" quaternary @click="router.push({ name: 'home' })">
          ← {{ t("plans.back") }}
        </NButton>
        <NText strong>{{ t("plans.title") }}</NText>
      </NSpace>
      <NButton size="small" quaternary :loading="loading" @click="load">
        {{ t("plans.refresh") }}
      </NButton>
    </NLayoutHeader>

    <NLayoutContent class="plans-content">
      <div class="list">
        <template v-if="loading && plans.length === 0">
          <NCard v-for="i in 3" :key="i" embedded class="plan-card">
            <NSkeleton text :repeat="4" />
          </NCard>
        </template>

        <NEmpty v-else-if="empty" :description="t('plans.empty')" />

        <NCard
          v-for="p in visible"
          :key="p.id"
          embedded
          class="plan-card"
          :title="p.name || `#${p.id}`"
        >
          <template #header-extra>
            <NSpace :size="6">
              <NTag
                v-if="auth.userInfo?.plan_id === p.id"
                size="small"
                :bordered="false"
                type="success"
              >
                {{ t("plans.tag.current") }}
              </NTag>
              <NTag
                v-if="!p.sell"
                size="small"
                :bordered="false"
                type="warning"
              >
                {{ t("plans.tag.notSelling") }}
              </NTag>
              <NTag
                v-if="!p.renew"
                size="small"
                :bordered="false"
                type="default"
              >
                {{ t("plans.tag.noRenew") }}
              </NTag>
            </NSpace>
          </template>

          <NText depth="3" class="meta">
            {{ t("plans.transferEnable", { gb: p.transfer_enable }) }}
          </NText>

          <ul v-if="featuresOf(p.content).length" class="features">
            <li
              v-for="(f, i) in featuresOf(p.content)"
              :key="i"
              :class="{ 'is-on': f.support, 'is-off': !f.support }"
            >
              <span class="check">{{ f.support ? "✓" : "✕" }}</span>
              <span class="feature-text">{{ f.feature }}</span>
            </li>
          </ul>
          <pre v-else-if="p.content" class="plan-body">{{ plainContent(p.content) }}</pre>

          <div v-if="pricesFor(p).length" class="prices">
            <div
              v-for="row in pricesFor(p)"
              :key="String(row.key)"
              class="price-row"
            >
              <span class="price-label">{{ row.label }}</span>
              <span class="price-value">¥ {{ yuan(row.cents) }}</span>
              <NButton
                v-if="p.sell"
                size="tiny"
                type="primary"
                ghost
                class="price-buy"
                @click="openPurchase(p, row.key, row.cents)"
              >
                {{ t("plans.buy") }}
              </NButton>
            </div>
          </div>
          <NText v-else depth="3" class="no-price">
            {{ t("plans.noPrices") }}
          </NText>

          <div v-if="p.reset_price !== null" class="reset-row">
            <NTag size="small" :bordered="false" type="info">
              {{ t("plans.resetPrice") }}: ¥ {{ yuan(p.reset_price ?? 0) }}
            </NTag>
          </div>
        </NCard>
      </div>
    </NLayoutContent>

    <PurchaseModal
      v-model:show="showPurchase"
      :plan="purchasePlan"
      :period-key="purchasePeriodKey"
      :price-cents="purchasePriceCents"
      @done="onPurchaseDone"
    />
  </NLayout>
</template>

<style scoped>
.plans-shell {
  min-height: 100vh;
  background: var(--n-color);
}
.plans-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  gap: 16px;
}
.plans-content {
  padding: 20px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 760px;
  margin: 0 auto;
}
.plan-card {
  border-radius: 10px;
}
.meta {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.plan-body {
  margin: 8px 0 12px;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 13px;
  line-height: 1.65;
}
.features {
  list-style: none;
  margin: 8px 0 12px;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.features li {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  font-size: 13px;
  line-height: 1.5;
}
.features .check {
  flex: 0 0 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  margin-top: 2px;
  border-radius: 50%;
  font-size: 11px;
  font-weight: 700;
}
.features .is-on .check {
  background: rgba(74, 222, 128, 0.18);
  color: #4ade80;
}
.features .is-off .check {
  background: rgba(255, 90, 122, 0.18);
  color: #ff5a7a;
}
.features .feature-text {
  flex: 1;
  word-break: break-word;
}
.features .is-off .feature-text {
  color: var(--n-text-color-3);
  text-decoration: line-through;
  text-decoration-color: rgba(255, 90, 122, 0.4);
}
.prices {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: 6px 12px;
  margin-top: 8px;
}
.price-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  padding: 4px 8px;
  background: var(--n-action-color, rgba(128, 128, 128, 0.06));
  border-radius: 6px;
}
.price-label {
  color: var(--n-text-color-3);
  flex: 1;
}
.price-value {
  font-variant-numeric: tabular-nums;
  font-weight: 500;
}
.price-buy {
  flex-shrink: 0;
}
.no-price {
  font-size: 12px;
  margin-top: 8px;
  display: block;
}
.reset-row {
  margin-top: 10px;
}
</style>
