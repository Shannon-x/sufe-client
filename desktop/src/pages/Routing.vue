<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useMessage, NModal, NSwitch } from "naive-ui";
import { useI18n } from "vue-i18n";
import AppIcon from "@/components/AppIcon.vue";
import { invoke } from "@/platform";
import { api } from "@/api";
import { useConnectionStore } from "@/stores/connection";
import { formatError } from "@/utils/error";
import type { RuleItem } from "@/types";
interface CustomRule {
  id: string;
  kind: string;
  value: string;
  target: string;
  enabled: boolean;
}
const conn = useConnectionStore(),
  message = useMessage(),
  { t } = useI18n();
const custom = ref<CustomRule[]>([]),
  system = ref<RuleItem[]>([]),
  query = ref(""),
  tab = ref("custom"),
  show = ref(false),
  busy = ref(false),
  loaded = ref(false),
  error = ref(""),
  needsReconnect = ref(false);
const kind = ref("DOMAIN-SUFFIX"),
  value = ref(""),
  target = ref("PROXY"),
  formError = ref("");
const kinds = [
  { value: "DOMAIN-SUFFIX", label: "域名及子域名", placeholder: "example.com" },
  { value: "DOMAIN", label: "完整域名", placeholder: "www.example.com" },
  { value: "DOMAIN-KEYWORD", label: "域名关键词", placeholder: "example" },
  { value: "IP-CIDR", label: "IPv4 网段", placeholder: "192.168.0.0/16" },
  { value: "IP-CIDR6", label: "IPv6 网段", placeholder: "2001:db8::/32" },
];
const labels: Record<string, string> = {
  PROXY: "代理连接",
  DIRECT: "直接连接",
  REJECT: "拦截请求",
};
const visibleCustom = computed(() =>
  custom.value.filter((r) =>
    `${r.value} ${r.kind} ${labels[r.target]}`
      .toLowerCase()
      .includes(query.value.toLowerCase()),
  ),
);
const visibleSystem = computed(() =>
  system.value
    .filter((r) =>
      `${r.type} ${r.payload} ${r.proxy}`
        .toLowerCase()
        .includes(query.value.toLowerCase()),
    )
    .slice(0, 300),
);
async function load() {
  busy.value = true;
  error.value = "";
  try {
    custom.value = await invoke<CustomRule[]>("fetch_custom_rules");
    loaded.value = true;
    if (conn.isConnected) system.value = await api.rules();
  } catch (e) {
    error.value = formatError(e, t);
  } finally {
    busy.value = false;
  }
}
async function save(next: CustomRule[]) {
  if (busy.value || !loaded.value) return false;
  busy.value = true;
  try {
    await invoke("save_custom_rules", { rules: next });
    custom.value = next;
    needsReconnect.value = conn.isConnected;
    message.success(
      conn.isConnected
        ? "规则已保存，重新连接后生效"
        : "规则已保存，下次连接时生效",
    );
    return true;
  } catch (e) {
    error.value = formatError(e, t);
    message.error(error.value);
    return false;
  } finally {
    busy.value = false;
  }
}
async function add() {
  formError.value = "";
  const text = value.value.trim();
  if (!text || text.length > 253 || /[\s,]/.test(text)) {
    formError.value = "请输入有效内容，不要包含网址协议、空格或逗号。";
    return;
  }
  if (
    !kind.value.startsWith("IP-CIDR") &&
    !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(text)
  ) {
    formError.value =
      "域名请使用英文字母、数字或连字符；国际域名请使用 Punycode。";
    return;
  }
  const ok = await save([
    ...custom.value,
    {
      id: crypto.randomUUID(),
      kind: kind.value,
      value: text,
      target: target.value,
      enabled: true,
    },
  ]);
  if (ok) {
    show.value = false;
    value.value = "";
  }
}
async function toggle(rule: CustomRule, enabled: boolean) {
  await save(
    custom.value.map((r) => (r.id === rule.id ? { ...r, enabled } : r)),
  );
}
async function remove(id: string) {
  await save(custom.value.filter((r) => r.id !== id));
}
async function apply() {
  busy.value = true;
  try {
    await conn.disconnect();
    await conn.connect();
    needsReconnect.value = false;
    system.value = await api.rules();
    message.success("已重新连接，自定义规则已生效");
  } catch (e) {
    error.value = formatError(e, t);
  } finally {
    busy.value = false;
  }
}
onMounted(load);
</script>
<template>
  <div>
    <div class="page-heading">
      <div>
        <h1>连接方式，由你定义</h1>
        <p>为常用网站和应用，选择恰到好处的网络路径。</p>
      </div>
      <button
        class="button primary"
        :disabled="busy || !loaded || custom.length >= 200"
        @click="show = true"
      >
        <AppIcon name="plus" :size="16" />添加规则
      </button>
    </div>
    <section class="rules-intro">
      <span class="intro-icon"><AppIcon name="rules" :size="26" /></span>
      <div>
        <strong>智能分流，让连接各得其所</strong>
        <p>
          自定义规则优先于订阅规则，按列表顺序依次匹配。其余请求继续遵循机场分流策略。
        </p>
      </div>
      <span class="badge"
        >{{ custom.filter((r) => r.enabled).length }} 条已启用</span
      >
    </section>
    <div v-if="error" class="error-note" role="alert">
      {{ error }} <button class="text-button" @click="load">重试</button>
    </div>
    <div v-if="needsReconnect" class="apply-note">
      <span
        ><AppIcon
          name="info"
          :size="15"
        />新规则将在下次连接生效，当前连接保持原有规则。</span
      ><button class="button soft" :disabled="busy" @click="apply">
        重新连接并应用
      </button>
    </div>
    <section class="panel rules-panel">
      <div class="rules-toolbar">
        <div class="rules-tabs">
          <button :class="{ active: tab === 'custom' }" @click="tab = 'custom'">
            自定义规则 <span>{{ custom.length }}</span></button
          ><button
            :class="{ active: tab === 'system' }"
            @click="tab = 'system'"
          >
            当前生效规则
          </button>
        </div>
        <label class="rules-search"
          ><AppIcon name="search" :size="15" /><input
            v-model="query"
            aria-label="搜索分流规则"
            placeholder="搜索规则、域名或出口"
        /></label>
      </div>
      <template v-if="tab === 'custom'"
        ><div v-if="!visibleCustom.length" class="empty-state">
          <span class="empty-rule-icon"
            ><AppIcon name="rules" :size="34"
          /></span>
          <h3>{{ query ? "没有找到相关规则" : "让常用网站，走你想走的路" }}</h3>
          <p>
            添加一条域名或 IP 规则，选择代理、直连或拦截。无需编辑复杂配置。
          </p>
          <button
            class="button soft"
            :disabled="busy || !loaded"
            @click="show = true"
          >
            <AppIcon name="plus" :size="15" />添加第一条规则
          </button>
        </div>
        <div v-else class="rule-list">
          <div
            v-for="(rule, index) in visibleCustom"
            :key="rule.id"
            class="rule-row"
          >
            <span class="rule-index">{{
              String(index + 1).padStart(2, "0")
            }}</span
            ><span class="rule-icon"
              ><AppIcon
                :name="
                  rule.target === 'REJECT'
                    ? 'shield'
                    : rule.target === 'DIRECT'
                      ? 'link'
                      : 'globe'
                "
                :size="19"
            /></span>
            <div class="rule-description">
              <strong>{{ rule.value }}</strong
              ><small>{{
                kinds.find((k) => k.value === rule.kind)?.label || rule.kind
              }}</small>
            </div>
            <span class="target-tag" :class="rule.target.toLowerCase()">{{
              labels[rule.target]
            }}</span
            ><NSwitch
              :value="rule.enabled"
              :disabled="busy"
              :aria-label="'启用规则 ' + rule.value"
              @update:value="(v) => toggle(rule, v)"
            /><button
              class="icon-button delete-rule"
              :disabled="busy"
              :aria-label="'删除规则 ' + rule.value"
              @click="remove(rule.id)"
            >
              <AppIcon name="trash" :size="15" />
            </button>
          </div></div
      ></template>
      <template v-else
        ><div v-if="!conn.isConnected" class="empty-state">
          <AppIcon name="activity" :size="34" />
          <h3>连接后查看实时分流规则</h3>
          <p>这里展示内核当前实际执行的规则，便于了解请求的网络路径。</p>
        </div>
        <div v-else class="system-list">
          <div
            class="system-rule"
            v-for="(rule, index) in visibleSystem"
            :key="index"
          >
            <span>{{ index + 1 }}</span
            ><code>{{ rule.type }}</code
            ><strong>{{ rule.payload || "所有未匹配请求" }}</strong
            ><span class="badge">{{ rule.proxy }}</span>
          </div>
          <p v-if="system.length > 300" class="muted">
            仅显示前 300 条匹配结果，请搜索定位具体规则。
          </p>
        </div></template
      >
    </section>
    <p class="rule-footnote">
      <AppIcon
        name="shield"
        :size="13"
      />规则只在本机保存，不会更改机场订阅。最多支持 200 条自定义规则。
    </p>
    <NModal
      v-model:show="show"
      preset="card"
      title="添加分流规则"
      style="width: 460px"
      :mask-closable="!busy"
      ><form @submit.prevent="add" class="rule-form">
        <label
          ><span class="field-label">匹配方式</span
          ><select v-model="kind" class="field">
            <option v-for="k in kinds" :key="k.value" :value="k.value">
              {{ k.label }}
            </option>
          </select></label
        ><label
          ><span class="field-label">匹配内容</span
          ><input
            v-model="value"
            class="field"
            :placeholder="kinds.find((k) => k.value === kind)?.placeholder"
            maxlength="253"
            autofocus /></label
        ><label
          ><span class="field-label">连接方式</span
          ><select v-model="target" class="field">
            <option value="PROXY">代理连接 · 使用当前所选代理组</option>
            <option value="DIRECT">直接连接 · 不经过代理</option>
            <option value="REJECT">拦截请求 · 阻止网络访问</option>
          </select></label
        >
        <div v-if="formError" class="error-note">{{ formError }}</div>
        <button
          class="button primary"
          :disabled="busy || !value.trim()"
          type="submit"
        >
          {{ busy ? "正在保存…" : "保存规则" }}
        </button>
      </form></NModal
    >
  </div>
