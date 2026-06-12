# T-COMMIT-004: Commit Checklist — conflict marker 検出(block)

- Status: done
- 依存: T-COMMIT-003 / ADR-0043 §rule 4
- 関連: lane W14-CHECK

## 背景

staged BLOB に conflict marker(`<<<<<<<` / `=======` / `>>>>>>>`)が残ったまま commit する事故を block する。

## スコープ

- `checklist.rs` に conflict marker ルールを追加(block)。
- **staged tree/index の BLOB を走査**(WT ファイルではない)。テキスト BLOB のみ対象(NUL 含む binary は skip)。
- 行頭が `<<<<<<< ` / `=======`(行全体一致 or `======= ` 後続)/ `>>>>>>> ` のいずれかにマッチで blocker。
- 大 BLOB は先頭 N(例 1MiB)まで走査。

## 完了条件

- [x] marker を含む staged テキストファイル → blocker
- [x] binary BLOB / marker なしテキスト → blocker にしない
- [x] unit test: marker あり(block)/ marker なし / binary(skip)/ 大ファイル先頭のみ走査、計 4+
- [x] `cargo test` 全パス + own-code warning 0
- [x] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/checklist.rs` / `tests/checklist_test.rs`
- `docs/tickets/T-COMMIT-004.md`

## 触ってはいけないファイル

- `src/ui/*` / 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`
2. tempdir に marker 入りファイルを stage して検証
3. binary(NUL 入り)で false positive しないこと

## リスク・規約

- `=======` は通常コードにも出現しうる(Markdown 罫線等)→ false positive あり得るが、marker は通常
  `<<<<<<<` / `>>>>>>>` とセットで出る。ルールは ADR-0043 どおり 3 種いずれかで block(保守側)。
  運用で false positive が問題なら「3 種が近接して揃う」判定へ強化を検討(本チケットは ADR どおりで可)
- 走査はバイト単位、UTF-8 境界を割らない(行単位処理)

## 実装メモ(W14-CHECK / 完了)

- `src/git/checklist.rs` に純ロジックを新規実装。`checklist(repo, status) -> (blockers, warnings)`。
  staged index entry → `repo.find_blob(entry.id)` で **index 側 BLOB** を読み走査(WT は読まない)。
- rule 4(block): `has_conflict_marker` が行頭 `<<<<<<< `(7文字+空白)/ `>>>>>>> ` / `=======`
  (7 個の `=` だけの行、または `======= ...`)を検出。`split_lines` は `\n` 分割 + 末尾 `\r` 除去(CRLF 対応)、
  バイト単位で UTF-8 境界を割らない。
- binary BLOB は marker 走査対象外(`blob_is_binary` = git2 `is_binary` または先頭 8 KiB の NUL 検出)。
- 走査は先頭 1 MiB(`MARKER_SCAN_BYTES`)のみ → 巨大ファイルでも固まらない。test
  `large_blob_prefix_only_scanned` が 1.5 MiB 後方の marker を検出しないことを確認。
- `plan_commit`(staging.rs)末尾で `checklist()` を呼び blockers/warnings に append。既存 rule 1〜3 は据え置き。
- test: `tests/checklist_test.rs` の `marker_text_file_blocks` / `clean_text_no_block` /
  `binary_with_marker_bytes_skipped` / `marker_only_in_unstaged_not_flagged` /
  `large_blob_prefix_only_scanned` / `plan_commit_surfaces_marker_blocker` + lib unit test
  `conflict_marker_lines` / `conflict_marker_in_text` / `crlf_lines_match_markers`。
