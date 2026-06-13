# T-CONFLICT-UI-005: monospace font / line number / code row height を整える

- Status: done
- Group: Layout
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

gpui-component CodeEditor InputState で monospace + 行番号 + 安定 line height

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
code_editor mode により monospace + 行番号 + 安定 line height(gpui-component 既定)。zoom は scaled_px で別途反映。

## 追記 (line-level rework done)
A/B pane は自前 row list に変わったため、code row は `terminal::pick_font_family()` を使い terminal と
同じ font family に統一。line number / code text / fixed row height は `theme::scaled_px` で zoom 対応。
Result pane は gpui-component CodeEditor のまま。
