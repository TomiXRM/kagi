# ADR-0036: カラーテーマ機構

- Status: Accepted / Date: 2026-06-13

## Context

色は現在 Catppuccin Mocha の `const u32` が mod.rs / inspector.rs / sidebar.rs /
context_menu.rs / terminal.rs 等に分散ハードコード。ユーザー要望:
Xcode Dark / Xcode Light / One Dark / One Light / Monokai(実体は tomixrm Warm Hybrid、
`docs/research/reference/tomixrm-warm-hybrid.json`、MIT)を追加し、メニューバーから切替。

## Decision

- **単一ソース `src/ui/theme.rs`**: `pub struct Theme` に**意味名**フィールドを定義
  (bg_base/surface/selected/panel/sidebar/modal/overlay、text_main/sub/muted/label、
  color_head/branch/remote/tag/success/warning/blocker、diff added/removed 背景、
  change-kind 5色、lane 6色、terminal 16色+selection、`dark: bool`)。
  既存 const は全て theme フィールドへ移行し、モジュール内の重複 const を廃止する
- **アクセスは global atomic**(シグネチャ churn 回避):
  ```rust
  static ACTIVE: AtomicUsize;            // index into THEMES
  pub fn theme() -> &'static Theme       // どこからでも theme().bg_base
  pub static THEMES: &[Theme]            // 6 entries(先頭 = Catppuccin Mocha = default)
  ```
  切替 = index 更新 + cx.notify(全 render が theme() を毎フレーム参照するため追加伝播不要)
- **テーマ定義 6 種**: Catppuccin Mocha(現行値を厳密移植)/ Xcode Dark / Xcode Light /
  One Dark / One Light / Monokai(= tomixrm Warm Hybrid の colors/tokenColors から抽出)。
  light テーマは反転前提の箇所(avatar 文字色等)も意味名フィールドで吸収する
- **メニュー**: View > Theme サブメニュー(Command Registry 経由、`theme.<slug>` command)。
  アクティブ項目は label 先頭 "✓ "。**label が変わるため切替時に `cx.set_menus` を再呼び出し**
  (disabled 機構と異なり再構築が必要)
- **連動**: terminal は `TerminalView::update_config` で生きている session にも適用。
  diff syntax highlight は dark → `HighlightTheme::default_dark()` / light → `default_light()`
- **永続化**: `~/.kagi/settings.json` の `{"theme": "<slug>"}`(oplog と同じ手書き JSON 方式、
  KAGI_LOG_DIR 対応)。起動時に読込、切替時に保存
- headless: `KAGI_THEME=<slug>` 起動上書き + `[kagi] theme: <slug> dark=<bool>` ログ

## Consequences

- 全 UI モジュールの `rgb(CONST)` を `rgb(theme().field)` に置換する大規模だが機械的な変更
- graph の lane_color / avatar_color(hsla 直計算)も theme の lane 配列・dark 判定に寄せる
- 将来のユーザー定義テーマ(JSON 読込)は THEMES を Vec 化すれば拡張可能(later)

## Amendment — 2026-10-11: operation roles, not RGB inference (#1079)

