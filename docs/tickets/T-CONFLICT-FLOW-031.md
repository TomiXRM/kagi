# T-CONFLICT-FLOW-031: merge commit 作成を Commit panel 側に分離する

- Status: done
- Group: Flow
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

Commit merge ボタンで 2-親 merge commit 作成(commit フロー/checklist 経由)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done, MVP)
新 `execute_merge_commit(repo, message)`(conflicts.rs): index unmerged が残れば拒否、`create_merge_commit`(message_override 対応に拡張)で HEAD+MERGE_HEAD の 2 親 commit → `cleanup_state`(MERGE_HEAD/MERGE_MSG 削除)。commit panel の commit ボタンは `start_commit` 内で `conflict_merge_commit_pending` を見て `finish_merge_commit` に分岐(plan_commit で message/staged を検証 = checklist 経由、実行のみ merge 化)。test `merge_commit_has_two_parents_and_cleans_state`(parent_count==2 + custom message + MERGE_HEAD 消去 + conflict 解除)。deferred: merge commit 専用の checklist 文言追加は未(汎用 commit plan を流用)。
