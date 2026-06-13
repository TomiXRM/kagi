# T-CONFLICT-FLOW-032: rebase/cherry-pick continue は OperationPlan 経由にする

- Status: done
- Group: Flow
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

--continue 相当の plan + 確認画面 → sequencer 継続

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done, MVP)
`plan_conflict_continue_route` が rebase/cherry-pick/revert を `ContinueRoute::SequencerPlan(OperationPlan)` に分岐(既存 `plan_conflict_continue` を box 化して流用)。`conflict_continue` が `ConflictContinuePlanModal` を立て、確認画面(`render_conflict_continue_modal` = `render_plan_modal_card` 流用)→ `confirm_conflict_continue` が `execute_conflict_continue`(stage + sequencer 半分)を実行 → oplog → reload。test `sequencer_continue_produces_a_plan`(cherry-pick が plan を返す、title に op 名)。deferred: 複数 commit rebase の「次の pick へ前進」は既存の `Staged` 委譲のまま(sequence executor 未実装)。
