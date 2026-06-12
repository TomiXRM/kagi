# T-COMMIT-006: Commit Checklist — large binary 検出(warn, override 可)

- Status: done
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

- [x] 閾値超の binary を stage → warn(サイズ・名前つき)
- [x] 同サイズのテキスト → warn しない
- [x] `KAGI_LARGE_BLOB_BYTES` で閾値を変えられる
- [x] unit test: binary 超過(warn)/ binary 以下 / 大テキスト(warn なし)/ env 閾値、計 4+
- [x] `cargo test` 全パス + own-code warning 0
- [x] 実装メモを本ファイル末尾に追記

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

## 実装メモ(W14-CHECK / 完了)

- `src/git/checklist.rs` の `checklist()` 内で rule 6(warn)を実装。binary BLOB かつサイズ > 閾値で warn。
- 閾値 `large_blob_threshold()`: 既定 5 MiB(`DEFAULT_LARGE_BLOB_BYTES`)、`KAGI_LARGE_BLOB_BYTES` で override
  (パース失敗時は既定値)。warn 文言にファイル名 + `human_bytes()` 整形サイズ。
- binary 判定は `blob_is_binary`(git2 `Blob::is_binary` または先頭 8 KiB の NUL 検出)。テキスト大ファイルは warn しない。
- test(env 競合回避): `KAGI_LARGE_BLOB_BYTES` を使う test は `ENV_LOCK: Mutex<()>` + 前値保存/復元の
  `with_threshold()` ヘルパで直列化(oplog_test.rs の `KAGI_LOG_DIR` パターンに準拠)。
- test: `large_binary_warns` / `small_binary_no_warn` / `large_text_no_warn` / `env_threshold_override`
  + lib unit test `threshold_env_default` / `human_bytes_fmt`。
