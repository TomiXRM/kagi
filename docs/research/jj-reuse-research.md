# jj (Jujutsu VCS) 流用調査

- 調査日: 2026-06-12 / 調査者: research subagent
- 対象: `jj-vcs/jj` shallow clone (`/tmp/kagi-research/jj`, version 0.42.0)
- ライセンス: **Apache-2.0**(原文確認済: `/tmp/kagi-research/jj/LICENSE` 冒頭 "Apache License Version 2.0"、`Cargo.toml` workspace `license = "Apache-2.0"`)
- 関連: ADR-0032(jj concepts adoption)、ADR-0031(流用ポリシー)

## 前提(kagi 側の制約)

- kagi の Git backend は **git2 0.21**(libgit2 binding)。jj の lib は **gix(gitoxide)** に全面依存。
  両者は別世界の型体系で、jj-lib を直接リンクすると gix が依存に乗る → 「バイナリ依存なし方針」「git2 単一 backend」と衝突する。
- jj-lib は約 **78,000 LOC** の単一巨大 crate(`/tmp/kagi-research/jj/lib`)。部分的な crate 切り出しは upstream に存在しない。
- jj は Apache-2.0 なので **コード流用自体はライセンス的に可能**。判断軸はライセンスではなく依存・コスト・アーキ適合性。

## 観点ごとの findings

### 1. Operation log / undo model

- ファイル: `/tmp/kagi-research/jj/lib/src/operation.rs`, `op_store.rs`, `op_walk.rs`, `protos/op_store.proto`, `protos/simple_op_store.proto`
- 主要型: `op_store::Operation`(`view_id` / `parents: Vec<OperationId>` / `metadata` / `commit_predecessors`)、`op_store::View`(head_ids・bookmarks・tags・git refs・wc_commit_ids のスナップショット)、`OpStore` トレイト(async, content-addressed)、`OperationMetadata`(time range / description / hostname / is_snapshot)。
- モデル: **content-addressed な operation DAG**。並行編集で fork し得る。undo は operation 履歴を親方向に辿る。serialization は **protobuf(prost)**。
- 依存: `async-trait`, `prost`。
- kagi 現状との差: kagi は `$HOME/.kagi/operations.jsonl`(append-only JSONL、`src/git/oplog.rs`)。jj は content-addressed object store + View スナップショット + DAG。**思想は近いが実装層が重い(protobuf + 独自 object store)**。

### 2. Revset

- ファイル: `/tmp/kagi-research/jj/lib/src/revset.rs`(約 241KB)、`revset_parser.rs`、`revset.pest`(PEG 文法 142 行)
- アーキ: pest パーサ → `UserRevsetExpression`(AST, `RevsetExpression<St: ExpressionState>` 50+ variant)→ symbol resolution → **Index トレイトに対する評価**。言語層は storage-agnostic。
- 依存: `pest`, `pest_derive`, `regex`, `itertools`。ただし `revset.rs` は `use crate::` が **37 箇所** = jj 内部(Index, Commit, Backend, op_store 等)と密結合。
- 評価: 言語仕様は美しく separable に見えるが、評価エンジンは jj の Index/Commit 型に強く依存。**そのまま切り出すと jj-lib のかなりの部分を引き込む**。

### 3. Working copy model

- ファイル: `working_copy.rs`(trait)、`local_working_copy.rs`(約 122KB 実装)、`protos/local_working_copy.proto`
- 主要型: `WorkingCopy` / `LockedWorkingCopy` トレイト、`LocalWorkingCopy`、`MergedTree`(conflict を内包)。TreeState proto に conflict 用の正負 tree_ids。
- 思想: **first-class conflict**(working copy 自体が conflict を保持できる)。jj 独自の working copy snapshot モデルで、Git の index 概念を置き換えている。
- 評価: kagi は git2 の index/worktree を直接使う方針。jj の working copy モデルは Git backend と排他的な設計思想であり、採用は backend 置き換えと同義。

### 4. Conflict model

- ファイル: `merge.rs`(`Merge<T>` 汎用型)、`conflicts.rs`(materialization)
- 主要型: **`Merge<T>`** = 正/負の項を交互に持つ汎用構造(`SmallVec<[T;1]>`)。`resolved()` / `from_vec()` / `removes()` / `adds()` 等。`MaterializedTreeValue`(表示用に展開)。
- 依存: `merge.rs` は `futures` / `itertools` / `smallvec` のみ(jj 内部型に**非依存** = 純データ構造)。**全 7 観点で最も separable**。
- 思想: conflict を「未解決の merge」として 3-way 以上を一般化して表現。Git の `<<<<<<<` マーカーは materialize 時にだけ生成。

### 5. Git backend 統合

