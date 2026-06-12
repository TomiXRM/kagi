# T-DISCARD-002: Discard UI — per-file ボタン + danger 確認 modal + async 実行

- Status: todo
- 依存: T-DISCARD-001
- 関連: ADR-0046、lane W17-DISCARD

## スコープ

- Commit Panel unstaged 行に hover Discard アイコン(untracked/conflicted 行は出さない or disabled+tooltip)
- danger 確認 modal(赤系・destructive 表示、ADR-0046 の文言)+ ESC cancel + backdrop occlude
- 実行は W15 パターン: `start_discard` + `discard_blocking` free fn、busy_op="discard"、toast、reload

## 完了条件

- [ ] modal 確認 → 実行 → 対象が unstaged から消える(GUI は PM 検証)
- [ ] `cargo test` 全パス、own-code warning 0
