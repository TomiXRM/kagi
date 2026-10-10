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
or Primary palette. Custom themes inherit or explicitly override `link`; full
standalone files require it. Existing TextView underlining, activation and
selection remain unchanged. See [the body-link guide](../themes.md#body-links).
