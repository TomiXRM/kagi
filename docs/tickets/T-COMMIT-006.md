# T-COMMIT-006: Commit Checklist — large binary 検出(warn, override 可)

- Status: todo
- 依存: T-COMMIT-003 / ADR-0043 §rule 6 / ADR-0039(override)
- 関連: lane W14-CHECK

## 背景

巨大な binary を誤って commit する事故を warn する。テキストの大ファイルは対象外(diff として正当なことが多い)。

## スコープ

- `checklist.rs` に large binary ルール(warn):
  - staged な **binary BLOB**(NUL 含む or git binary 判定)で、サイズ > 閾値 → warn。
  - 閾値: 既定 **5 MiB**、`KAGI_LARGE_BLOB_BYTES` で override(テストで小さく設定)。
  - warn 文言にファイル名 + サイズ。
- テキストの大ファイルは warn しない。

## 完了条件

- [ ] 閾値超の binary を stage → warn(サイズ・名前つき)
- [ ] 同サイズのテキスト → warn しない
- [ ] `KAGI_LARGE_BLOB_BYTES` で閾値を変えられる
- [ ] unit test: binary 超過(warn)/ binary 以下 / 大テキスト(warn なし)/ env 閾値、計 4+
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/checklist.rs` / `tests/checklist_test.rs`
- `docs/tickets/T-COMMIT-006.md`

## 触ってはいけないファイル

- `src/ui/*` / 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`(`KAGI_LARGE_BLOB_BYTES` を小さく設定してテスト)
2. tempdir のみ
3. binary 判定は NUL バイトで簡易に(git の binary 判定に合わせる)

## リスク・規約

- env テストは並行実行で競合しうる → `KAGI_LARGE_BLOB_BYTES` を使うテストは serial 化 or 独自値で衝突回避
  (oplog テストの `KAGI_LOG_DIR` 直列化パターンを参考)
