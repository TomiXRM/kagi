import { test, expect, Page } from "@playwright/test";

// The catalog page renders kagi UI stories on a gpui_web canvas.
// scripts/build-web.sh must have produced crates/kagi-web/dist first.

async function loadCatalog(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  await page.goto("/");
  await page.waitForSelector("body[data-kagi-ready='1']", { timeout: 30_000 });
  // A couple of frames so the first layout settles.
  await page.waitForTimeout(300);
  return errors;
}

test("catalog boots without errors", async ({ page }) => {
  const errors = await loadCatalog(page);
  expect(await page.locator("canvas").count()).toBeGreaterThan(0);
  expect(errors).toEqual([]);
});

// Guard for the pane-resize wrap jitter class (see src/ui/inspector.rs):
// on zed-main gpui a wrapped text element's measured height lags a width
// change by one frame, so the first frame AFTER a resize shows a one-line
// ghost shift that the next frame corrects. Sweep widths in small steps (a
// divider drag) and require the first post-resize frame to already equal
// the settled frame.
//
// Caveat: the native jitter came from the platform text system's measure
// cache and does not necessarily reproduce under gpui_web — this spec guards
// the browser rendering path and catches any layout that needs >1 frame to
// settle after a resize.
test("layout settles in one frame after each resize step", async ({ page }) => {
  await loadCatalog(page);
  const raf = () =>
    page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r(null))));
  const unstable: number[] = [];
  for (let width = 1280; width >= 900; width -= 20) {
    await page.setViewportSize({ width, height: 800 });
    await raf(); // first frame after the resize
    const early = await page.screenshot();
    await page.waitForTimeout(200); // fully settled
    const settled = await page.screenshot();
    if (!early.equals(settled)) unstable.push(width);
  }
  expect(
    unstable,
    `post-resize frame differed from settled frame at widths: ${unstable.join(", ")}`,
  ).toEqual([]);
});
