import { test, expect, type Page } from "@playwright/test";
test.use({ channel: "chrome", viewport: { width: 1440, height: 1000 } });

const baseUrl = process.env.SUFE_TEST_URL || "http://127.0.0.1:1420";
// Test-only replacement at the HTTP boundary. No mock hooks are compiled
// into the production desktop bundle and no live panel receives a write.
async function scenario(
  page: Page,
  kind: "preview" | "free" | "pending" | "coupon" | "support",
) {
  await page.addInitScript(
    ({ kind }) => {
      const w = window as any;
      w.__commerceCalls = [];
      w.__paymentStatus = 1;
      w.__commerceInvoke = async (
        command: string,
        args: Record<string, unknown>,
      ) => {
        w.__commerceCalls.push({ command, args });
        if (kind === "preview" || kind === "support") return { handled: false };
        const methods = [
          {
            id: 1,
            name: "支付宝",
            payment: "alipay",
            handling_fee_fixed: 25,
            handling_fee_percent: 2.5,
          },
          {
            id: 2,
            name: "微信支付",
            payment: "wechat",
            handling_fee_fixed: 75,
            handling_fee_percent: 0,
          },
        ];
        const order = {
          id: 27,
          trade_no: "TEST20260909",
          plan_id: 1,
          period: "month_price",
          type: 1,
          status: 0,
          total_amount: kind === "free" ? 0 : 1000,
          balance_amount: kind === "free" ? 1500 : 0,
          discount_amount: 0,
          surplus_amount: 0,
          created_at: 1788912000,
          payment_id: kind === "pending" ? 2 : null,
          handling_amount: kind === "pending" ? 75 : null,
          plan: { name: "轻享计划" },
        };
        if (command === "fetch_payment_methods")
          return { handled: true, value: kind === "free" ? [] : methods };
        if (command === "fetch_orders")
          return { handled: true, value: kind === "pending" ? [order] : [] };
        if (command === "fetch_order") return { handled: true, value: order };
        if (command === "save_order")
          return { handled: true, value: order.trade_no };
        if (command === "checkout_order")
          return {
            handled: true,
            value:
              kind === "free"
                ? { type: -1, data: true }
                : { type: 0, data: "https://pay.example.invalid/test" },
          };
        if (command === "check_order")
          return { handled: true, value: w.__paymentStatus };
        if (command === "check_coupon")
          return {
            handled: true,
            value: { id: 1, code: "YEAR25", type: 2, value: 25 },
          };
        if (command === "cancel_order")
          throw new Error(
            "A pending order must not be automatically cancelled",
          );
        return { handled: false };
      };
    },
    { kind },
  );
  await page.route("**/src/platform.ts*", async (route) => {
    await route.fulfill({
      contentType: "application/javascript",
      body: `
      import { previewInvoke } from '/src/preview/fixtures.ts';
      export const isDesktop = false;
      export const isPreview = true;
      export async function invoke(command, args = {}) { const result = await window.__commerceInvoke(command, args); if (result.handled) return result.value; return previewInvoke(command, args); }
      export async function listen(event, callback) { const handler = e => callback({ event, id: 0, payload: e.detail }); window.addEventListener(event, handler); return () => window.removeEventListener(event, handler); }
    `,
    });
  });
}
async function goto(page: Page, path: string) {
  await page.goto(`${baseUrl}/?preview=1#${path}`);
}
async function openFirstPlan(page: Page) {
  await page
    .locator(".plan-card")
    .first()
    .getByRole("button", { name: /选择此套餐|续费当前套餐/ })
    .click();
}

test("preview purchasing is visibly isolated and cannot place a real order", async ({
  page,
}) => {
  await scenario(page, "preview");
  await goto(page, "/plans");
  await expect(page.locator(".preview-banner")).toContainText("示例数据");
  await expect(page.locator(".plan-card")).toHaveCount(3);
  await openFirstPlan(page);
  await page
    .getByRole("button", { name: "创建订单并核对", exact: true })
    .click();
  await expect(page.locator(".payment-error")).toContainText("界面预览");
  await expect(
    page.getByRole("button", { name: "前往订单中心查看已有订单 →" }),
  ).toBeVisible();
});

