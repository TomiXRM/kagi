# ADR-0202: PR merge の local branch 削除を凍結して記録する

- 状態: 採用（#705）
- 決定日: 2026-09-22
- 関連: #701、ADR-0177、ADR-0184、ADR-0196

`gh pr merge -R <host/owner/repo>` は repository identity を固定する一方、
`CanDeleteLocalBranch = !Flags().Changed("repo")` により local 削除を行わない。
この分岐は [gh v2.96.0 の merge 実装](https://github.com/cli/cli/blob/v2.96.0/pkg/cmd/pr/merge/merge.go) で確認した。
`-R` は外さず、Kagi が既存 delete-branch family を merge の後段として呼ぶ。
fork の remote head は gh が削除しないことを EN/JA の plan warning で示す。
PR merge の計画も ADR-0141 に従い `cx.background_spawn` で作る。owner / worktree を
固定し、既存の `merge-plan` loading 表示から完成した plan だけを modal に渡す。
`finish_planning` が stale owner と失敗の latch を終端化し、別 modal を上書きしない。

承認時に `PrMergeLocalBranch` が WorktreeId、branch 名、完全 OID、HEAD を凍結する。
OID の `None` は承認時の不存在であり、後から現れた同名 branch の削除を許可しない。
計画時点で checked-out または PR head と異なる tip なら、型付き `keep_reason` と
「kept」の warning を凍結する。これは削除の約束ではなく、後から条件が変わっても
削除せず `Kept` / `Success` とする。計画後に初めて起きた drift の拒否とは区別する。
merge の成立を確認してから、identity、HEAD、OID、PR head との一致、checked-out
状態を再検証する。既存 delete-branch の ref/HEAD lock、完全 OID 比較、CAS 削除、
verify と mandatory backup ref を再利用し、別の削除 executor は作らない。
local 削除には **gh 成功と server 上の merge 成立の両方**を要求する。
gh が失敗し再読で merged でも local は触らず `Partial` とし、gh のエラー全文を
`error` と `after.dirty` の両方に残す。fork でも `Success` への昇格はしない。
gh exit 0 + `mergedAt: null` は queue 投入済みという既知の `Success` であり、
`after.dirty` に queued と明記して `Kept(Queued)` の EN/JA 通知を出す。
この場合は local 削除、reconcile park、transport hold のいずれも行わない。
再読不能の `Unknown` では local は未着手であり、reconcile 自体も書き込まない。

後段失敗は merge 成功を取り消さない。単一 `pr-merge` receipt を boundary で finalize
し、成功時は `local branch deleted: <name>@<oid>` と復旧 ref、失敗時は
`local branch not deleted: <reason>` を記録する。結果は
`PrMergeLocalOutcome::{Deleted, Absent, Kept, NotDeleted}` のまま UI に渡す。
`PrMergeLocalReason` は `PlanNote` を保持し、既知の拒否理由も EN/JA で描画する。
routine な `Absent` は receipt / footer だけに残し、acknowledgement notice を出さない。
後段で初めて分かった削除失敗は `Partial` と既存の再 merge 抑止を維持する。
元 branch の reflog は既存 family と同様に削除する。復旧保証は reflog ではなく、
ADR-0184 の backup ref と、その ref を保持する oplog receipt である。

PR #760 レビュー後、**未着手 local branch の不在を Unknown の照合条件にする
初案を撤回**した。local の不在を要求すると repo の write scope が閉じたままとなり、
Kagi の guarded delete 自体を使えず、外部での削除を強いるためである。
`RemoteExpectation::LocalBranch` は削除する。fork の Unknown は PR の `Merged`
だけを照合し、local branch を残したまま acknowledge できる。その後の削除は
ユーザーが通常の guarded delete と backup ref を使って実行できる。
fork では最初から存在しない base repository の head ref を削除証明に使わない。
same-repository PR の remote 削除 expectation は引き続き凍結 repository を読む。
再読で現れた別 OID を削除する処理、remote identity の再解釈、merge の自動再送はない。

検証は fake gh と実 Git fixture で、削除と復旧 ref、計画済み keep と後発 drift、
queued Success、承認後に現れた branch の保持、failed gh 後の非削除を観測する。
Unknown の merged 再読後、local を残した acknowledge と Kagi の通常の guarded
delete が通ることも検証する。ADR-0184 の外部 concurrent worktree 作成に関する制約は継承する。
