# T-CONFLICT-DASH-021: Path/Tree toggle を MVP から削除する

- Status: done
- Group: Dashboard
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

動かない toggle を撤去。folder grouping / search は後回し ticket(別途)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
Path/Tree toggle(dash_view_toggle)を撤去。ConflictMode.tree_view フィールドと ConflictViewPath/ConflictViewTree/ConflictTreeSoon Msg を削除。folder/tree grouping は将来 ticket(W33 memo 参照)。
