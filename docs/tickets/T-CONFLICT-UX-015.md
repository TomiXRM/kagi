# T-CONFLICT-UX-015: Result Preview / Edit mode を分ける

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

Preview(read-only)/ Edit(editable Result、編集中表示、Save で保存、marker 検査)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
Result pane に Preview(read-only Input.disabled)/ Edit(editable)segmented toggle + "editing" indicator。Edit の text は sync_conflict_editor_inputs が set_manual_text で buffer に取り込み、marker 残存は既存 gate で検査。
