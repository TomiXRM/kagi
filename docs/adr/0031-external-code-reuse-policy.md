# ADR-0031: 外部コード流用ポリシー(分類基準・ライセンスゲート・取り込み手順)

- Status: Proposed
- Date: 2026-06-12
- 関連調査: docs/research/jj-reuse-research.md, gitbutler-reuse-research.md, zed-gpui-reuse-research.md
- 関連 ADR: 0032(jj), 0033(gitbutler), 0034(zed/gpui), 既存 0001/0002/0006

## Context

jj / GitButler / Zed から設計・ロジックを取り込む際、無秩序な流用を避け、ライセンス・依存・コスト・アーキ適合性・メンテリスクを確認した上で採否を判断する共通基準が必要。本 ADR は分類基準とライセンスゲート、取り込み手順を定める。

## Decision

### 1. 分類(5 区分)

- **Adopt directly**: 許容ライセンス + kagi の依存/アーキと整合し、そのまま依存追加 or コード参照可能。
- **Port**: 許容ライセンスだが kagi の型(git2 等)に合わせて移植が必要。原典のライセンス表記を保持。
- **Reimplement**: ライセンスや依存の都合でコードは使わず、**概念のみ**を kagi 流に再実装(原典コードは見るが転写しない)。
- **Study only**: 現時点では採用しないが設計言語として記録(将来 ADR で再評価)。
- **Reject**: ライセンス汚染・依存衝突・アーキ不整合で採用しない。

### 2. ライセンスゲート(取り込み前に必ず通過)

- **必ず原文(LICENSE ファイル)を確認**してから判断する。crates.io/README の表記だけで判断しない。
- **Apache-2.0 / MIT / BSD**: コード流用可(Port/Adopt 可)。NOTICE/著作権表記の保持義務を守る。
- **GPL-3.0-or-later / AGPL**: kagi(非 GPL 配布想定)へのコード転写は**禁止(汚染)**。設計パターン参照のみ可 = Study/Reimplement 上限。
  - 該当: Zed の `terminal` / `terminal_view` / `ui` / `project` / `git` / `git_ui` / `editor` 等(`crates/gpui` のみ Apache-2.0 で例外)。
- **FSL-1.1-MIT(GitButler)**: kagi は Git GUI = FSL の "Competing Use"(実質的に類似する機能)に該当する蓋然性が高く、**コード流用は原則禁止**。concept adoption のみ。
  - 例外: 当該コミットが公開から **2 年経過し MIT へ転換済み**であることを commit 日付で原文確認できた場合のみ、Port を個別検討(運用負荷が高く原則使わない)。

### 3. 取り込み手順(チェックリスト)

候補ごとに以下 ⑩項目を research ドキュメントに記録してから採否を確定する:
1. ライセンス(原文確認の有無)
2. 依存 crate(特に gix vs git2、tokio、DB、Tauri 結合の有無)
3. UI / ロジック分離度
4. 単独 crate 切り出し可否
5. gpui アプリ統合可否
6. 流用 vs 再実装の判断
7. MVP or later
8. テスト戦略
9. 既存アーキ(plan→confirm→preflight→execute→verify→oplog、git2 単一 backend、無傷 in-memory merge、destructive 禁止)への影響
10. メンテリスク

### 4. ハードルール(禁止事項)

- 調査・ライセンス確認なしの外部コードコピー禁止。
- GPL crate からのコード転写禁止(gpui Apache を除く)。
- FSL コードの Competing-Use 流用禁止。
- jj を Git backend に直接採用しない(gix 依存・backend 置換)。
- GitButler Virtual Branch を MVP に入れない。
- Zed 内部 crate(gpui 以外)の依存追加禁止。
- 採用確定はユーザー承認後。本 ADR 群は **Status: Proposed** で提示する。

## Consequences

- 流用判断が記録・再現可能になり、ライセンスリスク(GPL 汚染・FSL Competing Use)を入口で遮断できる。
- 多くの候補が「concept adoption(Reimplement/Study)」に倒れるため、実装は kagi 流の再実装が中心となり工数は増えるが、依存の純度とライセンス安全性を確保できる。
- subagent への指示にも本ゲートを明記し、誤った転写を防ぐ。
