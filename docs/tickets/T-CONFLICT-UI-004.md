# T-CONFLICT-UI-004: A/B/Result pane に scrollbar を追加する

- Status: done
- Group: Layout
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

CodeEditor InputState の縦/横 scrollbar(ADR-0069)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
A/B/Result は gpui-component InputState code_editor("text") を使用。code_editor 既定で縦/横 scrollbar + line number 付き。Input::h_full() で pane 高さ充填。
