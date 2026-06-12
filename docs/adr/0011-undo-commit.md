# ADR-0011: Undo Commit Semantics(+ Redo の要否)

- Status: Accepted
- Date: 2026-06-12

## Context

Toolbar に Undo Commit を置く。「直前の local commit を取り消す」を、データを失わずに実現する。
Redo Commit の要否も判断する。

## Decision

### Undo Commit = 「branch ref を1つ戻す soft reset 相当」

- 実装: `git reset --soft HEAD~1` 相当を **git2 の参照操作のみ**で行う:
  `repo.reference("refs/heads/<branch>", parent_oid, true, msg)` で branch tip を付け替え、
  HEAD はそのまま branch を指す。**index と working tree には一切触れない**
  (= commit の変更内容が staged 状態で残る。`reset_default` も `checkout` も呼ばない)
- これにより「reset」という名前だが実態は **ref の付け替えのみ = 何も失われない**(Guarded)
- blocker 条件:
  - 対象 commit が **upstream に push 済み**(`graph_ahead_behind` で ahead=0、または対象 commit が
    upstream から到達可能)→ 履歴改変になるため禁止
  - 対象が merge commit(parent 複数。MVP では非対応)
  - unborn / detached HEAD / conflict 状態
- plan 表示: 取り消される commit(sha / summary)、「変更は staged のまま残る」ことの明示
- 復旧情報: undo 実行時に **元の commit sha を Operation Log に必ず記録**
  (`git reset --soft <sha>` 一発で戻れる旨を recovery に書く)

### Redo Commit = 実装しない(MVP / v0.2 とも)

理由:
1. Undo が soft 相当なので、**Redo は「もう一度 Commit ボタンを押す」ことと等価**
   (変更は staged のまま、message も Operation Log に残っている)。専用機構は冗長
2. 専用 Redo には commit metadata + patch の保存機構が必要で、コストに見合わない
3. 代替として、Undo の plan と Operation Log に元 sha を残すため、
   `git reset --soft <元sha>`(将来の "Restore" ボタン候補)で完全復元が可能

→ Toolbar には Redo ボタンを置かない。Operation Log の undo エントリに元 sha を表示することで代替。

## Consequences

- 「undo したら変更が消えた」事故が構造的に起こらない(WT/index 不変)
- push 済み commit の取り消し(revert)は別機能(v1.0 の undo/revert 設計)へ
- T-HT-008(Undo ADR チケット)は本 ADR で完了
