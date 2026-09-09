// Read-only smoke of native WebView2 and embedded assets, using an explicit
// diagnostic build on elevated Windows hosts when WebView2 ignores env flags.
// It never signs in, creates an
// order, establishes a tunnel, or changes the operating-system proxy.
import { chromium, expect } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
const root = path.resolve(import.meta.dirname, '../..');
const info = JSON.parse((await readFile(path.join(root, '.tools/native-smoke-current.json'), 'utf8')).replace(/^\uFEFF/, ''));
let browser;
let lastError;
for (let attempt = 0; attempt < 40; attempt++) {
  try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${info.port}`, { timeout: 2000 }); break; }
  catch (error) { lastError = error; await new Promise(resolve => setTimeout(resolve, 500)); }
}
if (!browser) throw lastError;
const context = browser.contexts()[0];
let page = context.pages().find(p => p.url().includes('tauri.localhost')) || context.pages()[0];
if (!page) page = await context.waitForEvent('page', { timeout: 15000 });
const failures = [];
page.on('pageerror', error => failures.push(error.message));
page.on('console', message => { if (message.type() === 'error') failures.push(message.text()); });
try {
  await page.reload();
  await page.waitForFunction(() => !!document.querySelector('#app')?.textContent?.trim(), undefined, { timeout: 30000 });
  await page.waitForFunction(() => typeof window.__TAURI_INTERNALS__?.invoke === 'function');
  const state = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('connection_state'));
  if (state.kind !== 'disconnected') throw new Error('Smoke requires a disconnected client; no connection state was changed by this script.');
  const config = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('fetch_client_config'));
  const guest = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('fetch_site_config'));
  await expect(page.getByRole('heading', { name: '欢迎回来', exact: true })).toBeVisible({ timeout: 30000 });
  await expect(page.getByPlaceholder('user@example.com')).toBeVisible();
  await expect(page.getByText('界面预览 · 当前为示例数据，不建立真实网络连接或订单。')).toHaveCount(0);
  await expect(page.getByText('当前为浏览器界面，请使用桌面客户端登录连接。')).toHaveCount(0);
  await page.screenshot({ path: path.join(root, 'artifacts/sufe-native-windows.png'), fullPage: true });
  await page.getByRole('button', { name: /^登\s*录$/ }).click();
  await expect(page.getByText('请填写所有字段').first()).toBeVisible();
  await page.getByRole('button', { name: '注册账号', exact: true }).click();
  await expect(page.getByPlaceholder('user@example.com')).toBeVisible();
  await expect(page.getByRole('button', { name: /^注\s*册$/ })).toBeVisible();
  await page.evaluate(() => { window.location.hash = '#/forget-password'; });
  await expect(page.getByRole('button', { name: '重置密码', exact: true })).toBeVisible();
  await expect(page.getByPlaceholder('user@example.com')).toBeVisible();
  if (failures.length) throw new Error(failures.join('\n'));
  const result = { nativeBridge: true, embeddedAssets: true, diagnosticBuild: info.diagnosticBuild === true, connectionState: state.kind,
    publicBackendConfig: !!guest, transportMode: config.transport_mode, apiCandidates: config.api_candidates,
    featureNames: Object.keys(config.features), previewFixtures: false, runtimeErrors: failures,
    authPages: ['login', 'register', 'forget-password'], emptyLoginValidation: true,
    realLoginTested: false, paymentSubmitted: false, tunnelStarted: false };
  await writeFile(path.join(root, 'artifacts/native-smoke.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} catch (error) {
  await page.screenshot({ path: path.join(root, 'artifacts/sufe-native-failure.png'), fullPage: true }).catch(() => {});
  console.error(JSON.stringify({ runtimeErrors: failures }));
  throw error;
} finally {
  // Application exit passes through the normal Rust cleanup hook. Closing the
  // window alone would intentionally leave the tray process running.
  await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('plugin:process|exit', { code: 0 })).catch(() => {});
  await browser.close().catch(() => {});
}
