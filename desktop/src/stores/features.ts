import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@/platform';
export type Feature = 'purchase'|'recharge'|'gift_card'|'checkin'|'notice'|'invite'|'tickets'|'chatwoot'|'custom_rules';
export interface ClientConfig { brand_name: string; features: Record<Feature, boolean>; chatwoot_base_url: string|null; chatwoot_inbox_identifier: string|null; config_source: string; transport_mode: string; api_candidates: number }
export const useFeaturesStore = defineStore('features', () => {
  const config = ref<ClientConfig|null>(null);
  const loading = ref(false);
  const error = ref('');
  let fetchedAt = 0;
  let pending: Promise<void>|null = null;
  // Do not expose a write flow before the trusted configuration has loaded.
  const defaults: Record<Feature, boolean> = { purchase:false, recharge:false, gift_card:false, checkin:false, notice:false, invite:false, tickets:false, chatwoot:false, custom_rules:false };
  function enabled(key: Feature) { return config.value?.features[key] ?? defaults[key]; }
  async function ensure(force = false): Promise<void> {
    if (pending) return pending;
    if (!force && config.value && Date.now()-fetchedAt < 60000) return;
    pending = (async () => { loading.value = true; try { config.value = await invoke<ClientConfig>('fetch_client_config'); fetchedAt=Date.now(); error.value=''; } catch { error.value='暂时无法更新客户端配置'; } finally { loading.value=false; pending=null; } })();
    return pending;
  }
  return { config, loading, error, enabled, ensure };
});
