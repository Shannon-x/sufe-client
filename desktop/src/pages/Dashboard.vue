<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useMessage } from "naive-ui";
import { useI18n } from "vue-i18n";
import AppIcon from "@/components/AppIcon.vue";
import NetworkMap from "@/components/NetworkMap.vue";
import TrafficChart from "@/components/TrafficChart.vue";
import { useAuthStore } from "@/stores/auth";
import { useConnectionStore } from "@/stores/connection";
import { useNodesStore } from "@/stores/nodes";
import { useNodePreferences } from "@/stores/nodePreferences";
import { usePlanStore } from "@/stores/plan";
import { useFeaturesStore } from "@/stores/features";
import { api } from "@/api";
import { bytes, duration, plainText } from "@/utils/format";
import { formatError } from "@/utils/error";
import type { Notice } from "@/types";
const auth = useAuthStore(),
  conn = useConnectionStore(),
  nodes = useNodesStore(),
  prefs = useNodePreferences(),
  plans = usePlanStore(),
  features = useFeaturesStore();
const router = useRouter(),
  message = useMessage(),
  { t } = useI18n();
const loading = ref(false),
  actionBusy = ref(false),
  error = ref(""),
  notices = ref<Notice[]>([]),
  now = ref(Date.now()),
  noticesError = ref(false);
