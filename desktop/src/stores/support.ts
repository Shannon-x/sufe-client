import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import {
  isPermissionGranted,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import {
  supportApi,
  type SupportConversation,
  type SupportMessage,
} from "@/api/support";
import { useFeaturesStore } from "@/stores/features";
import { useAuthStore } from "@/stores/auth";

function readableError(error: unknown): string {
  return error && typeof error === "object" && "message" in error
    ? String(error.message)
    : "暂时无法连接客服，请稍后重试。";
}
export const useSupportStore = defineStore("support", () => {
  const features = useFeaturesStore();
  const auth = useAuthStore();
  const conversations = ref<SupportConversation[]>([]);
  const messages = ref<Record<number, SupportMessage[]>>({});
  const selectedId = ref<number | null>(null);
  const loading = ref(false);
  const sending = ref(false);
  const error = ref("");
  const latestNotice = ref("");
  const viewing = ref(false);
  const markers = ref<Record<number, number>>({});
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running = false;
  let epoch = 0;
  let pollBusy = false;
  let failures = 0;
  let account = "";
  const enabled = computed(
    () =>
      features.enabled("chatwoot") &&
      !!features.config?.chatwoot_base_url &&
      !!features.config?.chatwoot_inbox_identifier,
  );
  const selected = computed(() =>
    conversations.value.find((c) => c.id === selectedId.value),
  );
  const currentMessages = computed(() =>
    selectedId.value ? (messages.value[selectedId.value] ?? []) : [],
  );
  const unread = computed(() =>
    Object.entries(messages.value).reduce(
      (sum, [id, rows]) =>
        sum +
        rows.filter(
          (m) =>
            m.message_type === 1 && m.id > (markers.value[Number(id)] ?? 0),
        ).length,
      0,
    ),
  );
  watch(
    () =>
      `${enabled.value}:${features.config?.chatwoot_base_url}:${features.config?.chatwoot_inbox_identifier}`,
    () => {
      stop();
      if (enabled.value && auth.session) void start();
    },
  );
  function storageKey() {
    return `sufe.support.seen:${account}:${features.config?.chatwoot_base_url}:${features.config?.chatwoot_inbox_identifier}`;
  }
  function markRead() {
    if (
      !viewing.value ||
      !selectedId.value ||
      document.visibilityState !== "visible" ||
      !document.hasFocus()
    )
      return;
    const latest = Math.max(0, ...currentMessages.value.map((m) => m.id));
    if (latest > (markers.value[selectedId.value] ?? 0)) {
      markers.value[selectedId.value] = latest;
      try {
        localStorage.setItem(storageKey(), JSON.stringify(markers.value));
      } catch {}
    }
    latestNotice.value = "";
  }
  function setViewing(value: boolean) {
    viewing.value = value;
    if (value) markRead();
  }
  function select(id: number) {
    selectedId.value = id;
    markRead();
  }
  function stop() {
    epoch++;
    running = false;
    pollBusy = false;
    if (timer) clearTimeout(timer);
    timer = undefined;
    conversations.value = [];
    messages.value = {};
    selectedId.value = null;
    markers.value = {};
    latestNotice.value = "";
    account = "";
    loading.value = false;
    sending.value = false;
    error.value = "";
    window.removeEventListener("focus", focus);
  }
  async function start() {
    if (running && account === auth.session?.email) return;
    stop();
    if (!auth.session) return;
    account = auth.session.email;
    const version = epoch;
    running = true;
    loading.value = true;
    await features.ensure();
    if (version !== epoch) return;
    if (!enabled.value) {
      loading.value = false;
      return;
    }
    try {
      const stored: unknown = JSON.parse(
        localStorage.getItem(storageKey()) || "{}",
      );
      if (stored && typeof stored === "object")
        markers.value = stored as Record<number, number>;
    } catch {}
    window.addEventListener("focus", focus);
    try {
      const result = await supportApi.restore();
      if (version !== epoch) return;
      conversations.value = Array.isArray(result.conversations)
        ? result.conversations.sort((a, b) => b.id - a.id)
        : [];
      selectedId.value = conversations.value[0]?.id ?? null;
      await refresh();
    } catch (e) {
      if (version === epoch) error.value = readableError(e);
    } finally {
      if (version === epoch) {
        loading.value = false;
        schedule();
      }
    }
  }
  function schedule() {
    if (timer) clearTimeout(timer);
    if (running && enabled.value)
      timer = setTimeout(
        () => void refresh(),
        failures
          ? Math.min(60000, 8000 * 2 ** failures)
          : document.visibilityState === "visible"
            ? 8000
            : 20000,
      );
  }
  async function notify(body: string) {
    latestNotice.value = body;
    try {
      if (await isPermissionGranted())
        sendNotification({
          title: "客户支持有新回复",
          body: "打开客户端查看客服消息。",
        });
    } catch {}
  }
  async function refresh() {
    if (!running || !enabled.value || pollBusy) return;
    pollBusy = true;
    const version = epoch;
    try {
      for (const conversation of conversations.value.slice(0, 5)) {
        const response = await supportApi.messages(conversation.id);
        if (version !== epoch) return;
        const rows = response
          .filter((m) => !m.private)
          .sort((a, b) => a.id - b.id);
        const previous = new Set(
          (messages.value[conversation.id] ?? []).map((m) => m.id),
        );
        const hadPrevious = messages.value[conversation.id] !== undefined;
        messages.value[conversation.id] = rows;
        const fresh = rows.filter(
          (m) =>
            m.message_type === 1 &&
            !previous.has(m.id) &&
            m.id > (markers.value[conversation.id] ?? 0),
        );
        markRead();
        if (
          hadPrevious &&
          fresh.length &&
          !(
            viewing.value &&
            selectedId.value === conversation.id &&
            document.hasFocus() &&
            document.visibilityState === "visible"
          )
        )
          void notify(fresh[fresh.length - 1].content || "客服发送了一个附件");
      }
      error.value = "";
      failures = 0;
    } catch (e) {
      if (version === epoch) {
        error.value = readableError(e);
        failures++;
      }
    } finally {
      if (version === epoch) {
        pollBusy = false;
        schedule();
      }
    }
  }
  function focus() {
    markRead();
    void refresh();
  }
  async function begin() {
    if (loading.value || !enabled.value) return;
    loading.value = true;
    error.value = "";
    const version = epoch;
    try {
      const result = await supportApi.start();
      if (version !== epoch) return;
      if (!conversations.value.some((c) => c.id === result.id))
        conversations.value.unshift(result);
      selectedId.value = result.id;
      await refresh();
    } catch (e) {
      if (version === epoch) error.value = readableError(e);
    } finally {
      if (version === epoch) loading.value = false;
    }
  }
  async function send(content: string): Promise<boolean> {
    if (!selectedId.value || sending.value || !content.trim()) return false;
    sending.value = true;
    error.value = "";
    const id = selectedId.value;
    const version = epoch;
    try {
      const sent = await supportApi.send(id, content.trim());
      if (version !== epoch) return false;
      const rows = messages.value[id] ?? [];
      if (!rows.some((m) => m.id === sent.id))
        messages.value[id] = [...rows, sent].sort((a, b) => a.id - b.id);
      markRead();
      return true;
    } catch (e) {
      if (version === epoch)
        error.value = `${readableError(e)} 发送结果尚未确认，请刷新会话后再决定是否重试。`;
      return false;
    } finally {
      if (version === epoch) sending.value = false;
    }
  }
  return {
    conversations,
    selectedId,
    selected,
    currentMessages,
    loading,
    sending,
    error,
    latestNotice,
    unread,
    enabled,
    start,
    stop,
    refresh,
    begin,
    select,
    send,
    setViewing,
    markRead,
  };
});
