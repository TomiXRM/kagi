# ADR-0032: jj(Jujutsu)からの概念採用

- Status: Proposed
- Date: 2026-06-12
- 関連調査: docs/research/jj-reuse-research.md
- 関連 ADR: 0031(流用ポリシー), 0002(git backend), 0005(conflict policy), 0011(undo commit)
- ライセンス: jj = Apache-2.0(原文確認済)

## Context

jj-lib(約 78,000 LOC、Apache-2.0)は revset / operation log / first-class conflict / working copy / gix backend を備える。Apache のためコード流用自体は可能だが、**gix(gitoxide)前提**で kagi の git2 0.21 単一 backend 方針(ADR-0002)と非互換。判断はライセンスではなく依存・コスト・アーキ適合性で行う。

## Decision

- **Reimplement(将来)**: `Merge<T>` 型の「正/負の項列で 2-way 以上の conflict を一般化表現する」概念。kagi の in-memory merge(ADR-0005)を将来 N-way へ拡張する核として、git2 型(`git2::Index`/`IndexConflict`)に合わせて**自前再実装**する。jj の Apache コードは見るが転写しない(git2 世界へ移植する方が型整合が良いため)。MVP 不要。
- **Study only**: Operation log の DAG + View スナップショット + `OperationMetadata`(description/hostname/is_snapshot/time range)。kagi は既に `$HOME/.kagi/operations.jsonl`(`src/git/oplog.rs`)を持つため、undo 粒度・metadata スキーマ拡張の設計言語として記録。content-addressed object store + protobuf は MVP に過剰で採用しない。
- **Study only(将来 Reimplement)**: revset query DSL。UX 価値は高いが評価エンジンが jj の Index に 37 箇所結合。将来 power-user 向けに pest ベースで git2 上に自前実装を検討(別 ADR)。MVP は Navigator フィルタ(ADR-0014)で十分。
- **Study only**: graph traversal の generation-number indexing と topo order。kagi は自前 lane layout(ADR-0003)を持つため概念参考のみ。
- **Study only**: storage abstraction(`Backend` trait)。backend 差し替えは魅力だが kagi は git2 固定で十分。over-abstraction を避ける。
- **Reject**: working copy model(first-class conflict WC)と gix Git backend 統合層。いずれも Git index/git2 を置換する設計思想で、採用 = backend 総入れ替え。kagi の方針と排他。

## Consequences

- jj からは「概念のみ」を取り込み、コードは転写しない。Apache でも gix 前提のため移植コストが高く、concept adoption が合理的。
- conflict 表現の N-way 拡張(Reimplement)は将来の partial/complex merge 強化の基盤になり得るが、MVP には入れない。
- revset は later に明確化。MVP のスコープが膨らまない。
- gix / working copy / backend に触れないことで git2 単一 backend 方針(ADR-0002)とバイナリ依存なし方針を維持。
