<script setup lang="ts">
withDefaults(defineProps<{ name: string; size?: number | string }>(), {
  size: 20,
});
const paths: Record<string, string> = {
  home: "M3 10 12 3l9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z",
  globe:
    "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18Z",
  grid: "M3 3h7v7H3ZM14 3h7v7h-7ZM3 14h7v7H3ZM14 14h7v7h-7Z",
  shield: "m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6Zm-4 9 3 3 5-6",
  power: "M12 3v9M6.4 5.8a9 9 0 1 0 11.2 0",
  lightning: "m13 2-9 12h7l-1 8 10-13h-7Z",
  arrow: "M5 12h14m-5-5 5 5-5 5",
  chevron: "m9 5 7 7-7 7",
  down: "M12 3v16m-6-6 6 6 6-6",
  up: "M12 21V5m-6 6 6-6 6 6",
  clock: "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM12 7v5l3 2",
  check: "m5 12 4 4L19 6",
  close: "m6 6 12 12M6 18 18 6",
  refresh:
    "M20 7V3m0 4h-4M4 17v4m0-4h4M5 8a8 8 0 0 1 13-3l2 2M4 17l2 2a8 8 0 0 0 13-3",
  search: "M16 10a6 6 0 1 1-12 0 6 6 0 0 1 12 0Zm-1 5 6 6",
  star: "m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z",
  signal: "M4 20v-4m5 4v-8m5 8V8m5 12V4",
  bell: "M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9ZM10 21h4",
  wallet: "M20 7V4H5a2 2 0 0 0 0 4h16v12H5a2 2 0 0 1-2-2V6m18 6h-6v4h6",
  bag: "M4 7h16l1 14H3ZM8 8V6a4 4 0 0 1 8 0v2",
  card: "M3 5h18v14H3ZM3 10h18M7 15h3",
  gift: "M3 9h18v4H3Zm2 4v8h14v-8M12 9v12M12 9C3 10 5 1 9 4l3 5Zm0 0c9 1 7-8 3-5Z",
  users:
    "M15 7a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM6 21v-3a6 6 0 0 1 12 0v3M19 4a3 3 0 0 1 0 6M21 21v-3a6 6 0 0 0-2-4M5 4a3 3 0 0 0 0 6M3 21v-3a6 6 0 0 1 2-4",
  user: "M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM4 21v-2a8 8 0 0 1 16 0v2",
  chat: "M21 12a9 9 0 0 1-9 9 11 11 0 0 1-4-1l-5 1 1-5a11 11 0 0 1-1-4 9 9 0 0 1 18 0ZM8 12h.01M12 12h.01M16 12h.01",
  headphones:
    "M3 14v-3a9 9 0 0 1 18 0v7a3 3 0 0 1-3 3h-4M3 12h4v7H3ZM17 12h4v7h-4Z",
  settings:
    "m9 3-1 3-3 1-2 3 2 2-1 3 2 3h3l2 3 3-1 1-3 3-1 2-3-2-2 1-3-2-3h-3l-2-3Zm6 9a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
  rules:
    "M8 3v5a4 4 0 0 0 4 4h4M8 21v-4a5 5 0 0 1 5-5h3M4 3h8M4 21h8m5-13 4 4-4 4",
  activity: "M3 12h4l3-8 4 16 3-8h4",
  terminal: "m4 5 6 6-6 6m9 0h7",
  moon: "M20 15A9 9 0 0 1 9 4a9 9 0 1 0 11 11Z",
  sun: "M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM12 2v2m0 16v2M2 12h2m16 0h2M5 5l1 1m12 12 1 1M5 19l1-1M18 6l1-1",
  logout: "M10 3H4v18h6m4-15 6 6-6 6m-6-6h12",
  link: "m10 14 4-4M8 16l-1 1a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0m0 2 1-1a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0",
  copy: "M8 8h13v13H8ZM16 4V2H2v14h2",
  plus: "M12 4v16M4 12h16",
  trash: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7",
  info: "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM12 11v6m0-10v.01",
  lock: "M5 10h14v11H5ZM8 10V6a4 4 0 0 1 8 0v4M12 14v3",
  menu: "M4 6h16M4 12h16M4 18h16",
  send: "m3 3 19 9-19 9 4-9Zm4 9h15",
};
</script>
<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="paths[name] || paths.globe" />
  </svg>
</template>
