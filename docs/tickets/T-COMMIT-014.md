# T-COMMIT-014: Undo Last Commit — oplog に before/after HEAD(既存で充足)

- Status: done 相当(既存 T-HT-009 / ADR-0011 で実装済み)
- 依存: ADR-0011 / 0041 / 既存 oplog
- 関連: requirements-commit-suite.md §Undo

## 背景・根拠

要件「Undo Last Commit: oplog に before/after HEAD」は **既存実装で充足**。

- `execute_undo_commit` は `UndoOutcome { undone: CommitId(元sha), now_at: CommitId(親sha) }` を返す
  (= before = 元 sha / after = 親 sha)。
- undo 実行時に **元 commit sha を Operation Log に記録**(`git reset --soft <元sha>` で完全復元できる旨つき)。
- oplog は `~/.kagi/operations.jsonl`(手書き JSON、serde 禁止、`KAGI_LOG_DIR` 対応)に追記。

→ before/after HEAD が oplog に残り、recovery 情報として元 sha が記録される。**新規 backend 不要**。

残るのは UI 表示(oplog エントリに元 sha を見せる)で、これは **T-COMMIT-012** に含む。

## 完了条件

- [x] undo の oplog エントリに元 sha(before)+ now_at(after)相当が記録される
- [x] recovery 文言に `git reset --soft <元sha>` が含まれる
- [ ] oplog エントリの元 sha を UI に表示(→ T-COMMIT-012 で実施)

## 触ってよいファイル

- なし(設計確認チケット。コード変更なし)。本ファイルのみ
- `docs/tickets/T-COMMIT-014.md`

## 触ってはいけないファイル

- `src/git/ops.rs` / `src/git/oplog.rs`(既存、変更不要)/ その他すべて

## テスト方法

- 既存 `tests/undo_test.rs` / oplog テストが回帰しないこと

## リスク・規約

- oplog は手書き JSON(serde 禁止)を維持
