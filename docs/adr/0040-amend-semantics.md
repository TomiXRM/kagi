# ADR-0040: Amend Semantics

- Status: **Proposed**(pushed 時の扱いがユーザー判断点 — 後述)/ Date: 2026-06-13

## Context

直前の commit を作り直す Amend を入れる。3 モード: **message only / staged を取り込む / 両方**。
amend は SHA を変える(= history-rewriting、ADR-0023)。pushed commit の amend をどう扱うか、
git2 でどう実装するか(in-memory 主義・ref-order 規則に整合)を決める。

## Decision

### 3 モードと意味

| モード | 入力 | 結果 |
|--------|------|------|
| message only | 新 message | tree は HEAD と同一、message だけ差し替えた新 commit。SHA は変わる |
| staged | staged 変更 | 直前 commit の tree に staged を畳み込んだ新 commit(message は据え置き) |
| both | 新 message + staged | 上記両方 |

- いずれも **新しい commit を作り、branch ref を新 commit に付け替える**(古い commit は到達不能になるが
  reflog/oplog から復元可能)。**SHA が変わることを plan に明示**(`旧 <short> → 新 <short>` を予測表示)。

### git2 実装方式(in-memory + ref-order 規則)

- `commit.amend(...)` は使わず、**明示的に new commit + ref 移動**で行う(ref-order 規則・in-memory 主義に整合):
  1. 親 = 旧 HEAD commit の **親**(amend は HEAD を置換するので parent は据え置き)
  2. tree: message only なら旧 HEAD の tree をそのまま。staged を含むなら **index から tree を write**
     (`index.write_tree_to(repo)` で in-memory tree を得る。WT には触れない)
  3. `repo.commit(None, author, committer, message, &tree, &[&parent])` で **ref を更新せず** commit object を作る
  4. blocker が無いことを確認後に `repo.reference("refs/heads/<branch>", new_oid, true, log_msg)` で
     **ref を最後に動かす**(ref-order 規則: object を先に作り ref は最後)
- author は旧 commit の author を**保持**(committer は現在のユーザー/時刻に更新)。これは git の amend 既定に一致。
- detached HEAD / unborn / merge commit(parents>1)/ root commit(message only は可、staged 畳み込みは
  parent 据え置きで可)は MVP の扱いを plan で明示。**merge commit の amend は MVP では blocker**。

### blocker / 確認

- checklist(ADR-0039 / 0043)を **通常 commit と同様に通す**(staged 畳み込み時は新 tree に対して検査)。
- amend は **history-rewriting** → ADR-0023 の **2段階確認**(plan Confirm 赤 → 追確認で「旧 SHA が失われる/
  reflog から復元可」を列挙 → 明示クリック)。
- 実行前に **旧 HEAD SHA を oplog に必ず記録**(recovery 起点)。oplog に before/after HEAD を残す。

### pushed commit の amend(★ユーザー判断点 — Proposed)

直前 commit が upstream から到達可能(= push 済み)の場合の扱いを 2 案で提示する。決定はユーザー:

- **案 A(強警告で続行可)**: 要件原文「pushed は強警告」に忠実。pushed なら warn(赤強調)+
  「次回 push は force が必要だが kagi は force push を提供しない/相手が pull 済みなら履歴が分岐する」を明示し、
  **2段階確認で続行は許可**。force push しない方針(ADR-0023)と矛盾しないが、ユーザーが CLI で force する前提。
- **案 B(blocker)**: undo last commit(ADR-0011)と同じく **pushed は blocker**(amend 不可)。
  「push 済みは履歴改変になるため amend 不可。新しい commit で修正してください」。最も安全だが要件の
  「強警告」より厳しい。

**推奨**: undo(ADR-0011)が pushed を blocker にしている整合性から **案 B 寄り**。ただし要件原文は「強警告」
なので、ユーザーが「自分の feature branch を一人で force し直す」運用を想定するなら案 A。→ **要決定**。

## Consequences

- `commit.amend` を避け new commit + ref 移動にすることで、cherry-pick / revert と同じ in-memory・ref-order
  規則に乗る(コードベースの一貫性)
- SHA 変化を予測表示するため plan に `旧→新 short SHA` を載せる(OperationPlan の既存フィールドで表現、
  preview_commits か title/predicted を使う。新フィールドは足さない方針)
- pushed 扱いが Proposed のため、実装チケット(T-COMMIT-009/010)は **決定後に backend を確定**する
