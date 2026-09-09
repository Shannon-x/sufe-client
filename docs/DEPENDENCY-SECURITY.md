# 前端依赖安全维护记录

检查日期：2026-09-09。工作目录：`desktop`。本次未使用 `npm audit fix --force`。

## 升级范围

| 依赖 | 处理结果 |
| --- | --- |
| Vite | 5.4.21 → 7.3.6；声明使用 `~7.3.6`，接收同维护分支的补丁 |
| @vitejs/plugin-vue | 5.2.x → 6.0.8，与 Vite 7、Vue 3 的 peer 范围匹配 |
| vue-i18n / @intlify | 9.14.4 → 9.14.5，应用现有 API 的兼容安全补丁 |
| PostCSS | 8.5.14 → 8.5.28 |
| nanoid | 3.3.12 → 3.3.18 |
| brace-expansion | 2.1.0 → 2.1.4 |
| maplibre-gl / pmtiles / fflate | 新地图使用本地 world-atlas SVG，已无这些库的引用，因此移除 |
| topojson-client / world-atlas / vfonts | 新地图与本地字体仍然使用，保留 |

根据 [Vite 官方维护列表](https://vite.dev/releases)，7.3 分支仍接收重要修复和安全补丁。已检查 [Vite 6 迁移说明](https://v6.vite.dev/guide/migration.html) 与 [Vite 7 迁移说明](https://v7.vite.dev/guide/migration.html)；项目不使用被移除的 SSR、Sass legacy 或 splitVendorChunk API。现有显式浏览器目标 `chrome105` / `safari14` 保持不变，避免默认浏览器目标升级影响 Tauri WebView 支持。

Vite 官方最低要求为 Node 20.19+ 或 22.12+。项目声明 Node `^22.12.0 || >=24.0.0`，本地使用 Node 24.20.0，CI 统一使用 Node 24。

## 验证

`npm audit --json`：9 项漏洞降为 0 项（包括原来的 1 项 critical 和 4 项 high）。这一结果只代表检查当时 npm 公告库中已知的依赖漏洞，不替代应用安全验证。

生产构建和 TypeScript 检查通过；金额测试 3 项通过；购买及客服 Playwright 回归测试 7 项通过，使用本机 Chrome 的独立测试浏览器运行。测试文件为 `desktop/tests/commerce.spec.ts`。

## 后续维护

vue-i18n 9 已结束官方常规支持。本次采用 9.14.5 的已发布安全补丁，避免将完整的国际化 API 迁移混入安全锁文件更新。下一次国际化调整时应按[官方维护说明](https://vue-i18n.intlify.dev/guide/maintenance.html)迁移至受维护主版本，并验证全部中英文文案与日期/金额格式。

