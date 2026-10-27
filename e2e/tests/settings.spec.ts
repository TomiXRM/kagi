import { test, expect } from "@playwright/test";

declare global {
  interface Window {
    kagiSettingsObserved: () => { language: string; requests: number };
  }
}

test("production Settings language control emits one intent and reseeds without feedback", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(String(error)));
  await page.goto("/?story=settings");
  await page.waitForSelector("body[data-kagi-ready='1']");
  await expect.poll(() => page.evaluate(() => window.kagiSettingsObserved())).toEqual({ language: "en", requests: 0 });
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Space");
  await expect.poll(() => page.evaluate(() => window.kagiSettingsObserved().language)).toBe("ja");
  const acknowledged = await page.evaluate(() => window.kagiSettingsObserved());
  expect(acknowledged.requests).toBe(1);
  await page.evaluate(() => {
    const { promise, resolve } = Promise.withResolvers<void>();
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    return promise;
  });
  expect(await page.evaluate(() => window.kagiSettingsObserved())).toEqual(acknowledged);
  expect(errors).toEqual([]);
});