- ファイル: `git_backend.rs`(約 94KB)、`git.rs`(import/export, 約 133KB)、`git_subprocess.rs`(約 47KB)
- **gix(gitoxide)を使用**(`lib/Cargo.toml`: `gix = { workspace=true, optional=true }`、feature `git = ["dep:gix"]`、default 有効)。libgit2/git2 は**不使用**。一部操作は `git` サブプロセス。
- 評価: kagi の git2 0.21 とは**バイナリ・型ともに非互換**。jj の git 統合層は流用不可(初期仮説「jj を Git backend に直接採用しない」を裏付け)。

### 6. Graph traversal / DAG

- ファイル: `graph.rs`(汎用アルゴリズム)、`dag_walk.rs` / `dag_walk_async.rs`、`default_index/`(12 モジュール)
- 主要型: `GraphNode<N,ID>` = `(N, Vec<GraphEdge<ID>>)`、`GraphEdge`(Missing/Direct/Indirect)、`dfs` / `topo_order_forward` / `topo_order_reverse` / `heads`。Index は generation number をキャッシュし revset の範囲クエリを高速化。
- **lane / レイアウトアルゴリズムは無し**(描画は UI 側に委ねる)。kagi は既に自前 lane layout(ADR-0003)を持つので、ここで得るものは「topo order + generation number indexing」の概念のみ。
- 依存: `futures`, `itertools`, `smallvec`。

### 7. Storage abstraction

- ファイル: `backend.rs`(`Backend` トレイト)
- `Backend` は object storage(file/tree/commit/symlink/copy の read/write、全 async)に純化。実装は `GitBackend`(gix)/ `SimpleBackend` / `SecretBackend`。
- revset / conflict / op-log は Backend トレイトに対して書かれており**理論上は別 backend で動く**。ただし「別 backend を実装する」コストは Git backend 相当の再実装に等しい。

## 候補テーブル

| 候補 | 分類 | 理由 | コスト | リスク |
|---|---|---|---|---|
| `Merge<T>` conflict 表現(3-way 一般化) | **Reimplement** | 純データ構造で依存最小。だが Apache コードを git2 世界に薄く再実装する方が型整合が良い。kagi の in-memory merge(ADR-0005)を将来 N-way へ拡張する核 | 中(設計のみなら小) | 低 |
| Operation log DAG + View スナップショット思想 | **Study only** | kagi は既に JSONL oplog 採用済(`src/git/oplog.rs`)。content-addressed object store + protobuf は MVP に過剰。undo 粒度・metadata 項目の参考にする | 小 | 低 |
| Revset 言語(query DSL) | **Study only**(将来 Reimplement) | UX 価値は高いが評価エンジンが jj Index と 37 箇所結合。git2 上で pest 文法を自前実装するなら later。MVP 不要 | 大 | 中(自前評価器の正しさ) |
| Working copy model(first-class conflict WC) | **Reject** | Git index を置換する設計思想。kagi の git2 方針と排他。採用 = backend 総入れ替え | 特大 | 高 |
| Git backend(gix 統合層) | **Reject** | gix 依存。kagi は git2 単一 backend 方針 | 特大 | 高 |
| Graph traversal / generation-number index | **Study only** | kagi は自前 lane layout 済。topo order と generation index は概念参考のみ | 小 | 低 |
| Storage abstraction(`Backend` trait) | **Study only** | backend 差し替え可能性は魅力だが kagi は git2 固定で十分。over-abstraction を避ける | 小 | 低 |

## kagi への具体的提案

1. **conflict 表現の将来拡張**: kagi の in-memory merge(ADR-0005)が 2-way 前提なら、`Merge<T>` の「正負項の列で N-way を表す」概念を **自前再実装**で取り込む余地あり。ただし Apache コードのコピーではなく、概念を kagi の git2 型(`git2::Index`/`git2::IndexConflict`)に合わせて書く。MVP では不要、later。
2. **oplog の metadata 設計**: jj の `OperationMetadata`(description / hostname / is_snapshot / time range)を kagi の operations.jsonl スキーマ拡張の参考にする(Study)。undo を「op 単位の巻き戻し」へ進化させる際の設計言語として有用。
3. **revset は明確に later**: 評価エンジンの jj 結合度が高く、MVP では Repository Navigator のフィルタ(ADR-0014)で十分。将来 power-user 向けに pest ベースの自前 query を検討(ADR で別途)。
4. **gix / working copy / backend は touch しない**: 初期仮説どおり Reject。git2 単一 backend 方針(ADR-0002)を維持。
5. **コピーは行わない**: jj は Apache なので法的にはコピー可能だが、gix 前提コードが大半で git2 世界へ移植コストが高く、概念採用(concept adoption)に留めるのが合理的。

## 確認できなかった事項

- `Merge<T>` を git2 型へ再実装した際の具体的な API 形(実装フェーズで設計)。
- revset 文法のうち kagi で意味を持つサブセット(将来 ADR で切り出し必要)。
