import { test, expect, type Page } from "@playwright/test";

type Probe = { id: string; x: number; y: number; width: number; height: number; focused: boolean; selected: boolean; disabled: boolean; name: string; value: string };
type Observed = {
  language: string; theme: string; pane: string; requests: number; events: string[];
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
async function click(page: Page, id: string) {
  for (let attempt = 0; attempt < 30; attempt++) {
    const control = await probe(page, id);
    const viewport = page.viewportSize()!;
    const x = control.x + control.width / 2;
    const y = control.y + control.height / 2;
    if (x >= 0 && x < viewport.width && y >= 4 && y < viewport.height - 4) {
      await page.mouse.click(x, y);
      await frames(page);
      return;
    }
    await page.mouse.move(Math.max(4, Math.min(viewport.width - 4, x)), viewport.height / 2);
    await page.mouse.wheel(0, y < 4 ? -240 : 240);
    await frames(page);
  }
  throw new Error(`Production control ${id} did not become visible: ${JSON.stringify(await probe(page, id))}`);
}
async function pane(page: Page, id: string) {
  await click(page, `settings-pane-${id}`);
  await expect.poll(async () => (await observed(page)).pane).toBe(id);
}
async function replaceDraft(page: Page, text: string) {
  await click(page, "settings-analyze-input");
  // wasm32 uses GPUI's non-macOS binding table. Type real key events: the
  // browser platform consumes keydown text, not Playwright's insertText shortcut.
  await page.keyboard.press("Control+A");
  await page.keyboard.type(text);
  await expect.poll(async () => (await observed(page)).draft).toBe(text);
}

test.use({ deviceScaleFactor: 1 });

test("actual language handler emits once; host seed and focus do not emit preference requests", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(String(error)));
  await boot(page);
  await pane(page, "general");
  const before = (await observed(page)).requests;
  await click(page, "settings-language-ja");
  await expect.poll(async () => (await observed(page)).language).toBe("ja");
  expect((await observed(page)).requests).toBe(before + 1);
  const acknowledged = (await observed(page)).requests;
  await host(page, "reseed");
  await page.keyboard.press("Tab");
  await frames(page);
  expect((await observed(page)).requests).toBe(acknowledged);
  expect(errors).toEqual([]);
});

test("theme focus is separate from selection and switch focus is separate from ON", async ({ page }) => {
  await boot(page);
  await click(page, "settings-theme-apple-light");
  const selected = (await observed(page)).theme;
  await page.keyboard.press("ArrowRight");
  await frames(page);
  let state = await observed(page);
  expect(state.theme).toBe(selected);
  const focusedTheme = state.controls.find(control => control.id.startsWith("settings-theme-") && control.focused);
  expect(focusedTheme).toBeDefined();
  expect(focusedTheme!.selected).toBe(false);
  await page.keyboard.press("Space");
  await frames(page);
  expect((await observed(page)).theme).toBe(focusedTheme!.id.slice("settings-theme-".length));
  await pane(page, "graph");
  await click(page, "compact-toggle");
  state = await observed(page);
  expect(state.graphCompact).toBe(true);
  expect((await probe(page, "compact-toggle")).selected).toBe(true);
  await page.keyboard.press("Tab");
  await frames(page);
  expect((await observed(page)).graphCompact).toBe(true);
  expect((await observed(page)).graphLaneCompact).toBe(false);
  await page.keyboard.press("Space");
  await frames(page);
  expect((await observed(page)).graphLaneCompact).toBe(true);
});

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

