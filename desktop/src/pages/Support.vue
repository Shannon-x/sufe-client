<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NAlert, NButton, NInput, NSpin } from "naive-ui";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { useSupportStore } from "@/stores/support";
import { useFeaturesStore } from "@/stores/features";
import AppIcon from "@/components/AppIcon.vue";
const router = useRouter();
const support = useSupportStore();
const features = useFeaturesStore();
const draft = ref("");
const stream = ref<HTMLDivElement | null>(null);
const attachmentError = ref("");
async function send() {
  const text = draft.value;
  if (await support.send(text)) draft.value = "";
}
function keydown(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    void send();
  }
}
function date(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}
async function openAttachment(url: string) {
  try {
    if (new URL(url).protocol !== "https:") throw new Error();
    await shellOpen(url);
  } catch {
    attachmentError.value = "此附件暂时无法打开，请联系客户支持。";
  }
}
watch(
  () => support.currentMessages.length,
  async () => {
    await nextTick();
    if (stream.value) stream.value.scrollTop = stream.value.scrollHeight;
    support.markRead();
  },
);
onMounted(() => {
  support.setViewing(true);
  void support.start();
});
onBeforeUnmount(() => support.setViewing(false));
</script>
<template>
  <section class="support-page">
    <div class="support-intro">
      <div>
        <span class="eyebrow">HERE TO HELP</span>
        <h2>每一个问题，都有人在意。</h2>
        <p>无论是连接、购买还是账户问题，我们都在这里。</p>
      </div>
      <NButton
        v-if="features.enabled('tickets')"
        secondary
        round
        @click="router.push({ name: 'tickets' })"
        >我的工单 ↗</NButton
      >
    </div>
    <div class="support-layout">
      <aside class="support-sidebar">
        <div class="advisor">
          <div class="advisor-icon">
            <AppIcon name="headphones" :size="28" />
          </div>
          <h3>客户支持</h3>
          <p>专业解答，耐心陪伴</p>
          <span class="availability">{{
            support.enabled ? "留言后会尽快为你回复" : "通过工单获得帮助"
          }}</span>
        </div>
        <div v-if="support.enabled" class="conversations">
          <label>会话记录</label
          ><button
            v-for="conversation in support.conversations"
            :key="conversation.id"
            type="button"
            :class="{ active: support.selectedId === conversation.id }"
            @click="support.select(conversation.id)"
          >
            <AppIcon name="chat" :size="17" /><span
              >咨询 #{{ conversation.id
              }}<small>{{
                conversation.status === "resolved" ? "已解决" : "跟进中"
              }}</small></span
            ><AppIcon name="chevron" :size="14" /></button
          ><NButton
            v-if="support.conversations.length"
            text
            type="primary"
            :loading="support.loading"
            @click="support.begin"
            >+ 发起咨询</NButton
          >
        </div>
        <div class="support-tips">
          <label>沟通小贴士</label>
          <p>描述遇到的问题、出现时间与错误提示，可以帮助我们更快解决。</p>
          <p>请勿发送密码、验证码或支付凭证中的敏感信息。</p>
        </div>
      </aside>
      <main class="chat-panel">
        <div class="chat-header">
          <div class="agent-avatar"><AppIcon name="chat" :size="20" /></div>
          <div>
            <strong>{{
              support.selected
                ? `客户支持 · #${support.selected.id}`
                : "与你保持连接"
            }}</strong
            ><small>{{
              support.selected?.status === "resolved"
                ? "这段会话已解决"
                : support.enabled
                  ? "消息将安全送达客服团队"
                  : "在需要的时候，找到帮助"
            }}</small>
          </div>
          <NButton
            v-if="support.enabled && support.selected"
            text
            :loading="support.loading"
            @click="support.refresh"
            ><AppIcon name="refresh" :size="17"
          /></NButton>
        </div>
        <div v-if="support.loading && !support.selected" class="chat-empty">
          <NSpin />
          <p>正在连接客户支持…</p>
        </div>
        <div v-else-if="!support.enabled" class="chat-empty">
          <div class="empty-illustration">
            <div class="bubble-shape">···</div>
            <div class="small-bubble">♡</div>
          </div>
          <h3>帮助就在这里</h3>
          <p>
            当前尚未开启在线会话。你可以提交工单，<br />我们会在工单中跟进并回复你的问题。
          </p>
          <NButton
            v-if="features.enabled('tickets')"
            type="primary"
            round
            @click="router.push({ name: 'tickets' })"
            >联系工单支持 <span class="space-arrow">→</span></NButton
          >
          <p v-else class="disabled-hint">
            当前客服渠道暂未开放，请通过服务商官网联系支持。
          </p>
        </div>
        <div v-else-if="!support.selected" class="chat-empty">
          <div class="empty-illustration">
            <div class="bubble-shape">···</div>
            <div class="small-bubble">♡</div>
          </div>
          <h3>你好，需要帮忙吗？</h3>
          <p>
            开始一段会话，告诉我们遇到的问题。<br />你的回复会保留在客户端，方便随时回来查看。
          </p>
          <NButton
            type="primary"
            round
            :loading="support.loading"
            @click="support.begin"
            >开始咨询 <span class="space-arrow">→</span></NButton
          >
        </div>
        <template v-else
          ><div ref="stream" class="message-stream">
            <div class="conversation-start">这是与客户支持的会话开始</div>
            <div
              v-if="!support.currentMessages.length"
              class="conversation-welcome"
            >
              你好，欢迎联系客户支持。请描述你的问题，我们会尽快回复。
            </div>
            <div
              v-for="message in support.currentMessages"
              :key="message.id"
              class="message-row"
              :class="{
                mine: message.message_type === 0,
                activity: message.message_type === 2,
              }"
            >
              <div
                v-if="message.message_type !== 0 && message.message_type !== 2"
                class="message-avatar"
              >
                <AppIcon name="headphones" :size="16" />
              </div>
              <div class="message-group">
                <span v-if="message.message_type === 1" class="sender-name">{{
                  message.sender?.available_name ||
                  message.sender?.name ||
                  "客户支持"
                }}</span>
                <div class="message-bubble">
                  <p v-if="message.content">{{ message.content }}</p>
                  <button
                    v-for="file in message.attachments"
                    :key="file.id"
                    type="button"
                    class="attachment"
                    @click="openAttachment(file.data_url)"
                  >
                    <AppIcon name="arrow" :size="14" />{{
                      file.fallback_title || "查看附件"
                    }}
                  </button>
                </div>
                <span class="message-time">{{ date(message.created_at) }}</span>
              </div>
            </div>
          </div>
          <div class="composer">
            <NInput
              v-model:value="draft"
              type="textarea"
              placeholder="输入消息，我们会认真倾听…"
              :autosize="{ minRows: 2, maxRows: 5 }"
              :disabled="support.sending"
              maxlength="4000"
              @keydown="keydown"
            />
            <div class="composer-actions">
              <span>Enter 发送 · Shift + Enter 换行</span
              ><NButton
                type="primary"
                :loading="support.sending"
                :disabled="!draft.trim()"
                @click="send"
                >发送 <AppIcon name="arrow" :size="15" style="margin-left: 8px"
              /></NButton>
            </div></div
        ></template>
        <NAlert
          v-if="support.error || attachmentError"
          type="warning"
          :show-icon="false"
          class="support-error"
          >{{ support.error || attachmentError
          }}<NButton
            v-if="support.enabled"
            text
            type="primary"
            @click="support.refresh"
            >重新同步</NButton
          ></NAlert
        >
      </main>
    </div>
  </section>
