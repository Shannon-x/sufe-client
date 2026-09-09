<script setup lang="ts">
import { onMounted, onUnmounted, ref, computed, watch } from "vue";
import { useMessage } from "naive-ui";
import { useI18n } from "vue-i18n";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import AppIcon from "@/components/AppIcon.vue";
import { useAuthStore } from "@/stores/auth";
import { useFeaturesStore } from "@/stores/features";
import {
  extensions,
  type GiftCardCheck,
  type GiftCardHistory,
  type Invites,
} from "@/api/extensions";
import { formatError } from "@/utils/error";
import { money, bytes } from "@/utils/format";
import { isDesktop } from "@/platform";
const auth = useAuthStore(),
  features = useFeaturesStore(),
  message = useMessage(),
  { t } = useI18n();
const code = ref(""),
  busy = ref(false),
  error = ref(""),
  preview = ref<GiftCardCheck | null>(null),
  checkedCode = ref(""),
  history = ref<GiftCardHistory | null>(null),
  invites = ref<Invites | null>(null),
  historyError = ref(""),
  inviteError = ref(""),
  inviteBusy = ref(false);
let generation = 0;
watch(() => [features.enabled('gift_card'), features.enabled('invite')], () => {
  generation++;
  preview.value = null;
  checkedCode.value = '';
  error.value = '';
  if (!features.enabled('gift_card')) history.value = null;
  if (!features.enabled('invite')) invites.value = null;
}, { flush: 'sync' });
onUnmounted(() => { generation++; });
const stats = computed(() => invites.value?.stat || []);
const rewardLabels: Record<string, string> = {
  balance: "账户余额",
  commission_balance: "推广余额",
  transfer_enable: "流量额度",
  traffic: "流量",
  days: "有效天数",
  expire_days: "延长有效期",
  plan_id: "订阅套餐",
  reset_traffic: "重置流量",
  reset_package: "重置已用流量",
  device_limit: "增加设备数量",
  invite_reward_rate: "邀请奖励比例",
};
const isMystery = computed(
  () =>
    Number(
      (preview.value?.code_info?.template as { type?: number } | undefined)
        ?.type,
    ) === 3,
);
const visibleRewards = computed(() =>
  Object.fromEntries(
    Object.entries(preview.value?.reward_preview || {}).filter(
      ([key]) => !["random_rewards", "weight"].includes(key),
    ),
  ),
);
function displayReward(key: string, value: unknown) {
  if (typeof value === "number") {
    if (/balance|amount/.test(key)) return money(value);
    if (["transfer_enable", "traffic"].includes(key)) return bytes(value);
    if (["days", "expire_days"].includes(key)) return `${value} 天`;
    if (key === "device_limit") return `${value} 台`;
    if (key === "invite_reward_rate") return `${Math.round(value * 100)}%`;
  }
  if (["reset_package", "reset_traffic"].includes(key))
    return value ? "是" : "否";
  if (key === "plan_id")
    return String(
      (preview.value?.code_info?.plan_info as { name?: string } | undefined)
        ?.name || `套餐 ${value}`,
    );
  return typeof value === "object" ? JSON.stringify(value) : String(value);
}
function historyDate(value: string | number) {
  const numeric = Number(value);
  const date = new Date(Number.isFinite(numeric) ? numeric * 1000 : value);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleDateString("zh-CN");
}
async function loadBenefits() {
  await features.ensure();
  const version = generation;
  if (features.enabled("gift_card"))
    try {
      const snapshot = await extensions.giftCardHistory();
      if (version !== generation) return;
      history.value = snapshot;
      historyError.value = "";
    } catch (e) {
      if (version === generation) historyError.value = formatError(e, t);
    }
  if (features.enabled("invite"))
    try {
      const snapshot = await extensions.fetchInvites();
      if (version !== generation) return;
      invites.value = snapshot;
      inviteError.value = "";
    } catch (e) {
      if (version === generation) inviteError.value = formatError(e, t);
    }
}
async function check() {
  if (!features.enabled('gift_card') || !code.value.trim() || busy.value) return;
  const version = generation, requestedCode = code.value.trim();
  busy.value = true;
  error.value = "";
  preview.value = null;
  try {
    const snapshot = await extensions.checkGiftCard(requestedCode);
    if (version !== generation) return;
    preview.value = snapshot;
    checkedCode.value = requestedCode;
  } catch (e) {
    if (version === generation) error.value = formatError(e, t);
  } finally {
    busy.value = false;
  }
}
async function redeem() {
  if (
    !features.enabled('gift_card') ||
    !preview.value?.can_redeem ||
    checkedCode.value !== code.value.trim() ||
    busy.value
  )
    return;
  busy.value = true;
  error.value = "";
  try {
    const result = await extensions.redeemGiftCard(checkedCode.value);
    message.success(result.message || "礼品卡兑换成功");
    code.value = "";
    preview.value = null;
    await Promise.allSettled([
      auth.refreshUser(),
      auth.refreshSubscribe(),
      loadBenefits(),
    ]);
  } catch (e) {
    error.value = formatError(e, t);
  } finally {
    busy.value = false;
  }
}
async function createInvite() {
  if (!features.enabled('invite') || inviteBusy.value) return;
  inviteBusy.value = true;
  try {
    await extensions.createInvite();
    invites.value = await extensions.fetchInvites();
    message.success("邀请码已生成");
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    inviteBusy.value = false;
  }
}
async function copy(text: string) {
  try {
    if (isDesktop) await writeText(text);
    else await navigator.clipboard.writeText(text);
    message.success("已复制邀请码");
  } catch {
    message.error("复制失败，请手动选择邀请码复制");
  }
}
onMounted(() => {
  void loadBenefits();
});
</script>
<template>
  <div>
    <div class="page-heading">
      <div>
        <h1>我的账户</h1>
        <p>你的订阅、权益与小小惊喜，都在这里。</p>
      </div>
      <button class="button" @click="loadBenefits">
        <AppIcon name="refresh" :size="15" />刷新账户
      </button>
    </div>
    <section class="panel account-profile">
      <span class="profile-avatar">{{
        auth.session?.email[0]?.toUpperCase()
      }}</span>
      <div>
        <span class="eyebrow">YOUR SUFE ACCOUNT</span>
        <h2>{{ auth.session?.email }}</h2>
        <span class="badge"
          ><AppIcon name="shield" :size="12" />{{
            auth.subscribe?.plan_id ? "已订阅用户" : "欢迎加入 Sufe"
          }}</span
        >
      </div>
      <RouterLink v-if="features.enabled('purchase')" to="/plans" class="button soft"
        >管理订阅<AppIcon name="arrow" :size="15"
      /></RouterLink>
    </section>
    <div class="balance-grid">
      <section class="panel balance">
        <span class="balance-icon"><AppIcon name="wallet" :size="23" /></span>
        <div>
          <p>账户余额</p>
          <h2>{{ money(auth.userInfo?.balance || 0) }}</h2>
          <small>购买套餐时自动抵扣</small>
        </div>
      </section>
      <section class="panel balance">
        <span class="balance-icon green"
          ><AppIcon name="gift" :size="23"
        /></span>
        <div>
          <p>推广佣金</p>
          <h2>{{ money(auth.userInfo?.commission_balance || 0) }}</h2>
          <small>邀请好友获得的奖励</small>
        </div>
      </section>
    </div>
    <div class="benefit-grid">
      <section v-if="features.enabled('gift_card')" class="panel">
        <div class="section-title">
          <h2>兑换一份好心情</h2>
          <span class="badge">礼品卡</span>
        </div>
        <p class="benefit-description">
          输入礼品卡兑换码，先查看奖励，再确认兑换。
        </p>
        <form @submit.prevent="check">
          <label class="field-label" for="gift-code">礼品卡兑换码</label>
          <div class="redeem-form">
            <input
              id="gift-code"
              v-model="code"
              class="field"
              placeholder="输入你的专属兑换码"
              maxlength="128"
              autocomplete="off"
              :disabled="busy"
              @input="preview = null"
            /><button
              class="button primary"
              :disabled="busy || !code.trim()"
              type="submit"
            >
              {{ busy ? "处理中…" : "查看奖励" }}
            </button>
          </div>
        </form>
        <div v-if="error" class="error-note" role="alert">{{ error }}</div>
        <div v-if="preview" class="reward-preview">
          <h3>
            {{
              preview.can_redeem
                ? isMystery
                  ? "盲盒奖励预览"
                  : "这份礼物包含"
                : "暂时无法兑换"
            }}
          </h3>
          <p v-if="preview.reason">{{ preview.reason }}</p>
          <p v-if="isMystery">
            盲盒奖励随机发放；以下为预览，最终奖励以兑换结果为准。
          </p>
          <dl v-if="preview.reward_preview">
            <template v-for="(value, key) in visibleRewards" :key="key"
              ><dt>{{ rewardLabels[String(key)] || key }}</dt>
              <dd>{{ displayReward(String(key), value) }}</dd></template
            >
          </dl>
          <button
            v-if="preview.can_redeem"
            class="button primary"
            :disabled="busy || checkedCode !== code.trim()"
            @click="redeem"
          >
            确认兑换
          </button>
        </div>
        <div class="benefit-note">
          <AppIcon name="lock" :size="13" />兑换成功后，奖励会自动计入你的账户。
        </div>
        <h3 class="history-title">最近兑换</h3>
        <div v-if="historyError" class="muted">暂时无法加载兑换记录</div>
        <div v-else-if="!history?.data?.length" class="small-empty">
          还没有兑换记录，期待你的第一份礼物。
        </div>
        <div
          v-for="entry in history?.data?.slice(0, 5)"
          :key="entry.id"
          class="history-row"
        >
          <span
            ><AppIcon name="gift" :size="14" />{{ entry.template_name }}</span
          ><small>{{ historyDate(entry.created_at) }}</small>
        </div>
      </section>
      <section v-if="features.enabled('invite')" class="panel">
        <div class="section-title">
          <h2>好连接，值得分享</h2>
          <span class="badge">邀请好友</span>
        </div>
        <p class="benefit-description">
          将邀请码分享给朋友，奖励以你的账户规则为准。
        </p>
        <div class="invite-stats">
          <div>
            <strong>{{ stats[0] ?? 0 }}</strong
            ><span>已邀请好友</span>
          </div>
          <div>
            <strong>{{ money(stats[1] || 0) }}</strong
            ><span>有效佣金</span>
          </div>
          <div>
            <strong>{{ stats[3] ?? 0 }}<small>%</small></strong
            ><span>返佣比例</span>
          </div>
        </div>
        <div v-if="inviteError" class="error-note">{{ inviteError }}</div>
        <div
          v-for="invite in invites?.codes"
          :key="invite.id"
          class="invite-code"
        >
          <div>
            <small>你的专属邀请码</small><strong>{{ invite.code }}</strong>
          </div>
          <button
            class="icon-button"
            aria-label="复制邀请码"
            @click="copy(invite.code)"
          >
            <AppIcon name="copy" :size="16" />
          </button>
        </div>
        <button
          class="button soft create-invite"
          :disabled="inviteBusy"
          @click="createInvite"
        >
          <AppIcon name="plus" :size="15" />{{
            inviteBusy ? "生成中…" : "生成邀请码"
          }}
        </button>
      </section>
    </div>
    <section
      v-if="!features.enabled('gift_card') && !features.enabled('invite')"
      class="panel empty-state"
    >
      <AppIcon name="gift" :size="34" />
      <h3>更多权益，敬请期待</h3>
      <p>新的账户福利开放后，会自动出现在这里。</p>
    </section>
  </div>
