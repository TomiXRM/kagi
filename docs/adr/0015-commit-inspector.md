# ADR-0015: Commit Inspector Right Panel

- Status: Accepted / Date: 2026-06-12

## Decision
- 並び: **1. Summary(タイトル・short SHA・copy ボタン・ref ラベル)→ 2. Metadata(author/authored・committer/committed・parents・full SHA・本文)→ 3. Contextual Actions(Create branch / Cherry-pick / Copy SHA。Revert・外部リンクは later)→ 4. Changed Files(count・status・Path⇄Tree トグル・選択ハイライト)**。Diff は main pane(T-UI-003 決定の維持 — 原文要件4の「右ペイン内 Diff Viewer」は main pane 方式で満たす)
- 原則「**情報が先、危険操作は後**」。history-changing は plan 経由(既存)
- copy SHA はクリップボード API(gpui)で full SHA をコピー(ZWSP 等の混入禁止 — raw 値を使う)
- 実装は `src/ui/inspector.rs` へ抽出

## Consequences
- 既存の Create branch/Cherry-pick ボタンが上部から Actions セクションへ移動(導線変更をユーザーに周知)
