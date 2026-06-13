# T-CONFLICT-UI-003: A/B/Result pane をリサイズ可能にする

- Status: done
- Group: Layout
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

A|B 比率、A・B/Result 比率の resize handle(W7 measured-bounds 方式)。bottom panel 境界も

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
A|B 比率(conflict_ab_split)と A·B/Result 比率(conflict_result_split)を W7 方式(measured-bounds canvas + Rc<Cell> + on_drag/DividerDrag + drag-move 絶対座標)で resize 可能に。DividerKind::ConflictAB / ConflictResult を追加。
