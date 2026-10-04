# ADR-0221: Operation Log の計画復旧説明を型付きで保存する（#1025）

- Status: **Accepted**
- Date: 2026-10-05
- Related: [ADR-0129](0129-plan-note-i18n.md)、[ADR-0149](0149-oplog-in-backend-run-and-schema.md)、[ADR-0196](0196-operation-lifecycle-contract.md)、[ADR-0214](0214-oplog-panel-and-point-restore.md)、#994、#1025

## 文脈

計画カードの復旧説明は `PlanRecovery { kind: RecoveryKind, commands }` にあり、英語と日本語は表示時に型から生成される。しかし従来の Operation Log は実行後に判明した `RecoveryHandle`（savepoint や backup ref）だけを保存し、承認時に示した説明やコマンドを保存しない。古いログの `after.dirty` や handle から計画時の説明を復元したふりをしてはいけない。

## 決定

1. backend が承認済み `OperationPlan` を持つ `run_recorded_with_events`、history move、branch cleanup、stash の run 経路、recorded remove の記録時に、任意の `recovery_plan` を `OpLogEntry` に添える。これは計画時の復旧案であって実行結果の証拠ではなく、既存の `recovery` handles / `ref_moves` を置き換えない。UI 独自記録、refusal、ConflictPlan / AbsorbPlan 等の計画を持たない境界には付けない。
2. wire は `{"recovery_plan":{"kind":{"kind":"Branch","payload":{"CreateBranch":{"name":"feature"}}},"commands":["git branch -d feature"]}}` のように種別・種別別 payload・コマンドを持つ。`kagi-domain` は依存なしを保ち、serde の private remote DTO は `kagi-git::oplog` が所有する。表示に用いる payload だけを写し、実行固有で表示しない GitHub PR merge の `base_repo` / `delete_branch` / `cross_repository` / `local_branch` は wire に含めない。PR merge の復旧説明は `number` のみを用いる。`PrMergeLocalReason::Plan(Box<PlanNote>)` を含む実行固有の型階層を oplog へ複製しない。表示に必要な入れ子型を忠実に写せない新しい variant ができた場合、部分的な説明を残すのでなくフィールド全体を記録しない。現時点で該当する variant はない。元の全 `OperationPlan` を再構成できる形式ではない。
3. 保存時に `None` はフィールドを省略する。読み込み時に欠落・null・間違った型・未知の kind / variant・不完全な payload はすべて `None` とし、その行の outcome その他の証拠は保持する。旧 Kagi は未知の追加フィールドを無視する。既存行は書き換えず、schema version を追加しない（ADR-0149）。retention は現行の entry 比較で新フィールドを round-trip し、保持した行のバイト列も変更しない。
4. Operation Log の復旧欄は **Success / Partial / Unknown に表示し、Refused / Failed では欄そのものを表示しない**。フィールドのない前者には英日で「not recorded / 記録されていません」と表示し、説明を捏造しない。欄は現在の表示言語で `plan_recovery_text` を再実行し、従来の計画カードと同じ表示行の分類（空白を除いた先頭が `git ` の行）で説明内のコマンド行を除く。構造化コマンドは `PlanRecovery::commands_for(ShellKind::current())` から英日別ラベルの行として描画・コピーし、重複させない。`cmd.exe` には POSIX 用のコマンドを提示せず説明だけを残す。欄は AX の名前を持つ。
5. ADR-0129 の blocker は durable な英語の `detail` として残す。一方 recovery は現在の言語で再描画する型付き表示値であり、blocker の英語保存規則を流用しない。ADR-0196 の outcome / recording / receipt の契約、ADR-0214 の ref 復元と記録済み移動の意味は変えない。

## 却下した案と限界

- 英語で描画済みの復旧文だけを保存すると後の言語切替で日本語にできず、`commands` の構造も失う。
- 未記録の旧ログから `after` や `RecoveryHandle` を解析して説明を合成すると計画時の約束と実際の証拠を混同する。
- `RecoveryKind` の全フィールドをそのまま保存するために依存のない domain に serde を導入したり、復旧文に使わない巨大な `PlanNote` 階層を oplog 側へ複製したりしない。表示に使わない実行文脈（GitHub PR merge の local branch 等）はこの追加フィールドの round-trip 対象外で、既存の receipt に保たれる。
