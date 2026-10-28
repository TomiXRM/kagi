import { test, expect } from "@playwright/test";
import { boot, host, pane, panes, probe, reveal, click, frames, observed, paintedPixels, expectSnappedLength, type Probe } from "./settings-helpers";

test.use({ deviceScaleFactor: 1 });

for (const appearance of ["apple-light", "apple-dark"]) {
  for (const language of ["en", "ja"]) {
    for (const zoom of [1, 1.67]) {
      test(`production kit geometry and live states: ${appearance} ${language} ${zoom}`, async ({ page }, info) => {
        test.setTimeout(120_000);
        await page.setViewportSize({ width: 715, height: 640 });
        await boot(page);
        await host(page, "appearance", appearance);
        await host(page, "language", language);
        await host(page, "zoom", String(zoom));
        await pane(page, "graph");
        for (const [id, size, weight] of [
          ["settings-pane-title", 15, 700],
          ["settings-pane-graph-label", 13, 600],
          ["settings-pane-appearance-label", 13, 400],
        ] as const) {
          const paint = (await probe(page, id)).paint;
          expect(paint, `${id} actual paint style`).not.toBeNull();
          expect(paint!.fontSize).toBeCloseTo(size * zoom, 4);
          expect(paint!.fontWeight).toBe(weight);
        }
        const hit = await reveal(page, "compact-toggle");
        const track = await probe(page, "compact-toggle-visual");
        const knob = await probe(page, "compact-toggle-knob");
        for (const [actual, expected] of [[track.width, 28], [track.height, 16], [knob.width, 12], [knob.height, 12]]) {
          expectSnappedLength(actual, expected * zoom);
        }
        expect(hit.height).toBeGreaterThanOrEqual(Math.floor(28 * zoom));
        expect(hit.selected).toBe(false);
        await page.mouse.move(2, 2); await frames(page);
        const off = await paintedPixels(page, [[track.x + track.width - 4 * zoom, track.y + track.height / 2], [knob.x + knob.width / 2, knob.y + knob.height / 2]]);
        expect(off.colors[1]).toEqual([255, 255, 255]);
        // OFF must not inherit the injected accent, while ON must use it.
        await host(page, "accent", "yellow");
        const offAccent = await paintedPixels(page, [[track.x + track.width - 4 * zoom, track.y + track.height / 2]]);
        expect(offAccent.colors[0]).toEqual(off.colors[0]);
        await click(page, "compact-toggle");
        await expect.poll(async () => {
          const delta = (await probe(page, "compact-toggle-knob")).x - knob.x;
          return [Math.floor(12 * zoom), Math.ceil(12 * zoom)].includes(delta);
        }).toBe(true);
        const onTrack = await probe(page, "compact-toggle-visual");
        const onKnob = await probe(page, "compact-toggle-knob");
        const on = await paintedPixels(page, [[onTrack.x + 4 * zoom, onTrack.y + onTrack.height / 2], [onKnob.x + onKnob.width / 2, onKnob.y + onKnob.height / 2]]);
        expect(on.colors[0]).not.toEqual(off.colors[0]);
        expect(on.colors[1]).toEqual([255, 255, 255]);
        expectSnappedLength(onKnob.x - knob.x, 12 * zoom);
        const requests = (await observed(page)).requests;
        await page.keyboard.press("Tab"); await frames(page);
        expect((await probe(page, "lane-compact-toggle")).focused).toBe(true);
        expect((await observed(page)).requests).toBe(requests);
        expect((await observed(page)).graphLaneCompact).toBe(false);
        const hash = await reveal(page, "settings-copy-hash");
        const branch = await probe(page, "settings-copy-branch");
        expect(Math.abs(hash.width - branch.width)).toBeLessThanOrEqual(1);
        expect(hash.selected).toBe(true);
        expect(branch.selected).toBe(false);
        const hashPaint = (await probe(page, "settings-copy-hash-label")).paint!;
        const branchPaint = (await probe(page, "settings-copy-branch-label")).paint!;
        expect(hashPaint.fontWeight).toBe(500);
        expect(branchPaint.fontWeight).toBe(400);
        expect(hashPaint.textColor).not.toBe(branchPaint.textColor);
        // Original label-derived segments, not the removed 144pt-per-item bank.
        expect(hash.width).toBeLessThan(100 * zoom);
        await pane(page, "smart_commit");
        for (const id of ["sc-provider-select", "sc-model-select"]) {
          const popup = await reveal(page, id);
          expect(popup.role).toBe("ComboBox");
          expect(popup.height).toBeGreaterThanOrEqual(Math.floor(28 * zoom));
          const visible = await probe(page, `${id}-visual`);
          expectSnappedLength(visible.height, 24 * zoom);
          const beforeOpen = (await observed(page)).requests;
          await click(page, id);
          await page.keyboard.press("Escape"); await frames(page);
          expect((await probe(page, id)).focused).toBe(true);
          expect((await observed(page)).requests).toBe(beforeOpen);
        }
        await pane(page, "appearance");
        for (const id of ["zoom-dec", "zoom-inc", "compact-threshold-dec", "compact-threshold-inc", "settings-theme-reload", "settings-theme-guide"]) {
          const control = await reveal(page, id);
          const visual = await probe(page, `${id}-visual`);
          expect(control.height).toBeGreaterThanOrEqual(Math.floor(28 * zoom));
          expectSnappedLength(visual.height, 24 * zoom);
          expect(visual.y).toBeGreaterThanOrEqual(control.y);
          expect(visual.y + visual.height).toBeLessThanOrEqual(control.y + control.height);
          if (id.endsWith("-dec") || id.endsWith("-inc")) expectSnappedLength(visual.width, 24 * zoom);
        }
        // A real button's hover/press are presentation-only until release.
        const button = await reveal(page, "settings-theme-reload");
        const visual = await probe(page, "settings-theme-reload-visual");
        const sample = [visual.x + 7 * zoom, visual.y + visual.height / 2];
        await page.mouse.move(2, 2); await frames(page);
        const rest = await paintedPixels(page, [sample]);
        const before = await observed(page);
        await page.mouse.move(button.x + button.width / 2, button.y + button.height / 2); await frames(page);
        const hover = await paintedPixels(page, [sample]);
        expect(hover.colors).not.toEqual(rest.colors);
        await page.mouse.down(); await frames(page);
        const pressed = await paintedPixels(page, [sample]);
        expect(pressed.colors).not.toEqual(hover.colors);
        expect((await observed(page)).requests).toBe(before.requests);
        await page.mouse.move(2, 2); await page.mouse.up(); await frames(page);
        expect((await observed(page)).requests).toBe(before.requests);
        await info.attach("production-fidelity.png", { body: pressed.png, contentType: "image/png" });
        await pane(page, "editor");
        await info.attach("production-editor.png", { body: await page.screenshot(), contentType: "image/png" });
      });
    }
  }
}