test("pale accent active focus paint contrasts on unselected themes, ON switches and adjacent selections", async ({ page }, info) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 900, height: 740 });
  await boot(page);
  const luminance = (rgb: number[]) => rgb.reduce((sum, value, channel) => {
    const s = value / 255;
    return sum + (s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4) * [0.2126, 0.7152, 0.0722][channel];
  }, 0);
  for (const appearance of ["apple-light", "apple-dark"]) {
    await host(page, "appearance", appearance);
    await host(page, "accent", "yellow");
    await pane(page, "appearance");
    await click(page, `settings-theme-${appearance}`);
    await page.keyboard.press("ArrowRight");
    await frames(page);
    const theme = (await observed(page)).controls.find(control => control.id.startsWith("settings-theme-") && control.focused)!;
    expect(theme.selected).toBe(false);
    for (const kind of ["unselected-theme", "on-switch", "disabled-provider", "segment", "segment-reverse", "sidebar-reverse"]) {
      let control = theme;
      if (kind === "on-switch") {
        await pane(page, "graph");
        await click(page, "compact-toggle");
        if (!(await observed(page)).graphCompact) await click(page, "compact-toggle");
        control = await probe(page, "compact-toggle");
        expect(control.selected).toBe(true);
      }
      if (kind === "disabled-provider") {
        await pane(page, "smart_commit");
        await click(page, "sc-provider-ollama");
        await page.keyboard.press("ArrowRight");
        await frames(page);
        control = await probe(page, "sc-provider-claude-code");
        expect(control.disabled).toBe(true);
        const requests = (await observed(page)).requests;
        await page.keyboard.press("Space");
        await frames(page);
        expect((await observed(page)).requests).toBe(requests);
        expect((await observed(page)).smartProvider).toBe("ollama");
      }
      if (kind === "segment" || kind === "segment-reverse") {
        const reverse = kind === "segment-reverse";
        await pane(page, "general");
        await click(page, reverse ? "settings-language-ja" : "settings-language-en");
        await page.keyboard.press(reverse ? "ArrowLeft" : "ArrowRight");
        await frames(page);
        control = await probe(page, reverse ? "settings-language-en" : "settings-language-ja");
        expect(control.selected).toBe(false);
      }
      if (kind === "sidebar-reverse") {
        await pane(page, "terminal");
        await page.keyboard.press("ArrowUp");
        await frames(page);
        control = await probe(page, "settings-pane-editor");
        expect(control.selected).toBe(false);
        expect((await probe(page, "settings-pane-terminal")).selected).toBe(true);
      }
      expect(control.focused).toBe(true);
      expect(await page.evaluate(() => document.hasFocus())).toBe(true);
      const ring = await probe(page, `${control.id}-focus-ring`);
      const borderInset = kind === "unselected-theme" ? 2 : 0;
      expect(ring.x).toBeCloseTo(control.x - borderInset - 5);
      expect(ring.y).toBeCloseTo(control.y - borderInset - 5);
      expect(ring.width).toBeCloseTo(control.width + 2 * (borderInset + 5));
      expect(ring.height).toBeCloseTo(control.height + 2 * (borderInset + 5));
      const midX = ring.x + ring.width / 2, midY = ring.y + ring.height / 2;
      const edges = (depth: number) => [
        [ring.x + depth, midY], [ring.x + ring.width - depth, midY],
        [midX, ring.y + depth], [midX, ring.y + ring.height - depth],
      ];
      const depths = [3.5, 2.5, 4.5, 0.5, 1.5];
      const paint = await paintedPixels(page, depths.flatMap(edges));
      for (let edge = 0; edge < 4; edge++) {
        const background = paint.colors[edge];
        for (const index of [1, 2]) {
          const color = paint.colors[index * 4 + edge];
          color.forEach((value, channel) => expect(Math.abs(value - background[channel]), `${kind} three-pixel gap`).toBeLessThanOrEqual(2));
        }
        for (const index of [3, 4]) {
          const depth = depths[index];
          const color = paint.colors[index * 4 + edge];
          const a = luminance(background), b = luminance(color);
          expect((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05),
            JSON.stringify({ appearance, kind, edge, depth, color, background, control, ring })).toBeGreaterThanOrEqual(3);
        }
      }
      await info.attach(`${appearance}-${kind}-yellow-active.png`, { body: paint.png, contentType: "image/png" });
      expect((await probe(page, control.id)).selected).toBe(control.selected);
    }
  }
});

