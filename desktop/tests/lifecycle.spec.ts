import { test, expect, type Page } from '@playwright/test';
test.use({ channel: 'chrome', viewport: { width: 1440, height: 1000 } });
const base = process.env.SUFE_TEST_URL || 'http://127.0.0.1:1420';

async function setup(page: Page, flags: Record<string, boolean> = {}) {
  await page.addInitScript((flags) => {
    const w = window as any;
    w.__flags = flags;
    w.__calls = [];
    w.__holds = {};
    w.__pending = {};
    w.__listeners = {};
    w.__hold = (command: string) => { w.__holds[command] = true; };
    w.__release = (command: string, value: unknown) => {
      delete w.__holds[command];
      for (const resolve of w.__pending[command] || []) resolve(value);
      w.__pending[command] = [];
    };
    w.__emit = (name: string, detail: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }));
  }, flags);
  // The mock transport exists solely in the browser test; production has no
  // state injection API and these commands never reach a real backend.
  await page.route('**/src/platform.ts*', route => route.fulfill({
    contentType: 'application/javascript',
    body: `
      import { previewInvoke } from '/src/preview/fixtures.ts';
      export const isDesktop = false, isPreview = true;
      export async function invoke(command, args = {}) {
        window.__calls.push({command, args});
        if (window.__holds[command]) return new Promise(resolve => (window.__pending[command] ||= []).push(resolve));
        if (command === 'logout') { window.__emit('xboard://session-expired', {}); return; }
        const result = await previewInvoke(command, args);
        if (command === 'fetch_client_config') result.features = {...result.features, ...window.__flags};
        return result;
      }
      export async function listen(event, callback) {
        window.__listeners[event] = (window.__listeners[event] || 0) + 1;
        const handler = e => callback({event, id:0, payload:e.detail});
        window.addEventListener(event, handler);
        return () => { window.__listeners[event]--; window.removeEventListener(event, handler); };
      }
    `,
  }));
}
async function goto(page: Page, path: string) {
  await page.goto(`${base}/?preview=1#${path}`);
  await expect(page.locator('.app-layout')).toBeVisible();
}
async function setFlags(page: Page, flags: Record<string, boolean>) {
  await page.evaluate(async flags => {
    Object.assign((window as any).__flags, flags);
    const path = '/src/stores/features.ts';
    await (await import(path)).useFeaturesStore().ensure(true);
  }, flags);
}
async function pending(page: Page, command: string) {
  await expect.poll(() => page.evaluate(command => ((window as any).__pending[command] || []).length, command)).toBeGreaterThan(0);
}