test("production groups reserve identical Light/Dark border and separator geometry", async ({ page }) => {
  await page.setViewportSize({ width: 715, height: 640 });
  await boot(page);
  const geometry: number[][] = [];
  for (const appearance of ["apple-light", "apple-dark"]) {
    await host(page, "appearance", appearance);
    await pane(page, "graph");
    const group = await reveal(page, "settings-graph-group");
    const separator = await probe(page, "settings-graph-group-separator-0");
    const first = await probe(page, "compact-toggle");
    const second = await probe(page, "lane-compact-toggle");
    expect(separator.height).toBe(1);
    // 12pt row inset inside the identically reserved 1px outer border.
    expect(separator.x - group.x).toBe(13);
    expect(group.x + group.width - separator.x - separator.width).toBe(13);
    expect(separator.y).toBeGreaterThanOrEqual(first.y + first.height);
    expect(separator.y + separator.height).toBeLessThanOrEqual(second.y);
    geometry.push([group.x, group.y, group.width, group.height, separator.x, separator.y, separator.width, separator.height, first.y, second.y]);
    await page.mouse.move(2, 2); await frames(page);
    const paint = await paintedPixels(page, [
      [separator.x + separator.width / 2, separator.y],
      [separator.x + separator.width / 2, separator.y - 2],
      [group.x, group.y + group.height / 2],
      [group.x + 2, group.y + group.height / 2],
    ]);
    expect(paint.colors[0]).not.toEqual(paint.colors[1]);
    if (appearance === "apple-light") expect(paint.colors[2]).not.toEqual(paint.colors[3]);
  }
  expect(geometry[1]).toEqual(geometry[0]);
});

