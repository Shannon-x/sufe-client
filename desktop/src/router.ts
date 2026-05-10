import { createRouter, createWebHashHistory } from "vue-router";
import { api } from "@/api";
import { useAuthStore } from "@/stores/auth";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/login",
      name: "login",
      component: () => import("@/pages/Login.vue"),
      meta: { requiresAuth: false },
    },
    {
      path: "/register",
      name: "register",
      component: () => import("@/pages/Register.vue"),
      meta: { requiresAuth: false },
    },
    {
      path: "/forget-password",
      name: "forget-password",
      component: () => import("@/pages/ForgetPassword.vue"),
      meta: { requiresAuth: false },
    },
    {
      path: "/",
      name: "home",
      component: () => import("@/pages/Home.vue"),
      meta: { requiresAuth: true },
    },
    {
      path: "/notices",
      name: "notices",
      component: () => import("@/pages/Notices.vue"),
      meta: { requiresAuth: true },
    },
    {
      path: "/plans",
      name: "plans",
      component: () => import("@/pages/Plans.vue"),
      meta: { requiresAuth: true },
    },
    {
      path: "/orders",
      name: "orders",
      component: () => import("@/pages/Orders.vue"),
      meta: { requiresAuth: true },
    },
    {
      path: "/tickets",
      name: "tickets",
      component: () => import("@/pages/Tickets.vue"),
      meta: { requiresAuth: true },
    },
    {
      path: "/tickets/:id(\\d+)",
      name: "ticket-detail",
      component: () => import("@/pages/TicketDetail.vue"),
      props: (route) => ({ id: Number(route.params.id) }),
      meta: { requiresAuth: true },
    },
    {
      path: "/:catchAll(.*)*",
      redirect: "/",
    },
  ],
});

router.beforeEach(async (to) => {
  const auth = useAuthStore();

  // Wait for cold-start hydration before letting any route resolve. This is
  // what stops a "/login → /" flash when a valid snapshot exists on disk.
  while (auth.bootstrapping) {
    await new Promise((r) => setTimeout(r, 16));
  }

  if (to.meta.requiresAuth && !auth.session) {
    return { name: "login", query: { redirect: to.fullPath } };
  }

  // For unauthenticated landing pages, redirect already-signed-in users home.
  if (
    (to.name === "login" || to.name === "register" || to.name === "forget-password") &&
    auth.session
  ) {
    return { name: "home" };
  }

  // Re-validate before rendering /login — defends against the case where the
  // user comes back to /login with a snapshot that's been revoked server-side
  // since cold start. checkLogin emits session-expired on failure, which the
  // auth store listens for.
  if (to.name === "login") {
    await api.checkLogin().catch(() => undefined);
  }

  return undefined;
});
