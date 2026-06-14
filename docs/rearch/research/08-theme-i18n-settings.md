# 08 — Theme / i18n / Settings / UI Zoom / Component Layer

- Status: Research (sub-agent #8, Kagi v1.0 re-architecture). RESEARCH ONLY.
- Date: 2026-06-14 / Branch: re-architecture
- Domain: theme, UI zoom, i18n, settings persistence, gpui-component reuse boundary.
- Primary sources (read in full): `src/ui/theme.rs` (1173 L), `src/ui/i18n.rs` (1024 L),
  `src/ui/assets.rs`, `src/ui/mod.rs` (zoom apply @9085, gpui-component init @16554-16591),
  `src/ui/tabs.rs` (session persistence @256-260/595-631), `src/ui/smart_commit.rs` (settings keys),
  `tests/i18n_test.rs`, ADRs 0036/0048/0006/0034/0031, `docs/research/gpui-component-audit.md` (383 L),
  `docs/research/zed-gpui-reuse-research.md`, and the **gpui-component 0.5.1 crate source**
  (`~/.cargo/.../gpui-component-0.5.1/src/`).
- Layering target: **domain** (pure: settings model + i18n keys/enums) → **app** (persistence,
  active theme/lang/zoom as AppState) → **ui** (theme tokens, thin wrappers over gpui-component,
  i18n lookup). Invariant: UI never calls git2 directly.

---

## 1. Kagi 現状

### 1.1 Theme tokens (`src/ui/theme.rs`, ADR-0036)

- **Single source**: `pub struct Theme` with ~80 **semantic** `u32` `0xRRGGBB` fields
  (backgrounds bg_base/bg_row_alt/surface/selected/panel/sidebar/modal/modal_overlay;
  text main/sub/muted/label; ref colours head/branch/remote/tag; status success/warning/blocker(+muted);
  diff added/removed/hunk; 6 change-kind badges; accent) + non-RGB extras: `lane_hsl: [(f32,f32,f32);6]`
  (graph lanes), `avatar_sat/avatar_light`, 19 terminal RGB triples + `term_selection` RGBA, `dark: bool`,
  `slug`, `name`.
- **Access = global atomic, zero signature churn**: `static ACTIVE: AtomicUsize` indexes
  `pub static THEMES: &[Theme]` (6 themes; index 0 = Catppuccin Mocha default, byte-exact port).
  `theme() -> &'static Theme` is called **every render frame**; switching = atomic store + `cx.notify()`.
  Helpers: `index_of(slug)`, `active_index()` (menu ✓), `badge_style(color)` (dark/light chip grammar),
  `Theme::lane_color(i)`.
- **6 themes**: catppuccin (default), xcode-dark, xcode-light, one-dark, one-light, monokai. 4 dark / 2 light.
- **Startup priority**: `KAGI_THEME` env (must be a known slug) → persisted `settings.json` `"theme"` →
  default. `init_active()` logs `[kagi] theme: <slug> dark=<bool>`.

### 1.2 i18n table (`src/ui/i18n.rs`, ADR-0048)

- **Dependency-free** enum-key + exhaustive `match` table. `enum Lang { En, Ja }` mirrors theme:
  `static ACTIVE: AtomicUsize` (0=En, 1=Ja), `lang()` read per render, `set_lang()` persists `"lang"`.
- `enum Msg` (~170 variants) → `Msg::t(self) -> &'static str` matches `(lang(), self)`. **Missing
  translation = compile error** (no fluent/gettext crate — dependency-purity rule).
- Parameterized strings are plain `fn`s here (`wip_row_note(n)`, `branch_exists_fmt(name)`,
  `branch_name_error(&BranchNameError)` etc.) so `format!` lives in i18n, not at call sites.
- **ADR-0048 invariant**: prose is localized; **Git domain words stay English in BOTH arms**
  (Pull/Push/Branch/Stash/Pop/Undo/commit/amend/checkout/cherry-pick/revert/discard/worktree/
  conflict/resolved/unresolved/merge/HEAD/upstream…). Tests pin this (`domain_words_stay_english_in_both_langs`).
- **Startup priority** (`resolve_lang`, no global mutation): `KAGI_LANG=en|ja` env → `settings.json`
  `"lang"` → `LC_ALL`/`LANG` starts with "ja" → En. `init_lang()` logs `[kagi] lang: <slug>`.
  `tests/i18n_test.rs` is an end-to-end stderr-log integration test of this priority.
- **Coupling note**: i18n imports `theme::{read_setting, write_setting}` — settings I/O currently lives
  inside `theme.rs`. i18n also reaches into `kagi::git::ops::{BranchNameError, WorktreePathError}` to
  localize keyed git-layer errors (the "keyed error → localized text" bridge is the only clean
  cross-layer pattern and should survive re-arch).

### 1.3 Uniform UI zoom (`theme.rs` @181-278, applied `mod.rs:9085`, ADR/ticket W27-UIPOLISH)

- Stored as **permille** integer in `static UI_ZOOM_PERMILLE: AtomicUsize` (1000 = 1.0×). Bounds
  `ZOOM_MIN=0.7 .. ZOOM_MAX=1.5`, step 0.1. `zoom()`, `clamp_zoom`, `set_zoom` (persists `"ui_zoom"`),
  `init_zoom()`.
- **Mechanism**: every frame `KagiApp::render` calls `window.set_rem_size(px(BASE_REM_PX * zoom()))`
  (BASE_REM_PX = 16, gpui default). Since Kagi uses `text_sm`/`text_xs` 260+ times (rem-based), this
  scales virtually all text. **Layout px do NOT scale via rem** → `scaled_px(n)` and `scaled(n)`
  (bare f32) helpers exist to scale literal layout dims (row heights, panel widths, graph lane/node
  geometry) so text↔layout don't drift on zoom (notably keeps the commit graph aligned with rem-scaled
  rows). gpui 0.2.2 has no global element-scale transform — this is the workaround.

