# T-CONFLICT-LINE-001: line 単位採用モデル(resolution.rs 拡張)

- Status: done
- 仕様: ADR-0071

## スコープ
- ConflictHunk に `line_select: Option<LineSelection>` 追加(current_taken/incoming_taken/order)。
- assemble() を line_select 優先・無ければ hunk choice にフォールバック(後方互換)。
- file/chunk/line の tri-state 集計ヘルパ + 親子伝播。unit test(line 採用・順序・後方互換)。

## 実装メモ (done)
`LineSelection { current_taken, incoming_taken, order }`、`LineOrder`、`SelectionSide`、`TriState`
を `resolution.rs` に追加。`assemble()` は `line_select=Some` を優先し、`None` では従来の
`HunkChoice` を使うため既存 hunk-level 動作は後方互換。file/chunk/line の親子伝播 API と
unit test(行採用・順序・fallback・tri-state)を追加。