test("host detail overlay occludes focused control paint", async ({ page }, info) => {
  await boot(page);
  await pane(page, "graph");
  await click(page, "compact-toggle");
  const control = await probe(page, "compact-toggle");
  expect(control.focused).toBe(true);
  const ring = await probe(page, `${control.id}-focus-ring`);
  const midX = ring.x + ring.width / 2, midY = ring.y + ring.height / 2;
  const points = [
    [ring.x + 0.5, midY], [ring.x + ring.width - 0.5, midY],
    [midX, ring.y + 0.5], [midX, ring.y + ring.height - 0.5],
  ];
  const before = await paintedPixels(page, points);
  before.colors.forEach(color => expect(color).not.toEqual([18, 52, 86]));
  await host(page, "detail-overlay", "on");
  await frames(page);
  expect((await probe(page, control.id)).focused).toBe(true);
  const covered = await paintedPixels(page, [...points, [midX, midY]]);
  covered.colors.forEach(color => expect(color).toEqual([18, 52, 86]));
  await info.attach("host-detail-slot-over-focused-control.png", { body: covered.png, contentType: "image/png" });
  await host(page, "detail-overlay", "off");
  await frames(page);
  expect((await probe(page, control.id)).focused).toBe(true);
  const restored = await paintedPixels(page, points);
  expect(restored.colors).toEqual(before.colors);
});

test("selected hover paint and segmented visible geometry remain distinct from hit targets", async ({ page }, info) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 900, height: 740 });
  await boot(page);
  for (const appearance of ["apple-light", "apple-dark"]) {
    await host(page, "appearance", appearance);
    for (const language of ["en", "ja"]) {
      await host(page, "language", language);
      for (const zoom of [1, 1.67]) {
        await host(page, "zoom", String(zoom));
        for (const destination of panes) {
          await pane(page, destination);
          const row = await probe(page, `settings-pane-${destination}`);
          expect(row.selected).toBe(true);
          await page.mouse.move(899, 4);
          await frames(page);
          const sample = [row.x + 3 * zoom, row.y + row.height / 2];
          const unhovered = await paintedPixels(page, [sample, [2, sample[1]]]);
          const [selected, background] = unhovered.colors;
          expect(selected.reduce((sum, value, channel) => sum + Math.abs(value - background[channel]), 0)).toBeGreaterThan(20);
          await page.mouse.move(row.x + row.width / 2, row.y + row.height / 2);
          await frames(page);
          const hoverPaint = await paintedPixels(page, [sample]);
          const [hovered] = hoverPaint.colors;
          hovered.forEach((value, channel) => expect(Math.abs(value - selected[channel])).toBeLessThanOrEqual(2));
          if (["graph", "terminal", "network", "smart_commit"].includes(destination)
            && ((appearance === "apple-light" && language === "en" && zoom === 1)
              || (appearance === "apple-dark" && language === "ja" && zoom === 1.67))) {
            await info.attach(`${appearance}-${language}-${zoom}-${destination}-hover.png`, { body: hoverPaint.png, contentType: "image/png" });
          }
        }
        const hit = await probe(page, `settings-language-${language}`);
        const track = await probe(page, "settings-language-visual");
        const pill = await probe(page, `settings-language-${language}-visual`);
        expect(hit.height).toBeGreaterThanOrEqual(28 * zoom - 0.5);
        if (zoom === 1) {
          expect(Math.abs(track.height - 24)).toBeLessThanOrEqual(0.5);
          expect(Math.abs(pill.height - 20)).toBeLessThanOrEqual(0.5);
        }
        // GPUI snaps absolute near/far edges independently (taffy.rs).
        // At 167%, hit47/track41/pill33 can satisfy each inset even though
        // subtracting the snapped edges differs from 24*1.67 by 0.92.
        for (const [visual, inset] of [[track, 2], [pill, 4]] as const) {
          expect(Math.abs(visual.y - hit.y - inset * zoom)).toBeLessThanOrEqual(0.5);
          expect(Math.abs(hit.y + hit.height - visual.y - visual.height - inset * zoom)).toBeLessThanOrEqual(0.5);
        }
        await page.mouse.move(hit.x + hit.width / 2, hit.y + hit.height / 2);
        await frames(page);
        const segmentPaint = await paintedPixels(page, [[hit.x + 3 * zoom, hit.y + hit.height / 2]]);
        const [color] = segmentPaint.colors;
        const expected = appearance === "apple-light" ? [255, 255, 255] : [102, 102, 108];
        color.forEach((value, channel) => expect(Math.abs(value - expected[channel])).toBeLessThanOrEqual(2));
        await info.attach(`${appearance}-${language}-${zoom}-segments.png`, { body: segmentPaint.png, contentType: "image/png" });
      }
    }
  }
});