test('disabled features block direct routes but keep existing orders accessible', async ({ page }) => {
  await setup(page, { purchase: false, tickets: false, custom_rules: false, notice: false, gift_card: false, invite: false });
  await goto(page, '/plans');
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.getByRole('link', { name: '订阅套餐', exact: true })).toHaveCount(0);
  await expect(page.getByRole('link', { name: '我的订单', exact: true })).toBeVisible();
  for (const path of ['/tickets', '/tickets/1', '/rules', '/notices']) {
    await page.evaluate(path => { location.hash = path; }, path);
    await expect(page).toHaveURL(/#\/$/);
  }
  await page.getByRole('link', { name: '我的订单', exact: true }).click();
  await expect(page).toHaveURL(/#\/orders$/);
  await page.evaluate(() => { location.hash = '/account'; });
  await expect(page.getByRole('heading', { name: '我的账户' })).toBeVisible();
  await expect(page.getByRole('link', { name: '管理订阅' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '查看奖励' })).toHaveCount(0);
});

test('server disabling purchase closes the active purchase flow', async ({ page }) => {
  await setup(page);
  await goto(page, '/plans');
  await page.locator('.plan-card').first().getByRole('button', { name: /选择此套餐|续费当前套餐/ }).click();
  await expect(page.getByRole('button', { name: '创建订单并核对', exact: true })).toBeVisible();
  await setFlags(page, { purchase: false });
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.getByRole('button', { name: '创建订单并核对', exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__calls.some((c: any) => c.command === 'save_order'))).toBe(false);
});

test('an obsolete gift-card preview cannot return after disabling and re-enabling the feature', async ({ page }) => {
  await setup(page);
  await goto(page, '/account');
  await page.evaluate(() => (window as any).__hold('gift_card_check'));
  await page.getByLabel('礼品卡兑换码').fill('TEST-CARD');
  await page.getByRole('button', { name: '查看奖励' }).click();
  await pending(page, 'gift_card_check');
  await setFlags(page, { gift_card: false });
  await setFlags(page, { gift_card: true });
  await page.evaluate(() => (window as any).__release('gift_card_check', { can_redeem: true, code_info: {}, reward_preview: { balance: 1000 } }));
  await expect(page.getByRole('button', { name: '查看奖励' })).toBeEnabled();
  await expect(page.getByRole('button', { name: '确认兑换' })).toHaveCount(0);
});

test('expired sessions discard late profile, subscription, node, plan, traffic and proxy responses', async ({ page }) => {
  await setup(page);
  await goto(page, '/account');
  await page.evaluate(async () => {
    const w = window as any;
    for (const command of ['current_user', 'current_subscribe', 'preview_subscribe_nodes', 'fetch_plans', 'current_traffic', 'proxies']) w.__hold(command);
    const a = '/src/stores/auth.ts', n = '/src/stores/nodes.ts', p = '/src/stores/plan.ts';
    const auth = (await import(a)).useAuthStore();
    void auth.refreshUser(); void auth.refreshSubscribe();
    void (await import(n)).useNodesStore().refresh();
    void (await import(p)).usePlanStore().ensure(true);
    w.__emit('xboard://connection-state', { kind: 'connected', mode: 'system_proxy', since: '2026-01-01T00:00:00Z', mixed_port: 7890 });
  });
  await pending(page, 'current_traffic');
  await pending(page, 'proxies');
  await page.evaluate(() => {
    const w = window as any;
    w.__emit('xboard://session-expired', {});
    w.__release('current_user', { email: 'old@example.invalid', balance: 9000 });
    w.__release('current_subscribe', { plan_id: 999 });
    w.__release('preview_subscribe_nodes', [{name:'OLD',kind:'ss',server:'old.invalid',port:443}]);
    w.__release('fetch_plans', [{id:999,name:'OLD'}]);
    w.__release('current_traffic', {up:99,down:99,up_total:99,down_total:99});
    w.__release('proxies', [{name:'OLD',now:'OLD',all:[]}]);
  });
  await expect(page).toHaveURL(/#\/login$/);
  const snapshot = await page.evaluate(async () => {
    const a='/src/stores/auth.ts', c='/src/stores/connection.ts', n='/src/stores/nodes.ts', p='/src/stores/plan.ts';
    const auth=(await import(a)).useAuthStore(), conn=(await import(c)).useConnectionStore();
    return { session:auth.session, user:auth.userInfo, subscribe:auth.subscribe, state:conn.state.kind, traffic:conn.traffic, history:conn.trafficHistory, proxies:conn.proxies, nodes:(await import(n)).useNodesStore().raw, plans:(await import(p)).usePlanStore().plans, listeners:(window as any).__listeners['xboard://connection-state'] };
  });
  expect(snapshot).toEqual({ session:null,user:null,subscribe:null,state:'disconnected',traffic:{up:0,down:0,up_total:0,down_total:0},history:[],proxies:[],nodes:[],plans:[],listeners:0 });
});

test('logout during kernel initialization does not resurrect the connection and disconnect runs after a late connect', async ({ page }) => {
  await setup(page);
  await goto(page, '/account');
  await page.evaluate(async () => {
    (window as any).__hold('connect');
    const p = '/src/stores/connection.ts';
    void (await import(p)).useConnectionStore().connect();
  });
  await pending(page, 'connect');
  await page.evaluate(() => (window as any).__emit('xboard://session-expired', {}));
  await expect(page).toHaveURL(/#\/login$/);
  await page.evaluate(() => (window as any).__release('connect', {kind:'connected',mode:'system_proxy',since:'2026-01-01T00:00:00Z',mixed_port:7890}));
  await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((c:any) => c.command === 'disconnect').length)).toBe(1);
  expect(await page.evaluate(async () => {
    const p='/src/stores/connection.ts'; return (await import(p)).useConnectionStore().state.kind;
  })).toBe('disconnected');
  const connectCount = await page.evaluate(() => (window as any).__calls.filter((c:any) => c.command === 'connect').length);
  await page.evaluate(() => (window as any).__emit('tray://connect', {}));
  expect(await page.evaluate(() => (window as any).__calls.filter((c:any) => c.command === 'connect').length)).toBe(connectCount);
});

test('a connection snapshot returned after dispose cannot restart listeners or traffic', async ({ page }) => {
  await setup(page);
  await goto(page, '/account');
  await page.evaluate(async () => {
    const p='/src/stores/connection.ts', conn=(await import(p)).useConnectionStore();
    await conn.dispose();
    (window as any).__hold('connection_state');
    void conn.hydrate();
  });
  await pending(page, 'connection_state');
  await page.evaluate(() => {
    const w=window as any;
    w.__emit('xboard://session-expired', {});
    w.__release('connection_state', {kind:'connected',mode:'system_proxy',since:'2026-01-01T00:00:00Z',mixed_port:7890});
  });
  await expect(page).toHaveURL(/#\/login$/);
  expect(await page.evaluate(async () => {
    const p='/src/stores/connection.ts';
    return {state:(await import(p)).useConnectionStore().state.kind,listeners:(window as any).__listeners['xboard://connection-state']};
  })).toEqual({state:'disconnected',listeners:0});
});