test("balance-covered order needs no payment channel and waits for actual activation", async ({
  page,
}) => {
  await scenario(page, "free");
  await goto(page, "/plans");
  await openFirstPlan(page);
  await page
    .getByRole("button", { name: "创建订单并核对", exact: true })
    .click();
  const activate = page.getByRole("button", { name: "立即开通", exact: true });
  await expect(activate).toBeEnabled();
  await activate.click();
  await expect(
    page.getByText("付款已确认，正在开通", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("订阅已就绪", { exact: true })).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as any).__commerceCalls.find(
          (c: any) => c.command === "checkout_order",
        )?.args.method,
    ),
  ).toBe(0);
  await page.evaluate(() => {
    (window as any).__paymentStatus = 3;
  });
  await page.getByRole("button", { name: "我已付款，检查状态" }).click();
  await expect(page.getByText("订阅已就绪", { exact: true })).toBeVisible();
});

test("pending order restores original channel and authoritative fee without creating or cancelling", async ({
  page,
}) => {
  await scenario(page, "pending");
  await goto(page, "/orders");
  await page.getByRole("button", { name: "继续支付", exact: true }).click();
  await expect(
    page.locator(".payment-methods").getByRole("button", { name: /支付宝/ }),
  ).toBeDisabled();
  await expect(
    page.locator(".payment-methods").getByRole("button", { name: /微信支付/ }),
  ).toBeEnabled();
  const pay = page.getByRole("button", {
    name: "确认支付 ¥10.75",
    exact: true,
  });
  await expect(pay).toBeEnabled();
  await pay.click();
  await expect(page.locator(".pending canvas")).toBeVisible();
  const calls = await page.evaluate(() => (window as any).__commerceCalls);
  expect(
    calls.filter((c: any) =>
      ["save_order", "cancel_order"].includes(c.command),
    ),
  ).toEqual([]);
  expect(calls.find((c: any) => c.command === "checkout_order").args).toEqual({
    tradeNo: "TEST20260909",
    method: 2,
  });
  await page.getByRole("button", { name: "稍后继续", exact: true }).click();
  await expect(page.locator(".purchase-modal")).toHaveCount(0);
});

test("coupon preview includes the selected billing period", async ({
  page,
}) => {
  await scenario(page, "coupon");
  await goto(page, "/plans");
  await page
    .locator(".plan-card")
    .first()
    .getByRole("button", { name: "年付", exact: true })
    .click();
  await openFirstPlan(page);
  await page.getByPlaceholder("输入优惠码，自动验证").fill("YEAR25");
  await expect(page.locator(".coupon-field")).toContainText("已优惠 ¥37.50");
  const request = await page.evaluate(() =>
    (window as any).__commerceCalls.find(
      (c: any) => c.command === "check_coupon",
    ),
  );
  expect(request.args).toEqual({
    code: "YEAR25",
    planId: 1,
    period: "year_price",
  });
});

test("unconfigured native support shows honest fallback, never a fake conversation", async ({
  page,
}) => {
  await scenario(page, "support");
  await goto(page, "/support");
  await expect(page.getByText("帮助就在这里", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: /联系工单支持/ }),
  ).toBeVisible();
  await expect(page.getByPlaceholder("输入消息，我们会认真倾听…")).toHaveCount(
    0,
  );
  const calls = await page.evaluate(() => (window as any).__commerceCalls);
  expect(calls.filter((c: any) => c.command === "support_request")).toEqual([]);
});

test("payment checks stop after ten minutes while the order remains resumable", async ({
  page,
}) => {
  await scenario(page, "pending");
  await goto(page, "/orders");
  await page.clock.install();
  await page.getByRole("button", { name: "继续支付", exact: true }).click();
  await page
    .getByRole("button", { name: "确认支付 ¥10.75", exact: true })
    .click();
  await expect(page.locator(".pending canvas")).toBeVisible();
  await page.clock.fastForward(10 * 60 * 1000 + 100);
  await expect(page.locator(".timeout-note")).toContainText("已暂停自动检查");
  const count = await page.evaluate(
    () =>
      (window as any).__commerceCalls.filter(
        (c: any) => c.command === "check_order",
      ).length,
  );
  await page.clock.fastForward(5 * 60 * 1000);
  expect(
    await page.evaluate(
      () =>
        (window as any).__commerceCalls.filter(
          (c: any) => c.command === "check_order",
        ).length,
    ),
  ).toBe(count);
  await expect(
    page.getByRole("button", { name: "稍后继续", exact: true }),
  ).toBeEnabled();
});

test("catalog and support stay usable at a phone-sized viewport", async ({
  page,
}) => {
  await scenario(page, "support");
  await page.setViewportSize({ width: 390, height: 844 });
  await goto(page, "/plans");
  await expect(page.locator(".plan-card")).toHaveCount(3);
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: test.info().outputPath("plans-mobile.png"),
    fullPage: true,
  });
  await goto(page, "/support");
  await expect(page.getByText("帮助就在这里", { exact: true })).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: test.info().outputPath("support-mobile.png"),
    fullPage: true,
  });
});
