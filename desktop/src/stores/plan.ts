import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api } from "@/api";
import type { Plan } from "@/types";

// Mirrors the cadence of useSiteStore — plans don't change often, and the
// Home / Plans pages both hit `ensure()` on mount. 5 minutes is loose enough
// that the panel admin can roll out price changes without users having to
// log out, but tight enough that a stale catalog won't linger past a single
// session.
const TTL_MS = 5 * 60 * 1000;

// Cached `/api/v1/user/plan/fetch` plus an id→Plan index. The index lets
// Home.vue translate the bare `plan_id` integer it gets from `current_user`
// into a human-readable plan name without a second round trip.
export const usePlanStore = defineStore("plan", () => {
  const plans = ref<Plan[]>([]);
  const fetchedAt = ref(0);
  const loading = ref(false);
  const error = ref<unknown>(null);

  const byId = computed<Map<number, Plan>>(() => {
    const m = new Map<number, Plan>();
    for (const p of plans.value) m.set(p.id, p);
    return m;
  });

  function nameFor(id: number | null | undefined): string | null {
    if (id == null) return null;
    return byId.value.get(id)?.name ?? null;
  }

  async function ensure(force = false): Promise<Plan[]> {
    if (!force && plans.value.length > 0 && Date.now() - fetchedAt.value < TTL_MS) {
      return plans.value;
    }
    loading.value = true;
    error.value = null;
    try {
      plans.value = await api.fetchPlans();
      fetchedAt.value = Date.now();
      return plans.value;
    } catch (e) {
      error.value = e;
      return plans.value;
    } finally {
      loading.value = false;
    }
  }

  function reset() {
    plans.value = [];
    fetchedAt.value = 0;
    error.value = null;
  }

  return { plans, fetchedAt, loading, error, byId, nameFor, ensure, reset };
});
