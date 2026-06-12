# ADR-0033: GitButler からの概念採用

- Status: Proposed
- Date: 2026-06-12
- 関連調査: docs/research/gitbutler-reuse-research.md
- 関連 ADR: 0031(流用ポリシー), 0004(destructive operation policy), 0011(undo commit), 0023(dangerous operations), 0025(worktree creation UX)
- ライセンス: GitButler = FSL-1.1-MIT(原文確認済: `LICENSE.md`)

## Context

GitButler は virtual branch / stacked branch / hunk assignment / oplog snapshot / worktree / agent workflow を備える。ただし **FSL-1.1-MIT** は "Competing Use"(本ソフトと実質的に類似する機能の商用提供)を禁止し、kagi(Git GUI)はこれに該当する蓋然性が高い。各バージョンは公開 2 年後に MIT へ転換するが、commit 単位の日付確認が必要。**コード流用は原則不可、concept adoption のみ**(ADR-0031 ゲート)。

## Decision

- **Reimplement(最優先 concept)**: oplog snapshot の atomicity。GitButler の `UnmaterializedOplogSnapshot`(操作前に snapshot を作り、**成功時にのみ oplog へ確定、失敗なら破棄** = all-or-nothing)思想を、kagi の `src/git/oplog.rs` に**自前再実装**で導入する。kagi の安全パイプライン(plan→confirm→preflight→execute→verify→oplog)と完全整合。GitButler コードは転写せず概念のみ。kagi は JSONL 別実装(GitButler は Git tree snapshot)。
- **Study only(将来 Reimplement)**: stacked branch のデータモデル(`StackSegment` の segment + base pointer による依存ブランチ連鎖)。kagi の change lane 思想と整合するが、but-graph は gix/petgraph 結合で FSL のためコード不可。将来導入時は kagi の git2 型で全面再設計。MVP には入れない。
- **Study only**: hunk assignment / hunk dependency のアルゴリズム(hunk をどの commit/lane へ割り当てるか、AmendableCommit/IntroducingCommit 判定)。将来 partial staging 高度化の参考。but-ctx/gix 結合・FSL でコード不可。
- **Study only**: worktree モデル(worktree id を stack id と直交させる設計)。ADR-0025 worktree UX の概念補強。but-db 結合のためコード不可。
- **Study only**: but-graph の segment 抽象、agent workflow(but-action/rules/llm)。前者は kagi の lane layout に過剰、後者は MVP スコープ外。将来 automation の設計言語として記録。
- **Reject(MVP)**: virtual / parallel branch(workspace commit による HEAD 管理・worktree 書き換え)。kagi の「destructive operation 禁止・無傷 in-memory merge」方針(ADR-0004/0005)と思想衝突。FSL Competing Use の中核機能でもある。**MVP に入れない判断を堅持**(ポリシー上の禁止事項)。

## Consequences

- GitButler からはコードを一切転写せず概念のみ採用。FSL Competing Use リスクを回避。
- oplog atomicity の Reimplement は kagi の安全パイプラインを堅牢化する高価値 concept。verify 失敗時に oplog を汚さない二段階確定を導入できる。
- stacked branch / hunk assignment / worktree / agent は Study に留置し、将来機能の設計言語として参照可能にする。MVP スコープは膨張しない。
- virtual branch を Reject(MVP)することで安全パイプラインと destructive 禁止方針の一貫性を維持。