for (const [language, appearance] of [["en", "apple-light"], ["ja", "apple-dark"]] as const) {
  test(`sidebar overflow preserves original roles at 715px 200%: ${language}`, async ({ page }, info) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 715, height: 640 });
    await boot(page);
    await host(page, "language", language);
    await host(page, "appearance", appearance);
    for (const zoom of [1, 1.67]) {
      await host(page, "zoom", String(zoom));
      const sidebar = await probe(page, "settings-sidebar");
      expectSnappedLength(sidebar.width, 212 * zoom);
      for (const destination of panes) {
        const item = await probe(page, `settings-pane-${destination}`);
        expectSnappedLength(item.height, 32 * zoom);
      }
    }
    await host(page, "zoom", "2");
    await pane(page, "appearance");
    let viewport = await probe(page, "settings-sidebar");
    expect(viewport.width).toBeLessThan(212 * 2);
    const contains = (outer: Probe, inner: Probe) => {
      expect(inner.x, `${inner.id} left`).toBeGreaterThanOrEqual(outer.x);
      expect(inner.y, `${inner.id} top`).toBeGreaterThanOrEqual(outer.y);
      expect(inner.x + inner.width, `${inner.id} right`).toBeLessThanOrEqual(outer.x + outer.width);
      expect(inner.y + inner.height, `${inner.id} bottom`).toBeLessThanOrEqual(outer.y + outer.height);
    };
    for (const destination of panes) {
      contains(viewport, await probe(page, `settings-pane-${destination}`));
    }
    await info.attach("sidebar-640h-200.png", { body: await page.screenshot(), contentType: "image/png" });
    // All eight rows fit at 640h. Exercise real overflow with a genuinely
    // shorter browser viewport, without shrinking production row geometry.
    await page.setViewportSize({ width: 715, height: 500 }); await frames(page);
    viewport = await probe(page, "settings-sidebar");
    const before = await observed(page);
    const lastBefore = await probe(page, "settings-pane-general");
    expect(lastBefore.y + lastBefore.height).toBeGreaterThan(viewport.y + viewport.height);
    for (const [index, destination] of panes.entries()) {
      const item = await probe(page, `settings-pane-${destination}`);
      expect(item.focused).toBe(true);
      expect(item.name.length).toBeGreaterThan(0);
      expectSnappedLength(item.height, 64);
      contains(viewport, item);
      contains(viewport, await probe(page, `${item.id}-focus-ring`));
      expect((await observed(page)).pane).toBe("appearance");
      expect((await observed(page)).requests).toBe(before.requests);
      if (index < panes.length - 1) {
        await page.keyboard.press("ArrowDown"); await frames(page);
      }
    }
    await page.keyboard.press("Enter"); await frames(page);
    expect((await observed(page)).pane).toBe("general");
    expect((await probe(page, "settings-pane-general")).selected).toBe(true);
    await page.keyboard.press("Home"); await frames(page);
    contains(viewport, await probe(page, "settings-pane-appearance-focus-ring"));
    await page.keyboard.press("Enter"); await frames(page);
    const firstBeforeWheel = await probe(page, "settings-pane-appearance");
    const beforeWheel = (await observed(page)).requests;
    await page.mouse.move(viewport.x + viewport.width / 2, viewport.y + viewport.height / 2);
    await page.mouse.wheel(0, 1000); await frames(page);
    const browsed = await probe(page, "settings-pane-appearance");
    expect(browsed.y).toBeLessThan(firstBeforeWheel.y);
    contains(viewport, await probe(page, "settings-pane-general"));
    await info.attach("sidebar-overflow-500h-200.png", { body: await page.screenshot(), contentType: "image/png" });
    await host(page, "reseed");
    expect((await probe(page, "settings-pane-appearance")).y).toBeCloseTo(browsed.y);
    expect((await observed(page)).requests).toBe(beforeWheel);
    await page.setViewportSize({ width: 715, height: 640 }); await frames(page);
    viewport = await probe(page, "settings-sidebar");
    for (const destination of panes) {
      await pane(page, destination);
      const item = await probe(page, `settings-pane-${destination}`);
      expect(item.selected).toBe(true);
      contains(viewport, item);
      await info.attach(`${destination}-200.png`, { body: await page.screenshot(), contentType: "image/png" });
    }
    await pane(page, "appearance");
    for (const prefix of ["zoom", "compact-threshold"]) {
      await reveal(page, `${prefix}-inc`);
      const decrease = await probe(page, `${prefix}-dec`);
      const increase = await probe(page, `${prefix}-inc`);
      const value = await probe(page, `${prefix}-value`);
      const detail = await probe(page, "settings-detail-scroll");
      const row = await probe(page, prefix === "zoom" ? "settings-zoom-row" : "settings-threshold-row");
      for (const control of [decrease, value, increase]) {
        contains(detail, control);
        contains(row, control);
      }
      expect(increase.x + increase.width - decrease.x).toBeGreaterThanOrEqual(224 - 0.5);
      expect(Math.abs(decrease.y + decrease.height / 2 - increase.y - increase.height / 2)).toBeLessThanOrEqual(0.5);
      await info.attach(`${prefix}-200.png`, { body: await page.screenshot(), contentType: "image/png" });
    }
  });
}

