# T-CONFLICT-UX-011: both 採用順序ボタンを A/B と Result の間に移動する

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

[Both: current→incoming][Both: incoming→current]。両方 check 時に順序選択、既定表示+切替

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
A·B と Result の間に both-order strip([Both: current→incoming][Both: incoming→current])。両側 accept 時に current-first を active 表示。
