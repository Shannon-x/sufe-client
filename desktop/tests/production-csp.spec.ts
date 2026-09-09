import { test, expect } from '@playwright/test';
import { createServer, type Server } from 'node:http';
import { readFile } from 'node:fs/promises';
import { resolve, sep, extname } from 'node:path';

// Run after `npm run build`. This deliberately serves dist, without Vite or
// preview fixtures, and uses the exact policy shipped in the native app.
test.use({ channel: 'chrome' });
const desktop = process.cwd();
const dist = resolve(desktop, 'dist');
let server: Server;
let origin: string;
let csp: string;
const mime: Record<string, string> = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png',
  '.ico': 'image/x-icon', '.woff2': 'font/woff2', '.woff': 'font/woff',
};

test.beforeAll(async () => {
  const config = JSON.parse(await readFile(resolve(desktop, 'src-tauri/tauri.conf.json'), 'utf8'));
  csp = config.app.security.csp;
  expect(csp).not.toContain("'unsafe-eval'");
  // Fail immediately if the caller forgot to build, rather than serving dev code.
  await readFile(resolve(dist, 'index.html'));
  server = createServer(async (req, res) => {
    res.setHeader('Content-Security-Policy', csp);
    res.setHeader('Cache-Control', 'no-store');
    const pathname = new URL(req.url ?? '/', 'http://localhost').pathname;
    if (pathname === '/csp-probe.js') {
      res.setHeader('Content-Type', mime['.js']);
      res.end("try { new Function('return 1')(); document.body.dataset.eval = 'allowed'; } catch (e) { document.body.dataset.eval = e.name; }");
      return;
    }
    if (pathname === '/csp-probe.html') {
      res.setHeader('Content-Type', mime['.html']);
      res.end('<!doctype html><body><script src="/csp-probe.js"></script></body>');
      return;
    }
    try {
      const file = resolve(dist, `.${decodeURIComponent(pathname === '/' ? '/index.html' : pathname)}`);
      if (!file.startsWith(dist + sep)) { res.writeHead(403).end(); return; }
      const content = await readFile(file);
      res.setHeader('Content-Type', mime[extname(file)] ?? 'application/octet-stream');
      res.end(content);
    } catch { res.writeHead(404).end(); }
  });
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('No test server address');
  origin = `http://127.0.0.1:${address.port}`;
});

test.afterAll(async () => {
  if (server) await new Promise<void>((done, reject) => server.close((error) => error ? reject(error) : done()));
});

for (const form of [
  { route: 'login', heading: '欢迎回来', submit: '登 录', fields: 2, validation: '请填写所有字段' },
  { route: 'register', heading: '注册新账号', submit: '注 册', fields: 3, validation: '请填写所有必填字段' },
  { route: 'forget-password', heading: '找回密码', submit: '重置密码', fields: 4, validation: '请填写所有必填字段' },
]) {
  test(`production CSP renders and validates ${form.route}`, async ({ page }) => {
    const errors: string[] = [];
    const violations: string[] = [];
    page.on('pageerror', (error) => errors.push(String(error)));
    page.on('console', (message) => {
      if (/unsafe-eval|EvalError|Content Security Policy/i.test(message.text())) violations.push(message.text());
    });
    const response = await page.goto(`${origin}/#/${form.route}`);
    expect(response?.headers()['content-security-policy']).toBe(csp);
    await expect(page.getByRole('heading', { name: form.heading, exact: true })).toBeVisible();
    await expect(page.locator('form input')).toHaveCount(form.fields);
    await expect(page.getByPlaceholder('user@example.com')).toBeVisible();
    await page.getByRole('button', { name: form.submit, exact: true }).click();
    await expect(page.getByText(form.validation, { exact: true }).first()).toBeVisible();
    expect(errors).toEqual([]);
    expect(violations).toEqual([]);
  });
}

test('production CSP still blocks dynamic code execution', async ({ page }) => {
  await page.goto(`${origin}/csp-probe.html`);
  // A real same-origin script attempts Function(). Playwright evaluate itself
  // can bypass CSP, so it would not prove that the page's policy is enforced.
  await expect(page.locator('body')).toHaveAttribute('data-eval', 'EvalError');
});