for (const [language, appearance, zoom] of [["en", "apple-light", 1], ["ja", "apple-dark", 1.67]] as const) {
  test(`production popup eligibility and live model catalogs: ${language} ${zoom}`, async ({ page }, info) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 715, height: 640 });
    await boot(page);
    await host(page, "language", language);
    await host(page, "appearance", appearance);
    await host(page, "zoom", String(zoom));
    await pane(page, "smart_commit");
    const beforeProvider = (await observed(page)).requests;
    await click(page, "sc-provider-select");
    expect((await observed(page)).controls.some(control => control.id === "settings-popup-menu")).toBe(true);
    const trigger = await probe(page, "sc-provider-select");
    const menu = await probe(page, "settings-popup-menu");
    // A below-trigger popup can overlap the trigger's centre at JA/167%.
    // Click its actual exposed top band, not the menu's padding.
    const point = { x: trigger.x + trigger.width / 2, y: (trigger.y + Math.min(trigger.y + trigger.height, menu.y)) / 2 };
    expect(point.y).toBeGreaterThan(trigger.y);
    expect(point.y).toBeLessThan(trigger.y + trigger.height);
    expect(point.y).toBeLessThan(menu.y);
    await info.attach("popup-reclick-hit.json", { body: JSON.stringify({ trigger, menu, point }), contentType: "application/json" });
    await page.mouse.click(point.x, point.y); await frames(page);
    expect((await observed(page)).controls.some(control => control.id === "settings-popup-menu")).toBe(false);
    // The pointer path must not replace keyboard activation or reopen on release.
    await page.keyboard.press("Space"); await frames(page);
    expect((await observed(page)).controls.some(control => control.id === "settings-popup-menu")).toBe(true);
    await host(page, "close");
    await host(page, "reopen");
    expect((await observed(page)).controls.some(control => control.id === "settings-popup-menu")).toBe(false);
    expect((await observed(page)).smartProvider).toBe("ollama");
    await click(page, "sc-provider-select");
    const providers = (await observed(page)).controls.filter(control => control.role === "MenuItemRadio");
    expect(providers.map(control => control.id)).toEqual(expect.arrayContaining([
      "sc-provider-ollama", "sc-provider-claude-code", "sc-provider-codex",
    ]));
    expect(providers.filter(control => control.selected)).toHaveLength(1);
    expect(providers.find(control => control.id === "sc-provider-claude-code")!.disabled).toBe(true);
    await page.keyboard.press("ArrowUp"); await frames(page);
    expect((await observed(page)).requests).toBe(beforeProvider);
    await info.attach("provider-menu.png", { body: await page.screenshot(), contentType: "image/png" });
    await page.keyboard.press("Enter"); await frames(page);
    expect((await observed(page)).smartProvider).toBe("codex");
    await click(page, "sc-provider-select");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter"); await frames(page);
    expect((await observed(page)).smartProvider).toBe("ollama");
    for (const count of [0, 1, 20]) {
      const models = Array.from({ length: count }, (_, index) =>
        `モデル-${index}-long-local-model-name-01234567890123456789`);
      await host(page, "models", JSON.stringify(models));
      const before = await observed(page);
      const opener = await probe(page, "sc-model-select");
      expect(opener.disabled).toBe(count === 0);
      await click(page, "sc-model-select");
      if (count === 0) {
        expect((await observed(page)).controls.some(control => control.id === "settings-popup-menu")).toBe(false);
        expect((await observed(page)).smartModel).toBe(before.smartModel);
        expect((await observed(page)).requests).toBe(before.requests);
        continue;
      }
      // The previous saved value remains visible but disabled, before the new
      // eligible catalog. Real menu keys must skip it without reordering data.
      if (before.smartModel && !models.includes(before.smartModel)) {
        expect((await probe(page, `sc-model-${before.smartModel}`)).disabled).toBe(true);
      }
      await page.keyboard.press(count === 1 ? "ArrowDown" : "ArrowUp");
      await frames(page);
      expect((await observed(page)).smartModel).toBe(before.smartModel);
      expect((await observed(page)).requests).toBe(before.requests);
      await page.keyboard.press("Enter"); await frames(page);
      expect((await observed(page)).smartModel).toBe(models.at(-1));
      expect((await observed(page)).requests).toBe(before.requests + 1);
      const group = await probe(page, "settings-smart-group");
      const modelRow = await probe(page, "settings-model-row");
      const trigger = await probe(page, "sc-model-select");
      const triggerText = await probe(page, "sc-model-select-text");
      // A wrapped row must contribute its actual height to its group, not
      // merely keep its control somewhere inside the overall viewport.
      for (const control of [modelRow, trigger]) {
        expect(control.x).toBeGreaterThanOrEqual(group.x + 1);
        expect(control.x + control.width).toBeLessThanOrEqual(group.x + group.width - 1);
        expect(control.y).toBeGreaterThanOrEqual(group.y + 1);
        expect(control.y + control.height).toBeLessThanOrEqual(group.y + group.height - 1);
      }
      expect(trigger.y + trigger.height).toBeLessThanOrEqual(modelRow.y + modelRow.height);
      expect(triggerText.name).toBe(models.at(-1));
      expect(triggerText.value).toMatch(/(?:…|\.\.\.)$/);
      expect(Math.abs(triggerText.x - trigger.x - 8 * zoom)).toBeLessThanOrEqual(0.5);
      expect(Math.abs(triggerText.x + triggerText.width - trigger.x - trigger.width + 18 * zoom)).toBeLessThanOrEqual(0.5);
      await info.attach(`long-model-row-${count}.png`, { body: await page.screenshot(), contentType: "image/png" });
      await info.attach(`long-model-row-${count}.json`, { body: JSON.stringify({ group, modelRow, trigger, triggerText }), contentType: "application/json" });
      await click(page, "sc-model-select");
      const rows = (await observed(page)).controls.filter(control => control.role === "MenuItemRadio");
      expect(rows).toHaveLength(count);
      expect(rows.filter(control => control.selected)).toHaveLength(1);
      for (const [index, model] of models.entries()) {
        const item = await reveal(page, `sc-model-${model}`);
        expect(item.name).toBe(model);
        expect(item.disabled).toBe(false);
        if (index === 0 || index === models.length - 1) {
          const leaf = await probe(page, `${item.id}-text`);
          expect(leaf.name).toBe(model);
          expect(leaf.value).toMatch(/(?:…|\.\.\.)$/);
          expect(leaf.x).toBe(item.x);
          expect(leaf.width).toBe(item.width);
          expect(leaf.height).toBeLessThanOrEqual(Math.ceil(20 * zoom));
          const points = [[leaf.x + leaf.width - 1, leaf.y + 1]];
          for (let y = Math.ceil(leaf.y); y < leaf.y + leaf.height; y++) {
            for (let x = Math.ceil(leaf.x + leaf.width - 16 * zoom); x < leaf.x + leaf.width; x++) points.push([x, y]);
          }
          const paint = await paintedPixels(page, points);
          expect(paint.colors.slice(1).some(color => color.reduce((sum, value, channel) =>
            sum + Math.abs(value - paint.colors[0][channel]), 0) > 45),
          "ellipsis has actual ink at the usable trailing edge").toBe(true);
          await info.attach(`models-${count}-${index}.png`, { body: paint.png, contentType: "image/png" });
          await info.attach(`models-${count}-${index}.json`, { body: JSON.stringify({ item, leaf }), contentType: "application/json" });
          if (count === 20 && index === count - 1) {
            await page.mouse.move(2, 2); await frames(page);
            const point = { x: item.x + item.width / 2, y: item.y + item.height / 2 };
            await page.mouse.move(point.x, point.y);
            // Actual GPUI hover delay; the resulting full-name tooltip image
            // is visually reviewed, not inferred from the item's AX label.
            await page.waitForTimeout(1000); await frames(page);
            await info.attach("model-full-name-tooltip.png", { body: await page.screenshot(), contentType: "image/png" });
            await info.attach("model-tooltip-target.json", { body: JSON.stringify({ item, point, expectedFullName: model }), contentType: "application/json" });
          }
        }
      }
      const selected = (await observed(page)).smartModel;
      await host(page, "models", "[]");
      expect((await observed(page)).smartModel).toBe(selected);
      expect((await probe(page, "sc-provider-select")).focused).toBe(true);
      expect((await probe(page, "sc-model-select")).disabled).toBe(true);
      expect((await observed(page)).controls.some(control => control.id === "settings-popup-menu")).toBe(false);
    }
    expect((await observed(page)).smartEnabled).toBe(false);
    expect((await observed(page)).pendingConsent).toBe(false);
  });
}