async function captionInk(page: Page, caption: Probe) {
  const png = await page.screenshot({
    type: "png",
    clip: {
      x: Math.ceil(caption.x), y: Math.ceil(caption.y),
      width: Math.floor(caption.width) - 1, height: Math.floor(caption.height) - 1,
    },
  });
  return page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), ch => ch.charCodeAt(0));
    const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d")!;
    context.drawImage(image, 0, 0);
    image.close();
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    const isInk = (x: number, y: number) => {
      const at = (y * canvas.width + x) * 4;
      return Math.abs(data[at] - data[0])
        + Math.abs(data[at + 1] - data[1])
        + Math.abs(data[at + 2] - data[2]) > 90;
    };
    let left = canvas.width;
    let right = -1;
    for (let y = 0; y < canvas.height; y++) {
      for (let x = 0; x < canvas.width; x++) {
        if (isInk(x, y)) {
          left = Math.min(left, x);
          right = Math.max(right, x);
        }
      }
    }
    let suffixUpper = 0;
    let suffixLower = 0;
    for (let y = 0; y < canvas.height; y++) {
      for (let x = Math.max(0, right - 8); x <= right; x++) {
        if (isInk(x, y)) {
          if (y < canvas.height / 2) suffixUpper++;
          else suffixLower++;
        }
      }
    }
    return { width: Math.max(0, right - left + 1), suffixUpper, suffixLower };
  }, png.toString("base64"));
}

test("short theme captions paint names and long names paint constrained ellipses", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 740 });
  await boot(page);
  for (const appearance of ["apple-light", "apple-dark"]) {
    await click(page, `settings-theme-${appearance}`);
    for (const slug of ["apple-light", "apple-dark"]) {
      const card = await probe(page, `settings-theme-${slug}`);
      const caption = await probe(page, `settings-theme-caption-${slug}`);
      expect(caption.width).toBeGreaterThan(card.width * 0.6);
      // Accessible names or fixture strings would pass if only "..." painted.
      const ink = await captionInk(page, caption);
      expect(ink.width, `${appearance}: ${slug} must paint a readable short name`).toBeGreaterThan(35);
    }
    for (const slug of ["color-vision", "monokai"]) {
      const card = await probe(page, `settings-theme-${slug}`);
      const caption = await probe(page, `settings-theme-caption-${slug}`);
      const leaf = await probe(page, `settings-theme-caption-text-${slug}`);
      // value comes from the painted StyledText's actual TextLayout, not its input.
      expect(leaf.value).toMatch(/.+…$/);
      expect(card.name).not.toBe(leaf.value);
      expect(leaf.x).toBeGreaterThanOrEqual(caption.x);
      expect(leaf.x + leaf.width).toBeLessThanOrEqual(caption.x + caption.width + 1);
      const ink = await captionInk(page, caption);
      expect(ink.width).toBeGreaterThan(35);
      // The actual ellipsis suffix paints baseline dots, not a clipped tall letter.
      expect(ink.suffixUpper).toBe(0);
      expect(ink.suffixLower).toBeGreaterThanOrEqual(3);
    }
  }
});

