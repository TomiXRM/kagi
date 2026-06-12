# ADR-0041: Undo Last Commit Semantics(ADR-0011 の追補)

- Status: Accepted / Date: 2026-06-13

## Context

Commit スイート要件に「Undo Last Commit: soft reset 相当 / pushed 対象外 / reset hard 禁止 /
oplog に before/after HEAD」がある。これは既存 ADR-0011 + T-HT-009 で実装済み。本 ADR は
**新規実装ではなく、要件との突合と差分(あれば)の確認**のみを目的とする追補。

## Decision

### 要件 ↔ 既存実装の突合(差分なし)

| 要件 | 既存実装(ADR-0011 / T-HT-009) | 判定 |
|------|--------------------------------|------|
| soft reset 相当 | `repo.reference("refs/heads/<branch>", parent_oid, true, msg)` で ref を1つ戻すのみ。index・WT・HEAD(symbolic)に触れない → 変更は staged のまま残る | **充足** |
| pushed 対象外 | `graph_descendant_of(upstream, head)` または `upstream==head` で push 済み判定 → blocker。upstream 未設定なら無条件で可 | **充足** |
| reset hard 禁止 | checkout 系・`reset_default`・`index.*` を一切呼ばない(grep で確認済み)。ref 付け替えのみ | **充足** |
| oplog に before/after HEAD | undo 実行時に元 commit sha を Operation Log に記録。`UndoOutcome { undone, now_at }` で before/after を返す | **充足** |

→ **要件は既存実装で完全に充足。新規 backend 実装は不要**。

### 再確認(回帰防止のための不変条件)

- **reset --hard / git clean / checkout 系を Undo Last Commit に追加するのは絶対禁止**(ADR-0011 / 0023 / 0024)。
  「undo したら変更が消えた」事故は WT/index 不変により構造的に起こらない — この性質を将来も維持する。
- blocker 集合は据え置き: detached / unborn / conflict / merge commit(parents>1)/ root commit / pushed。
- recovery 文言は `git reset --soft <元sha>` で完全復元可能 + 元 sha を残す(据え置き)。

### UI 配線(MVP で残っている差分)

- backend(`plan_undo_commit` / `execute_undo_commit`)は完了。**未配線なら UI を Header / Commit Panel に出す**
  のは PM の main 側作業(本スイートの backend 追加は不要)。oplog の undo エントリに元 sha を表示(Redo 代替、
  ADR-0011 の決定どおり)。

## Consequences

- T-COMMIT-013(Undo soft 相当)/ T-COMMIT-014(oplog before/after)は **done 相当**(根拠は本 ADR + T-HT-009)
- push 済み commit の取り消しは Undo ではなく **revert**(別機能、ADR-0022/0023 §Revert)で扱う — 据え置き
- 本 ADR は設計確認のため、コード変更を伴わない
