# T-COMMIT-018: Fixup / Squash commit 作成(prefix のみ、autosquash later)

- Status: todo / v0.2
- 依存: ADR-0045 / 既存 `execute_commit` / log・snapshot(対象 subject 取得)
- 関連: lane W14-FIXUP

## 背景

`fixup! <subject>` / `squash! <subject>` という message の **通常 commit を作るだけ**。履歴は書き換えない
(history-additive、1段階確認)。autosquash 実行(rebase -i 相当)は MVP 外。

## スコープ(ADR-0045 厳守)

- 対象 commit を選び(graph / Inspector)、staged を `"fixup! " + subject` or `"squash! " + subject` の
  message で commit。**既存 `execute_commit` の message 組み立てを流用**(新 pipeline 不要)。
- 対象 subject は既存 log/snapshot の 1 行目から取得(prefix 連結のみ、改行なし)。
- checklist(ADR-0039/0043)は通常 commit と同様に通す。
- 対象が現在 branch から到達不能なら warn(autosquash 対象にならない旨)。commit 自体は作れる。
- **autosquash の実行はしない**(履歴書き換え。later の専用 ADR)。

## 完了条件

- [ ] fixup!/squash! prefix の commit が作れる(subject が正しく連結)
- [ ] checklist の block/warn が効く
- [ ] 到達不能な対象で warn
- [ ] unit test: fixup 生成 / squash 生成 / subject 連結 / 到達不能 warn、計 4+
- [ ] `cargo test` 全パス + own-code warning 0、rebase / 履歴書き換え API を使っていない
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/ops.rs` or `src/git/staging.rs`(message 組み立て、`execute_commit` 流用)/ `src/git/mod.rs`
- `src/ui/commit_panel.rs` / `src/ui/inspector.rs`(対象選択 → fixup/squash エントリ)
- `tests/fixup_test.rs`(新規)
- `docs/tickets/T-COMMIT-018.md`

## 触ってはいけないファイル

- `Cargo.toml` / 他チケットのファイル / undo・amend backend

## テスト方法

1. `cargo test`(exit code 確認)
2. fixture / tempdir のみ
3. grep で rebase / 履歴書き換え API の不使用を確認

## リスク・規約

- 履歴を一切書き換えない(prefix commit を足すだけ)
- 文字列切り詰めは `chars()` ベース。subject に prefix を付けるだけで本文は据え置き