test("catalog replacements preserve live theme focus and repair removed choices", async ({ page }, info) => {
  await page.setViewportSize({ width: 715, height: 640 });
  await boot(page);
  const name = "Zzz surviving focused custom palette — a deliberately long valid theme name";
  const slug = "fixture-long-caption";
  const id = `settings-theme-${slug}`;
  await host(page, "custom-theme-name", name);
  await click(page, "settings-theme-apple-light");
  const catalog = (await observed(page)).themeCatalog.filter(theme => !theme.dark);
  await page.keyboard.press("Home"); await frames(page);
  for (let index = 0; index < catalog.findIndex(theme => theme.slug === slug); index++) {
    await page.keyboard.press("ArrowRight"); await frames(page);
  }
  const before = await observed(page);
  expect((await probe(page, id)).focused).toBe(true);
  const focusedInside = async (controlId: string) => {
    const card = await probe(page, controlId);
    const ring = await probe(page, `${controlId}-focus-ring`);
    const strip = await probe(page, "settings-themes-light-scroll");
    const detail = await probe(page, "settings-detail-scroll");
    expect(card.focused).toBe(true);
    expect(ring.x).toBeGreaterThanOrEqual(strip.x);
    expect(ring.x + ring.width).toBeLessThanOrEqual(strip.x + strip.width);
    expect(ring.y).toBeGreaterThanOrEqual(detail.y);
    expect(ring.y + ring.height).toBeLessThanOrEqual(detail.y + detail.height);
  };
  await focusedInside(id);
  await info.attach("catalog-focus-before.png", { body: await page.screenshot(), contentType: "image/png" });
  // The core installer creates a new real ThemeKey, just as native reload does,
  // while the visible catalog order, selected slug and focused slug survive.
  await host(page, "custom-theme-name", name);
  const replaced = await observed(page);
  expect(replaced.themeCatalog.find(theme => theme.slug === slug)!.key)
    .not.toBe(before.themeCatalog.find(theme => theme.slug === slug)!.key);
  expect(replaced.themeCatalog.map(theme => theme.slug)).toEqual(before.themeCatalog.map(theme => theme.slug));
  expect(replaced.theme).toBe(before.theme);
  expect(replaced.events).toEqual(before.events);
  await focusedInside(id);
  await info.attach("catalog-focus-replaced.png", { body: await page.screenshot(), contentType: "image/png" });
  await host(page, "remove-custom-themes");
  const removed = await observed(page);
  expect(removed.themeCatalog.some(theme => theme.slug === slug)).toBe(false);
  expect(removed.controls.some(control => control.id === id)).toBe(false);
  expect(removed.theme).toBe(before.theme);
  expect(removed.events).toEqual(before.events);
  await focusedInside(`settings-theme-${before.theme}`);
  expect((await probe(page, `settings-theme-${before.theme}`)).selected).toBe(true);
  await info.attach("catalog-focus-removed.png", { body: await page.screenshot(), contentType: "image/png" });
});

