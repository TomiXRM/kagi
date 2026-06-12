# T-DISCARD-003: Discard all changes — 一覧付き modal + skipped 明示

- Status: done
- 依存: T-DISCARD-002
- 関連: ADR-0046

## スコープ

- unstaged セクションヘッダに「Discard all」ボタン
- modal に対象ファイル一覧(件数 + list、スクロール可)。untracked/conflicted は「skipped」表示
- 対象 0 件なら disabled

## 完了条件

- [x] 複数ファイル discard が 1 オペで実行され oplog 1 entry にまとまる
- [x] `cargo test` 全パス、own-code warning 0

## 実装メモ

- unstaged セクションヘッダに「Discard all」ボタンを追加(`cp-discard-all`、"Stage all" 隣)。
  discard 対象(untracked=Added と conflicted を除いた件数 `discard_eligible_count`)が
  **0 件なら disabled**(muted 色・ハンドラ無し)、>0 で赤 + click → `open_discard_all_modal()`。
- `open_discard_all_modal()`: `discard_partition()` で eligible / skipped を分け、eligible 全件で
  `plan_discard` → `DiscardModal{paths=eligible, skipped, is_all:true}`。modal は対象一覧 +
  skipped(untracked/conflicted)を「Skipped (N):」として明示表示。
- 実行は per-file と同じ `start_discard`/`discard_blocking` を通る → **複数ファイルでも
  execute_discard 1 回 = oplog 1 entry**(tests/discard_test.rs::discard_multi_file_one_outcome で
  backups.len()==2 / summary に両 path を確認)。headless KAGI_DISCARD_ALL=1 でも 1 オペで動作確認済み。