The token owner is now `crates/kagi-ui-core/src/theme.rs` and the per-theme
files. `KagiButton` callers pass an explicit `ButtonRole`: Primary/Current use
filled Primary, Incoming uses filled Info, success/warning/danger actions use
the existing translucent tint, neutral actions explicitly choose ghost or the
editor's neutral tint. Reusing an RGB value across theme tokens cannot alter
the operation's variant. In particular Color Vision's shared blue/orange
tokens remain intentional; tinted side choices were an inference bug, not a
colour-vision design exception. See [the role table](../themes.md#button-operation-roles).

## Amendment — 2026-10-11: own filled Info tokens (#1078)

The bridge maps all four `button_info*` tokens from `color_remote`, not the
upstream mode preset. The resting fill exactly matches Incoming pane/marker;
foreground and hover/active use the same contrast-preserving opaque-fill
derivation as Primary. Generic `info` notices keep their prior branch colour,
and status/destructive tinted actions are unchanged.

## Amendment — 2026-10-11: independent body-link foreground (#1077)

`Theme.link` is a distinct RGB token copied into gpui-component `colors.link`,
not inferred from `color_branch`. Built-ins meet 4.5:1 on their body `bg_base`.
Apple Light uses `#0066cc`; already-legible dark themes keep their prior colour.
Other insufficient body-link shades change only this token, never the ref-chip
or Primary palette. Custom themes inherit or explicitly override `link`; a
standalone file that omits it defaults to its own `color_branch` for compatibility
with pre-link files. Existing TextView underlining, activation and selection
remain unchanged. See [the body-link guide](../themes.md#body-links).

## Amendment — 2026-10-14: 200% global UI zoom (#1553)

Extend the existing `theme::ZOOM_MAX` to 2.0 rather than introduce a second
text-size setting. Text and scaled geometry share the existing 14.4px/rem
base; 200% therefore doubles the current 100% presentation. The existing
10% stepping, shortcuts, permille settings store and legacy 156%/167%
values remain supported. Only 200% disables Increase; Decrease reaches 190%.
The Settings range description is updated in EN and JA.
Native Settings' existing pane TabList wraps when its natural label widths
exceed the existing 715px host. At 200% EN, the previous single centered
row placed General at x=-19.5 in a 715px viewport. Wrapping retains complete
labels, all four targets and the original one-stop/arrow-key owner without
shrinking text or widening the Settings host.

Reuse the existing bounded composer, toolbar overflow and compact navigator.
Compact Cmd-F defers its existing `open_changed` callback until the shortcut's
KagiApp update releases its mutable lease: the suspension guard reads that
same owner. The previous immediate call panicked on the real compact search
path; no new search, read-model or focus state is introduced.
The compact breakpoint remains strictly below 1000 logical px, independent
of zoom; exact 1000px continues exercising desktop layout. Native regression
`zoom_200_operable` covers 1000×720, 1400×900 and compact 940×720 across EN/JA
and Apple Light/Dark, including search input, footer containment/editing,
native Settings pane/stepper operations, persistence and reopening. This is
not OS Dynamic Type support or native VoiceOver-delivery evidence.

## Amendment — 2026-10-16: restrained Nagi presets (#1635)

Feedback: too many simultaneous UI colours obscure the information hierarchy.
Add `nagi-light` / `nagi-dark` through the existing `Theme` registry; keep Apple
Light / Dark, existing custom themes, saved choices and index-zero default.
Names are Nagi Light / Nagi Dark in both English and Japanese.
`theme_nagi.rs` holds the pair using the existing struct-update convention.
No additional theme store, SDK patch or per-frame palette conversion is added.

Reference: the existing Apple Light / Dark graph and confirmation surfaces,
with the same fixture, viewport, language and zoom. Retain their neutral chrome,
input boundaries, tab treatment, icon-only toolbar default and dense geometry;
do not inherit their broad hue palettes or system-accent substitution.
`Theme::uses_neutral_dark_chrome` is the single predicate for the shared dark
elevation/boundary treatment, used by the existing renderers.

Choose four low-saturation families: blue for interaction, green for success,
ochre for warning, brick for danger. Graph lanes reuse these and grays; generated
initial avatars are grayscale. Syntax colours structural roles rather than
every identifier. ANSI magenta/cyan reuse brick/green; external images and
terminal truecolor remain content, not theme-owned colour. Complete inherited
component roles in the existing theme bridge so hover links, charts, switches,
progress and semantic variants cannot reintroduce the kit's vivid palette.
Token snapshots are still rebuilt only by that bridge.

State contract: rest, hover, pressed, focus-visible, selected and inactive-window
selection retain the existing control/state owners. Ordinary selected rows use
neutral fills with normal semantic ink, not Apple's opaque on-accent ink.
Disabled controls retain native disabling. Loading, empty and error behavior
is unchanged; only their existing semantic tokens differ. There are no new
Git writes, modal slots, focus handles, animations or confirmation routes.

The pinned Custom Button derives a 40% pressed tint and ignores its foreground
override; subdued semantic ink on that tint can fall below 4.5:1. Nagi's
Success/Warning/Danger/NeutralTinted roles therefore use the existing Default
variant's neutral state fills and native normal/disabled labels. Filled Primary,
Destructive and Current/Incoming roles remain distinct and contrast-aware.
This is presentation of explicit roles, not RGB-based inference of an operation.

Use [CUDO's three principles](https://cudo.jp/?page_id=86),
[Apple HIG Color](https://developer.apple.com/design/human-interface-guidelines/color)
and WCAG's [Use of Color](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html),
[Contrast Minimum](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)
and [Non-text Contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).
The palette tests require readable ordinary text/code (4.5:1), graph/focus
contrast (3:1), selection-overlay legibility and four-family containment,
including the component bridge. Native coverage checks fixed-accent pixels and
selected identity across activation, alongside existing theme/button scenarios.

This is not whole-app WCAG compliance or CUD certification. In the repository's
protan/deutan simulations, the green/brick semantic pair becomes close; pale
diff fills and repeated lanes cannot identify meaning alone. Existing `A/M/D/R`,
`+/-`, labels, icons, graph geometry and focus/selection indicators are required.
Neutral selection fills alone are below 3:1. Keep the separate Color Vision
blue/orange preset; do not market Nagi as its replacement. Real-window
comparisons and people with differing visual abilities remain necessary.
