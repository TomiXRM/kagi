# T-CONFLICT-UX-013: Save を Save resolution に改名し意味を定義する

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

ADR-0068: ボタン名 Save resolution。merge commit 作成ではない

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
ボタンは既存「Save resolution」(`Msg::EditorSave`)のまま。意味を ADR-0068 に従い WT 書き込み + stage(marker block)に再定義(実装は T-CONFLICT-UX-014)。merge commit は作らない(Continue→commit panel→Commit で作成)。