### 1.4 Settings persistence (`theme.rs` @298-416)

- **No serde**. Hand-written flat JSON `{ "k": "v", ... }` in `~/.kagi/settings.json` (honours
  `KAGI_LOG_DIR`, else `$HOME`/`$USERPROFILE`). Mirrors `oplog.rs`.
- `settings_path()`, `parse_string_value(text, key)` (minimal scanner, no escapes on write side besides
  `settings_escape` for `"`/`\`), `read_setting(key)`, `write_setting(key, Option<&str>)`.
  `write_setting` **re-reads all known keys and rewrites the whole object** so independent writers don't
  clobber each other.
- **All values are strings** (slugs, model names, "0"/"1" flags, permille int as string, US-encoded list).
- `const SETTINGS_KEYS: [&str; 10]` is the **only registry**: `theme, lang, ui_zoom,
  smart_commit_llm_enabled, smart_commit_model, smart_commit_lang, smart_commit_style, session_repos,
  session_active, mergetool`. Writers must list their key here or it gets dropped on the next foreign
  write. `mergetool` is read-only from Kagi (user-set; ADR-0060). Env vars (`KAGI_THEME`, `KAGI_LANG`,
  `KAGI_LOG_DIR`) override but are **not persisted**. No `KAGI_COMPACT` exists in current source (the
  prompt mentioned it; only `Size`-style compactness exists inside gpui-component, not as a Kagi setting).
- **Distributed writers**: `tabs.rs` (session_repos/active — US `\u{1f}`-joined repo paths),
  `smart_commit.rs` (4 keys), `theme.rs` (theme/ui_zoom), `i18n.rs` (lang). All funnel through
  `theme::{read,write}_setting`. There is **no central settings module** — settings live in `theme.rs`
  by historical accident.

### 1.5 How much gpui-component is already used (audit: `docs/research/gpui-component-audit.md`)

- **Adopted today**: `Input`/`InputState` (IME, T025), `Icon`/`IconName` (+ `KagiAssets` embeds 12 lucide
  SVGs via `include_bytes!`), `Tooltip`, `Root`/`WindowExt` (window first layer), `highlighter`
  (`HighlightTheme` for CodeEditor/diff), `Sizable`. `gpui_component::init(cx)` at startup.
- **Theme bridge (one-way, W12-GCADOPT)**: `sync_gpui_component_theme(cx)` copies Kagi's `theme()` into
  `gpui_component::Theme::global_mut(cx).colors` (~40 of ~103 `ThemeColor` fields actually read) +
  `gc.mode` + overrides `gc.highlight_theme` editor surfaces. Called after `gpui_component::init` and on
  every theme switch. **Nothing reads back** — Kagi `theme()` stays single source (ADR-0036). This is the
  load-bearing pattern that lets Kagi adopt any gpui-component widget and force it into Kagi's palette.
- **Recommended-but-not-yet (audit §3)**: Scrollbar (high), `push_notification`/toast (high), real
  `Checkbox` (high), `Dialog`, `ContextMenuExt`/`PopupMenu`, `resizable`, `Progress`/`Spinner`/`Skeleton`,
  `Badge`. **Kept hand-rolled (deliberate)**: commit list (uniform_list + graph canvas), file tree
  (Git-status coupled), avatar, app menu bar (`cx.set_menus`), W10 vertical toolbar (Button can't stack
  icon-over-label).

---

## 2. 参考プロジェクトの実装方針

### 2.1 gpui-component 0.5.1 (read from crate source — corrects the audit's gaps)

- **Theme**: `Theme` is a `gpui::Global` holding `colors: ThemeColor` (103 Hsla fields) + `mode:
  ThemeMode` + `font_size`/`mono_font_size: Pixels` + `radius`/`radius_lg` + `highlight_theme`. There is
  a **JSON theme system Kagi is not using**: `theme/schema.rs` (`ThemeSet`/`ThemeConfig`/
  `ThemeConfigColors`, serde), `theme/registry.rs` (`ThemeRegistry` global, `themes()` map keyed by name,
  `watch_dir(path, cx, on_load)` hot-reload, default themes from bundled `default-theme.json`).
  `Theme::change(mode, window, cx)` swaps light/dark configs; `apply_config(&ThemeConfig)` pushes a config
  into the live theme. So gpui-component can load user JSON themes from a dir and hot-reload them.
- **i18n (Kagi is not using)**: `lib.rs:92` `rust_i18n::i18n!("locales", fallback="en")`; `locale()`/
  `set_locale(&str)` re-export rust_i18n; the crate ships `locales/ui.yml` (translations for its own
  built-in widget strings, e.g. dialog OK/Cancel). So **gpui-component already localizes its own
  component chrome** via rust_i18n — Kagi's Esc/OK/Cancel etc. inside adopted Dialogs would follow
  `set_locale`, independent of Kagi's `Msg` table.
- **Sizing/scaling**: `styled.rs` `enum Size { XSmall, Small, Medium, Large, Size(px) }` + `Sizable`
  trait (`with_size`, `xsmall/small/large`). Components size off `Size` (fixed px tables, e.g. button
  height 26/30/34/40) and off `theme.font_size`/`radius` — **gpui-component does NOT do a global rem
  zoom**; its scaling is per-component `Size` + theme font_size. There is no single "UI zoom" knob.
- **No settings persistence** in gpui-component — it expects the host app to own config.

### 2.2 Zed theme & settings (`crates/theme`, `crates/settings`; GPL — concept only, ADR-0031/0034)

- **Theme**: JSON theme files → `ThemeRegistry` (load/list/get by name, user theme dirs, family/appearance
  split light/dark). `ThemeColors` is a large named-token struct (very close to Kagi's semantic model and
  gpui-component's `ThemeColor`). System-appearance follow. **Code is GPL → pattern only**: the
  registry + JSON-token + light/dark-pair pattern is what to imitate (and gpui-component already
  implements an Apache version of it).
- **Settings**: layered JSON merge (default → user → project → language-specific) with a typed schema +
  JSON-schema generation + file watching. **GPL → Kagi must hand-roll persistence** (ADR-0034 explicitly:
  "設定の永続化・マージは kagi 独自実装"). The *layering* idea (defaults + user override) is the
  transferable concept; Kagi's flat KV is the minimal end of that spectrum.
- gpui core (Action/Keymap/`actions!`) is Apache and already adopted — keybindings are out of this
  domain but settings for them would live in the same settings layer.

### 2.3 OpenLogi / other

- Not theme/i18n/settings-relevant for this domain (OpenLogi learnings doc is about Git UX, not config).
  No transferable settings infra.

---

## 3. 採用すべき設計 (recommended for v1.0)

### 3.0 Three-layer split (the core re-arch move)

Today `theme.rs` is a god-module: theme registry **+** settings file I/O **+** zoom **+** gpui-component
bridge, and i18n/tabs/smart_commit all reach into it. Split along the target layering:

- **domain** (pure, no gpui, no fs):
  - `settings model`: a typed `struct Settings { theme: String, lang: Lang, ui_zoom: Zoom,
    smart_commit: SmartCommitCfg, session: SessionCfg, mergetool: Option<String> }` with
    parse/serialize to/from a flat KV (or serde — see §3.2) and clamping/validation. Pure, unit-testable.
  - `i18n keys`: `enum Lang`, `enum Msg`, `Msg::t`, param helpers — **already pure**, just move out of
    `ui/` into a domain/i18n crate-module. The `BranchNameError → text` bridge stays at the seam
    (domain i18n can depend on domain git error enums — both pure).
- **app** (owns state + persistence, no rendering):
  - A single `Config`/`Settings` service that owns the **persistence layer** (read/write the file once at
    startup, hold the parsed `Settings` as `AppState`, write-through on change). Replaces the scattered
    `theme::write_setting` calls. Active theme index / lang / zoom become fields of AppState (or stay
    atomics seeded from it — see §3.3). One writer = no clobber-by-foreign-write hazard, the
    `SETTINGS_KEYS` registry becomes the struct fields.
- **ui**: `theme tokens` (the `Theme` struct + `THEMES` table + `theme()` accessor stay, but
  *resolution/persistence* moves to app), thin component wrappers, `Msg::t` lookups, the one-way
  gpui-component bridge.

### 3.1 Settings / persistence layer (config file under `~/.kagi/`)

- **Keep `~/.kagi/settings.json` + `KAGI_LOG_DIR` override** (don't break existing user files / tests).
- **Centralize** into one `app::settings` module that owns the file and exposes a typed `Settings` plus
  `load()` (once at startup) and `set_*`/`save()` (write-through). Distributed `write_setting("…")` callers
  (tabs, smart_commit, i18n, theme, zoom) call typed setters instead.
- **Adopt serde_json for the read/write** of this single struct. The "dependency-purity / mirror oplog"
  rationale for the hand-rolled parser was about avoiding fluent/gettext-class deps; `serde`/`serde_json`
  are already in the gpui-component dependency tree (it uses them for `ThemeConfig`), so adding them as a
  direct dep costs nothing new and removes the fragile `parse_string_value` scanner, the manual
  `SETTINGS_KEYS` round-trip, and the all-values-are-strings limitation (zoom can be a real number,
  session a real list). *If* the team wants to keep zero new direct deps, the flat-KV approach still works
  — but then keep it behind the typed `Settings` API so call sites never touch raw keys. **Recommendation:
  serde, behind the typed boundary.**
- **Forward-compat**: parse unknown keys into a `#[serde(flatten)] extra: Map` (or keep round-tripping)
  so a newer Kagi's keys survive an older binary — preserves today's "don't drop foreign keys" property.

### 3.2 Theme system — keep Kagi's custom registry; do NOT migrate to gpui-component ThemeProvider

- **Keep**: the `Theme` semantic-token struct + `THEMES` + atomic `theme()` + per-frame read. It is the
  single source of truth (ADR-0036), drives terminal/graph/avatar/diff (things gpui-component's
  `ThemeColor` does **not** model), and the byte-exact Catppuccin default + tests are a regression anchor.
- **Keep the one-way bridge** `sync_gpui_component_theme` as the only coupling to gpui-component's
  `ThemeColor`/`mode`/`highlight_theme`. As more components are adopted, extend the mapped-field set; never
  read back.
- **Do not** put gpui-component's `ThemeProvider`/`ThemeRegistry`/`Theme::change` in charge — that would
  create the dual-source-of-truth the audit §0 warns against, and it can't express Kagi's lane/terminal/
  avatar tokens.
- **Optional future** (later ticket, not v1.0 blocker): expose Kagi themes as **user-editable JSON** under
  `~/.kagi/themes/`. Two clean options: (a) Kagi's own tiny JSON schema → `Vec<Theme>` (THEMES becomes a
  Vec, ADR-0036 already anticipates this); (b) reuse gpui-component's `ThemeRegistry::watch_dir` for the
  *gpui-component* `ThemeColor` half only. (a) is preferred to keep the single semantic model.

