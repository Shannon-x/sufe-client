<script setup lang="ts">
import { onMounted, ref } from "vue";
import { NSwitch, useMessage, useDialog } from "naive-ui";
import { useI18n } from "vue-i18n";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import AppIcon from "@/components/AppIcon.vue";
import { useThemeStore } from "@/stores/theme";
import { useConnectionStore } from "@/stores/connection";
import { useFeaturesStore } from "@/stores/features";
import { api } from "@/api";
import { isDesktop } from "@/platform";
import { formatError } from "@/utils/error";
import type { TunnelMode, KernelHealth, HelperStatus } from "@/types";
const theme = useThemeStore(),
  conn = useConnectionStore(),
  features = useFeaturesStore(),
  message = useMessage(),
  dialog = useDialog(),
  { t } = useI18n();
const autoConnect = ref(
    localStorage.getItem("xboard.autoConnectOnStart") === "1",
  ),
  guard = ref(true),
  notifications = ref(false),
  loading = ref(false),
  serviceBusy = ref(false),
  updateBusy = ref(false),
  appVersion = ref(""),
  kernelVersion = ref(""),
  health = ref<KernelHealth | null>(null),
  helper = ref<HelperStatus | null>(null),
  error = ref("");
async function load() {
  loading.value = true;
  error.value = "";
  const result = await Promise.allSettled([
    api.appVersion(),
    api.kernelVersion(),
    api.kernelHealth(),
    api.helperStatus(),
    api.proxyGuardEnabled(),
  ]);
  if (result[0].status === "fulfilled") appVersion.value = result[0].value;
  if (result[1].status === "fulfilled")
    kernelVersion.value = result[1].value.version || result[1].value.raw;
  if (result[2].status === "fulfilled") health.value = result[2].value;
  if (result[3].status === "fulfilled") helper.value = result[3].value;
  if (result[4].status === "fulfilled") guard.value = result[4].value;
  if (result.some((r) => r.status === "rejected"))
    error.value = "部分状态暂时无法读取，请刷新或查看运行日志。";
  if (isDesktop)
    try {
      notifications.value = await isPermissionGranted();
    } catch {}
  loading.value = false;
}
function saveAuto(value: boolean) {
  try {
    localStorage.setItem("xboard.autoConnectOnStart", value ? "1" : "0");
    if (value) localStorage.removeItem("xboard.manualDisconnect");
    autoConnect.value = value;
    message.success("启动偏好已保存");
  } catch {
    message.error("偏好保存失败，请重试");
  }
}
async function changeMode(mode: TunnelMode) {
  if (loading.value || conn.isBusy) return;
  loading.value = true;
  try {
    await conn.setMode(mode);
    message.success("连接模式已更新");
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}
async function changeGuard(value: boolean) {
  if (loading.value) return;
  loading.value = true;
  try {
    await api.setProxyGuardEnabled(value);
    guard.value = value;
    localStorage.setItem("sufe.proxyGuard", value ? "1" : "0");
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    loading.value = false;
  }
}
async function enableNotifications() {
  if (!isDesktop) {
    message.info("请在桌面客户端中设置通知");
    return;
  }
  try {
    notifications.value = (await requestPermission()) === "granted";
    message.info(
      notifications.value
        ? "已允许通知，新客服消息会及时提醒你"
        : "通知未启用，你仍可在应用内查看消息",
    );
  } catch (e) {
    message.error(formatError(e, t));
  }
}
async function serviceAction(remove = false) {
  if (serviceBusy.value) return;
  serviceBusy.value = true;
  try {
    if (remove) await api.helperUninstall();
    else await api.helperInstall();
    await load();
    message.success(remove ? "辅助服务已卸载" : "辅助服务已准备就绪");
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    serviceBusy.value = false;
  }
}
async function update() {
  if (!isDesktop) {
    message.info("请在桌面客户端中检查更新");
    return;
  }
  updateBusy.value = true;
  try {
    const available = await check();
    if (!available) {
      message.success("当前已经是最新版本");
      return;
    }
    dialog.info({
      title: "发现新版本 " + available.version,
      content: available.body || "安装更新后将重新启动客户端。",
      positiveText: "安装并重启",
      negativeText: "稍后",
      onPositiveClick: async () => {
        try {
          if (conn.isConnected) await conn.disconnect();
          await available.downloadAndInstall();
          await relaunch();
        } catch (e) {
          message.error(formatError(e, t));
          return false;
        }
      },
    });
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    updateBusy.value = false;
  }
}
onMounted(load);
</script>
<template>
  <div>
    <div class="page-heading">
      <div>
        <h1>把 Sufe 调成你喜欢的样子</h1>
        <p>简单设置，让每一次连接更顺手。</p>
      </div>
      <button class="button" :disabled="loading" @click="load">
        <AppIcon name="refresh" :size="15" :class="{ spin: loading }" />刷新状态
      </button>
    </div>
    <div v-if="error" class="error-note">{{ error }}</div>
    <div class="settings-grid">
      <div class="settings-main">
        <section class="panel">
          <h2 class="settings-title">
            <AppIcon name="sun" :size="18" />外观与启动
          </h2>
          <div class="setting-row">
            <div>
              <strong>界面主题</strong>
              <p>为白天和夜晚，选择舒适的色彩</p>
            </div>
            <div class="segmented">
              <button
                :class="{ active: !theme.dark }"
                @click="theme.dark = false"
              >
                <AppIcon name="sun" :size="14" />浅色</button
              ><button
                :class="{ active: theme.dark }"
                @click="theme.dark = true"
              >
                <AppIcon name="moon" :size="14" />深色
              </button>
            </div>
          </div>
          <div class="setting-row">
            <div>
              <strong>启动时自动连接</strong>
              <p>打开应用后，在订阅有效时自动连接；手动断开后暂停</p>
            </div>
            <NSwitch
              :value="autoConnect"
              aria-label="启动时自动连接"
              @update:value="saveAuto"
            />
          </div>
          <div class="setting-row">
            <div>
              <strong>客服消息通知</strong>
              <p>应用内始终展示未读消息，系统通知需授权</p>
            </div>
            <span v-if="notifications" class="badge enabled"
              ><AppIcon name="check" :size="12" />已允许</span
            ><button v-else class="button small" @click="enableNotifications">
              开启通知
            </button>
          </div>
        </section>
        <section class="panel">
          <h2 class="settings-title">
            <AppIcon name="shield" :size="18" />网络连接
          </h2>
          <div class="setting-row">
            <div>
              <strong>连接模式</strong>
              <p>
                {{
                  health?.tun_supported
                    ? "默认使用 TUN 接管系统网络；首次连接请允许系统授权"
                    : "正在检查 TUN 支持状态，请刷新后重试"
                }}
              </p>
            </div>
            <select
              :value="conn.mode"
              class="field mode-select"
              :disabled="loading || conn.isBusy"
              aria-label="连接模式"
              @change="
                changeMode(
                  ($event.target as HTMLSelectElement).value as TunnelMode,
                )
              "
            >
              <option value="system_proxy">系统代理</option>
              <option v-if="health?.tun_supported" value="tun">TUN 模式</option>
            </select>
          </div>
          <div class="setting-row">
            <div>
              <strong>系统代理守卫</strong>
              <p>连接期间定时检查并恢复系统代理设置</p>
            </div>
            <NSwitch
              :value="guard"
              :disabled="loading"
              aria-label="系统代理守卫"
              @update:value="changeGuard"
            />
          </div>
          <div v-if="helper?.supported" class="setting-row helper-row">
            <div>
              <strong
                >TUN 辅助服务
                <span class="inline-status" :class="{ ok: helper.reachable }">{{
                  helper.reachable
                    ? "运行正常"
                    : helper.installed
                      ? "需要修复"
                      : "尚未安装"
                }}</span></strong
              >
              <p>安装时需要系统管理员权限</p>
            </div>
            <div class="helper-actions">
              <button
                class="button small"
                :disabled="serviceBusy || conn.isBusy || conn.isConnected"
                @click="serviceAction(false)"
              >
                {{
                  serviceBusy
                    ? "处理中…"
                    : helper.installed
                      ? "修复服务"
                      : "安装服务"
                }}</button
              ><button
                v-if="helper.installed"
                class="text-button"
                :disabled="serviceBusy || conn.isConnected"
                @click="serviceAction(true)"
              >
                卸载
              </button>
            </div>
          </div>
          <div class="info-note">
            切换连接模式会重新建立连接，正在进行的下载可能暂时中断。
          </div>
        </section>
        <section class="panel">
          <h2 class="settings-title">
            <AppIcon name="info" :size="18" />关于客户端
          </h2>
          <div class="setting-row">
            <div>
              <strong>Sufe {{ appVersion || "—" }}</strong>
              <p>专属机场客户端 · 让连接，自由一点</p>
            </div>
            <button class="button small" :disabled="updateBusy" @click="update">
              {{ updateBusy ? "检查中…" : "检查更新" }}
            </button>
          </div>
          <div class="setting-row">
            <div>
              <strong>mihomo 内核</strong>
              <p class="kernel-version">
                {{ kernelVersion || "暂未读取版本" }}
              </p>
            </div>
            <span class="badge" :class="{ enabled: health?.mihomo_present }">{{
              health?.mihomo_present ? "内核已就绪" : "待检查"
            }}</span>
          </div>
        </section>
      </div>
      <aside>
        <section class="panel security-card">
          <span class="security-icon"
            ><AppIcon name="shield" :size="31"
          /></span>
          <h2>为稳定连接，多想一步</h2>
          <p>
            你的账号凭据通过系统安全存储保护。服务端配置会自动同步到客户端。
          </p>
          <div class="diagnostic-row">
            <span>请求通道</span
            ><strong>{{
              features.config?.transport_mode === "stealth"
                ? "Stealth 加密"
                : features.config?.transport_mode === "preview"
                  ? "界面预览"
                  : "HTTPS"
            }}</strong>
          </div>
          <div class="diagnostic-row">
            <span>配置来源</span
            ><strong>{{
              features.config?.config_source === "built_in"
                ? "内置配置"
                : features.config?.config_source === "unavailable"
                  ? "尚未就绪"
                  : features.config?.config_source || "内置配置"
            }}</strong>
          </div>
          <RouterLink to="/logs" class="text-button"
            >查看运行日志<AppIcon name="arrow" :size="14"
          /></RouterLink>
        </section>
        <section class="help-card">
          <AppIcon name="headphones" :size="22" />
          <h3>需要一点帮助？</h3>
          <p>遇到连接或账户问题，我们随时愿意帮忙。</p>
          <RouterLink
            :to="features.enabled('chatwoot') ? '/support' : '/tickets'"
            class="text-button"
            >联系支持<AppIcon name="arrow" :size="14"
          /></RouterLink>
        </section>
      </aside>
    </div>
  </div>
</template>
<style scoped>
.settings-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 270px;
  gap: 22px;
}
.settings-main {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.settings-title {
  display: flex;
  gap: 9px;
  align-items: center;
  margin-bottom: 14px;
  font-size: 15px;
}
.settings-title svg {
  color: #aa9dc6;
}
.setting-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 20px 0;
  border-bottom: 1px solid var(--border);
}
.setting-row:last-child {
  border: 0;
  padding-bottom: 0;
}
.setting-row strong {
  font-size: 12px;
  font-weight: 550;
}
.setting-row p {
  font-size: 10px;
  color: var(--muted);
  margin: 5px 0 0;
  line-height: 1.8;
}
.segmented {
  display: flex;
  padding: 3px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--surface-soft);
  flex-shrink: 0;
}
.segmented button {
  display: flex;
  gap: 5px;
  align-items: center;
  padding: 6px 10px;
  background: none;
  border: 0;
  border-radius: 6px;
  font-size: 10px;
  color: var(--muted);
}
.segmented button.active {
  background: var(--surface);
  color: var(--primary);
  box-shadow: 0 2px 5px #28284208;
}
.mode-select {
  font-size: 11px;
  max-width: 115px;
  padding: 8px 10px;
}
.button.small {
  font-size: 10px;
  padding: 7px 12px;
}
.badge.enabled {
  color: var(--green);
  background: var(--green-soft);
}
.inline-status {
  color: #c7a05a;
  font-size: 9px;
  font-weight: 400;
  margin-left: 8px;
}
.inline-status.ok {
  color: var(--green);
}
.helper-actions {
  display: flex;
  align-items: center;
  gap: 12px;
}
.helper-actions .text-button {
  font-size: 10px;
}
.info-note {
  font-size: 10px;
  margin-top: 18px;
}
.kernel-version {
  max-width: 280px;
  overflow-wrap: anywhere;
}
.security-card {
  padding: 26px 22px;
}
.security-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 61px;
  height: 61px;
  background: var(--lavender);
  color: var(--primary);
  border-radius: 18px;
  margin-bottom: 22px;
}
.security-card h2 {
  font-size: 15px;
  margin-bottom: 12px;
}
.security-card > p {
  font-size: 11px;
  color: var(--muted);
  line-height: 2;
  margin-bottom: 24px;
}
.diagnostic-row {
  display: flex;
  justify-content: space-between;
  font-size: 10px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border);
  gap: 10px;
}
.diagnostic-row span {
  color: var(--muted);
  flex-shrink: 0;
}
.diagnostic-row strong {
  font-weight: 400;
  text-align: right;
  overflow-wrap: anywhere;
}
.security-card > .text-button {
  margin-top: 22px;
  font-size: 11px;
}
.help-card {
  padding: 27px 22px;
}
.help-card > svg {
  color: #a79cb9;
}
.help-card h3 {
  margin: 12px 0 7px;
  font-size: 13px;
}
.help-card p {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.9;
}
.help-card .text-button {
  font-size: 11px;
}
.error-note {
  margin-bottom: 20px;
}
@media (max-width: 1120px) {
  .settings-grid {
    grid-template-columns: 1fr;
  }
  .settings-grid > aside {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
  }
  .security-card {
    padding: 22px;
  }
  .setting-row p {
    font-size: 11px;
  }
}
@media (max-width: 560px) {
  .setting-row {
    gap: 13px;
    padding: 18px 0;
  }
  .setting-row > div:first-child {
    min-width: 0;
  }
  .setting-row p {
    font-size: 10px;
    max-width: 195px;
  }
  .setting-row strong {
    font-size: 11px;
  }
  .settings-grid > aside {
    grid-template-columns: 1fr;
  }
  .helper-row {
    flex-wrap: wrap;
  }
  .segmented button {
    padding: 6px 8px;
  }
  .inline-status {
    display: block;
    margin: 4px 0 0;
  }
  .info-note {
    line-height: 1.9;
  }
  .mode-select {
    max-width: 104px;
  }
}
</style>
