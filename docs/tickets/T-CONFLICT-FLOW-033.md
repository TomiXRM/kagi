# T-CONFLICT-FLOW-033: Save/Continue/Commit の責務を ADR に明記する

- Status: done
- Group: Flow
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

ADR-0068 として完了(本 ticket は実装が ADR に従うことの確認)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
4 操作が別物として実装されたことを確認: Save=`execute_conflict_save`(WT+stage、marker block)/ Continue=`plan_conflict_continue_route`(merge→commit panel、sequencer→plan modal、即 commit しない)/ Commit=`execute_merge_commit`(2 親 + cleanup)/ Abort=従来どおり(`execute_conflict_abort`、ORIG_HEAD)。W26 の `plan_conflict_continue` は sequencer 用に残し、merge の即 commit 経路は route 分岐で置換。headless KAGI_* 経路は `execute_conflict_continue`(従来 merge 即 commit)を保持。
