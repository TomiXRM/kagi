# T-CONFLICT-UX-012: hunk 単位 accept model を明確化する

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

hunk 単位(MVP)。data model は line 単位へ拡張可能に(v0.2 line accept の布石)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
MVP は hunk 単位 model(HunkModel/HunkChoice)を file-level UI で操作。data model は hunk 配列 + Manual(line)拡張余地ありで line-level(v0.2)の布石を維持。
