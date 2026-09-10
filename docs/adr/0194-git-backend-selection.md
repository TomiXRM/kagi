# ADR-0194: Git backend は libgit2 を既定にし、CLI は明示 mode に限る

- Status: Accepted
- Date: 2026-09-10
- Related: [#627](https://github.com/TomiXRM/kagi/issues/627)、[ADR-0146](0146-run-git-hardening.md)、[ADR-0193](0193-index-stat-cache-refresh.md)
- 証拠: [`RPT-627`](../research/627/RPT-627.md)（統合報告。知見 ID `F-nn` で引用する）、
  `docs/research/627/E4-platform-matrix.md`（決定表）

## 文脈

出発点は「libgit2 を使い続けると危険なのか」という問いだった。GitKraken も VS Code も
内部に Git CLI を持つと分かっており、CLI に寄せれば Git 本体と挙動が揃うのではないか、
そして libgit2 は遅いのではないか、という仮説があった。

7 つのパッケージ（P0 harness、P1–P5 実験、P6 統合）で実測した。結果は仮説の**逆**だった。

### 安全性 — libgit2 は repository config 由来のコード実行を構造的に避ける

libgit2 の `statuses` / `diff_index_to_workdir` / index `add_path` / `checkout_tree` の
4 経路は、いずれも `.gitattributes` の filter を**実行しない**。filter が走ることを
確認した bare `git add` を対照に取ってある。

kagi は「実行前に何が起きるかを予測して見せる」ことが存在理由なので、**予測は敵対的な
repository でも走る**。予測経路を CLI に置けば、`.gitattributes` や `.git/config` を
書ける者がその経路でコードを実行できる。実際 v0.37.0 では `run_git` の hardening を
迂回する経路が rebase 経由で shipped していた（ADR-0146 系の修正で塞いだ）。

**予測を libgit2 に置くことは、この攻撃面を実装の注意深さではなく構造で消す。**

### 速度 — 遅さの主因は backend ではなかった

「libgit2 は遅い」という観測の主因は **index stat cache** だった。libgit2 は index の
stat data と一致しない file の内容を毎回ハッシュし直し、`git status` と違って refresh を
書き戻さない。50,000 file の repo を実機で開くと 4.25 秒かかり、何か触るまで直らなかった。
ADR-0193 で repair を入れて **0.09 秒**になった。

backend を揃えて測ると、composite の `snapshot` は全規模で libgit2 が速い
（S 約41x、M 約6x、L 約1.4x）。CLI 合成は 9 プロセスを起動する。
`status` 単体は 20k 以上で CLI が速いが、それは composite の既定を覆さない。

## 決定

1. **予測と読み取りの既定 backend は libgit2。** 主根拠は速度ではなく上記の安全性で、
   速度とトレードしない。
2. **CLI は利用者が明示選択する `CLI compatibility mode` としてのみ提供する。**
3. **repo size / cache / host / OS による暗黙の自動切替は採らない。**
4. **`snapshot` の CLI 全置換は採らない。** 現行の CLI 合成は 4 情報を返せず情報等価でない
   （common-dir の `FETCH_HEAD` mtime、commit budget 外の detached worktree root、
   annotated tag の peeling、linked worktree の WIP counts）。原理的な限界ではなく
   現合成の未実装だが、埋めるには追加プロセスとコストがかかる。
5. **意味論を変えない局所最適化は backend 選択から独立に扱う。** pathspec status と
   index stat cache repair はこれに当たり、既定 backend を変える根拠にしない。

### なぜ自動切替を採らないか

`status` 単体の crossover（20k 付近）は実測された事実であり、E4 に残してある。それでも
サイズで backend を切り替えない。kagi は実行前の予測を見せる製品なので、**予測経路の
挙動が入力サイズで暗黙に変わると製品の前提が壊れる**。P3 が測った backend 間の意味論差が、
repo が大きくなった瞬間に無言で発現することになる。速度のために整合性を捨てる形なので
採らない。利用者が明示的に選んだ mode なら、挙動の違いは説明可能なまま残る。

## 帰結

- 新しい backend を operation に採るには、意味論・安全性・性能を
  **macOS / Linux / Windows の全て**で確認する。揃わない operation は現行 backend に
  据え置く。これは既存の CLI operation を差し戻す理由ではない（`fetch` / `push` は
  CLI のまま）。
- 現在の証拠は **macOS arm64 の単一 host**である。Linux / Windows は未取得で、
  `CLI compatibility mode` の既定昇格・OS 別規則は未確定。
- P5 の D2（syscall / bytes、root 権限が必要）と F の境界条件（衝突する apply、
  大きい fixture、untracked 復元）は未確定のまま残る。F の食い違い 0 件は探索範囲の
  結果であり、混在が安全であることの証明ではない。
- **全 write の `plan → confirm → preflight → execute → verify → oplog` は、
  backend 選択と無関係に不変条件である。** F の結果はこれを緩める根拠にならない。
- production の `snapshot()` に stage 単位の計測 seam が無いため、libgit2 側の
  最も重い段は `working_tree_status` 以外分かっていない。A2 の局所候補を他段へ
  一般化する前に、private helper を露出しない seam の設計が要る。