for (const [language, appearance, zoom] of [
  ["en", "apple-light", 1],
  ["ja", "apple-dark", 1.67],
] as const) {
  test(`composite numeric controls own their complete rows at 715px: ${language} ${zoom}`, async ({ page }, info) => {
    await page.setViewportSize({ width: 715, height: 1064 });
    await boot(page);
    await host(page, "language", language);
    await host(page, "appearance", appearance);
    await host(page, "zoom", String(zoom));
    const navigation = (await observed(page)).controls.filter(control => panes.some(pane => control.id === `settings-pane-${pane}`));
    const viewport = await probe(page, "settings-detail-scroll");
    const initialZoom = await probe(page, "settings-zoom-row");
    await page.mouse.move(viewport.x + viewport.width / 2, viewport.y + viewport.height / 2);
    await page.mouse.wheel(0, initialZoom.y - viewport.y - 20);
    await frames(page);

    const state = await observed(page);
    const control = (id: string) => {
      const result = state.controls.find(control => control.id === id);
      expect(result, id).toBeDefined();
      return result!;
    };
    const contains = (outer: Probe, inner: Probe) => {
      expect(inner.x, `${inner.id} left inside ${outer.id}`).toBeGreaterThanOrEqual(outer.x);
      expect(inner.y, `${inner.id} top inside ${outer.id}`).toBeGreaterThanOrEqual(outer.y);
      expect(inner.x + inner.width, `${inner.id} right inside ${outer.id}`).toBeLessThanOrEqual(outer.x + outer.width);
      expect(inner.y + inner.height, `${inner.id} bottom inside ${outer.id}`).toBeLessThanOrEqual(outer.y + outer.height);
    };
    const zoomRow = control("settings-zoom-row");
    const thresholdRow = control("settings-threshold-row");
    const motionRow = control("settings-motion-row");
    for (const [row, ids] of [
      [zoomRow, ["settings-zoom-help", "zoom-dec", "zoom-value", "zoom-inc"]],
      [thresholdRow, ["settings-threshold-help", "compact-threshold-dec", "compact-threshold-value", "compact-threshold-inc", "compact-threshold-help"]],
      [motionRow, ["settings-motion-help", "reduce-motion-toggle"]],
    ] as const) {
      expect(row.height, row.id).toBeGreaterThan(0);
      for (const id of ids) contains(row, control(id));
    }
    expect(zoomRow.y + zoomRow.height).toBeLessThanOrEqual(thresholdRow.y);
    expect(thresholdRow.y + thresholdRow.height).toBeLessThanOrEqual(motionRow.y);
    for (const prefix of ["zoom", "compact-threshold"]) {
      const decrease = control(`${prefix}-dec`);
      const value = control(`${prefix}-value`);
      const increase = control(`${prefix}-inc`);
      expect(Math.abs(decrease.y - increase.y), `${prefix} is one horizontal unit`).toBeLessThanOrEqual(0.5);
      expect(decrease.x + decrease.width).toBeLessThan(value.x);
      expect(value.x + value.width).toBeLessThan(increase.x);
      expect(value.y).toBeGreaterThanOrEqual(decrease.y);
      expect(value.y + value.height).toBeLessThanOrEqual(decrease.y + decrease.height);
    }
    const thresholdHelp = control("compact-threshold-help");
    expect(thresholdHelp.y).toBeGreaterThanOrEqual(control("compact-threshold-inc").y + control("compact-threshold-inc").height);
    await info.attach(`${language}-${zoom}-numeric-rows.png`, { body: await page.screenshot(), contentType: "image/png" });
    await info.attach(`${language}-${zoom}-row-geometry.json`, {
      body: Buffer.from(JSON.stringify(state.controls.filter(control => /zoom|threshold|motion/.test(control.id)), null, 2)),
      contentType: "application/json",
    });

    await page.mouse.wheel(0, 100000);
    await frames(page);
    await expect.poll(async () => {
      const state = await observed(page);
      return Math.abs(state.scrollY + state.scrollMax);
    }).toBeLessThanOrEqual(1);
    contains(viewport, await probe(page, "settings-theme-folder-open"));
    const after = await observed(page);
    expect(after.controls.filter(control => panes.some(pane => control.id === `settings-pane-${pane}`))).toEqual(navigation);
    expect(after.zoom).toBe(state.zoom);
    expect(after.compactThreshold).toBe(70);
    await info.attach(`${language}-${zoom}-appearance-bottom.png`, { body: await page.screenshot(), contentType: "image/png" });
  });
}

