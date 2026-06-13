# ADR-0067: Continue / Abort / Skip Safety Policy

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §2.6 / ADR-0056 / 0066 / 既存 plan_conflict_continue/abort(W26)

## Decision

Conflict Mode 中の Continue / Abort / Skip / Mark resolved / Save は **直接実行せず Plan 経由**
(GitOperationPlan または ConflictOperationPlan → confirm → preflight → execute → verify → oplog)。

### Continue 前チェックリスト(全て満たすまで Continue disabled)
- unresolved files count == 0
- conflict marker 残存なし(ADR-0066)
- index が resolved 状態(unmerged entry なし)
- binary conflict 残なし
- required file が削除されていない
- commit message 非空(merge commit を作る場合)
- checklist(ADR-0043)blocker なし

### Continue の実行
- merge: 解決を index へ → marker 再検査 → merge commit 作成(message 経由)
- rebase/cherry-pick/revert: sequencer continue(次の step へ)。W26 の `plan_conflict_continue` を拡張

### Abort(常に可能 = 安全弁)
- 確認ダイアログを出す。**保存済み resolution が失われる可能性**を明示
- `cleanup_state` + ORIG_HEAD 復帰(force/reset --hard/clean 不使用、ADR-0056/0057)。buffer は
  oplog 参照付きで退避(完全には消さない)

### Skip
- **rebase / cherry-pick / revert のみ**(merge では非表示)。sequencer の現 step をスキップ
- W26 段階では未実装(UI は「terminal で `--skip`」案内)。本 ADR で正式 plan 化を予約