for (const [language, appearance, zoom] of [["en", "apple-light", 1], ["ja", "apple-dark", 1.67]] as const) {
  test(`production text leaves and Analyze action rail: ${language} ${zoom}`, async ({ page }, info) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 715, height: 640 });
    await boot(page);
    await host(page, "appearance", appearance);
    await host(page, "language", language);
    await host(page, "zoom", String(zoom));
    await pane(page, "editor");
    const help = await probe(page, "settings-markdown-help");
    const text = await probe(page, "settings-markdown-help-text");
    const row = await probe(page, "settings-markdown-row");
    const preview = await probe(page, "settings-markdown-preview");
    const source = await probe(page, "settings-markdown-source");
    expect(preview.y).toBe(source.y);
    expect(preview.height).toBe(source.height);
    expect(Math.abs(preview.width - source.width)).toBeLessThanOrEqual(1);
    expect(preview.x + preview.width).toBeLessThan(source.x);
    if (zoom === 1) {
      expect((await probe(page, "settings-markdown-default-view-visual")).height).toBe(24);
    }
    // These are the actual StyledText layouts consumed by paint, not parent
    // boxes or an accessibility/string-copy surrogate for wrapping.
    expect(text.value).toBe(help.value);
    expect(text.width).toBeLessThanOrEqual(help.width);
    expect(text.height).toBeGreaterThan(text.paint!.fontSize * 1.5);
    expect(text.y + text.height).toBeLessThanOrEqual(row.y + row.height);
    expect(help.x + help.width <= preview.x || help.y + help.height <= preview.y).toBe(true);
    for (const control of [preview, source]) {
      const label = await probe(page, `${control.id}-label`);
      const leaf = await probe(page, `${control.id}-text`);
      expect(leaf.value).toBe(label.name);
      expect(leaf.width).toBeLessThanOrEqual(label.width);
      expect(leaf.x).toBeGreaterThanOrEqual(control.x);
      expect(leaf.x + leaf.width).toBeLessThanOrEqual(control.x + control.width);
      expect(leaf.y).toBeGreaterThanOrEqual(control.y);
      expect(leaf.y + leaf.height).toBeLessThanOrEqual(control.y + control.height);
    }
    const points = [[row.x + 1, row.y + 1]];
    for (let y = Math.floor(text.y + text.height / 2) + 1; y < text.y + text.height - 1; y += 2) {
      for (let x = Math.ceil(text.x); x < text.x + text.width; x += 2) points.push([x, y]);
    }
    const paint = await paintedPixels(page, points);
    const background = paint.colors[0];
    expect(paint.colors.slice(1).filter(color => color.reduce((sum, value, channel) =>
      sum + Math.abs(value - background[channel]), 0) > 45).length,
    "actual glyph ink in the lower wrapped description lines").toBeGreaterThan(5);
    await info.attach("editor-constrained-text.png", { body: paint.png, contentType: "image/png" });
    await info.attach("editor-text-layout.json", { body: JSON.stringify({ row, help, text, preview, source }), contentType: "application/json" });

    await pane(page, "analyze");
    const footer = async (state: string, disabled: boolean) => {
      await reveal(page, "settings-analyze-save");
      const viewport = await probe(page, "settings-detail-scroll");
      const group = await probe(page, "settings-analyze-group");
      const status = await probe(page, "settings-analyze-status");
      const reset = await probe(page, "settings-analyze-reset");
      const save = await probe(page, "settings-analyze-save");
      expect(save.disabled).toBe(disabled);
      expect(status.name.length).toBeGreaterThan(0);
      for (const control of [status, reset, save]) {
        expect(control.x).toBeGreaterThanOrEqual(viewport.x);
        expect(control.x + control.width).toBeLessThanOrEqual(viewport.x + viewport.width);
        expect(control.y).toBeGreaterThanOrEqual(viewport.y);
        expect(control.y + control.height).toBeLessThanOrEqual(viewport.y + viewport.height);
      }
      const statusText = await probe(page, "settings-analyze-status-text");
      const innerLeft = group.x + 1 + 12 * zoom;
      const innerRight = group.x + group.width - 1 - 12 * zoom;
      expect(status.x).toBeGreaterThanOrEqual(innerLeft - 0.5);
      expect(status.x + status.width).toBeLessThanOrEqual(innerRight + 0.5);
      expect(statusText.value).toBe(status.name);
      expect(statusText.x).toBeGreaterThanOrEqual(status.x);
      expect(statusText.x + statusText.width).toBeLessThanOrEqual(status.x + status.width);
      expect(statusText.y + statusText.height).toBeLessThanOrEqual(status.y + status.height);
      if (language === "ja" && state === "saved") {
        expect(statusText.height).toBeGreaterThan(statusText.paint!.fontSize * 1.5);
      }
      expect(Math.abs(status.x - group.x - 1 - 12 * zoom)).toBeLessThanOrEqual(0.5);
      expect(Math.abs(save.x + save.width - group.x - group.width + 1 + 12 * zoom)).toBeLessThanOrEqual(0.5);
      if (reset.y !== save.y) expect(reset.x + reset.width).toBe(save.x + save.width);
      await info.attach(`analyze-footer-${state}.png`, { body: await page.screenshot(), contentType: "image/png" });
      await info.attach(`analyze-footer-${state}.json`, { body: JSON.stringify({ viewport, group, status, statusText, reset, save }), contentType: "application/json" });
    };
    const edit = async (value: string) => {
      await click(page, "settings-analyze-input");
      await page.keyboard.press("Control+A");
      await page.keyboard.type(value);
      await expect.poll(async () => (await observed(page)).draft).toBe(value);
    };
    await footer("clean", true);
    await host(page, "save-mode", "deferred");
    await edit("first/\n");
    await footer("dirty", false);
    await click(page, "settings-analyze-save");
    expect((await observed(page)).pendingSaves).toBe(1);
    await footer("pending", false);
    await host(page, "ack", "success");
    await footer("saved", true);
    await edit("second/\n");
    await host(page, "save-mode", "failed");
    await click(page, "settings-analyze-save");
    await reveal(page, "settings-analyze-error");
    await footer("failed", false);
    expect((await observed(page)).baseline).toBe("first/\n");
    expect((await observed(page)).draft).toBe("second/\n");
  });
}