test("all ordinary preferences use real controls and unsupported native effects are explicit", async ({ page }) => {
  await boot(page);
  await click(page, "settings-accent-theme");
  expect((await observed(page)).followsSystemAccent).toBe(false);
  await click(page, "settings-toolbar-icon_only");
  expect((await observed(page)).toolbar).toBe("icon_only");
  await click(page, "zoom-inc");
  expect((await observed(page)).zoom).toBeGreaterThan(1);
  await click(page, "compact-threshold-inc");
  expect((await observed(page)).compactThreshold).toBe(71);
  await click(page, "reduce-motion-toggle");
  expect((await observed(page)).reduceMotion).toBe(true);
  expect((await probe(page, "settings-theme-folder-open")).disabled).toBe(true);
  await click(page, "settings-theme-reload");
  expect((await observed(page)).unsupported).toContain("Native filesystem theme discovery is unsupported in browser verification");
  await pane(page, "graph");
  await click(page, "settings-copy-branch");
  expect((await observed(page)).copyTarget).toBe("branch");
  await pane(page, "editor");
  await click(page, "settings-markdown-source");
  expect((await observed(page)).markdown).toBe("source");
  await pane(page, "terminal");
  await click(page, "terminal-auto-lock-toggle");
  expect((await observed(page)).terminalAutoLock).toBe(true);
  await pane(page, "network");
  await click(page, "auto-fetch-toggle");
  expect((await observed(page)).autoFetch).toBe(false);
});

test("Smart Commit requires host consent; disabled provider and disappearing models preserve state", async ({ page }) => {
  await boot(page);
  await pane(page, "smart_commit");
  await click(page, "sc-enabled-toggle");
  expect((await observed(page)).pendingConsent).toBe(true);
  expect((await observed(page)).smartEnabled).toBe(false);
  await host(page, "close");
  await host(page, "reopen");
  expect((await observed(page)).pendingConsent).toBe(false);
  expect((await observed(page)).smartEnabled).toBe(false);
  await click(page, "sc-enabled-toggle");
  await host(page, "consent", "accept");
  expect((await observed(page)).smartEnabled).toBe(true);
  expect((await probe(page, "sc-provider-claude-code")).disabled).toBe(true);
  await click(page, "sc-provider-claude-code");
  expect((await observed(page)).smartProvider).toBe("ollama");
  await click(page, "sc-model-llama3.2:3b");
  expect((await observed(page)).smartModel).toBe("llama3.2:3b");
  await host(page, "models", "[]");
  expect((await observed(page)).smartModel).toBe("llama3.2:3b");
  expect((await observed(page)).controls.some(control => control.id.startsWith("sc-model-") && control.focused)).toBe(false);
  await click(page, "sc-provider-codex");
  expect((await observed(page)).smartProvider).toBe("codex");
});

test("Analyze retains newer drafts across delayed/failing results, pane and fixture close/reopen", async ({ page }) => {
  await boot(page);
  await pane(page, "analyze");
  expect((await probe(page, "settings-analyze-save")).disabled).toBe(true);
  await host(page, "save-mode", "deferred");
  await replaceDraft(page, "first/\n");
  await click(page, "settings-analyze-save");
  expect((await observed(page)).pendingSaves).toBe(1);
  await replaceDraft(page, "newer/\n");
  await pane(page, "general");
  await pane(page, "analyze");
  await host(page, "close");
  await host(page, "reopen");
  expect((await observed(page)).draft).toBe("newer/\n");
  await host(page, "ack", "success");
  expect((await observed(page)).baseline).toBe("first/\n");
  expect((await observed(page)).draft).toBe("newer/\n");
  expect((await observed(page)).dirty).toBe(true);
  await host(page, "save-mode", "failed");
  await click(page, "settings-analyze-save");
  await probe(page, "settings-analyze-error");
  expect((await observed(page)).baseline).toBe("first/\n");
  expect((await observed(page)).draft).toBe("newer/\n");
  await click(page, "settings-analyze-reset");
  expect((await observed(page)).pendingReset).toBe(true);
  await host(page, "close");
  await host(page, "reopen");
  expect((await observed(page)).pendingReset).toBe(false);
  expect((await observed(page)).draft).toBe("newer/\n");
  await click(page, "settings-analyze-reset");
  await host(page, "reset", "accept");
  expect((await observed(page)).draft).toBe("node_modules/\ntarget/\n");
  expect((await observed(page)).dirty).toBe(false);
});