let timer: number | undefined;
const nickname = computed(() => auth.session?.email.split("@")[0] || "朋友");
const greeting = computed(() =>
  new Date(now.value).getHours() < 12
    ? "上午好"
    : new Date(now.value).getHours() < 18
      ? "下午好"
      : "晚上好",
);
const used = computed(
  () => (auth.subscribe?.u || 0) + (auth.subscribe?.d || 0),
);
const total = computed(() => auth.subscribe?.transfer_enable || 0);
const remaining = computed(() => Math.max(0, total.value - used.value));
const percent = computed(() =>
  total.value ? Math.min(100, (used.value / total.value) * 100) : 0,
);
const expired = computed(
  () =>
    auth.subscribe?.expired_at != null &&
    auth.subscribe.expired_at * 1000 < now.value,
);
const exhausted = computed(() => total.value > 0 && used.value >= total.value);
const canConnect = computed(
  () =>
    !!auth.subscribe?.plan_id &&
    !auth.userInfo?.banned &&
    !expired.value &&
    !exhausted.value,
);
const selected = computed(
  () =>
    conn.effectiveProxy ||
    prefs.selected ||
    nodes.sidebarNodes[0]?.name ||
    "自动选择可用线路",
);
const days = computed(() =>
  auth.subscribe?.expired_at
    ? Math.max(
        0,
        Math.ceil((auth.subscribe.expired_at * 1000 - now.value) / 86400000),
      )
    : null,
);
const uptime = computed(() =>
  conn.state.kind === "connected"
    ? duration((now.value - new Date(conn.state.since).getTime()) / 1000)
    : "00:00:00",
);
const stateLabel = computed(() =>
  conn.isConnected
    ? "连接已建立"
    : conn.isBusy
      ? "正在为你连接"
      : conn.state.kind === "error"
        ? "连接遇到问题"
        : "准备好，出发吧",
);
const stateDescription = computed(() =>
  conn.isConnected
    ? "享受更自由、更顺畅的网络体验。"
    : conn.isBusy
      ? "正在准备线路，请稍候…"
      : expired.value
        ? "订阅已到期，续费后即可重新连接。"
        : exhausted.value
          ? "本期流量已用尽，前往订阅页查看可用方案。"
          : !auth.subscribe?.plan_id
            ? "选择一份订阅，开启你的全新旅程。"
            : "一键连接，让世界近在眼前。",
);
const recommendations = computed(() =>
  [...nodes.sidebarNodes]
    .sort(
      (a, b) =>
        Number(prefs.favorites.includes(b.name)) -
        Number(prefs.favorites.includes(a.name)),
    )
    .slice(0, 3),
);
async function refresh() {
  loading.value = true;
  error.value = "";
  const r = await Promise.allSettled([
    auth.refreshUser(),
    auth.refreshSubscribe(),
    nodes.refresh(),
    plans.ensure(),
  ]);
  if (r[1].status === "rejected")
    error.value = "暂时无法同步订阅，请检查网络后重试。";
  if (features.enabled("notice"))
    try {
      notices.value = (await api.fetchNotices()).slice(0, 2);
      noticesError.value = false;
    } catch {
      noticesError.value = true;
    }
  loading.value = false;
}
async function toggleConnection() {
  if (actionBusy.value) return;
  if (!conn.isConnected && !canConnect.value) {
    await router.push(features.enabled('purchase') ? '/plans' : '/support');
    return;
  }
  actionBusy.value = true;
  error.value = "";
  try {
    if (conn.isConnected) {
      await conn.disconnect();
      localStorage.setItem("xboard.manualDisconnect", "1");
    } else {
      await conn.connect();
      localStorage.removeItem("xboard.manualDisconnect");
      const group = conn.primaryGroup;
      if (prefs.selected && group?.all.includes(prefs.selected))
        await conn.selectProxy(group.name, prefs.selected);
    }
  } catch (e) {
    error.value = formatError(e, t);
  } finally {
    actionBusy.value = false;
  }
}
async function selectNode(name: string) {
  try {
    if (conn.isConnected && conn.primaryGroup)
      await conn.selectProxy(conn.primaryGroup.name, name);
    prefs.select(name);
    message.success(conn.isConnected ? "已切换线路" : "将在连接时使用此线路");
  } catch (e) {
    message.error(formatError(e, t));
  }
}
onMounted(async () => {
  timer = window.setInterval(() => (now.value = Date.now()), 1000);
  await refresh();
  const autoLogin =
    sessionStorage.getItem("xboard.autoConnectAfterLogin") === "1";
  sessionStorage.removeItem("xboard.autoConnectAfterLogin");
  const autoStart =
    localStorage.getItem("xboard.autoConnectOnStart") === "1" &&
    localStorage.getItem("xboard.manualDisconnect") !== "1";
  if (
    (autoLogin || autoStart) &&
    canConnect.value &&
    !conn.isConnected &&
    !conn.isBusy
  )
    await toggleConnection();
});
onUnmounted(() => clearInterval(timer));
</script>
<template>
  <div class="dashboard">
    <div class="page-heading">
      <div>
        <h1>
          {{ greeting }}，{{ nickname }}
          <AppIcon class="greeting-star" name="sun" :size="26" />
        </h1>
        <p>世界很大，你的连接可以很简单。</p>
      </div>
      <button class="button" :disabled="loading" @click="refresh">
        <AppIcon name="refresh" :size="15" :class="{ spin: loading }" />刷新状态
      </button>
    </div>
    <div v-if="error" class="error-note dashboard-error" role="alert">
      {{ error }} <RouterLink to="/logs">查看运行日志 →</RouterLink>
    </div>
    <section class="connection-hero" :class="{ connected: conn.isConnected }">
      <div class="hero-info">
        <span class="hero-kicker"
          ><span class="status-dot" />{{
            conn.isConnected
              ? "CONNECTED"
              : conn.isBusy
                ? "CONNECTING"
                : "YOUR PRIVATE CONNECTION"
          }}</span
        >
        <h2>{{ stateLabel }}</h2>
        <p>{{ stateDescription }}</p>
        <div class="power-row">
          <button
            class="power-button"
            :disabled="actionBusy || conn.isBusy"
            :aria-label="
              conn.isConnected
                ? '断开连接'
                : canConnect
                  ? '立即连接'
                  : '选择套餐'
            "
            @click="toggleConnection"
          >
            <AppIcon
              :name="conn.isBusy ? 'refresh' : 'power'"
              :size="27"
              :class="{ spin: conn.isBusy }"
            />
          </button>
          <div>
            <strong>{{
              conn.isConnected
                ? "点击断开连接"
                : conn.isBusy
                  ? "连接中…"
                  : canConnect
                    ? "点击立即连接"
                    : "选择适合你的套餐"
            }}</strong
            ><span>{{
              conn.isConnected ? "已连接 " + uptime : "安全连接，从这里开始"
            }}</span>
          </div>
        </div>
        <button class="selected-node" @click="router.push('/nodes')">
          <AppIcon name="globe" :size="15" /><span>{{ selected }}</span
          ><AppIcon name="chevron" :size="13" />
        </button>
      </div>
      <div class="hero-map">
        <div class="hero-map-heading">
          <span>GLOBAL NETWORK</span
          ><span class="hero-chip"
            ><AppIcon name="shield" :size="12" />{{
              conn.currentMode === "tun" ? "TUN 网络接管" : "系统代理"
            }}</span
          >
        </div>
        <NetworkMap :selected="selected" :connected="conn.isConnected" />
        <div class="map-footer">
          <span><i class="status-dot" />{{ nodes.raw.length }} 条精选线路</span
          ><span>{{ nodes.countries.length }} 个地区 · 随心切换</span>
        </div>
      </div>
    </section>
    <div v-if="conn.wasDowngraded" class="info-note dashboard-error">
      本次连接使用系统代理。TUN 权限未就绪，可在设置中检查辅助服务。
    </div>
    <section class="stat-grid" aria-label="连接概况">
      <div class="stat-card">
        <span class="stat-icon purple"><AppIcon name="down" :size="19" /></span>
        <div>
          <span class="stat-label">下载速度</span
          ><strong>{{ bytes(conn.traffic.down) }}<small>/s</small></strong>
        </div>
        <span class="stat-trend">实时</span>
      </div>
      <div class="stat-card">
        <span class="stat-icon blue"><AppIcon name="up" :size="19" /></span>
        <div>
          <span class="stat-label">上传速度</span
          ><strong>{{ bytes(conn.traffic.up) }}<small>/s</small></strong>
        </div>
        <span class="stat-trend">实时</span>
      </div>
      <div class="stat-card">
        <span class="stat-icon orange"
          ><AppIcon name="activity" :size="19"
        /></span>
        <div>
          <span class="stat-label">本期剩余流量</span
          ><strong
            >{{ bytes(remaining)
            }}<small>/ {{ bytes(total, 0) }}</small></strong
          >
        </div>
      </div>
      <div class="stat-card">
        <span class="stat-icon green"><AppIcon name="clock" :size="19" /></span>
        <div>
          <span class="stat-label">订阅剩余时间</span
          ><strong
            >{{ auth.subscribe?.plan_id ? (days ?? "长期") : "—"
            }}<small v-if="days !== null">天</small></strong
          >
        </div>
        <span v-if="days !== null && days <= 7" class="stat-trend warning"
          >即将到期</span
        >
      </div>
    </section>
    <div class="dashboard-middle">
      <section class="panel traffic-panel">
        <div class="section-title">
          <h2>流量趋势</h2>
          <span class="chart-period"><i class="status-dot" />最近 2 分钟</span>
        </div>
        <div class="chart-summary">
          <strong>{{
            bytes(conn.traffic.down_total + conn.traffic.up_total)
          }}</strong
          ><span>本次连接累计流量</span>
        </div>
        <div class="chart-grid">
          <TrafficChart :history="conn.trafficHistory" :height="95" />
          <div v-if="conn.trafficHistory.length < 2" class="chart-placeholder">
            连接后，流量趋势将在这里呈现
          </div>
        </div>
        <div class="chart-axis">
          <span>120 秒前</span><span>90 秒</span><span>60 秒</span
          ><span>30 秒</span><span>现在</span>
        </div>
      </section>
      <section class="panel plan-panel">
        <div class="section-title">
          <h2>我的订阅</h2>
          <span class="badge">{{
            expired ? "已到期" : auth.subscribe?.plan_id ? "使用中" : "待开通"
          }}</span>
        </div>
        <div class="plan-name">
          <span class="plan-gem"><AppIcon name="lightning" :size="26" /></span>
          <div>
            <h3>
              {{ plans.nameFor(auth.subscribe?.plan_id) || "开启你的连接之旅" }}
            </h3>
            <p>
              {{
                days === null
                  ? "按需选择，自由连接"
                  : days > 0
                    ? new Date(
                        (auth.subscribe?.expired_at || 0) * 1000,
                      ).toLocaleDateString("zh-CN") + " 到期"
                    : "续费即可继续使用"
              }}
            </p>
          </div>
        </div>
        <div class="quota-label">
          <span>本期流量</span
          ><strong
            >{{ bytes(used) }} <small>/ {{ bytes(total, 0) }}</small></strong
          >
        </div>
        <div class="quota-track"><i :style="{ width: percent + '%' }" /></div>
        <div class="quota-hint">
          {{
            auth.subscribe?.reset_day
              ? auth.subscribe.reset_day + " 天后重置流量"
              : "流量与有效期以账户为准"
          }}
        </div>
        <RouterLink
          v-if="features.enabled('purchase')"
          to="/plans"
          class="button soft plan-cta"
          >{{ auth.subscribe?.plan_id ? "续费 / 升级套餐" : "浏览订阅套餐"
          }}<AppIcon name="arrow" :size="16"
        /></RouterLink>
      </section>
    </div>
    <div class="dashboard-bottom">
      <section class="panel recommended-panel">
        <div class="section-title">
          <h2>
            精选线路 <span>{{ nodes.raw.length }}</span>
          </h2>
          <RouterLink to="/nodes" class="text-button"
            >查看全部<AppIcon name="chevron" :size="13"
          /></RouterLink>
        </div>
        <div v-if="!recommendations.length" class="empty-state compact">
          <AppIcon name="globe" :size="27" />
          <p>
            {{
              nodes.lastError
                ? "线路暂时无法加载，请刷新重试"
                : "订阅后即可查看你的专属线路"
            }}
          </p>
        </div>
        <button
          v-for="node in recommendations"
          :key="node.name"
          class="recommended-node"
          :class="{ selected: node.name === selected }"
          @click="selectNode(node.name)"
        >
          <span class="country-flag">{{ node.geo?.flag || "◎" }}</span>
          <div>
            <strong>{{ node.name }}</strong
            ><small
              >{{ node.kind.toUpperCase() }} ·
              {{ node.geo?.country || "全球线路" }}</small
            >
          </div>
          <span v-if="prefs.latency[node.name]" class="node-delay"
            ><AppIcon name="signal" :size="12" />{{
              prefs.latency[node.name]
            }}
            ms</span
          ><span v-else-if="node.name === selected" class="node-selected"
            >当前选择</span
          ><AppIcon v-else name="chevron" :size="14" />
        </button>
      </section>
      <section v-if="features.enabled('notice')" class="panel notice-panel">
        <div class="section-title">
          <h2>最新公告</h2>
          <RouterLink to="/notices" class="text-button"
            >全部公告<AppIcon name="chevron" :size="13"
          /></RouterLink>
        </div>
        <div v-if="!notices.length" class="empty-state compact">
          <AppIcon name="bell" :size="27" />
          <p>{{ noticesError ? "公告暂时无法加载" : "暂时没有新公告" }}</p>
        </div>
        <RouterLink
          v-for="notice in notices"
          :key="notice.id"
          to="/notices"
          class="notice-preview"
          ><span class="notice-icon"><AppIcon name="bell" :size="16" /></span>
          <div>
            <h3>{{ notice.title }}</h3>
            <p>{{ plainText(notice.content) }}</p>
            <small
              >{{
                notice.created_at
                  ? new Date(notice.created_at * 1000).toLocaleDateString(
                      "zh-CN",
                    )
                  : "最新消息"
              }}<span v-if="notice.tags?.length">{{
                notice.tags[0]
              }}</span></small
            >
          </div></RouterLink
        >
      </section>
    </div>
  </div>
