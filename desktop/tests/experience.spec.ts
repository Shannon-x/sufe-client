import { test, expect } from "@playwright/test";
test.use({ channel: "chrome", viewport: { width: 1365, height: 950 } });
const base = process.env.SUFE_TEST_URL || "http://127.0.0.1:1420";
test("plain browser releases startup veil and shows a usable login form", async ({
  page,
}) => {
  const failures: string[] = [];
  page.on("pageerror", (e) => failures.push(e.message));
  await page.goto(base + "/#/login");
  await expect(page.getByText("欢迎回来", { exact: true })).toBeVisible();
  await expect(
    page.getByText("当前为浏览器界面，请使用桌面客户端登录连接。"),
  ).toBeVisible();
  await expect(page.getByPlaceholder("user@example.com")).toBeEnabled();
  expect(failures).toEqual([]);
});
test("preview connection state, traffic and explicit disconnect stay consistent", async ({
  page,
}) => {
  await page.goto(base + "/?preview=1#/");
  await expect(
    page.getByText("界面预览 · 当前为示例数据，不建立真实网络连接或订单。"),
  ).toBeVisible();
  await page.getByRole("button", { name: "立即连接", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "断开连接", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "连接已建立", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "断开连接", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "立即连接", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("尚未连接", { exact: true })).toBeVisible();
  await page.screenshot({ path: "../artifacts/sufe-dashboard-desktop.png", fullPage: true });
});
test("node selection and favorites survive reload without silently connecting", async ({
  page,
}) => {
  await page.goto(base + "/?preview=1#/nodes");
  await page.getByLabel("搜索线路或协议").fill("东京");
  await expect(page.locator(".node-card")).toHaveCount(1);
  await page
    .getByRole("button", { name: "收藏 日本 · 东京 01", exact: true })
    .click();
  await page.locator(".node-main").click();
  await expect(page.locator(".node-card.selected")).toHaveCount(1);
  await page.reload();
  await page.getByRole("button", { name: /我的收藏/ }).click();
  await expect(page.locator(".node-card")).toHaveCount(1);
  await expect(page.getByText("尚未连接", { exact: true })).toBeVisible();
});
test("custom rules reject injection and allow add, disable and delete", async ({
  page,
}) => {
  await page.goto(base + "/?preview=1#/rules");
  await page.getByRole("button", { name: "添加规则", exact: true }).click();
  await page.getByLabel("匹配内容", { exact: true }).fill("example.com,DIRECT");
  await page.getByRole("button", { name: "保存规则", exact: true }).click();
  await expect(
    page.getByText("请输入有效内容，不要包含网址协议、空格或逗号。"),
  ).toBeVisible();
  await page.getByLabel("匹配内容", { exact: true }).fill("example.com");
  await page.getByRole("button", { name: "保存规则", exact: true }).click();
  await expect(
    page.getByRole("switch", { name: "启用规则 example.com" }),
  ).toBeChecked();
  await page.getByRole("switch", { name: "启用规则 example.com" }).click();
  await expect(
    page.getByRole("switch", { name: "启用规则 example.com" }),
  ).not.toBeChecked();
  await page.getByRole("button", { name: "删除规则 example.com" }).click();
  await expect(
    page.getByRole("heading", { name: "让常用网站，走你想走的路" }),
  ).toBeVisible();
});
test("main pages render at phone size without horizontal overflow or runtime errors", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const failures: string[] = [];
  page.on("pageerror", (e) => failures.push(e.message));
  for (const path of [
    "/",
    "/nodes",
    "/rules",
    "/account",
    "/settings",
    "/logs",
    "/connections",
    "/notices",
  ]) {
    await page.goto(base + "/?preview=1#" + path);
    await expect(page.locator("#main-content")).toBeVisible();
    await expect(page.locator("#main-content")).not.toBeEmpty();
    await expect
      .poll(() =>
        page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      )
      .toBe(true);
    if (path === "/") await page.screenshot({ path: "../artifacts/sufe-dashboard-mobile.png", fullPage: true });
  }
  expect(failures).toEqual([]);
  await page.screenshot({
    path: "../artifacts/sufe-notices-mobile.png",
    fullPage: true,
  });
});