</template>
<style scoped>
.rules-intro {
  display: flex;
  align-items: center;
  gap: 18px;
  background: var(--lavender);
  padding: 23px 25px;
  border: 1px solid #a891ee20;
  border-radius: 16px;
  margin-bottom: 24px;
}
.intro-icon {
  width: 51px;
  height: 51px;
  border-radius: 15px;
  background: var(--surface);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--primary);
  flex-shrink: 0;
}
.rules-intro strong {
  font-size: 14px;
  font-weight: 550;
}
.rules-intro p {
  font-size: 11px;
  color: var(--muted);
  margin: 6px 0 0;
}
.rules-intro > .badge {
  margin-left: auto;
  flex-shrink: 0;
}
.rules-panel {
  padding: 0 24px;
}
.rules-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--border);
  gap: 18px;
}
.rules-tabs {
  display: flex;
  gap: 24px;
}
.rules-tabs button {
  background: none;
  border: 0;
  border-bottom: 2px solid transparent;
  padding: 20px 0;
  font-size: 12px;
  color: var(--muted);
}
.rules-tabs button.active {
  border-bottom-color: var(--primary);
  color: var(--primary);
  font-weight: 550;
}
.rules-tabs span {
  background: var(--lavender);
  padding: 1px 5px;
  border-radius: 4px;
  font-size: 10px;
  margin-left: 5px;
}
.rules-search {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--muted);
}
.rules-search input {
  width: 175px;
  border: 0;
  font-size: 11px;
  background: none;
  outline: none;
  color: var(--text);
}
.empty-rule-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 78px;
  height: 78px;
  background: var(--surface-soft);
  border: 1px solid var(--border);
  border-radius: 22px;
  color: #b6a8d9;
}
.empty-state {
  padding: 65px 16px;
}
.empty-state h3 {
  font-size: 16px;
  margin-top: 22px;
}
.empty-state p {
  font-size: 12px;
  max-width: 340px;
}
.empty-state .button {
  font-size: 12px;
}
.rule-row {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 19px 0;
  border-bottom: 1px solid var(--border);
}
.rule-row:last-child {
  border: 0;
}
.rule-index {
  font-size: 10px;
  color: var(--muted);
}
.rule-icon {
  width: 34px;
  height: 34px;
  background: var(--surface-soft);
  color: #a69aba;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.rule-description {
  min-width: 0;
  flex: 1;
}
.rule-description strong {
  font-size: 12px;
  display: block;
  overflow-wrap: anywhere;
}
.rule-description small {
  font-size: 10px;
  color: var(--muted);
  display: block;
  margin-top: 3px;
}
.target-tag {
  font-size: 10px;
  border-radius: 5px;
  padding: 4px 9px;
  background: var(--lavender);
  color: var(--primary);
}
.target-tag.direct {
  background: var(--green-soft);
  color: var(--green);
}
.target-tag.reject {
  background: #ef9aab18;
  color: var(--red);
}
.delete-rule {
  border: 0;
  width: 30px;
  height: 30px;
}
.rule-footnote {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 10px;
  color: var(--muted);
  margin-top: 20px;
}
.rule-form {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.rule-form .button {
  margin-top: 5px;
}
.system-rule {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px 0;
  font-size: 11px;
  border-bottom: 1px solid var(--border);
}
.system-rule > span:first-child {
  color: var(--muted);
  font-size: 10px;
  width: 26px;
}
.system-rule strong {
  flex: 1;
  font-weight: 400;
  overflow-wrap: anywhere;
}
.system-rule code {
  width: 100px;
  color: var(--muted);
}
.error-note {
  margin-bottom: 20px;
}
.apply-note {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 20px;
  padding: 14px 18px;
  border-radius: 12px;
  background: var(--surface);
  margin-bottom: 20px;
  font-size: 11px;
}
.apply-note > span {
  display: flex;
  gap: 8px;
  align-items: center;
  color: var(--muted);
}
.apply-note button {
  font-size: 10px;
}
@media (max-width: 850px) {
  .rules-toolbar {
    flex-wrap: wrap;
    padding-bottom: 14px;
    gap: 8px;
  }
  .rules-intro > .badge {
    display: none;
  }
  .rule-row {
    gap: 10px;
  }
  .rules-intro {
    padding: 20px;
  }
  .rules-panel {
    padding: 0 18px;
  }
}
@media (max-width: 560px) {
  .rules-intro {
    align-items: flex-start;
    gap: 12px;
  }
  .intro-icon {
    width: 40px;
    height: 40px;
  }
  .rules-intro strong {
    font-size: 12px;
  }
  .rules-intro p {
    font-size: 10px;
    line-height: 1.9;
  }
  .rule-index,
  .rule-icon {
    display: none;
  }
  .rule-description strong {
    font-size: 11px;
  }
  .target-tag {
    font-size: 9px;
    padding: 3px 6px;
  }
  .rule-row {
    gap: 9px;
  }
  .rule-footnote {
    font-size: 9px;
    align-items: flex-start;
  }
  .rule-footnote svg {
    flex-shrink: 0;
  }
  .rules-tabs {
    gap: 20px;
  }
  .apply-note {
    flex-wrap: wrap;
    gap: 12px;
  }
  .system-rule {
    gap: 7px;
    font-size: 10px;
  }
  .system-rule code {
    width: 76px;
  }
  .system-rule .badge {
    max-width: 80px;
    white-space: normal;
  }
}
</style>
