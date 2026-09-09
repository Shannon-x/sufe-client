import { defineStore } from "pinia";
import { ref } from "vue";
import { listen } from "@/platform";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api } from "@/api";
import type { LoginSummary, SubscribeInfo, UserInfo } from "@/types";

// Single source of truth for "who is signed in?" on the frontend. The
// matching backend state lives in `AppState::auth` + Persistence + the OS
// keychain — see `desktop/src-tauri/src/commands/session.rs`.
export const useAuthStore = defineStore("auth", () => {
  const session = ref<LoginSummary | null>(null);
  const userInfo = ref<UserInfo | null>(null);
  const subscribe = ref<SubscribeInfo | null>(null);

  // True from app start until the first `bootstrap()` resolves. Router
  // beforeEach blocks on this so the user never sees a `/login` flash
  // when a valid snapshot is sitting on disk.
  const bootstrapping = ref(true);
  let unlistenExpired: UnlistenFn | null = null;
  let generation = 0;
  let userRequest = 0;
  let subscribeRequest = 0;
  let bootstrapRequest: Promise<void> | null = null;

  function clearSession() {
    generation++;
    session.value = null;
    userInfo.value = null;
    subscribe.value = null;
  }

  function bootstrap(): Promise<void> {
    if (bootstrapRequest) return bootstrapRequest;
    bootstrapRequest = restore();
    return bootstrapRequest;
  }

  async function restore() {
    const version = generation;
    // Always listen for the backend's expiry signal — covers both the
    // explicit `logout` command and the `check_login → unauthorized`
    // path. The backend wipes its own state before emitting; we just
    // mirror that on the frontend.
    try {
    if (!unlistenExpired) {
      unlistenExpired = await listen("xboard://session-expired", () => {
        clearSession();
      });
    }

      const restored = await api.hydrateSession();
      if (version !== generation) return;
      if (restored) {
        session.value = restored;
        // Revalidate the restored session. If the bearer is dead the
        // backend will emit `session-expired`, which our listener handles.
        // A transport failure is not evidence that the bearer was revoked.
        const ok = await api.checkLogin().catch(() => null);
        if (version !== generation) return;
        if (ok === false) {
          clearSession();
        } else {
          await Promise.all([
            refreshUser().catch(() => {}),
            refreshSubscribe().catch(() => {}),
          ]);
        }
      }
    } catch {
      // Native IPC unavailable on a plain browser; release the loading veil.
      // A restored session is retained on transient transport failures.
    } finally {
      bootstrapping.value = false;
    }
  }

  async function login(args: {
    email: string;
    password: string;
    captchaType?: string;
    captchaToken?: string;
  }) {
    const version = ++generation;
    const summary = await api.login(args);
    if (version !== generation) throw new Error("登录状态已变化，请重试。");
    userInfo.value = null;
    subscribe.value = null;
    session.value = summary;
    await Promise.allSettled([refreshUser(), refreshSubscribe()]);
    if (version !== generation) throw new Error("登录已失效，请重新登录。");
    return summary;
  }

  async function register(args: {
    email: string;
    password: string;
    emailCode: string;
    inviteCode?: string;
    captchaType?: string;
    captchaToken?: string;
  }) {
    const version = ++generation;
    const summary = await api.register(args);
    if (version !== generation) throw new Error("登录状态已变化，请重试。");
    userInfo.value = null;
    subscribe.value = null;
    session.value = summary;
    await Promise.allSettled([refreshUser(), refreshSubscribe()]);
    if (version !== generation) throw new Error("登录已失效，请重新登录。");
    return summary;
  }

  async function refreshUser() {
    if (!session.value) return;
    const version = generation, request = ++userRequest;
    const value = await api.currentUser();
    if (version === generation && request === userRequest && session.value) userInfo.value = value;
  }

  async function refreshSubscribe() {
    if (!session.value) return;
    const version = generation, request = ++subscribeRequest;
    const value = await api.currentSubscribe();
    if (version === generation && request === subscribeRequest && session.value) subscribe.value = value;
  }

  async function logout() {
    const version = ++generation;
    await api.logout();
    // The backend emits `session-expired`, but clear synchronously too so
    // the next render doesn't briefly show stale info.
    if (version === generation) clearSession();
  }

  return {
    session,
    userInfo,
    subscribe,
    bootstrapping,
    bootstrap,
    login,
    register,
    logout,
    refreshUser,
    refreshSubscribe,
  };
});
