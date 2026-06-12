# ADR-0018: Status Bar Repository State Summary

- Status: Accepted / Date: 2026-06-12

## Decision
表示項目(左→右): branch / upstream 名(`→ origin/main`)/ ↑↓ / ● dirty / +staged / ~unstaged / **!conflict** / **⧉stash 数** / 最終 refresh 時刻 / **background operation(実行中の op 名 or external change 検知)** / 直近結果 / タブ icons。
0 件の数値は非表示(ノイズ抑制)。↑↓は Header と重複表示(判断材料のため意図的)。

## Consequences
- StatusBarSummary に conflict_count / stash_count / upstream_name / busy: Option<String> を追加