test("Page keys scroll detail without moving focus or stealing Analyze editing", async ({ page }) => {
  await page.setViewportSize({ width: 715, height: 400 });
  await boot(page);
  await click(page, "settings-theme-apple-light");
  const before = await observed(page);
  const viewport = await probe(page, "settings-detail-scroll");
  expect(before.scrollMax).toBeGreaterThan(0);
  await page.keyboard.press("PageDown");
  await frames(page);
  const down = await observed(page);
  expect(down.scrollY).toBeLessThan(before.scrollY);
  expect(await probe(page, "settings-detail-scroll")).toEqual(viewport);
  expect((await probe(page, "settings-theme-apple-light")).focused).toBe(true);
  await page.keyboard.press("PageUp");
  await frames(page);
  expect((await observed(page)).scrollY).toBeGreaterThan(down.scrollY);
  expect((await probe(page, "settings-theme-apple-light")).focused).toBe(true);
  await pane(page, "analyze");
  await click(page, "settings-analyze-input");
  const editing = await observed(page);
  expect(editing.scrollMax).toBeGreaterThan(0);
  await page.keyboard.press("PageDown");
  await page.keyboard.press("PageUp");
  await frames(page);
  expect((await observed(page)).scrollY).toBe(editing.scrollY);
  expect((await observed(page)).draft).toBe(editing.draft);
  await page.keyboard.press("Control+A");
  await page.keyboard.type("still-editing/");
  expect((await observed(page)).draft).toBe("still-editing/");
});

for (const language of ["en", "ja"]) {
  for (const theme of ["apple-light", "apple-dark"]) {
    for (const layout of ["normal", "narrow", "zoom167"]) {
      test(`${language} ${theme} ${layout}: every pane uses the same production renderer`, async ({ page }, info) => {
        test.setTimeout(120_000);
        const errors: string[] = [];
        page.on("pageerror", error => errors.push(String(error)));
        await page.setViewportSize(layout === "normal" ? { width: 900, height: 740 } : { width: 715, height: 600 });
        await boot(page);
        await host(page, "language", language);
        await host(page, "appearance", theme);
        if (layout === "zoom167") await host(page, "zoom", "1.67");
        for (const destination of panes) {
          await pane(page, destination);
          const state = await observed(page);
          const navigation = state.controls.filter(control => panes.some(pane => control.id === `settings-pane-${pane}`));
          expect(navigation).toHaveLength(8);
          expect(navigation.filter(control => control.selected).map(control => control.id)).toEqual([`settings-pane-${destination}`]);
          for (const control of state.controls) {
            expect(control.width, control.id).toBeGreaterThan(0);
            expect(control.height, control.id).toBeGreaterThan(0);
            expect(control.x, control.id).toBeGreaterThanOrEqual(-1);
            expect(control.x + control.width, control.id).toBeLessThanOrEqual(page.viewportSize()!.width + 1);
          }
          await info.attach(`${destination}.png`, { body: await page.screenshot(), contentType: "image/png" });
        }
        // Real viewport resize is a separate stimulus, not a fixture-only state update.
        await page.setViewportSize({ width: 800, height: 680 });
        await frames(page);
        await click(page, `settings-language-${language === "en" ? "ja" : "en"}`);
        expect((await observed(page)).language).toBe(language === "en" ? "ja" : "en");
        expect(errors).toEqual([]);
      });
    }
  }
}
