# ADR-0202: PR merge の local branch 削除を凍結して記録する

- 状態: 採用（#705）
- 決定日: 2026-09-22
- 関連: #701、ADR-0177、ADR-0184、ADR-0196

`gh pr merge -R <host/owner/repo>` は repository identity を固定する一方、
`CanDeleteLocalBranch = !Flags().Changed("repo")` により local 削除を行わない。
この分岐は [gh v2.96.0 の merge 実装](https://github.com/cli/cli/blob/v2.96.0/pkg/cmd/pr/merge/merge.go) で確認した。
`-R` は外さず、Kagi が既存 delete-branch family を merge の後段として呼ぶ。
fork の remote head は gh が削除しないことを EN/JA の plan warning で示す。

承認時に `PrMergeLocalBranch` が WorktreeId、branch 名、完全 OID、HEAD を凍結する。
OID の `None` は承認時の不存在であり、後から現れた同名 branch の削除を許可しない。
merge の成立を確認してから、identity、HEAD、OID、PR head との一致、checked-out
状態を再検証する。既存 delete-branch の ref/HEAD lock、完全 OID 比較、CAS 削除、
verify と mandatory backup ref を再利用し、別の削除 executor は作らない。
承認時の PR head と一致する local tip だけを対象にし、server 上の PR merge 成立後に処理する。
merge が不明な間は local 削除を実行せず、reconcile 自体も書き込まない。

後段失敗は merge 成功を取り消さない。単一 `pr-merge` receipt を boundary で finalize
し、成功時は `local branch deleted: <name>@<oid>` と復旧 ref、失敗時は
`local branch not deleted: <reason>` を記録する。結果は
`PrMergeLocalOutcome::{Deleted, Absent, NotDeleted}` のまま UI に渡し、EN/JA の
notice を出す。後段失敗は `Partial` と既存の再 merge 抑止を維持する。
元 branch の reflog は既存 family と同様に削除する。復旧保証は reflog ではなく、
ADR-0184 の backup ref と、その ref を保持する oplog receipt である。

Unknown の凍結 expectation は PR 状態と local branch を別々に照合する。
fork では最初から存在しない base repository の head ref を削除証明に使わない。
same-repository PR の remote 削除 expectation は引き続き凍結 repository を読む。
merged の再読だけでは local 削除を解決済みにせず、local ref の不存在も要求する。
再読で現れた別 OID を削除する処理、remote identity の再解釈、merge の自動再送はない。

検証は fake gh と実 Git fixture で、削除と復旧 ref、checked-out／OID 変化拒否、
承認後に現れた branch の保持、fork warning、merged のみでは acknowledge できない
reconcile を観測する。ADR-0184 の外部 concurrent worktree 作成に関する制約は継承する。
