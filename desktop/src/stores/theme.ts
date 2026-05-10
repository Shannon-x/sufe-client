import { defineStore } from "pinia";
import { ref, watch } from "vue";

const KEY = "xboard.theme";

export const useThemeStore = defineStore("theme", () => {
  const initial = localStorage.getItem(KEY);
  const dark = ref<boolean>(
    initial === null
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
      : initial === "dark",
  );

  watch(dark, (next) => {
    localStorage.setItem(KEY, next ? "dark" : "light");
  });

  function toggle() {
    dark.value = !dark.value;
  }

  return { dark, toggle };
});
