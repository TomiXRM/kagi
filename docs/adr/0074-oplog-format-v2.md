# ADR-0074: Operation Log Format v2

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture. See `docs/rearch/research/03-git-backend.md`, supersedes the format implied by ADR-0004/0011 oplog usage.

## Decision

`~/.kagi/operations.jsonl`(append-only JSONL)を v2 に拡張する:

- 各エントリに **operation 種別、引数、before/after の HEAD SHA・branch tips、影響ファイル、recovery handle**(discard の ODB blob OID 等)を含める。
- 1 operation = 1 行を維持(crash-safe append)。
- 旧 v1 行(stringified summary のみ)は読み取り時に best-effort で表示し、書き込みは v2 のみ。
- パイプライン末尾(verify 後)で `OperationController` が**必ず1回**追記する(v0.2.0 は呼び出し側任意で、抜けがあった)。

## なぜ

v0.2.0 の oplog は before/after を**文字列要約**でしか持たず、実際の復旧(undo / discard 復元)に使える機械可読情報(SHA、blob OID、引数)が無い。Kagi の安全思想「nothing is silently lost / 失敗時の復旧手順を事前提示」を実体のある復旧機能に育てるには、構造化された before/after が要る。jj の operation log(content-addressed op-DAG)の**概念**を借りるが、独自の軽量 JSONL に留める。

## 代替案

1. v1 のまま(文字列要約)。
2. jj 風 content-addressed op-store(独立 DAG)。
3. 本決定の JSONL v2(構造化フィールド追加、単純追記)。

## 捨てた案

- 案1: 復旧機能を機械的に実装できない。却下。
- 案2: 独立 op-store/DAG は実装・保守コストが過大で MVP には過剰。protobuf/gix も不要。却下。概念(undo by stepping、op metadata)だけ採用。

## 将来の負債 / リスク

- ファイル肥大: ローテーション/上限(在メモリは 500 件 ring buffer のまま)を将来必要に応じて追加。
- スキーマ進化: 未知フィールドを壊さない serde 境界(`#[serde(flatten)]` 的)で前方互換を保つ。
- v1→v2 移行で既存ログの一部情報は失われる(後追いで SHA を埋められない)— 許容する。

## Consequences

- undo / discard 復元 UI が oplog から recovery handle を引けるようになる(将来チケット)。
- oplog パネル(Bottom Panel)は v2 フィールドで before→after を具体表示できる。
