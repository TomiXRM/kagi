# ADR-0018: Status Bar Repository State Summary

- Status: Accepted / Date: 2026-06-12

## Decision
表示項目(左→右): branch / upstream 名(`→ origin/main`)/ ↑↓ / ● dirty / +staged / ~unstaged / **!conflict** / **⧉stash 数** / 最終 refresh 時刻 / **background operation(実行中の op 名 or external change 検知)** / 直近結果 / タブ icons。
0 件の数値は非表示(ノイズ抑制)。↑↓は Header と重複表示(判断材料のため意図的)。

## Consequences
- StatusBarSummary に conflict_count / stash_count / upstream_name / busy: Option<String> を追加

## Amendment — 2026-10-12: bounded branch identity (#1310)

The dense footer keeps its existing summary ordering and panel controls. Long
branch identity shrinks to a zoom-scaled 60-reference-pixel minimum and end
ellipsis; Tooltip and the AX label retain the complete branch name. Status chips
(including ahead/behind and upstream) and the refresh timer retain natural widths.
Feedback shares compression using its natural flex basis, remains a single-line
truncated first-line preview, and precedes a nonshrinking Terminal / Operation Log
cluster. The original footer body and durable Operation Log detail are unchanged.

This policy applies to both toolbar presentations, EN/JA and 100%/167% zoom.
Identity is a read-only label, not an additional Tab stop. Hover exposes its full
name; selected/pressed/disabled/loading/error input states are not applicable to
that label. Existing panel-button hover, toggled state, focus-visible rings,
keyboard activation and localized names remain owned by `panel_button`.