### 3.3 i18n — keep the enum+match table; reuse the pure code, set gpui-component locale alongside

- **Keep** `enum Msg` + exhaustive `match` + ADR-0048 domain-words-stay-English. The compile-time
  completeness guarantee and zero-dep purity are real wins; don't swap to rust_i18n/fluent for Kagi's
  own strings.
- **Move it to the domain layer** (it's already pure except the `theme::{read,write}_setting` import for
  persistence and the git-error bridge). Persistence goes through §3.1; the git-error bridge stays.
- **Bridge to gpui-component's rust_i18n**: when Kagi sets its language, also call
  `gpui_component::set_locale("ja"/"en")` so adopted Dialog/Notification/Input chrome (OK/Cancel/etc.)
  matches. This is new but tiny and prevents English component chrome under a Japanese UI. (gpui-component
  ships `locales/ui.yml`; "ja" coverage there should be spot-checked — see §6.)
- **Keep `KAGI_LANG` + `LANG`/`LC_ALL` resolution and the stderr log line** — `tests/i18n_test.rs`
  depends on `[kagi] lang: <slug>`.

### 3.4 Uniform UI zoom — keep the rem-size approach; consolidate the scaled() helpers

- **Keep** `window.set_rem_size(BASE_REM_PX * zoom())` per frame + `scaled_px`/`scaled` for layout dims.
  gpui-component offers **no global zoom** (only per-component `Size` + `theme.font_size`), so the rem
  approach remains Kagi's only uniform-zoom mechanism on gpui 0.2.2.
