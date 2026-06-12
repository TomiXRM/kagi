# T-COMMIT-013: Undo Last Commit — soft reset 相当 backend(既存で充足)

- Status: done 相当(既存 T-HT-009 / ADR-0011 で実装済み)
- 依存: ADR-0011 / 0041
- 関連: requirements-commit-suite.md §Undo

## 背景・根拠

要件「Undo Last Commit: soft reset 相当 / pushed 対象外 / reset hard 禁止」は **既存実装で完全充足**。

- `plan_undo_commit` / `execute_undo_commit`(src/git/ops.rs)が **ref 付け替えのみ**(`repo.reference(
  "refs/heads/<branch>", parent_oid, true, msg)`)で soft reset 相当を実現。index・WT・HEAD に触れない。
- pushed は blocker(`graph_descendant_of(upstream, head)` / `upstream==head`)。upstream 未設定なら可。
- reset --hard / checkout 系 / `reset_default` を一切使わない(T-HT-009 で grep 確認済み)。
- merge commit / detached / unborn / root commit も blocker。

→ ADR-0041(本スイートの突合 ADR)で要件 ↔ 実装の一致を確認済み。**新規 backend 不要**。

## 完了条件

- [x] soft 相当 backend が存在し、変更が staged のまま残る(T-HT-009 のテストで担保)
- [x] pushed / merge / detached / root が blocker
- [x] reset --hard / checkout 系を使っていない(grep 済み)
- [x] ADR-0041 で要件突合済み

## 触ってよいファイル

- なし(設計確認チケット。コード変更なし)。本ファイルのみ
- `docs/tickets/T-COMMIT-013.md`

## 触ってはいけないファイル

- `src/git/ops.rs` の undo backend(既存、不変条件維持)/ その他すべて

## テスト方法

- 既存 `tests/undo_test.rs`(T-HT-009 の 8 ケース)が回帰しないこと

## リスク・規約

- 将来も ref 付け替えのみの不変条件を壊さない(reset --hard を絶対に足さない)
