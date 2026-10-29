import { expect, type Page } from "@playwright/test";

type Probe = { id: string; x: number; y: number; width: number; height: number; focused: boolean; selected: boolean; disabled: boolean; name: string; value: string; role: string; paint: { fontSize: number; fontWeight: number; textColor: number } | null };
type Observed = {
  language: string; theme: string; themeCount: number; pane: string; requests: number; events: string[];
  themeCatalog: { slug: string; dark: boolean; key: string }[];
  visible: boolean; unsupported: string[]; pendingSaves: number; pendingReset: boolean; pendingConsent: boolean;
  smartEnabled: boolean; smartProvider: string; smartModel: string | null; graphCompact: boolean; graphLaneCompact: boolean;
  terminalAutoLock: boolean; autoFetch: boolean; reduceMotion: boolean; zoom: number; toolbar: string;
  followsSystemAccent: boolean; compactThreshold: number; copyTarget: string; markdown: string;
  draft: string; baseline: string; dirty: boolean; controls: Probe[];
  scrollY: number; scrollMax: number;
};
declare global {
  interface Window {
    kagiSettingsObserved: () => Observed;
    kagiSettingsCommand: (command: string, value?: string) => void;
  }
}
const panes = ["appearance", "graph", "editor", "terminal", "network", "smart_commit", "analyze", "general"] as const;
const observed = (page: Page) => page.evaluate(() => window.kagiSettingsObserved());
async function frames(page: Page) {
  await page.evaluate(() => {
    const { promise, resolve } = Promise.withResolvers<void>();
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    return promise;
  });
}
async function host(page: Page, command: string, value?: string) {
  await page.evaluate(({ command, value }) => window.kagiSettingsCommand(command, value), { command, value });
  await frames(page);
}
async function boot(page: Page) {
  await page.goto("/?story=settings");
  await page.waitForSelector("body[data-kagi-ready='1']");
  await expect.poll(async () => (await observed(page))?.controls.length ?? 0).toBeGreaterThan(8);
}
async function probe(page: Page, id: string) {
  await expect.poll(async () => (await observed(page)).controls.some(control => control.id === id)).toBe(true);
  return (await observed(page)).controls.find(control => control.id === id)!;
}
// Geometry comes from the actual production element after layout. Scroll and
// pointer/keyboard events still go through Chromium/GPUI, not fixture callbacks.
async function reveal(page: Page, id: string) {
  for (let attempt = 0; attempt < 30; attempt++) {
    const control = await probe(page, id);
    const viewport = await probe(page, id.startsWith("settings-pane-")
      ? "settings-sidebar"
      : control.role === "MenuItemRadio" ? "settings-popup-menu" : "settings-detail-scroll");
    const top = viewport.y + 1;
    const bottom = viewport.y + viewport.height - 1;
    if (control.y < top || control.y + control.height > bottom) {
      await page.mouse.move(viewport.x + viewport.width / 2, viewport.y + viewport.height / 2);
      await page.mouse.wheel(0, control.y < top ? control.y - top : control.y + control.height - bottom);
      await frames(page);
      continue;
    }
    if (control.role === "RadioButton" && id.startsWith("settings-theme-")) {
      const strip = await themeStrip(page, id);
      const left = strip.x + 7;
      const right = strip.x + strip.width - 7;
      if (control.x < left || control.x + control.width > right) {
        await page.mouse.move(strip.x + strip.width / 2, control.y + control.height / 2);
        await page.mouse.wheel(control.x < left ? control.x - left : control.x + control.width - right, 0);
        await frames(page);
        continue;
      }
    }
    expect(control.x).toBeGreaterThanOrEqual(viewport.x - 1);
    expect(control.x + control.width).toBeLessThanOrEqual(viewport.x + viewport.width + 1);
    return control;
  }
  throw new Error(`Production control ${id} did not become visible: ${JSON.stringify(await probe(page, id))}`);
}
async function click(page: Page, id: string) {
  const control = await reveal(page, id);
  await page.mouse.click(control.x + control.width / 2, control.y + control.height / 2);
  await frames(page);
}
async function pane(page: Page, id: string) {
  await click(page, `settings-pane-${id}`);
  await expect.poll(async () => (await observed(page)).pane).toBe(id);
}

export async function themeStrip(page: Page, id: string) {
  const slug = id.replace(/-focus-ring$/, "").slice("settings-theme-".length);
  const palette = (await observed(page)).themeCatalog.find(theme => theme.slug === slug);
  expect(palette, `authoritative palette for ${slug}`).toBeDefined();
  return probe(page, `settings-themes-${palette!.dark ? "dark" : "light"}-scroll`);
}
export { observed, frames, host, boot, probe, reveal, click, pane, paintedPixels, luminance, panes };
export type { Probe, Observed };

// DPR=1: a length is the difference of two independently snapped edges.
// This is exact at integral scales, and only floor/ceil are valid otherwise.
export function expectSnappedLength(actual: number, logical: number) {
  expect([Math.floor(logical), Math.ceil(logical)]).toContain(actual);
}

async function paintedPixels(page: Page, points: number[][]) {
  // One real frame per state: recapturing the GPU for each pixel is both slow
  // and less coherent than sampling all assertions from the attached image.
  const png = await page.screenshot({ type: "png" });
  const colors = await page.evaluate(async ({ encoded, points }) => {
    const bytes = Uint8Array.from(atob(encoded), ch => ch.charCodeAt(0));
    const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d")!;
    context.drawImage(image, 0, 0);
    image.close();
    return points.map(([x, y]) => {
      const pixelX = Math.floor(Math.floor(x) * canvas.width / innerWidth);
      const pixelY = Math.floor(Math.floor(y) * canvas.height / innerHeight);
      if (pixelX < 0 || pixelY < 0 || pixelX >= canvas.width || pixelY >= canvas.height) {
        throw new Error(`Paint sample is outside the actual screenshot: ${x},${y}`);
      }
      return Array.from(context.getImageData(pixelX, pixelY, 1, 1).data).slice(0, 3);
    });
  }, { encoded: png.toString("base64"), points });
  return { png, colors };
}

const luminance = (rgb: number[]) => rgb.reduce((sum, value, channel) => {
  const s = value / 255;
  return sum + (s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4) * [0.2126, 0.7152, 0.0722][channel];
}, 0);

// Consumer expectation: paired families first in each appearance, then stable
// catalog order. Keep this independent from production's sort implementation.
export function themeDisplayOrder(catalog: Observed["themeCatalog"]) {
  return [false, true].flatMap(dark => {
    const mode = catalog.filter(theme => theme.dark === dark);
    const paired = dark
      ? ["apple-dark", "catppuccin", "nagi-dark", "one-dark"]
      : ["apple-light", "catppuccin-latte", "nagi-light", "one-light"];
    return [...paired.flatMap(slug => mode.filter(theme => theme.slug === slug)),
      ...mode.filter(theme => !paired.includes(theme.slug))];
  });
}