- **Refinements**: (1) store zoom as a typed `Zoom(f32)` in `Settings` (validated/clamped at the
  boundary) instead of a free atomic + permille string. (2) Optionally also drive
  `gpui_component::Theme.font_size = px(16 * zoom)` so adopted components' intrinsic text tracks the same
  zoom (today only rem-based text scales; gpui-component widgets use `theme.font_size`, which is currently
  left at 16). (3) Keep an atomic mirror for the per-frame hot path if `Settings` lives behind a lock —
  `theme()`/`zoom()`/`lang()` are read every frame and must stay lock-free.

### 3.5 Thin Kagi component layer over gpui-component

- **Adopt the audit's high/medium wins as thin wrappers** in a `ui::components` module so call sites use
  Kagi-flavoured helpers, not raw gpui-component types: Scrollbar overlay, `push_notification` (toast),
  real `Checkbox`, `Dialog`, `ContextMenuExt`/`PopupMenu`, `resizable` (with size persisted via §3.1),
  `Progress`/`Spinner`/`Skeleton`, `Badge`. Each wrapper: (a) takes Kagi domain types, (b) relies on the
  one-way theme bridge for colour, (c) localizes via `Msg::t`, (d) keeps headless-log parity.
- **Lean on gpui-component** for: Input(IME), Icon, Tooltip, Root, highlighter, Scrollbar, Dialog,
  Notification, Checkbox, Progress/Spinner, Badge, Resizable, PopupMenu/ContextMenu — i.e. stateless/
  low-coupling widgets where Kagi has no special Git semantics.