</template>
<style scoped>
.support-page {
  color: #242539;
}
.support-intro {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  margin: 5px 0 28px;
}
.eyebrow {
  font-size: 10px;
  letter-spacing: 1.8px;
  font-weight: 600;
  color: #aaa7b9;
}
.support-intro h2 {
  font-size: 27px;
  letter-spacing: -0.6px;
  font-weight: 650;
  margin: 11px 0 10px;
}
.support-intro p {
  font-size: 13px;
  color: #9390a4;
  margin: 0;
}
.support-layout {
  display: grid;
  grid-template-columns: 240px minmax(0, 1fr);
  gap: 22px;
  min-height: 570px;
}
.support-sidebar {
  background: #fff;
  border: 1px solid #ece9f3;
  border-radius: 24px;
  overflow: hidden;
  padding: 28px 19px;
  display: flex;
  flex-direction: column;
}
.advisor {
  text-align: center;
}
.advisor-icon {
  width: 65px;
  height: 65px;
  border-radius: 22px;
  background: linear-gradient(145deg, #eeebff, #e4dff7);
  display: grid;
  place-items: center;
  color: #9786d3;
  margin: 0 auto 15px;
}
.advisor h3 {
  font-size: 17px;
  margin: 0 0 6px;
}
.advisor p {
  font-size: 11px;
  color: #aaa1b4;
  margin: 0;
}
.availability {
  display: inline-block;
  font-size: 10px;
  color: #ad9cb8;
  background: #f8f5fa;
  border-radius: 6px;
  margin: 16px 0 26px;
  padding: 6px 9px;
}
.conversations {
  border-top: 1px solid #f2eef6;
  padding-top: 20px;
}
.conversations label,
.support-tips label {
  font-size: 10px;
  color: #bbb2c3;
  display: block;
  margin: 0 8px 12px;
}
.conversations > button {
  display: flex;
  align-items: center;
  width: 100%;
  text-align: left;
  padding: 12px 9px;
  gap: 10px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: #a093b2;
  cursor: pointer;
}
.conversations > button.active {
  background: #f2edfc;
  color: #8c75ce;
}
.conversations > button span {
  flex: 1;
  font-size: 11px;
}
.conversations > button small {
  display: block;
  font-size: 9px;
  margin-top: 3px;
  color: #b5a7c9;
}
.conversations :deep(.n-button) {
  font-size: 11px;
  margin: 15px 8px;
}
.support-tips {
  margin-top: auto;
  padding-top: 35px;
}
.support-tips p {
  font-size: 10px;
  line-height: 1.8;
  color: #b5aabd;
  margin: 10px 8px;
}
.chat-panel {
  border-radius: 24px;
  background: #fff;
  border: 1px solid #ece9f3;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 570px;
}
.chat-header {
  display: flex;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid #f3eff7;
  padding: 21px 25px;
}
.agent-avatar {
  width: 36px;
  height: 36px;
  border-radius: 12px;
  display: grid;
  place-items: center;
  background: #f0ecfc;
  color: #a18ad8;
}
.chat-header strong {
  display: block;
  font-size: 12px;
  font-weight: 600;
}
.chat-header small {
  display: block;
  font-size: 10px;
  color: #b4a8bd;
  margin-top: 5px;
}
.chat-header > div:nth-child(2) {
  flex: 1;
}
.chat-header :deep(.n-button) {
  color: #b5a8c0;
}
.chat-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 38px 25px;
  text-align: center;
}
.empty-illustration {
  width: 125px;
  height: 103px;
  position: relative;
  margin-bottom: 25px;
}
.bubble-shape {
  width: 88px;
  height: 67px;
  background: #efe9fc;
  border-radius: 23px 23px 7px 23px;
  color: #baa9e1;
  font-size: 43px;
  line-height: 45px;
  letter-spacing: 4px;
  transform: rotate(-8deg);
  position: absolute;
  top: 8px;
  left: 6px;
  box-shadow: 0 9px 27px #9380bc09;
}
.small-bubble {
  width: 44px;
  height: 36px;
  position: absolute;
  right: 2px;
  bottom: 5px;
  background: #faf3e9;
  border: 4px solid white;
  color: #d8b997;
  font-size: 19px;
  line-height: 31px;
  border-radius: 14px 14px 14px 4px;
  transform: rotate(9deg);
}
.chat-empty h3 {
  font-size: 19px;
  font-weight: 550;
  margin: 0 0 10px;
}
.chat-empty p {
  font-size: 11px;
  color: #b5a9c0;
  line-height: 1.9;
  margin: 0 0 24px;
}
.space-arrow {
  margin-left: 20px;
}
.chat-empty :deep(.n-button) {
  font-size: 11px;
  padding: 0 23px;
  height: 37px;
}
.message-stream {
  flex: 1;
  min-height: 280px;
  max-height: 440px;
  overflow: auto;
  padding: 23px;
}
.conversation-start {
  text-align: center;
  color: #cabfd3;
  font-size: 10px;
  margin-bottom: 27px;
}
.conversation-welcome {
  font-size: 12px;
  color: #b1a3bf;
  text-align: center;
  padding: 30px 20px;
}
.message-row {
  display: flex;
  align-items: start;
  gap: 8px;
  margin: 19px 0;
}
.message-avatar {
  width: 29px;
  height: 29px;
  display: grid;
  place-items: center;
  background: #f3eefb;
  color: #b5a0cd;
  border-radius: 10px;
  flex-shrink: 0;
}
.message-group {
  max-width: 78%;
}
.sender-name {
  display: block;
  font-size: 9px;
  color: #bcaaca;
  margin-bottom: 5px;
}
.message-bubble {
  background: #f6f2fa;
  border-radius: 3px 14px 14px 14px;
  padding: 11px 15px;
  font-size: 12px;
  color: #766488;
  line-height: 1.8;
  overflow-wrap: anywhere;
}
.message-bubble p {
  margin: 0;
  white-space: pre-wrap;
}
.message-time {
  display: block;
  font-size: 9px;
  color: #c9bcd2;
  margin-top: 6px;
}
.mine {
  justify-content: flex-end;
}
.mine .message-bubble {
  background: #8170d8;
  color: white;
  border-radius: 14px 3px 14px 14px;
}
.mine .message-time {
  text-align: right;
}
.activity {
  justify-content: center;
}
.activity .message-bubble {
  font-size: 10px;
  background: transparent;
  color: #bbafc7;
}
.activity .message-time {
  display: none;
}
.attachment {
  background: transparent;
  border: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  color: inherit;
  cursor: pointer;
  padding: 4px 0;
  text-decoration: underline;
}
.composer {
  border-top: 1px solid #f2edf7;
  padding: 15px 22px 20px;
}
.composer :deep(.n-input) {
  border: 0;
  background: transparent;
  font-size: 12px;
}
.composer :deep(.n-input__border),
.composer :deep(.n-input__state-border) {
  display: none;
}
.composer-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 10px;
}
.composer-actions > span {
  font-size: 9px;
  color: #c2b6cd;
}
.composer-actions :deep(.n-button) {
  font-size: 11px;
  height: 33px;
  border-radius: 9px;
}
.support-error {
  margin: 12px 20px;
  font-size: 11px;
}
.support-error :deep(.n-button) {
  margin-left: 10px;
  font-size: 11px;
}
@media (max-width: 850px) {
  .support-layout {
    grid-template-columns: 1fr;
  }
  .support-sidebar {
    display: none;
  }
  .support-intro h2 {
    font-size: 23px;
  }
  .chat-panel {
    min-height: 560px;
  }
}
</style>
