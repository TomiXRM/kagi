# T-CONFLICT-LINE-001: line 単位採用モデル(resolution.rs 拡張)

- Status: todo(実装は flow レーン merge 後)
- 仕様: ADR-0071

## スコープ
- ConflictHunk に `line_select: Option<LineSelection>` 追加(current_taken/incoming_taken/order)。
- assemble() を line_select 優先・無ければ hunk choice にフォールバック(後方互換)。
- file/chunk/line の tri-state 集計ヘルパ + 親子伝播。unit test(line 採用・順序・後方互換)。