- **Keep hand-rolled** (Git-semantics-heavy or already optimized): commit list (uniform_list + graph
  canvas), file tree, avatar (Kagi colour calc + GitHub W11), app menu bar (`cx.set_menus`), W10 vertical
  toolbar layout. The boundary rule: *if the widget needs Git/graph/status knowledge or per-frame canvas
  control, hand-roll it; otherwise wrap gpui-component.*

---

## 4. 採用しない設計

- **gpui-component `ThemeProvider`/`ThemeRegistry`/`Theme::change` as the source of truth** — dual SoT;
  can't model lane/terminal/avatar tokens. (Keep the one-way bridge only.)
- **rust_i18n / fluent / gettext for Kagi's own strings** — loses compile-time completeness + adds a dep;
  ADR-0048 already rejected this. (Only *call* gpui-component's `set_locale` for *its* chrome.)
- **Zed `crates/settings` / `crates/theme` code** — GPL, Kagi is non-GPL; pattern-only (ADR-0031/0034).
- **Layered/project-scoped settings merge (Zed-style)** in v1.0 — over-engineered for a single
  per-user config; one user file is enough. (Door left open via §3.1's typed model.)
- **Per-component `Size`-based scaling as the zoom story** — doesn't give uniform zoom; rem approach stays.
- **Replacing commit list / file tree / avatar with gpui-component** — audit-confirmed net-negative.
- **A `KAGI_COMPACT` density mode** — not in current source; out of scope unless explicitly requested.

---

## 5. リスク

- **Per-frame hot path**: `theme()`, `zoom()`, `lang()` are read every render. If settings move behind a
  `Mutex`/`RwLock`, contention or lock-in-render is a regression risk. Mitigation: keep lock-free atomic
  mirrors (current pattern) seeded from `Settings` at load and on each setter; `Settings` struct is the
  write-side/persistence owner, atomics are the read-side cache.
- **Foreign-key clobber**: today's "re-read all known keys, rewrite whole object" guards multi-writer
  files. A naive serde `Settings` that doesn't `#[serde(flatten)]` extras would **drop unknown keys**
  written by a newer/other binary. Must preserve the round-trip (§3.1).
- **Settings I/O lives in `theme.rs`**: moving it is a wide mechanical change (i18n, tabs, smart_commit all
  import `theme::{read,write}_setting`). Risk of breaking the `KAGI_LOG_DIR` test seam used by
  `i18n_test.rs` and oplog. Keep `settings_path()` semantics identical.
- **gpui-component locale coverage**: `set_locale("ja")` is only useful if `locales/ui.yml` has Japanese
  for the chrome strings Kagi surfaces (Dialog buttons etc.). Unverified — may need a Kagi-supplied
  override or accepting English chrome in those widgets.
- **Zoom drift**: any newly adopted gpui-component widget that sizes off `theme.font_size` (not rem) won't
  follow Kagi's zoom unless §3.4(2) is done; mixed-scale UI is a subtle visual bug.
- **JSON theme migration (if pursued)**: turning `THEMES` from a `&'static` slice into runtime-loaded Vec
  changes `theme()`'s `&'static` return (it relies on static lifetime). Would need an arena/`OnceCell`/
  `Box::leak` or an `Arc<Theme>` swap — non-trivial; defer past v1.0.
- **serde adoption**: low risk (already transitively present), but it's a *policy* decision vs the
  "dependency-purity / mirror oplog" rationale in ADR-0036/0048; needs an ADR note either way.

## 6. 未解決事項

- **serde vs keep hand-rolled KV** for the central settings struct — needs a call (ADR addendum to 0036).
  Recommendation: serde behind a typed boundary; confirm with PM.
- **Do active theme/lang/zoom stay process-global atomics, or become AppState fields read via `cx`?**
  Atomics are simplest for the per-frame accessor and headless tests; AppState is "cleaner" layering but
  forces a `cx`/`Window` into every `theme()`/`Msg::t` call site (260+ sites). Likely keep atomics,
  document them as the read-cache of the app `Settings`. Needs PM ruling.
- **Where exactly does the domain/i18n module live** (own crate vs module) given `Msg` is read from
  `ui/`? And does the git-error→text bridge belong in domain-i18n or at the app seam?
- **`gpui_component::set_locale` coverage for "ja"** — audit `locales/ui.yml`; decide whether to ship a
  Kagi override file or accept English component chrome.
- **User-editable JSON themes under `~/.kagi/themes/`** — wanted for v1.0 or later? If yes, Kagi-schema
  (Vec<Theme>) vs gpui-component `ThemeRegistry::watch_dir`; resolve the `&'static` lifetime of `theme()`.
- **Should UI zoom also set `gpui_component::Theme.font_size`** so adopted widgets track zoom? (§3.4(2)) —
  needs a visual decision once more components are adopted.
- **Window/per-tab vs global** for theme/lang/zoom — current model is global; confirm v1.0 keeps it global
  (multi-window is unsupported today per `MultiWindowUnsupported`).
