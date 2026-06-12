# ADR-0045: Fixup/Squash Commit Workflow

- Status: Accepted / Date: 2026-06-13

## Context

`fixup!` / `squash!` commit を作る機能(v0.2)。本来の autosquash(`git rebase -i --autosquash`)は
**履歴書き換え**なので MVP / 本スイートでは**実装しない**。ここでは「後で autosquash できる commit を
**作るだけ**」のワークフローを定める。

## Decision

### MVP は「prefix commit の作成のみ」

- ユーザーが対象 commit を選び(commit graph / Inspector から)、staged 変更を **`fixup! <対象の subject>`**
  または **`squash! <対象の subject>`** という message の **通常の commit** として作る。
- これは **history-additive**(新 commit を足すだけ。ADR-0023)。**履歴は一切書き換えない**。
  → 確認は通常 commit と同じ **1段階**。amend のような 2段階確認は不要。
- message の `fixup!`/`squash!` prefix は git の autosquash 規約に一致。後でユーザーが CLI で
  `git rebase -i --autosquash` すれば畳み込める(kagi はそれを**実行しない**)。

### 実装方式

- 既存 `execute_commit` を流用し、**message を `"fixup! " + 対象 subject`(または `"squash! " + subject`)で
  組み立てるだけ**。新しい backend pipeline は作らない。
- 対象 commit の subject 取得は既存 log/snapshot から(1行目)。subject に改行は無いので prefix 連結のみ。
- checklist(ADR-0039/0043)は通常 commit と同様に通す(staged 必須、conflict marker block 等)。
- 対象 commit の選択 availability: 対象は **現在 branch から到達可能**であることを推奨(autosquash 対象に
  なり得るため)。到達不能でも commit 自体は作れるが、warn で「この commit は現在 branch に含まれないため
  autosquash 対象になりません」を出す。

### autosquash 実行は MVP 外(later)

- `git rebase -i --autosquash` 相当(fixup/squash を実際に畳み込む)は **履歴書き換え**のため本スイートでは
  設計・実装しない。将来やるなら ADR-0023 の history-rewriting + 2段階確認 + in-memory rebase の専用 ADR が要る。

## Consequences

- 「fixup/squash を作る」だけなら既存 commit pipeline の message 組み立てで完結 → 低リスク・低コスト
- 履歴書き換えを一切しないので、本スイートの「安全な履歴編集」方針(force/rewrite を最小化)に整合
- autosquash 実行は別フェーズ(専用 ADR)に切り出し。MVP の fixup commit はその下準備として無駄にならない
