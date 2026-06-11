import { defineStore } from "pinia";
import { ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

  async function bootstrap() {
    // Always listen for the backend's expiry signal — covers both the
    // explicit `logout` command and the `check_login → unauthorized`
    // path. The backend wipes its own state before emitting; we just
    // mirror that on the frontend.
    if (!unlistenExpired) {
      unlistenExpired = await listen("xboard://session-expired", () => {
        session.value = null;
        userInfo.value = null;
        subscribe.value = null;
      });
    }

    try {
      const restored = await api.hydrateSession();
      if (restored) {
        session.value = restored;
        // Revalidate the restored session. If the bearer is dead the
        // backend will emit `session-expired`, which our listener handles.
        // On IPC failure we default to `false` (logged out) so the router
        // guard kicks the user back to /login instead of stranding them
        // on a broken Home with stale state.
        const ok = await api.checkLogin().catch(() => false);
        if (!ok) {
          session.value = null;
        } else {
          await Promise.all([
            refreshUser().catch(() => {}),
            refreshSubscribe().catch(() => {}),
          ]);
        }
      }
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
    const summary = await api.login(args);
    session.value = summary;
    await Promise.allSettled([refreshUser(), refreshSubscribe()]);
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
    const summary = await api.register(args);
    session.value = summary;
    await Promise.allSettled([refreshUser(), refreshSubscribe()]);
    return summary;
  }

  async function refreshUser() {
    userInfo.value = await api.currentUser();
  }

  async function refreshSubscribe() {
    subscribe.value = await api.currentSubscribe();
  }

  async function logout() {
    await api.logout();
    // The backend emits `session-expired`, but clear synchronously too so
    // the next render doesn't briefly show stale info.
    session.value = null;
    userInfo.value = null;
    subscribe.value = null;
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
