import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "@/api";
import type { SiteConfig } from "@/types";

const TTL_MS = 5 * 60 * 1000;

// Holds the cached `/guest/comm/config` payload across login / register /
// forget-password navigations. Each page calls `ensure()` on setup; only
// the first call within `TTL_MS` actually hits the backend.
export const useSiteStore = defineStore("site", () => {
  const config = ref<SiteConfig | null>(null);
  const fetchedAt = ref(0);
  const loading = ref(false);
  const error = ref<unknown>(null);

  async function ensure(force = false): Promise<SiteConfig | null> {
    if (!force && config.value && Date.now() - fetchedAt.value < TTL_MS) {
      return config.value;
    }
    loading.value = true;
    error.value = null;
    try {
      config.value = await api.fetchSiteConfig();
      fetchedAt.value = Date.now();
      return config.value;
    } catch (e) {
      error.value = e;
      return config.value;
    } finally {
      loading.value = false;
    }
  }

  return { config, fetchedAt, loading, error, ensure };
});
