import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { useAuthStore } from "./auth";
export const useNodePreferences = defineStore("nodePreferences", () => {
  const auth = useAuthStore();
  const revision = ref(0);
  const latency = ref<Record<string, number | null>>({});
  const key = computed(() => `sufe.nodes.${auth.session?.email || "guest"}`);
  function read(): { favorite: string[]; selected: string | null } {
    revision.value;
    try {
      return {
        ...{ favorite: [], selected: null },
        ...JSON.parse(localStorage.getItem(key.value) || "{}"),
      };
    } catch {
      return { favorite: [], selected: null };
    }
  }
  const favorites = computed(() => read().favorite);
  const selected = computed(() => read().selected);
  function save(next: ReturnType<typeof read>) {
    localStorage.setItem(key.value, JSON.stringify(next));
    revision.value++;
  }
  function toggleFavorite(name: string) {
    const v = read();
    v.favorite = v.favorite.includes(name)
      ? v.favorite.filter((n) => n !== name)
      : [...v.favorite, name];
    save(v);
  }
  function select(name: string) {
    save({ ...read(), selected: name });
  }
  return { latency, favorites, selected, toggleFavorite, select };
});
