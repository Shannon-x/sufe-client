// Runs the actual HTML/JavaScript snippets embedded in CaptchaWidget.kt.
// The official provider scripts are replaced only at this test's HTTP
// boundary. This does not bypass a real challenge or contact a live backend.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '../../desktop/node_modules/playwright/index.mjs';

const source = await readFile(new URL('../app/src/main/kotlin/com/xboard/client/ui/components/CaptchaWidget.kt', import.meta.url), 'utf8');
const body = source.slice(source.indexOf('private fun captchaHtml'));
const template = body.match(/return """(<!doctype html>[\s\S]*?)"""/)[1];
const report = body.match(/val callback = "([^"]+)"/)[1];
const fragments = [
  ['turnstile', body.match(/"turnstile" -> """([\s\S]*?)"""\.trimIndent/)[1], 'https://challenges.cloudflare.com/turnstile/v0/api.js'],
  ['recaptcha-v3', body.match(/"recaptcha-v3" -> """([\s\S]*?)"""\.trimIndent/)[1], 'https://www.recaptcha.net/recaptcha/api.js'],
  ['recaptcha', body.match(/else -> """([\s\S]*?)"""\.trimIndent/)[1], 'https://www.recaptcha.net/recaptcha/api.js'],
];
const browser = await chromium.launch({ channel: 'chrome' });
try {
  for (const [kind, fragment, url] of fragments) {
    const page = await browser.newPage();
    const key = 'public-test-key</script><script>window.injected=true</script>';
    const script = fragment.replaceAll('$key', JSON.stringify(key).replaceAll('<', '\\u003c'));
    const html = template.replace('$callback', report).replace('$script', script).replace('$url', url);
    await page.route('https://brand.example.invalid/**', route => route.fulfill({ contentType: 'text/html', body: html }));
    await page.route(url, route => route.fulfill({ contentType: 'application/javascript', body: `
      window.providerCalls = [];
      const provider = {
        render: (id, options) => { window.options = options; providerCalls.push(['render', id, options.sitekey]); return 7; },
        reset: id => providerCalls.push(['reset', id]),
        ready: callback => callback(),
        execute: (key, options) => { providerCalls.push(['execute', key, options.action]); return Promise.resolve('one-use-test-token'); }
      };
      window.turnstile = window.grecaptcha = provider;
      window.loaded();
    ` }));
    await page.goto('https://brand.example.invalid/__sufe_captcha__/');
    await page.waitForFunction(() => window.__sufeResult?.kind === 'ready');
    assert.equal(await page.evaluate(() => window.injected), undefined, 'site keys cannot inject HTML');
    if (kind === 'recaptcha-v3') {
      assert.deepEqual(await page.evaluate(() => window.providerCalls), [], 'v3 must not execute on page load');
      await page.evaluate(() => window.__sufeExecute('send_email'));
      await page.waitForFunction(() => window.__sufeResult?.kind === 'token');
      const calls = await page.evaluate(() => window.providerCalls);
      assert.equal(calls[0][2], 'send_email');
      assert.equal(calls[0][1], key);
    } else {
      assert.deepEqual((await page.evaluate(() => window.providerCalls))[0], ['render', kind === 'turnstile' ? '#challenge' : 'challenge', key]);
      await page.evaluate(() => window.options.callback('one-use-test-token'));
      assert.equal(await page.evaluate(() => window.__sufeResult.token), 'one-use-test-token');
      await page.evaluate(() => window.__sufeReset());
      assert.deepEqual((await page.evaluate(() => window.providerCalls)).at(-1), ['reset', 7]);
      await page.evaluate(() => window.options['expired-callback']());
      assert.equal(await page.evaluate(() => window.__sufeResult.kind), 'expired');
    }
    await page.route('https://evil.example.invalid/**', route => route.fulfill({ contentType: 'application/javascript', body: 'window.evil=true' }));
    await page.evaluate(() => new Promise(resolve => {
      const script = document.createElement('script'); script.src = 'https://evil.example.invalid/inject.js';
      script.onload = script.onerror = () => resolve(); document.head.append(script);
    }));
    assert.equal(await page.evaluate(() => window.evil), undefined, 'CSP must reject untrusted scripts');
    await page.close();
    process.stdout.write(`PASS ${kind}: render/execute, one-use result, injection boundary\n`);
  }
} finally { await browser.close(); }