</template>
<style scoped>
.account-profile {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 28px;
}
.profile-avatar {
  width: 68px;
  height: 68px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 30px;
  border-radius: 20px;
  background: var(--lavender);
  color: var(--primary);
}
.account-profile h2 {
  margin: 5px 0 10px;
  font-size: 20px;
  overflow-wrap: anywhere;
}
.account-profile .eyebrow {
  font-size: 9px;
}
.account-profile > .button {
  margin-left: auto;
  font-size: 11px;
}
.balance-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 20px;
  margin: 20px 0;
}
.balance {
  display: flex;
  align-items: center;
  gap: 20px;
}
.balance-icon {
  width: 50px;
  height: 50px;
  border-radius: 15px;
  background: var(--lavender);
  color: var(--primary);
  display: flex;
  align-items: center;
  justify-content: center;
}
.balance-icon.green {
  background: var(--green-soft);
  color: var(--green);
}
.balance p {
  font-size: 11px;
  color: var(--muted);
  margin: 0 0 6px;
}
.balance h2 {
  font-size: 28px;
  margin: 0;
  letter-spacing: -0.9px;
}
.balance small {
  font-size: 10px;
  color: var(--muted);
}
.benefit-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 20px;
  align-items: start;
}
.benefit-description {
  font-size: 11px;
  color: var(--muted);
  margin-bottom: 22px;
}
.redeem-form {
  display: flex;
  gap: 10px;
}
.redeem-form .field {
  min-width: 0;
  font-size: 12px;
}
.redeem-form .button {
  font-size: 11px;
  padding: 10px 12px;
}
.benefit-note {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--muted);
  font-size: 9px;
  margin: 14px 0 22px;
}
.history-title {
  font-size: 12px;
  border-top: 1px solid var(--border);
  padding-top: 20px;
}
.small-empty {
  font-size: 11px;
  color: var(--muted);
  padding: 10px 0;
}
.history-row {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  padding: 12px 0;
  border-top: 1px solid var(--border);
  font-size: 11px;
}
.history-row > span {
  display: flex;
  align-items: center;
  gap: 7px;
}
.history-row small {
  font-size: 9px;
  color: var(--muted);
}
.invite-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  padding: 18px 0;
  background: var(--surface-soft);
  border-radius: 12px;
  margin-bottom: 20px;
}
.invite-stats > div {
  text-align: center;
  border-right: 1px solid var(--border);
}
.invite-stats > div:last-child {
  border: 0;
}
.invite-stats strong {
  display: block;
  font-size: 20px;
  font-weight: 600;
}
.invite-stats span {
  display: block;
  font-size: 9px;
  color: var(--muted);
  margin-top: 4px;
}
.invite-stats small {
  font-size: 10px;
}
.invite-code {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--lavender);
  padding: 15px 18px;
  border-radius: 10px;
  margin-top: 10px;
}
.invite-code small {
  font-size: 9px;
  color: var(--muted);
  display: block;
  margin-bottom: 5px;
}
.invite-code strong {
  letter-spacing: 2px;
  font-size: 17px;
  font-weight: 550;
}
.invite-code .icon-button {
  background: transparent;
  border: 0;
  color: var(--primary);
}
.create-invite {
  width: 100%;
  margin-top: 15px;
  font-size: 11px;
}
.error-note,
.reward-preview {
  margin-top: 16px;
}
.reward-preview {
  background: var(--green-soft);
  padding: 18px;
  border-radius: 12px;
  font-size: 12px;
}
.reward-preview dl {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
.reward-preview dd {
  margin: 0;
  text-align: right;
  overflow-wrap: anywhere;
}
.reward-preview .button {
  margin-top: 10px;
  font-size: 11px;
}
@media (max-width: 1050px) {
  .benefit-grid {
    grid-template-columns: 1fr;
  }
  .account-profile {
    padding: 22px;
  }
}
@media (max-width: 560px) {
  .account-profile {
    gap: 14px;
    flex-wrap: wrap;
  }
  .account-profile h2 {
    font-size: 16px;
  }
  .profile-avatar {
    width: 48px;
    height: 48px;
    font-size: 24px;
    border-radius: 14px;
  }
  .account-profile > .button {
    margin-left: 62px;
  }
  .balance-grid {
    gap: 12px;
  }
  .balance {
    padding: 18px 14px;
    gap: 10px;
  }
  .balance-icon {
    width: 33px;
    height: 38px;
    border-radius: 10px;
    flex-shrink: 0;
  }
  .balance h2 {
    font-size: 22px;
  }
  .balance small {
    font-size: 8px;
  }
  .balance-icon svg {
    width: 18px;
  }
  .redeem-form {
    gap: 7px;
  }
}
</style>
