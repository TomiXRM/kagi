import { test, expect, type Page } from "@playwright/test";

type Probe = { id: string; x: number; y: number; width: number; height: number; focused: boolean; selected: boolean; disabled: boolean };
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
  expect(before.scrollMax).toBeGreaterThan(0);
  await page.keyboard.press("PageDown");
  await frames(page);
  const down = await observed(page);
  expect(down.scrollY).toBeLessThan(before.scrollY);
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