</template>
<style scoped>
.dashboard-middle > *,
.dashboard-bottom > * {
  min-width: 0;
}
.greeting-star {
  font-size: 27px;
  color: #c5b3e9;
  display: inline-block;
  vertical-align: middle;
  margin-left: 5px;
}
.dashboard-error {
  margin-bottom: 18px;
}
.dashboard-error a {
  float: right;
}
.connection-hero {
  display: grid;
  grid-template-columns: 1fr 1.2fr;
  padding: 30px 32px 24px;
  border-radius: 20px;
  background: linear-gradient(112deg, #eeebfd, #f3f0fc 58%, #eeebf9);
  border: 1px solid #e5dff6;
  gap: 20px;
  min-height: 281px;
}
.hero-kicker {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 9px;
  letter-spacing: 1.6px;
  color: #9886c8;
  font-weight: 600;
}
.hero-kicker .status-dot {
  width: 5px;
  height: 5px;
}
.hero-info h2 {
  font-size: 27px;
  letter-spacing: -0.5px;
  margin: 17px 0 8px;
  color: #4a426e;
  font-weight: 650;
}
.hero-info > p {
  font-size: 12px;
  color: #9188ab;
  max-width: 320px;
  margin-bottom: 22px;
}
.power-row {
  display: flex;
  align-items: center;
  gap: 15px;
}
.power-button {
  background: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #8d73d9;
  width: 61px;
  height: 61px;
  border-radius: 50%;
  border: 5px solid #e5defa;
  box-shadow: 0 4px 12px #7c64b010;
  transition:
    transform 0.2s,
    box-shadow 0.2s;
}
.power-button:hover {
  transform: scale(1.04);
  box-shadow: 0 5px 20px #7c64b024;
}
.connected .power-button {
  background: #8871d8;
  color: #fff;
  border-color: #e2d8f9;
}
.power-row strong {
  display: block;
  font-size: 13px;
  font-weight: 550;
  color: #665488;
}
.power-row span {
  font-size: 10px;
  color: #a195b8;
  display: block;
  margin-top: 3px;
  font-variant-numeric: tabular-nums;
}
.selected-node {
  border: 0;
  background: #ffffff85;
  padding: 8px 11px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: #7c6b9c;
  margin-top: 20px;
  max-width: 100%;
}
.selected-node > span {
  max-width: 245px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hero-map {
  min-width: 0;
  display: flex;
  flex-direction: column;
  position: relative;
}
.hero-map-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: #b4a6cc;
  font-size: 8px;
  letter-spacing: 1.4px;
}
.hero-chip {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 9px;
  letter-spacing: 0;
  border: 1px solid #ddd5eb;
  padding: 4px 7px;
  border-radius: 5px;
  color: #9b8bb6;
}
.hero-map :deep(.network-map) {
  min-height: 190px;
  flex: 1;
}
.map-footer {
  display: flex;
  justify-content: space-between;
  color: #a499b8;
  font-size: 9px;
  margin-top: 5px;
}
.map-footer > span:first-child {
  display: flex;
  align-items: center;
  gap: 6px;
}
.map-footer .status-dot {
  width: 4px;
  height: 4px;
  color: #9883c9;
}
.stat-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 15px;
  margin: 20px 0;
}
.stat-card {
  display: flex;
  align-items: center;
  padding: 19px 16px;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--surface);
  gap: 12px;
  position: relative;
  min-width: 0;
}
.stat-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 37px;
  height: 37px;
  border-radius: 11px;
  flex-shrink: 0;
}
.purple {
  color: #a08ce0;
  background: #f1edfc;
}
.blue {
  color: #7b9bdb;
  background: #edf3ff;
}
.orange {
  color: #d5a674;
  background: #fcf2e8;
}
.green {
  color: #64b59e;
  background: #eaf7f0;
}
.stat-label {
  display: block;
  font-size: 10px;
  color: var(--muted);
  margin-bottom: 5px;
}
.stat-card strong {
  font-size: 19px;
  font-weight: 650;
  letter-spacing: -0.6px;
  white-space: nowrap;
}
.stat-card small {
  font-size: 9px;
  color: var(--muted);
  font-weight: 400;
  margin-left: 5px;
  letter-spacing: 0;
}
.stat-trend {
  position: absolute;
  right: 11px;
  top: 12px;
  color: #b4b3c1;
  font-size: 8px;
}
.stat-trend.warning {
  color: #d39855;
}
.dashboard-middle {
  display: grid;
  grid-template-columns: 1.45fr 1fr;
  gap: 20px;
}
.chart-period {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
  color: var(--muted);
}
.chart-period i {
  width: 4px;
  height: 4px;
  background: #9c89d9;
}
.chart-summary {
  display: flex;
  gap: 12px;
  align-items: baseline;
  margin-bottom: 8px;
}
.chart-summary strong {
  font-size: 24px;
  font-weight: 600;
  letter-spacing: -0.8px;
}
.chart-summary > span {
  font-size: 10px;
  color: var(--muted);
}
.chart-grid {
  position: relative;
  background: repeating-linear-gradient(
    to top,
    transparent,
    transparent 32px,
    var(--border) 33px,
    var(--border) 34px
  );
  height: 135px;
}
.chart-grid :deep(.traffic-chart) {
  padding-top: 3px;
  gap: 8px;
}
.chart-grid :deep(.legend) {
  font-size: 9px;
  justify-content: flex-end;
  color: var(--muted);
}
.chart-grid :deep(.canvas) {
  height: 95px;
}
.chart-placeholder {
  position: absolute;
  inset: 45px 0 0;
  display: flex;
  justify-content: center;
  align-items: center;
  font-size: 10px;
  color: var(--muted);
}
.chart-axis {
  display: flex;
  justify-content: space-between;
  font-size: 8px;
  color: #b3b4c2;
  margin-top: 11px;
}
.plan-name {
  display: flex;
  gap: 12px;
  align-items: center;
  margin-bottom: 24px;
}
.plan-gem {
  width: 47px;
  height: 47px;
  background: var(--lavender);
  color: var(--primary);
  border-radius: 13px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.plan-name h3 {
  font-size: 17px;
  letter-spacing: -0.4px;
  margin: 0 0 3px;
}
.plan-name p {
  font-size: 10px;
  color: var(--muted);
  margin: 0;
}
.quota-label {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 10px;
  color: var(--muted);
  margin-bottom: 8px;
}
.quota-label strong {
  font-size: 11px;
  color: var(--text);
  font-weight: 550;
}
.quota-label small {
  font-size: 10px;
  color: var(--muted);
  font-weight: 400;
}
.quota-track {
  height: 6px;
  background: var(--lavender);
  border-radius: 6px;
  overflow: hidden;
}
.quota-track > i {
  display: block;
  height: 100%;
  border-radius: 6px;
  background: #a491ea;
}
.quota-hint {
  font-size: 9px;
  color: var(--muted);
  margin-top: 9px;
}
.plan-cta {
  display: flex;
  margin-top: 20px;
  padding: 10px;
  font-size: 11px;
  justify-content: space-between;
}
.dashboard-bottom {
  display: grid;
  grid-template-columns: 1.1fr 1fr;
  gap: 20px;
  margin-top: 20px;
}
.recommended-panel .section-title,
.notice-panel .section-title {
  margin-bottom: 14px;
}
.section-title h2 > span {
  font-size: 10px;
  display: inline-flex;
  vertical-align: middle;
  background: var(--surface-soft);
  color: var(--muted);
  border: 1px solid var(--border);
  padding: 0 6px;
  border-radius: 4px;
  margin-left: 6px;
  font-weight: 400;
}
.recommended-node {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 13px 4px;
  background: none;
  border: 0;
  border-bottom: 1px solid var(--border);
  text-align: left;
}
.recommended-node:last-child {
  border-bottom: 0;
  padding-bottom: 2px;
}
.recommended-node:hover {
  background: var(--surface-soft);
}
.country-flag {
  width: 32px;
  height: 32px;
  background: var(--surface-soft);
  border: 1px solid var(--border);
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  overflow: hidden;
  flex-shrink: 0;
}
.recommended-node > div {
  min-width: 0;
  flex: 1;
}
.recommended-node strong {
  font-size: 11px;
  font-weight: 550;
  display: block;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.recommended-node small {
  font-size: 8px;
  color: var(--muted);
  display: block;
  margin-top: 3px;
}
.recommended-node > svg {
  color: var(--muted);
}
.node-delay {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 9px;
  color: var(--green);
}
.node-selected {
  font-size: 9px;
  color: var(--primary);
  background: var(--lavender);
  border-radius: 4px;
  padding: 3px 6px;
}
.notice-preview {
  display: flex;
  gap: 12px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
  color: var(--text);
}
.notice-preview:last-child {
  border: 0;
}
.notice-icon {
  height: 31px;
  width: 31px;
  border-radius: 9px;
  background: var(--surface-soft);
  color: #aaa2c3;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.notice-preview > div {
  min-width: 0;
}
.notice-preview h3 {
  font-size: 11px;
  font-weight: 550;
  margin: 0 0 4px;
}
.notice-preview p {
  font-size: 10px;
  color: var(--muted);
  margin: 0 0 7px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.notice-preview small {
  font-size: 8px;
  color: #adafbe;
  display: flex;
  gap: 10px;
  align-items: center;
}
.notice-preview small > span {
  background: var(--surface-soft);
  padding: 1px 4px;
  border-radius: 3px;
}
.empty-state.compact {
  padding: 25px 8px;
}
.empty-state.compact p {
  font-size: 11px;
  margin: 10px 0 0;
}
:global([data-theme="dark"]) .connection-hero {
  background: linear-gradient(110deg, #302943, #242239);
  border-color: #3a324d;
}
:global([data-theme="dark"]) .hero-info h2 {
  color: #d7cdef;
}
:global([data-theme="dark"]) .hero-info > p {
  color: #9f93b7;
}
:global([data-theme="dark"]) .selected-node {
  background: #302c43;
}
@media (min-width: 1450px) {
  .connection-hero {
    min-height: 310px;
    padding: 34px 40px;
  }
  .hero-info h2 {
    font-size: 31px;
  }
  .hero-map :deep(.network-map) {
    min-height: 210px;
  }
  .stat-card {
    padding: 23px 20px;
  }
  .stat-card strong {
    font-size: 23px;
  }
  .chart-grid {
    height: 154px;
  }
  .chart-grid :deep(.canvas) {
    height: 114px;
  }
}
@media (max-width: 1100px) {
  .connection-hero {
    padding: 26px;
    grid-template-columns: 1fr 1fr;
  }
  .hero-info h2 {
    font-size: 23px;
  }
  .stat-grid {
    gap: 10px;
  }
  .stat-card {
    padding: 16px 12px;
    gap: 8px;
  }
  .stat-icon {
    width: 29px;
    height: 32px;
  }
  .stat-card strong {
    font-size: 16px;
  }
  .stat-trend {
    display: none;
  }
  .stat-card small {
    font-size: 8px;
  }
  .dashboard-middle {
    grid-template-columns: 1.2fr 1fr;
    gap: 15px;
  }
  .dashboard-bottom {
    gap: 15px;
  }
  .section-title h2 {
    font-size: 15px;
  }
  .plan-panel {
    padding: 21px;
  }
  .stat-label {
    font-size: 9px;
  }
}
@media (max-width: 900px) {
  .stat-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .stat-card strong {
    font-size: 21px;
  }
  .connection-hero {
    padding: 24px;
  }
  .hero-map-heading > span:first-child {
    display: none;
  }
  .hero-map-heading {
    justify-content: flex-end;
  }
  .hero-info h2 {
    font-size: 22px;
  }
  .hero-kicker {
    font-size: 7px;
  }
  .power-row strong {
    font-size: 11px;
  }
  .power-button {
    width: 54px;
    height: 54px;
  }
  .dashboard-middle {
    grid-template-columns: 1fr 1fr;
  }
  .dashboard-bottom {
    grid-template-columns: 1fr;
  }
  .map-footer {
    font-size: 8px;
  }
}
@media (max-width: 560px) {
  .connection-hero {
    grid-template-columns: 1fr;
    min-height: 0;
    position: relative;
    overflow: hidden;
    padding: 26px;
  }
  .hero-info {
    position: relative;
    z-index: 2;
  }
  .hero-info h2 {
    font-size: 26px;
  }
  .hero-info > p {
    font-size: 11px;
    max-width: 240px;
  }
  .hero-map {
    position: absolute;
    inset: 72px -115px 25px 100px;
    opacity: 0.35;
    pointer-events: none;
  }
  .hero-map-heading,
  .map-footer {
    display: none;
  }
  .hero-map :deep(.map-caption) {
    display: none;
  }
  .hero-kicker {
    font-size: 8px;
  }
  .power-row strong {
    font-size: 12px;
  }
  .selected-node {
    margin-top: 22px;
  }
  .dashboard-middle {
    grid-template-columns: 1fr;
  }
  .dashboard-bottom {
    grid-template-columns: 1fr;
  }
  .stat-card {
    padding: 17px 12px;
  }
  .stat-card strong {
    font-size: 19px;
  }
  .stat-label {
    font-size: 9px;
  }
  .stat-grid {
    margin: 16px 0;
    gap: 10px;
  }
  .greeting-star {
    font-size: 21px;
  }
  .page-heading .button {
    font-size: 10px;
    padding: 9px;
  }
  .page-heading .button svg {
    width: 12px;
  }
  .dashboard-bottom {
    margin-top: 16px;
  }
  .chart-grid {
    height: 145px;
  }
}
</style>
