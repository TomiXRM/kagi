# T-COMMIT-004: Commit Checklist — conflict marker 検出(block)

- Status: todo
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

- [ ] marker を含む staged テキストファイル → blocker
- [ ] binary BLOB / marker なしテキスト → blocker にしない
- [ ] unit test: marker あり(block)/ marker なし / binary(skip)/ 大ファイル先頭のみ走査、計 4+
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

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
