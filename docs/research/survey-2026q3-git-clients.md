# git クライアント外部サーベイ（GUI / TUI / CLI / 新世代 VCS / AI 統合）

調査日: 2026-09-03 / 担当スライス: git クライアントとその実装リポジトリ
調査手法: GitHub API（スター数・言語・ライセンス・最終 push は 2026-09-03 取得）、crates.io API（バージョン）、各プロジェクトの公式 docs / README / CHANGELOG の直読み、補助的に web 検索。
既存 `docs/research/` と重複する領域は各項目に「**既存調査あり**」と明記し、深追いせず差分のみ記述する。

---

## 1. サマリ

Kagi に効く上位 5 点（すべて「安全性優先・コミットグラフ中心」という Kagi の芯を強化する方向）:

1. **operation log を「view スナップショットの DAG」へ格上げ**（jj）: 1 操作 = 「全 ref + heads + WIP の完全スナップショット」。`undo` だけでなく `op restore`（任意時点へ全復元）/ `op revert`（過去の 1 操作だけ打ち消し）/ `--at-op`（過去時点で読み取り専用に repo を眺める）の 3 操作が生える。Kagi の oplog は現在「操作の列 + undo/redo」で、**時点復元と選択的取り消しが無い**のが最大の差分。
2. **absorb（変更の自動吸収）を GUI 一級市民に**（jj / Sapling / git-absorb）: 「未コミット hunk を、その行を最後に触った mutable な祖先コミットへ自動配分」。曖昧なら working copy に残す＝**安全側にフェイル**する設計で、Kagi の plan→confirm と極めて相性が良い。GUI で「どの hunk がどのコミットへ行くか」を確認してから execute できるのは既存 GUI に無い空白地帯。
3. **change ID（コミットの同一性を rewrite 越しに保つ ID）**（jj）: amend/rebase でハッシュが変わってもコミットの「同一性」が保たれる。Kagi のグラフは既に ghost connector で squash merge を追跡しているが、**同一 change の系譜（evolog）を持たない**。amend/rebase 前後をグラフ上で線でつなぐ表示は Kagi の差別化になる。
4. **「解決状態と出口が常に見える」+ 「打った git コマンドを見せる」**（Sublime Merge / GitHub Desktop の反面教師 / Tower）: Sublime Merge の "Real Git — View the exact Git commands you're using" は Kagi の plan/preflight 表示の延長線上にあり、AI エージェントに渡す監査ログとしても効く。
5. **AI native は「エージェントに repo を触らせる」より「エージェントの成果物を安全に取り込む」側で勝てる**（Cursor / GitButler `but` / vibe-kanban / Conductor）: 業界の潮流は「worktree ごとに並列エージェント → 人間が diff を裁定 → Apply/Reject」。Kagi は既に worktree 管理 + repo タブ + in-memory merge + oplog を持つので、**「並列エージェント worktree の裁定盤」は既存資産の再配置だけで到達できる最短の AI native 化**。

---

## 2. 詳細

### 2.1 新世代 VCS / VCS そのものを変えるもの

#### Jujutsu (jj) — 深掘り

- **何か**: Git 互換（Git backend）でありながらデータモデルを置き換えた VCS。「working copy がコミット」「conflict がコミットに入る」「全操作が op log に残る」の 3 本柱。
- **リポジトリ / 規模 / 言語**: <https://github.com/jj-vcs/jj> / ★31,347 / Rust / Apache-2.0 / 最終 push 2026-09-02。CLI crate `jj-cli` の最新安定版は **0.44.0**（crates.io, 2026-08-06 更新）。既存調査は 0.42.0 時点。
- **出典**:
  - 用語モデル: <https://docs.jj-vcs.dev/latest/glossary/>
  - first-class conflicts: <https://docs.jj-vcs.dev/latest/conflicts/>
  - operation log: <https://docs.jj-vcs.dev/latest/operation-log/>
  - CLI reference（absorb / arrange / run / bisect / op 系の全オプション）: <https://docs.jj-vcs.dev/latest/cli-reference/>
  - CHANGELOG（`jj converge` 等の未リリース機能）: <https://github.com/jj-vcs/jj/blob/main/CHANGELOG.md>
- **既存調査あり**: `docs/research/jj-reuse-research.md` が ①op log の内部実装（protobuf / content-addressed OpStore）②revset の評価エンジン結合度 ③working copy モデル ④`Merge<T>` conflict 表現 ⑤gix 依存による流用不可 ⑥graph traversal ⑦Backend trait を既にカバー済み。**以下は既存調査に無い差分のみ**。

**差分 (a) change ID vs commit ID**（glossary）:
`change ID` は 16 バイトのランダム ID で、`jj log` では **z–k を「数字」に使う 12 文字**として表示される（`0-9a-f` の代わりに `z-k`。commit ID の 16 進表示と目で区別できるようにするため）。rewrite（describe / rebase / squash / working copy 編集）は commit ID を変えるが **change ID は保つ**。さらに `xyz/0`, `xyz/1` という **change offset** で「同じ change の N 世代前の版」を指せる。1 つの change に visible な commit が 2 つ以上ある状態を **divergent change** と呼び、log に "divergent" ラベルが出る。visible/hidden は「view の anonymous head から到達可能か」で定義され、hidden な commit も commit ID か `change ID + offset` で参照できる（＝**捨てても消えない**）。
`jj evolog` が「1 つの change の時間変化」を表示するサブコマンドとして独立している。

**差分 (b) auto-rebase of descendants**（conflicts doc）:
first-class conflicts の帰結として「rewrite されたコミットの子孫が自動で rewrite される」。公式ドキュメントはこれを Mercurial の Changeset Evolution の代替と位置づけ、`git rebase/merge/cherry-pick --continue` が**不要になる**（＝解決フローが「conflict コミットを checkout → 直す → amend」の 1 本に統一される）と明記している。加えて「merge commit の変更内容を『マージされた親との差分』として定義するので merge commit も正しく rebase できる。これは `git rerere` の主要ユースケースを解消する」と述べている。`jj abandon --restore-descendants` / `jj run --restore-descendants` は「子孫の**内容**（diff ではなく tree）を保つ」オプションで、自動 rebase の副作用を打ち消す逃げ道になっている。

**差分 (c) conflict のマテリアライズ形式が 3 種類**（conflicts doc）:
Kagi の conflict editor は diff3 を持つが、jj のデフォルトは **diff3 でも snapshot でもない第 3 の形式**:
```
<<<<<<< conflict 1 of 1
%%%%%%% diff from: <base>
\\\\\\\        to: <side A>
 apple
-grape
+grapefruit
 orange
+++++++ <side B>
APPLE
GRAPE
ORANGE
>>>>>>> conflict 1 of 1 ends
```
「1 つの snapshot（片側の全文）＋ 他の各側を snapshot への diff として表示」する形式で、`ui.conflict-marker-style` で `snapshot` / `git`(=diff3) に切替可。狙いは公式に「各側を目で見比べる手間を省く」「**N 側 conflict でも snapshot 1 つ + diff N-1 個で表現でき、diff を順に当てるだけで解決できる**」と説明されている。git 形式は 2 側しか表せないので 3 側以上では snapshot 形式にフォールバックする。マーカーが本文と衝突しうる場合は **マーカーを 15 文字に伸ばす**（`<<<<<<<<<<<<<<<`）。末尾改行が無い側があるときは各項に改行を足し、代わりに `>>>>>>>` 行の改行を落とす、というエッジケースまで仕様化されている。
- **Kagi への示唆**: Kagi の conflict editor は「ours/theirs/manual + diff3」だが、**「base スナップショット + 各側の diff」ビューを 3 つ目のモードとして持つと、3-way 以上（octopus / criss-cross）に自然に拡張できる**。長マーカー・末尾改行のエッジケースは Kagi の marker parse 実装の**テストケース候補**としてそのまま使える。
- **難易度**: 表示モード追加のみなら M、N-way データモデル化は L。

**差分 (d) `jj absorb`**（CLI reference）:
> "This command splits changes in the source revision and moves each change to the closest mutable ancestor where the corresponding lines were modified last. If the destination revision cannot be determined unambiguously, the change will be left in the source revision."

オプション: `-f/--from <REVSET>`（既定 `@`）、`-t/--into <REVSETS>`（既定 `mutable()`、祖先のみ対象）、`-i/--interactive`（hunk を選んで吸収、選んだ hunk が複数の祖先に分配され得る）、`--tool`、`[FILESETS]`（パス限定）。全部吸収され description が空なら source revision は abandon される。そして **「`jj absorb` の変更は `jj op show -p` でレビューできる」**と明記されている＝op log がそのまま「AI/自動化の結果を人間が検証する窓」になっている。
- **Kagi への示唆**: これが Kagi に最も刺さる 1 機能。「未コミット hunk → その行を最後に触った祖先コミット」の割当計算は git2 の blame + `git log -L` 相当で実装でき、**`plan` に「hunk → 宛先コミット」の対応表を出して confirm、曖昧なものは working tree に残す**という形で Kagi の安全パイプラインにそのまま乗る。GitButler の hunk-dependency（既存調査あり）と同じ問題を、FSL でない Apache/BSD 実装（jj / git-absorb）の思想で解けるのが利点。
- **難易度**: M（割当計算 S〜M + preflight/execute/verify + oplog 統合）。

**差分 (e) op log 系の 3 コマンド + `--no-integrate-operation`**（operation-log doc / CLI reference）:
- `jj undo`（直前 1 操作を戻す）/ `jj redo` / **`jj op revert <op>`（最新でない特定の操作だけを打ち消す）** / **`jj op restore <op>`（repo 全体をその時点の姿へ戻す）**。
- `jj op log` は **DAG**。並行実行で fork/merge し得る（＝ロックフリー並行性が op log の存在理由だと公式に書かれている）。`x-` が親、`x+` が子。
- **`--at-op=<id>`**: 任意コマンドを「その操作直後の repo」に対して実行。read-only 用途（`jj log`/`st`/`diff`）が想定で、working copy の snapshot は行わない。
- **`--no-integrate-operation`**: 操作は実行して operation object は作るが **op log に統合せず、operation ID だけを返す**。返った ID を `--at-op` で覗く / `op restore` で適用 / `op integrate` で後から取り込む。ただし「repo 外の副作用は防げない（`jj git push --no-integrate-operation` は実際に push する）」と警告付き。
- `jj op abandon` / `jj op diff` / `jj op show`（`-p` で patch）も独立コマンド。
- **Kagi への示唆**: `--no-integrate-operation` は **Kagi の「plan → confirm → preflight → execute → verify」の verify を「実際にやってから見る」へ強化する設計言語**。Kagi は in-memory merge で無傷予測ができるので同じ目的をより安全に達成しているが、`op restore`（時点復元）と `op revert`（選択的取り消し）の 2 操作は Kagi の oplog に**明確に欠けている**（現在は `HistoryEntry::undo` による直列 undo/redo のみ）。oplog の各エントリに「全 ref + HEAD + stash + WIP のスナップショット」を持たせれば両方が生える。GitButler の oplog snapshot（既存調査あり）と同じ結論に別ルートで到達している＝**設計として堅い**証拠。
- **難易度**: L（oplog スキーマ v3 + 復元プランナ）。時点復元だけなら M。

**差分 (f) 新しめのコマンド群（既存調査 0.42 以降の動き）**（CLI reference / CHANGELOG）:
- **`jj arrange [REVSETS]`** — "Interactively arrange the commit graph"。対話的にコミットグラフを並べ替える TUI。既定対象は `revsets.arrange` 設定。CHANGELOG に「スタックが端末より高いとき選択コミットが見えるようスクロールする」修正が入っており、**実質「グラフ直接編集 UI」がコア CLI に入った**ことを示す。
- **`jj run [-j N] -- <cmd>`** — 「revision 集合の各コミットを隔離 working copy に checkout → コマンド実行 → 結果で amend」。既定で working copy を再利用してビルド成果物を保つ（`--clean` で毎回クリーン）、`--restore-descendants`、並列 `-j`、失敗コマンドの exit code は `jj run` 自体の exit code に影響しない。例: `jj run -j 4 -- pre-commit run ...`。
- **`jj bisect run --range <revset> -- <cmd>`** — revset 範囲に対する自動 bisect。公式例が `jj bisect run --range v1.0..main -- bash -c "jj duplicate xyz -B @ && cargo test"`（＝**テストだけを各世代に当てて bisect** する）というのが上手い。
- **`jj fix`** — フォーマッタ等を履歴全体に当てる。**`jj metaedit`** — 内容を変えずメタデータだけ変更。**`jj gerrit upload`** — Gerrit 連携がコアに。**`jj sign` / `jj unsign`**。**`jj parallelize`** / **`jj simplify-parents`** / **`jj interdiff`**（2 revision の diff の diff）。
- CHANGELOG (Unreleased): **`jj converge`** — divergent change を「両者を置き換える新コミット」で自動解消。ヒューリスティクスで解けなければユーザに問う、非対話モードでは prompt が必要なら abort。**「自動で解こうとするが、確信が持てなければ人間に必ず聞く」**という設計は Kagi の思想と完全一致。
- また colocated repo で **Git HEAD を worktree 単位で内部管理**する変更が入り、jj workspace ごとに独立 Git HEAD を持つ準備が進んでいる。
- **Kagi への示唆**: ①`interdiff`（レビュー 2 版の差分の差分）は **PR review / pushed amend の説明に直結**して価値が高い。②`jj run` の「コミット列に対してコマンドを流す + 結果で amend + worktree 隔離 + 並列」は git-branchless `git test` と同型で、Kagi の worktree 管理と埋め込みターミナルを組み合わせれば「スタック全体を CI 前に緑にする」機能になる。③`jj converge` の「自動解決 + 確信なければ必ず問う」は Kagi の AI 機能に適用すべき憲法。
- **難易度**: interdiff = S、stack-wide コマンド実行 = M、arrange 相当のグラフ直接編集 = L。

#### Sapling (sl) — 深掘り

- **何か**: Meta の Mercurial 派生 VCS。Git リポジトリを扱える（`sl` は Git backend で動く）。UX の中心は smartlog。
- **リポジトリ / 規模 / 言語**: <https://github.com/facebook/sapling> / ★7,001 / Rust（旧 Python 部分あり）/ **GPL-2.0** / 最終 push 2026-09-02。→ **コード流用は ADR-0031 のライセンスゲートで不可。概念のみ。**
- **出典**: smartlog <https://sapling-scm.com/docs/overview/smartlog/> / `sl undo` <https://sapling-scm.com/docs/commands/undo/> / `sl absorb` <https://sapling-scm.com/docs/commands/absorb/> / ISL <https://sapling-scm.com/docs/addons/isl/> / ReviewStack <https://sapling-scm.com/docs/addons/reviewstack/>

**(a) smartlog — 「リポジトリの心象風景」を 1 画面で作らせる**
公式は動機をこう書いている: 「リポジトリの心象を組み立てられないことが分散 VCS 学習の最大の壁。心象が貧弱だと、コマンドが何をするか分からず、ミスから復帰できず、暗記したコマンドをコピペして、行き詰まったら re-clone する」。だから `log`/`branch` を組み合わせさせるのをやめ、**smartlog を UX の中心（`sl` 単体で起動）にした**。
表示するもの: ①未 push のコミット ②main と重要ブランチの位置（設定可）③それらの graph 関係 ④現在位置 `@` ⑤**古くなったコミット（land / rebase / amend 済み）を `x` マークと "Landed as YYY" で表示** ⑥各コミットの短ハッシュ・日付・著者・ローカル/リモート bookmark・タイトル（task 番号や PR 番号も設定で表示可）。
決定的なのは **「自分に関係ないコミットを全部隠す」**こと。main ブランチは左側の**破線**で表現され、数千コミットを省略していることを示す。
`sl ssl`（super smartlog）は GitHub を叩いて **PR 番号 + CI 通過 + レビュー状態（Unreviewed ✗ / Unreviewed ✓ / Approved ✓）をグラフ行に直接埋め込む**。ネットワーク待ちがあるので別 alias に分けている。
- **Kagi への示唆**: Kagi のグラフは「全部を正しく描く」方向、smartlog は「関係ないものを消す」方向。Kagi は既にブランチ solo 表示を持つので、**「smartlog モード = 未 push コミット + main + 重要ブランチ + landed マーカーだけを残し、main の長い直線を破線で圧縮するフィルタプリセット」**として実装できる。特に **"Landed as YYY"（squash merge されたコミットを `x` + 着地先リンクで示す）は Kagi の ghost connector が既に検出している情報の、より人間に優しい提示**。PR/CI/レビュー状態をグラフ行に埋め込む案（`sl ssl`）は Kagi の GitHub 連携資産をグラフへ流し込むだけで、ネットワーク待ちは別トグルにすればよい。
- **難易度**: フィルタプリセット = S〜M、グラフ行への PR/CI バッジ = M。

**(b) `sl undo` の UX が図抜けている**（公式 docs）
- 「直前の **local** コマンド」を戻す。local の定義は「checkout 中のコミットを変えた / ローカルコミットの内容を変えた / ローカル bookmark を変えた」コマンド（`goto`/`commit`/`amend`/`rebase` 等）。**read-only コマンドと非 local コマンドは undo 履歴上でスキップされる**（＝「戻る」が空振りしない）。
- `--step N` / 位置引数で N 手戻る。`-a/--absolute` で「相対 undo でなくコマンド index 指定」。
- **`-k/--keep`: working copy の状態を保って undo**。例として「`commit`/`amend` を undo しつつ変更は pending changes として残す」を挙げている。
- **`-p/--preview`: undo 後の smartlog がどうなるかをグラフで先に見せる**。**`-i/--interactive`: その preview を対話にして undo 履歴を前後に歩ける**。
- hybrid コマンド（`pull --rebase` 等）は **ローカル部分だけ undo し、remote bookmark は戻さない**と明記。`Branch` 引数で undo のスコープを draft コミット群に限定できる。
- undo できないもの: working copy の未コミット変更、remote bookmark の変更。
- **Kagi への示唆**: **「undo の結果グラフを実行前にプレビューする」は Kagi の plan/confirm 設計と 100% 合致し、かつ既存 GUI（GitKraken/Fork/SourceTree/GitHub Desktop、既存調査あり）が誰もやっていない**。Kagi は in-memory でグラフを再計算できるので、confirm モーダルに「before / after のミニグラフ」を出すのは技術的に届く。さらに ①read-only 操作を undo 履歴からスキップ ②`--keep`（履歴は戻すが作業内容は残す）③「ローカルだけ戻し、リモートは戻さない」の明示 は Kagi oplog の**セマンティクス設計としてそのまま採用すべき**。特に②は「amend を取り消したいが書いたコードは失いたくない」という現実の要求に直接答える。
- **難易度**: undo プレビュー = M、`--keep` 相当 = M、履歴スキップ = S。

**(c) `sl absorb`**（公式 docs）: jj との差分が重要。
- **「absorb は working copy に書き込まない」**と明記。
- 曖昧なら working copy に残す（jj と同じ）。
- **対象外の revset を明示: `.%(public() | merge() | immutable)`**（＝public / merge / immutable なコミットは変更しない）。
- **変更適用後に空になったコミットは削除される**。
- **既定で「これから何をするか」を表示して確認を求める**。確信があれば `-a/--apply-changes` で即適用。`-n/--dry-run`、`-I/--include` / `-X/--exclude`、`-P/--immutable`。
- 何か吸収できたら exit 0、何も吸収しなかったら 1。
- **Kagi への示唆**: **「既定が dry-run + 確認、`-a` で明示的に即適用」という既定値の向きが Kagi の思想そのもの**。`public() | merge() | immutable` を触らないという境界定義は、Kagi の「pushed / protected を触らない」ルールに直訳できる。exit code で「吸収ゼロ」を区別するのは自動化/AI から呼ぶときの契約として綺麗。
- **難易度**: M。

**(d) ISL（Interactive Smartlog, `sl web`）**（公式 docs）: 「CLI の全機能は無いが、日常ワークフローを簡単にしローカル変更の極めて明快な絵を出す」設計。学ぶべき具体点:
- コミットに **"You are here"** インジケータ（`@` の人間語化）。
- **ドラッグ&ドロップで rebase**。ただし **未コミット変更があるときは D&D rebase を禁止**（「conflict の扱いが難しくなるから」と明記）→ **Kagi の preflight（dirty tree ブロック, ADR-0105）と同じ判断に独立到達している**。
- D&D rebase は「上に積まれた全コミットを含む `sl rebase`」であり、スタック内の並べ替えは `sl histedit` を使えと**明示的に誘導**する（＝UI が万能を装わない）。
- **実行した Sapling コマンドの引数を UI が表示する**（「CLI で再現できるように」）。
- `sl status` 等は裏で自動実行して UI を常に最新に保つ。**コマンドは自動でキューに積まれ、実行中でも次の操作を続けられる**。
- **Kagi への示唆**: ①"You are here" のような人間語ラベル ②「実行した git コマンドを見せる」（Sublime Merge と同じ結論）③**操作キューイング（実行中でも次の操作を積める）** は Kagi の repo worker thread（ADR-0073）と相性が良い。④「UI が苦手な操作は素直に別機能へ誘導する」姿勢は、Kagi が force 系を持たない設計の説明の仕方としても参考になる。
- **難易度**: コマンド表示 = S、操作キュー = M。

**(e) ReviewStack**（公式 docs）: reviewstack.dev。GitHub PR の URL のドメインを差し替えるだけで開く、**stacked changes 専用の PR レビュー UI**。`sl pr submit` / `sl ghstack` / 素の `ghstack` が作ったスタックを認識し、**スタック内コミットを行き来するドロップダウン**を出す。狙いは「各変更を独立に議論・承認できるようにし、著者がスタック上部に新コミットを足しても下部の会話を壊さない」。`shift+N/P` でスタック内 PR 移動、`ctrl/cmd+.` で timeline 表示トグル、`alt+A/R/C` で Approve/Request changes/Comment。公式が「GitHub の PR UI ほど成熟していないので、足りない機能のために GitHub へのリンクを必ず置く」と明記している点も誠実。
- **Kagi への示唆**: Kagi の PR review conversation 機能に **「スタック内 PR ナビゲーション（shift+N/P）」と「足りない機能は GitHub へ逃がすリンクを必ず置く」** を移植する価値がある。後者は Kagi が `gh` CLI 経由である現状の制約を、欠点でなく設計として提示する方法。
- **難易度**: S〜M。

#### GitButler — **既存調査あり**（差分のみ）

- **リポジトリ / 規模 / 言語**: <https://github.com/gitbutlerapp/gitbutler> / ★21,611 / Rust + Svelte/Tauri / **FSL-1.1-MIT** / 最終 push 2026-09-02。
- `docs/research/gitbutler-reuse-research.md` が virtual branch / stacked branch データモデル / hunk assignment・dependency / oplog snapshot / gix backend / worktree / but-graph / but-action・but-rules・but-llm を既にカバー。**ライセンスゲート（Competing Use によりコード流用不可）も確定済み。以下は 2026 年の AI/CLI 方面の差分のみ。**
- **差分: `but` CLI（2026 リリース）**。出典: <https://blog.gitbutler.com/but-cli> / <https://docs.gitbutler.com/cli/cheat> / <https://docs.gitbutler.com/cli-guides/cli-tutorial/rubbing> / <https://docs.gitbutler.com/ai-agents/overview>。
  - 提供機能: stash なしの並列/スタックブランチ、**operation log による無制限 undo**、履歴編集コマンド群（`reword` / `amend` / `squash` / `absorb` / `move` / `split` / `uncommit`）、GitHub/GitLab PR 連携、**全コマンドの JSON 出力（スクリプトと AI エージェント向け）**。
  - **`but agent setup`**（リリース 0.20.4, 2026-06-26）: エージェント向けに VCS 挙動を調整する対話ウィザード。
  - リリース 0.20.0: 「GitButler は `but` CLI と agent skill 経由でコーディングエージェントから直接使える。複数のエージェントセッションが `but` CLI で並列/スタックで作業し、branch & commit し、履歴を整理できる」。
  - **MCP サーバ**を提供し `gitbutler_update_branches` ツールを露出（自動コミット / save point）。
  - **Rules**: 「ファイル変更 → 自動でこのブランチに割り当て」を宣言的に書ける。サブフォルダ単位で独立ブランチへ自動分割も可。GUI では対象ファイルを選んで **"Split off changes"** で新ブランチへ切り出せる。
  - 出典 <https://blog.gitbutler.com/vcbench>（Agentic Version Control Benchmarks）: **「エージェント（Codex / Claude Code）が選択的コミット・分割・squash・amend をするとき、GitButler は Git や Jujutsu より速くトークンも少ない」**と主張（selective commit 22.4s、multi-amend 51.3s、split commit 42.1s）。数値は自社ベンチなので鵜呑みにはできないが、**「AI 時代の VCS の評価軸は『エージェントが履歴整理に費やすトークン数と時間』である」という問題設定自体が重要**。
- **Kagi への示唆**: ①**「全コマンドが JSON を返す」を Kagi の CLI/IPC 面の設計原則にする**（Kagi は single-instance ソケットを既に持つ）。エージェントが Kagi を「安全な git 実行エンジン」として使えるようになり、`push --force` / `reset --hard` を持たないという Kagi の性質が**エージェントに対するガードレール**として初めて商品価値になる。②`Rules`（ファイル変更 → ブランチ自動割当）は Kagi では **「エージェントが触ったファイル群 → 意味のあるコミット分割の提案」**という plan として出せる。③自社ベンチの評価軸（トークン/時間）は Kagi の AI 機能を売るときの物差しとして借用できる。
- **難易度**: JSON 出力 API = M、コミット分割提案 = M〜L。
- **取り込まない**: virtual branch は既存調査の結論どおり Reject（HEAD 侵襲 + FSL + Kagi の安全パイプラインと思想衝突）。

#### git-branchless — 深掘り

- **何か**: Git の上に「monorepo 規模の高速ワークフロー」を載せるツール群。`git undo` / smartlog / `git move` / `git test` / revset。
- **リポジトリ / 規模 / 言語**: <https://github.com/arxanas/git-branchless> / ★4,122 / Rust / **Apache-2.0**（＝ Kagi にとって最も扱いやすいライセンス）/ 最終 push 2026-09-01 / 2,049 commits。
- **出典**: README <https://github.com/arxanas/git-branchless> / Architecture <https://github.com/arxanas/git-branchless/wiki/Architecture> / `git undo` <https://github.com/arxanas/git-branchless/wiki/Command:-git-undo> / `git test` <https://github.com/arxanas/git-branchless/wiki/Command:-git-test> / Revsets <https://github.com/arxanas/git-branchless/wiki/Reference:-Revsets> / 記事 <https://blog.waleedkhan.name/git-undo/> <https://blog.waleedkhan.name/in-memory-rebases/>

**(a) event log — Kagi oplog にとって最も現実的な参照実装**
Architecture ドキュメントより:
- Git の**フックを仕込んで**リポジトリ内のイベントを監視し、**SQLite に順序付きイベント列（event log）として記録**する。
- 起動時に**全イベントをメモリに読み込み、リプレイして現在の repo 状態を再構成**する（`EventReplayer`）。
- **undo の実装は「直近のイベントを取り出して逆イベントを適用する」**。ここで著者自身が設計上の弱点を認めている: 「逆イベントが意味を持たない場合がある。draft コミット A を main 上の upstream 版に rewrite したとき、その逆は main のコミットを draft に rewrite することになってしまう。**専用の "undo" イベント型を導入した方がよいかもしれない**」。
- 未実装の計画として **checkpoint**: 「イベントログが長いと遅くなるので、repo 状態のコピーを含む合成イベント（checkpoint）を入れ、最新 checkpoint から先だけリプレイすればリプレイ量を上限付けられる」。「数千イベントでは問題が出ていないので優先していない」。
- reflog との比較: 「reflog は単一 ref の過去位置を見る道具。rebase のような複雑な操作を逆算するのは面倒。`git undo` は**リポジトリ全体の状態**というより高い抽象で動く。reflog では原理的に取り消せない操作（一部の branch 生成/更新/削除）もある」。
- v0.4.0 以降、**working copy snapshot** を取るコマンドについては working copy の変更も undo できる。untracked file は原理的に対象外。
- **Kagi への示唆（最重要）**: **Kagi の JSONL oplog は「イベント列 + 逆適用」型で、git-branchless と同じ設計の同じ弱点を持つ可能性が高い**。①「逆イベントが意味を持たない」問題 → **専用 undo イベント型**（＝undo 自体も記録し、逆算しない）を Kagi は先回りして採用すべき。②**checkpoint**（一定間隔で全 ref スナップショットを埋め込み、リプレイ量を有界化）は、oplog が長期運用で重くなる前に入れておくべき安価な保険。③jj/GitButler の「毎回フルスナップショット」と git-branchless の「イベント列」の中間＝**イベント列 + 定期 checkpoint** が、Kagi の JSONL 形式を捨てずに `op restore`（時点復元）を得る最短路。
- **難易度**: undo イベント型 = S〜M、checkpoint = M。

**(b) `git undo -i` の UI**: 既定は直前操作の undo を提案。`-i/--interactive` で**矢印キーでリポジトリの過去の姿を前後に歩き**、Enter で「実行される操作の一覧」を出して y/N で確認する（`Will apply these actions: 1. Hide commit 8d4738cd new message / Confirm? [yN]`）。→ **「時間軸を歩く → 実行計画を見せる → 確認」という Kagi の plan/confirm と同型の 3 段**。wiki には Tower の「Git で困る 13 パターン」に対する対応表まで載っている（#2 削除ファイルの復元、#7 古いリビジョンへの reset、#9 消したコミットの復元、#10 消したブランチの復元 が `git undo` で解決、など）。
- **Kagi への示唆**: **この対応表がそのまま Kagi の「安全機能の価値説明」と受け入れテストのカタログになる**。「Kagi ではこの 13 パターンのうち N 個が oplog から 1 クリックで戻せる」は製品訴求として強い。
- **難易度**: S（表を借りて自機能の充足度を測るだけ）。

**(c) `git test run`** — jj `run` の Git 版で、実装が具体的:
- `--exec '<cmd>'` を revset のコミット集合に対して実行。既定 revset は現在のスタック。`git test run -x '<cmd>' draft()`。
- **strategy が 2 つ**: `working-copy`（作業ツリーで実行、並列不可、ビルド成果物を保てる、追加チェックアウト無し）と `worktree`（branchless 管理の worktree、**並列可**、untracked/ビルド成果物は共有されないが同一 worktree 内では次回に保たれる、ジョブごとに最大 1 チェックアウト増）。
- **`--jobs N`（0 = CPU 数）。N>1 は worktree strategy を含意**。
- **キャッシュのキーが「コマンド + コミットの tree ID」**。だから**コミットメッセージや祖先関係だけが変わったコミットではテストが再実行されない**。`git test clean`、`--no-cache`。
- `git test show -c/-x` で過去結果を再表示、`-v`/`-vv` で出力の詳細度。コマンドは `git config branchless.test.alias.<name>` で名前付き登録できる。
- **Kagi への示唆**: Kagi は worktree 管理と埋め込みターミナルを持つので、**「スタック内の各コミットに対してユーザ定義コマンドを worktree で並列実行し、tree ID キャッシュで結果を再利用し、グラフ行に緑/赤バッジを出す」**が現実的に作れる。AI 時代の効き方が大きい: **エージェントが作ったスタックを push 前に全コミット緑にできる**。`working-copy` / `worktree` の 2 strategy と「tree ID をキャッシュキーにする」は設計をそのまま借用可（Apache-2.0）。
- **難易度**: M〜L。

**(d) revset + segmented changelog DAG + in-memory rebase**:
- revset 関数群は jj より小さく Git 語彙に寄っている: `all()` `none()` `union/intersection/difference` `only(x,y)` `range(x,y)` `ancestors/descendants` `ancestors.nth(x,n)` `parents.nth(x,n)` `children` `roots` `heads` `merges()` `main()` `public()` `draft()` **`stack([x])`（x を含むスタックの draft コミット全部、引数なしで HEAD のスタック）** `branches([pattern])` `message(pattern)` `paths.changed(pattern)` `author.name/email/date(...)`。文法は LALRPOP。
- **segmented changelog DAG** を採用し merge-base を O(log n) で計算（出典: <https://github.com/quark-zju/gitrevset/issues/1>）。sparse index、マルチスレッド、in-memory 操作（working copy を触らないので `git status` を遅くせずビルド成果物も壊さない）。linux（100 万コミット超）と gecko-dev（70 万超）でベンチ済み、と主張。
- **Kagi への示唆**: **`stack()` / `draft()` / `public()` / `paths.changed()` の 4 つは、Kagi の Repository Navigator フィルタに入れると即効性がある小さな語彙**。jj の revset は評価器が jj 内部と密結合（既存調査の結論）だが、**branchless の関数セットは「Git の語彙 + スタック概念」に閉じており、Kagi の git2 上で自前実装しやすい規模**。「in-memory で working copy を触らずに rebase する」は Kagi の in-memory merge 方針の正当性を裏付ける独立事例。
- **難易度**: フィルタ語彙 4 つ = S〜M、完全な revset DSL = L。

#### Graphite (`gt`)

- **何か**: stacked PR の商用サービス + CLI。`gt create` / `gt submit` / `gt restack` / `gt sync` / merge queue / AI Reviews。
- **リポジトリ**: `withgraphite/graphite-cli` は **2026-09-03 時点で GitHub API 404（公開リポジトリとして存在しない）**＝ソース非公開化。公式 docs: <https://graphite.com/docs/command-reference> / <https://graphite.com/docs/cli-changelog> / <https://graphite.com/docs/configure-cli>
- **仕組み（学ぶべきデータモデル）**: 自社ブログ <https://graphite.com/blog/git-key-value> が「native git を key-value store として使う」方法を明かしている。**ブランチのメタデータ（親ブランチ等）を JSON blob にして `.git/refs/branch-metadata/<branch>` という ref に `git update-ref` で書く**。ユーザは `ls .git/refs/branch-metadata` で見え、素の git コマンドで検査・変更・削除できる。その後 CLI changelog によると **メタデータのストレージを git object/ref から SQLite へ移行**し、「stale なメタデータの再検証を、現在のコマンドに関係する変更があったときだけ再計算する」形にして性能を改善した。設定は user レベルが `~/.config/graphite/user_config`、repo レベルが `.git` 配下、debug ログが `~/.local/share/graphite/debug`。
- **Kagi への示唆**: **「ブランチの親子関係（stack 構造）という Git が持たない情報をどこに置くか」の実装例が 2 つ手に入る**: ①**`refs/` 配下の独自 ref に JSON**（＝素の git で検査可・可搬・fetch/push もできる。透明性が高く Kagi の「見せる」思想に合う）②**SQLite キャッシュ**（速いが不透明）。Tower は同じ問題を **git config に置いた**（後述）。Kagi が将来 stacked branch を持つなら、**「正本は git config か独自 ref（透明）、キャッシュは別（速い）」の二層**が答え。Kagi は既に branch cleanup で PR 情報を扱うので、キャッシュ層の置き場所の判断材料になる。
- **難易度**: M（データモデル選択自体は S、stacked branch 機能全体は L）。
- **取り込まない**: merge queue と AI Reviews はサービス側機能でローカル GUI の射程外。

#### stacked branch ツール群（比較表として）

| ツール | リポジトリ / ★ / 言語 / ライセンス | 状態の持ち方 | 学ぶべき点 |
|---|---|---|---|
| `spr` | <https://github.com/spacedentist/spr> / crates.io `spr` 1.3.7 (2025-08-25) / Rust / MIT | **1 ローカルコミット = 1 PR**。コミットを amend/rebase して `spr diff` を再実行 | 「コミットが PR の単位」という最小モデル。`spr diff` 更新時に**更新内容を説明する短文をユーザに聞く**（＝更新の意図を記録させる） |
| `spr` (別実装) | <https://github.com/ejoffe/spr> / ★1,287 / Go / MIT | 同系 | — |
| `ghstack` | <https://github.com/ezyang/ghstack> / ★1,015 / Python / MIT / 最終 push 2026-07-29 | main の上のコミット列 → 各コミットに PR。repo メタを `.git/ghstack-repo-info.json` にキャッシュ | **`automsg = claude` / `codex` 設定が本命**（後述 §2.5） |
| `git-town` | <https://github.com/git-town/git-town> / ★3,371 / Go / MIT | ブランチ親子関係を管理。`hack`/`sync`/`propose`/`append`/`prepend`/`set-parent`/`combine`/`detach`/`swap`/`diff-parent` | **`git town undo`: 「直前に完走した git town コマンドの逆操作を行い、実行前の状態に戻す」**（出典 <https://www.git-town.com/commands/undo>）。**「複合操作を 1 コマンドで戻す」という Kagi oplog と同じ契約**。`combine`（スタック内の隣接 2 ブランチを 1 つに）/`swap`（並べ替え）/`detach`（スタックから外す）という**スタック編集の語彙**が整理されている |
| `av` (Aviator) | <https://github.com/aviator-co/av> / ★511 / Go / MIT / 最終 push 2026-08-31 | `av tree` でスタック可視化、`av sync` で GitHub fetch → restack → push | **`av sync` の出力設計が優秀**: 「✓ GitHub fetch is done / ✓ Restack is done」＋ツリー図＋**「push が不要なブランチ（PR が既に merge 済み）」と「push したブランチ（Remote/Local/PR の各 SHA を並記）」を分けて列挙**。＝**「何をして何をしなかったか」を必ず報告する**。さらに **`av` は公式に coding agent 用プラグイン（<https://github.com/aviator-co/agent-plugins> の `av-cli` skill）を出し、エージェントに「生 git ではなく av を使え」と教える**。`av`: PR/コミットの split・reorder も持つ |
| `git-spice` (`gs`) | <https://github.com/abhinav/git-spice> / ★753 / Go / GPL-3.0 | `gs branch create` / `gs stack submit` / `gs repo sync` / `gs stack restack` | **「push/pull するまで完全オフライン動作、外部依存なし」を売りにしている**。GitHub/GitLab/Bitbucket/Gitea/Forgejo をサポート。**短縮形（`gs bc` / `gs ss` / `gs rs` / `gs sr`）を一級市民として文書化** |
| `git-machete` | <https://github.com/VirtusLab/git-machete> / ★1,137 / Python / MIT | ブランチの木構造を**定義ファイル**に持つ | **`git machete status` が「このリポジトリにどんなブランチがあるか / 何がどこに merge(rebase/push/pull) されるのか」に即答することを設計目標に掲げる**。**`git machete traverse` が木を半自動で辿り、各ブランチで rebase/merge/push を「やるか?」と順に聞く**＝**バッチ操作を 1 ステップずつ確認に分解する UX**。これは Kagi の二段確認の自然な拡張形 |

- **Kagi への示唆（横断）**: ①**`git town undo` / `av sync` の報告 / `git machete traverse` の逐次確認**の 3 つは、いずれも Kagi の「plan → confirm → …→ verify」を**複数ブランチにまたがる複合操作へ拡張する**ときの UX テンプレート。特に `av sync` の「push したもの / しなかったもの + その理由」を必ず出す報告様式は、Kagi の verify 表示の目標水準にすべき。②`av` と GitButler が揃って「**エージェント向けに『生 git を使うな、この CLI を使え』という skill を配布している**」のは重要な潮流。Kagi も同じことをできる立場にある（後述 §3 #4）。
- **難易度**: 複合操作の逐次確認 UX = M、報告様式の強化 = S。

### 2.2 GUI クライアント

> conflict 解決 UX に関しては **既存調査あり**: `docs/research/conflict-ux-gui-clients.md`（GitKraken / Fork / SourceTree / GitHub Desktop の 10 観点比較）。ここでは **conflict 以外**の軸に限定する。

#### Tower（Mac 17.1.1 / 2026-08-11, Windows 13.2 / 2026-08-18）

- **何か**: 商用 Git GUI（10 万ユーザ以上を謳う）。プロプライエタリ。
- **出典**: <https://www.git-tower.com/mac> / リリースノート <https://www.git-tower.com/release-notes> / Tower 17 blog <https://www.git-tower.com/blog/tower-mac-17> / Tower Pro <https://www.git-tower.com/pro>
- **学ぶべき点 1 — Merge Detection（Tower 17 の目玉）**: 「squash / rebase merge では base ブランチのコミットがローカルブランチのものと別物になるので、ブランチが本当に merge されたか分からない」という問題に対し、**4 つの独立チェック**を持つ: ①ホスティングサービス（GitHub/GitLab）の **merged PR とローカルブランチの照合** ②Git 自身の merge 検出 ③merge commit 解析 ④**リモート追跡ブランチが消えていること**の検出。そして「再設計した *Fully Merged* ヒントは、**どのチェックが一致したかを正確に表示し、merge commit または PR への直接リンクを付ける**ので、削除する前に検証できる」。branch cleanup のヒントに **Archive アクション**が加わった。
- **学ぶべき点 2 — Undo Anything (⌘+Z)**: トップページで「Undo everything (⌘+Z)」を第一の売りに置いている。
- **学ぶべき点 3 — stacked branches の状態の持ち方**: 「17.1 で **stacked フラグと親ブランチ関係を git config に保存する**ようになった」。＝Graphite（独自 ref → SQLite）とは別解。Pro 機能として Graphite 連携、Stacked Branches、Advanced Custom Workflows、Automatic Branch Management、PR merged branch detection、self-hosted service account を提供。
- **学ぶべき点 4 — AI Commits**: 「組み込みまたはカスタムのプロンプトプリセットでコミットメッセージを生成」（出典: <https://tower.macupdate.com/>）。**プロンプトをプリセットとしてユーザに開放**している点が肝。
- **その他の語彙**: Conflict Wizard、single-line staging、Quick actions（コマンドパレット）、Quick open、Worktrees、Workflows、Automatic stashing & fetching、Drag and drop。
- **Kagi への示唆**: **Kagi は既に squash merge 検出（ghost connector, ADR-0139）と PR 経由の merged 判定（`list_merged_prs`, ADR-0138）を持つ**ので、Merge Detection 自体は新機能ではない。取り込むべきは **「どのチェックが根拠になったかを UI に明示し、merge commit / PR への検証リンクを添える」という提示方法**——Kagi の「破壊的操作の前に必ず根拠を見せる」思想の直系。branch cleanup の **Archive（消す代わりに退避）** も安全側の選択肢として価値がある。「AI Commit のプロンプトをプリセットとして開放」は Kagi の smart commit（ADR-0090/0099）に直接足せる。stacked branch の状態を **git config** に置く選択は Kagi にとって最も透明で安全（素の git で検査でき、Kagi が消えても壊れない）。
- **難易度**: 根拠明示 UI = S〜M、Archive = S、プロンプトプリセット = S。

#### Sublime Merge

- **何か**: Sublime Text 開発元の商用 Git GUI。Mac/Win/Linux。
- **出典**: <https://www.sublimemerge.com/>
- **仕組み / 売り**: 「クロスプラットフォームの軽快な GUI toolkit、比類なき syntax highlighting エンジン、**自前の高性能 Git 読み取りライブラリ**」で性能を出す。ファイル/hunk/行の staging（**1 行以上を選んで hunk を複数の変更に分割**できる）。side-by-side diff + syntax highlight + **character diff**。**hunk の上端/下端をドラッグして表示コンテキスト行数を対話的に増やせる**。リポジトリ全体に対する**打ちながらの即時検索**（コミットメッセージ / 著者 / パス / **内容**）。
- **決定的な学び — "Real Git"**: 「Sublime Merge を使っているとき、あなたは Git を使っている。**使っている Git コマンドを正確に表示し**、コマンドラインと Sublime Merge をシームレスに行き来できる」。
- その他: Command Palette、Commit Editing、Blame and File History、Submodule Management、Command Line Integration、Git Flow、テーマ機構。
- **Kagi への示唆**: ①**「実行する/した git コマンドを正確に見せる」を Kagi の plan/preflight/verify に組み込む**——Kagi は git2 直叩きなので「等価な git コマンド」を表示する形になるが、**安全パイプラインの各段で「これから何が起きるか」を git 語彙で見せるのは Kagi の存在理由と完全一致**し、かつ AI エージェントに渡す監査ログにもなる。②**hunk 境界をドラッグしてコンテキスト行を伸縮**は、Kagi の split diff view に足せる小さくて効く UX。③**内容までスコープに入れた即時検索**は Kagi の Analyze 機能群と自然に繋がる。
- **難易度**: git コマンド表示 = M（git2 操作 → 等価コマンド文の生成が必要）、hunk コンテキスト伸縮 = S〜M。

#### GitKraken Desktop

- **conflict UX は既存調査あり**（`conflict-ux-gui-clients.md` §1）。
- **conflict 以外の差分**: Team View が「共有ファイルに『衝突しそう』アイコンを事前表示」する早期検知機能を持つ（既存調査に記載あり）。GitLens（<https://github.com/gitkraken/vscode-gitlens> / ★9,915 / TypeScript）を同社が持ち、**VS Code 側で inline blame / commit graph / worktree 管理を提供**している。
- **Kagi への示唆**: 「他人が同じファイルを触っている」という**事前の衝突予告**は、Kagi の in-memory merge による conflict 予測と組み合わせると強い（Kagi はローカルで予測、GitKraken はチームで予告）。ただしサーバ側インフラを要するので Kagi では **「fetch 済みのリモートブランチとの衝突予測」**に縮退させるのが現実的。
- **難易度**: M。

#### Fork / SourceTree

- **既存調査あり**（`conflict-ux-gui-clients.md` §2, §3）。Fork の「conflict ラベルを ours/theirs でなく実ブランチ名にする」（1.0.73）は既存調査で Kagi が採るべき設計として採用済み。ここで追加すべき差分は無い。

#### SourceGit（OSS GUI として最重要の比較対象）

- **何か**: 無料・OSS のクロスプラットフォーム Git GUI。
- **リポジトリ / 規模 / 言語**: <https://github.com/sourcegit-scm/sourcegit> / **★5,861** / **C#（Avalonia）** / MIT / 最終 push 2026-09-02。
- **仕組み / 機能**: git CLI（>= 2.25.1）必須。ビジュアルコミットグラフ、リモートごとの SSH、Merge/Rebase/**Reset/Revert**/Cherry-pick、Amend/Reword/Squash、interactive rebase、worktrees、archive、**save as patch / apply**、file history、blame、**Revision Diffs / Branch Diff**、**Image Diff（Side-By-Side / Swipe / Blend）**、**Git command logs**、commit 検索、GitFlow、Git LFS、bisect、**Issue Link**、**Workspace**、**Custom Action**、GitHub/GitLab/Gitea/Gitee/Bitbucket への PR 作成、**AI によるコミットメッセージ生成**、**組み込みの conventional commit ヘルパー**、外部テーマ（<https://github.com/sourcegit-scm/sourcegit-theme>）、**13 言語 UI（日本語含む）**。
- **Kagi への示唆**: ①**Image Diff の 3 モード（Side-By-Side / Swipe / Blend）** は Kagi の画像レンダリングに足せる具体案で、実装コストが小さく体験差が大きい。②**"Git command logs"** は Sublime Merge の "Real Git" と同じ結論に独立到達。③**Conventional Commit ヘルパー**は Kagi の smart commit と組み合わせると「AI が書いた本文 + 規約に沿った type/scope」の形で有用。④**Custom Action**（ユーザ定義アクション）は lazygit の custom commands と同型で、Kagi の埋め込みターミナル資産があれば安全に提供できる。⑤テーマを別リポジトリでコミュニティ配布する運用は Kagi のテーマ機構の将来形。
- **難易度**: Image Diff モード = S〜M、git command log = M、conventional commit ヘルパー = S、custom action = M。
- **取り込まない**: Reset(--hard 相当)/force push 等。Kagi の存在理由に反する。

#### Gitnuro

- **何か**: FOSS のマルチプラットフォーム Git クライアント。「newbies and pros のための」。
- **リポジトリ / 規模 / 言語**: <https://github.com/JetpackDuba/Gitnuro> / ★2,747 / **Kotlin（Jetbrains Compose + JGit）** / GPL-3.0 / 最終 push 2026-08-30。
- **仕組み**: 「**web 技術に頼らず**、使い方に制約を課さない OSS のマルチプラットフォーム Git クライアントを提供する」ことを目標に掲げる。機能: hunk の stage/unstage、**特定行の stage/unstage**、side-by-side diff、interactive rebase、blame、file history、**画像の変更を左右比較**、submodule、テーマ、force push（あり）。未実装として「stash を log tree に表示」「diff の syntax highlight」を挙げている。
- **Kagi への示唆**: **Kagi と最も近い立場（ネイティブ GUI・OSS・非 web 技術）の競合**であり、Kagi は「stash を commit graph に表示（ADR-0088）」「diff の syntax highlight」を**既に持っている**＝Gitnuro の未実装項目が Kagi の実装済み項目という関係。差別化は明確で、Kagi の優位（安全パイプライン、oplog、force 系の不在、conflict editor、Analyze、worktree、埋め込みエディタ/ターミナル）を維持すればよい。**ライセンスが GPL-3.0 なのでコード参照は不可**。
- **難易度**: —（新規取り込み項目なし）。

#### GitFiend

- **何か**: 「人間のために設計された」商用（無料 DL）Git クライアント。Win/Mac/Linux。
- **出典**: <https://gitfiend.com/> / <https://gitfiend.com/releases> / <https://github.com/GitFiend/rust-server>
- **仕組み**: **コアを Rust で書き直し**、「**repo データの照会と git コマンド実行を行う内部サーバプロセス（Rust 製）**」を持つ Electron アプリ（`GitFiend/gitfiend-core` = 内部サーバのリポジトリ）。機能: instant refresh / fetch / push / pull / stage / commit、stash、branch、tag、filter、ファイル単位の履歴、**Git Reset（コミット右クリック → 「何が起きるか説明するダイアログを出し、reset モードを選ばせる」）**、選択ファイルの変更破棄、**未 push コミットを 1 つに squash するプレビュー**、`ctrl/cmd-f` からの横断検索（コード / ファイル / コミットメッセージ / ブランチ / ユーザ名）。
- **Kagi への示唆**: **「Reset のダイアログで『何が起きるか』を説明してからモードを選ばせる」は、Kagi の plan/confirm を破壊的操作に適用した最小実装例**（Kagi は `reset --hard` を持たないが、soft/mixed 相当を将来入れるならこの提示形が正解）。「未 push コミットの squash **プレビュー**」も Kagi の plan と同型。UI プロセスと「git 実行サーバプロセス」を分離するアーキテクチャは、Kagi の repo worker thread（ADR-0073）と同じ動機。
- **難易度**: S（提示方法の借用）。

#### Aurora Editor

- <https://github.com/AuroraEditor/AuroraEditor> / ★1,335 / Swift / MIT / **archived（最終 push 2025-08-26）**。開発停止しており参照価値は低い。Swift ネイティブ IDE + git 統合という方向性のみ記録。
- **難易度**: —。

#### jj / Sapling 用 GUI・TUI（Kagi にとって最も直接的な参照対象）

- **GG (Gui for JJ)**: <https://github.com/gulbanana/gg> / ★857 / **Rust（Tauri）** / **Apache-2.0** / 最終 push 2026-05-12。README のコンセプト文が核心: 「**Jujutsu のコンポーザブルなプリミティブを活かして、リポジトリの対話的なビューを提示する。大きなアイデア: もし常に interactive rebase の途中にいて、しかもそれが良いことだったら?**」。左ペインでログを query/browse、クリックで revision 選択、shift+click で範囲。`jj` バイナリ不要（jj-lib を直接リンク）。`revset-aliases.immutable_heads()` を「編集できる履歴の範囲を決めるので特に重要」と扱う。
- **jjui**: <https://github.com/idursun/jjui> / ★2,136 / Go / MIT / 最終 push 2026-09-02（jj v0.37+ 必須）。学ぶべき UI 語彙が濃い:
  - **revset をオートコンプリート + シグネチャヘルプ付きで編集できる**（＝クエリ言語を GUI で「書ける」ようにする最良例）
  - グラフ上で rebase / squash（`S` で squash し次の revision を自動選択、`j`/`k` で選択変更）
  - revision 詳細ビュー（`l`）: 選択ファイルの restore（`r`、ダイアログで `i` を押すと**対話的 chunk restore**）、split（`s`）、diff（`d`）
  - **op log ビュー（`o`）と、選択操作の restore（`r`）**
  - preview ペイン（`p`）: 選択が revision なら `jj show`、ファイルなら `jj diff`、操作なら `jj op show` の出力を出す。`ctrl+n/p/d/u` でスクロール
  - `u` = undo、`U` = redo、**`v` = evolog（change の変遷）**、`A` = absorb、`f` = **ace jump（画面上の revision へ 2 打鍵でジャンプ）**
- **Sapling ISL** (§2.1 参照)。
- **Kagi への示唆**: **Kagi が「AI native な git GUI」を目指すとき、機能の見本市として最も近いのは GUI/TUI の jj クライアント**。具体的に借用価値が高いのは ①**revset/クエリ入力欄のオートコンプリート + シグネチャヘルプ**（Kagi の Navigator フィルタを DSL 化するなら必須）②**op log を一級のビューにし、行を選んで restore できる**（Kagi の oplog は既にあるので**ビュー化 + 時点復元**を足すだけ）③**選択対象の種類に応じて preview ペインの内容が切り替わる**単一プレビュー設計 ④**evolog（change の変遷）専用ビュー** ⑤**ace jump**（大きなグラフでのキーボードナビゲーション）。GG は Apache-2.0 なので**設計参照が法的に最も安全**。
- **難易度**: op log ビュー = M、preview ペイン統一 = M、ace jump = S、revset 補完 = M（DSL 実装は別途 L）。

#### エディタ内蔵の git UX（Zed / VS Code / JetBrains）

- **Zed**: <https://github.com/zed-industries/zed> / ★89,648 / Rust / 出典 <https://zed.dev/docs/git.html>。**既存調査あり**（`docs/research/zed-gpui-reuse-research.md` は gpui 流用観点）。ここでは git UX の差分:
  - **Git Panel**: 作業ツリーとステージング領域の状態。どのリポジトリ/ブランチがアクティブか、変更ファイルと各ファイルの staging 状態を一望。**コマンドラインでの変更が即座に反映**される（監視）。
  - **リポジトリのアクティベーションが遅延式**: プロジェクトが git repo（またはそのサブディレクトリ）に根付いていれば全 repo が即座にアクティブ。そうでなければ（ホームディレクトリやプロジェクト集合フォルダ）**プロジェクト直下の repo は即アクティブ、深いものはファイルを開いたときにアクティブ化**。「非アクティブな repo は完全にインデックスされ検索可能で、git 機能（status/diff/branch）だけがアクティベーションを待つ」。`file_scan_depth` に連動（0 にすると遅延なし、マルチフォルダでは各ルートから別々に測る）。
  - パネルは flat list 既定 / **Tree View 切替**、dock 位置変更可。**Inline Blame**（現在行の blame をインラインに、遅延設定可）、gutter インジケータ、hunk スタイル設定。
  - 「Zed がネイティブ対応していない操作は統合ターミナルを使え」と**明示的に線を引いている**。
  - **Kagi への示唆**: ①**「repo を遅延アクティベートするが、インデックス/検索は先に済ませる」**という段階的初期化は、Kagi が repo タブで多数リポジトリを開くときの性能設計として直接使える（Kagi は既に branch cleanup / ghost connector を背景スレッドへ逃がす対応をしている＝同じ問題に直面済み）。②「対応しない操作はターミナルへ」という線引きの明示は、force 系を持たない Kagi が**堂々と欠落を説明する**やり方の見本。③Tree View / flat list の切替と inline blame は Kagi の diff/エディタ面に足せる。
  - **難易度**: 遅延アクティベーション = M、inline blame = M、tree/flat 切替 = S。
- **VS Code / GitLens**: VS Code は 3-way merge editor を内蔵（conflict UX は **既存調査あり**: `docs/research/conflict-ux-editors.md`）。GitLens（★9,915 / TypeScript / <https://github.com/gitkraken/vscode-gitlens>）が inline blame・commit graph・worktree・"Visualize code authorship at a glance" を提供。**Kagi の Analyze 機能（hotspots / coupling / ownership / file history）は GitLens の "untapped knowledge within each repository" と同じ問題設定**であり、Kagi は既にこの領域を持っている。差分として学ぶ点は少ない。
- **JetBrains**: 部分コミット（changelist）と Local History が独自資産。**[推測]** ここは一次資料を当たれておらず、Kagi への具体的示唆を確定できていない（§5 参照）。

### 2.3 TUI / CLI

#### lazygit

- <https://github.com/jesseduffield/lazygit> / **★81,910** / Go / MIT / 最終 push 2026-09-02。この分野の圧倒的トップ。
- **出典**: <https://github.com/jesseduffield/lazygit>（README の Features 節）
- **学ぶべき仕組み**:
  - **行単位 staging**: 選択行で space、`v` で範囲選択開始、`a` で現在の hunk 全体を選択。
  - **interactive rebase を「TODO ファイル編集」から解放**: `i` で開始し、TODO コミットに対し squash(`s`)/fixup(`f`)/drop(`d`)/edit(`e`)/上へ(`ctrl+k`)/下へ(`ctrl+j`)、`m` で rebase オプションメニュー → continue。**さらに「rebase を明示的に開始せずに、単発の操作としても同じキーが効く」**（コミット上で `s` を押せばその場で squash）。範囲選択（shift+down）で複数コミットの移動/fixup も可。
  - **`shift+a` で任意の古いコミットに staged 変更を amend**（裏で interactive rebase を走らせる）。
  - **Rebase magic（custom patches）**: 古いコミットの中から**行単位で「カスタムパッチ」を組み立て**、そのパッチを ①元コミットから削除 ②新コミットとして切り出し ③index に逆適用 …等ができる。README の例は「古いコミットに残った冗長なコメントを消す」。
  - **cherry-pick が copy/paste メタファ**: コミット上で `shift+c`（copy）→ `shift+v`（paste = cherry-pick）。
  - `b` で bisect（good/bad をマークして開始）、`/` で任意ビューのフィルタ、`w` でブランチから worktree を作って切り替え、**custom command system**（キーバインドにユーザ定義コマンドを割り当て、組み込みアクションを自作で再現できるほど柔軟）。
  - **`shift+d` → reset オプションメニュー → "nuke"**: 「`git status` に出るもの全部（dirty submodule 含む）を消す」。
- **Kagi への示唆**: ①**「interactive rebase の各操作を、rebase を開始せずに単発操作として提供する」**は Kagi にとって極めて重要。Kagi の安全パイプラインは「1 操作 = 1 plan/confirm/execute/verify/oplog」なので、**「コミットを選んで squash」「古いコミットに amend」を個別操作として提供する方がむしろ自然**で、interactive rebase の TODO 編集 UI を作るより Kagi の思想に合う。②**cherry-pick を copy/paste メタファで見せる**のは人間に優しいラベリングの好例（Fork のブランチ名ラベルと同系）。③**custom patch（古いコミットから行単位でパッチを抜き出して移動する）**は「AI が作った巨大コミットを後から整理する」という AI 時代の需要に直結し、Kagi の in-memory merge 基盤の上で安全に作れる。④**"nuke" は Kagi が絶対に持たない機能**（`git clean` 相当）——ここは明確に決別点。
- **難易度**: 単発 squash/amend 操作 = M（Kagi は pushed amend を持つので基盤あり）、copy/paste メタファ = S、custom patch = L。

#### gitui

- <https://github.com/gitui-org/gitui> / ★22,458 / Rust / MIT / crates.io `gitui` **0.28.1**（2026-03-24）。
- **出典**: <https://github.com/gitui-org/gitui>（README）
- **学ぶべき仕組み**:
  - 動機が Kagi と同じ: 「**人気の git GUI はどれも巨大リポジトリで破綻するか、応答しなくなって使えなくなる**。GUI の体験と快適さをターミナルで、しかも可搬・高速・無料・OSS で」。
  - **ベンチマークを README に載せている**（Linux の git repo, 90 万コミット超のパース）: gitui 24s / 0.17GB / freeze なし / crash なし、lazygit 57s / 2.6GB / freeze あり / 時々 crash、tig 4m20s / 1.3GB。
  - **context based help（「大量のホットキーを暗記する必要がない」）**、**キーボードのみでの操作**、hook 対応（pre-commit / commit-msg / post-commit / prepare-commit-msg）、ファイル/hunk/**行**単位の stage/unstage/revert/reset、stash（save/pop/apply/drop/inspect）、コミットログの **検索**、submodule、**async git API**。
  - 「unsafe forbidden」バッジ。
- **Kagi への示唆**: ①**「巨大リポジトリでの応答性を数字で示す」**——Kagi も同じ主張（安全性 + 応答性）をするなら、**同種のベンチ表（時間 / メモリ / freeze / crash）を計測して公開する**のが最も説得力がある。gitui が使っている題材（linux repo のフルパース）はそのまま流用できる。②**context based help（現在の文脈で使えるキーだけを出す）** は Kagi のキーボード操作性に効く。③**git hook の明示的サポート（pre-commit / commit-msg / prepare-commit-msg）** は Kagi の commit 経路で対応状況を明記すべき項目（`prepare-commit-msg` は AI コミットメッセージと衝突しうるので特に）。
- **難易度**: ベンチ計測 = S、context help = M、hook 対応の明示 = S〜M。

#### tig / gitu / serie

- **tig**: <https://github.com/jonas/tig> / ★13,318 / **C** / GPL-2.0 / 最終 push 2026-07-27。「Text-mode interface for git」。古参。gitui のベンチでは 90 万コミットのパースに 4m20s / 1.3GB（バイナリは 0.6MB で最小）。**Kagi への示唆**: 参照価値は「ページャとしての最小構成」のみ。GPL-2.0 でコード参照不可。
- **gitu**: <https://github.com/altsem/gitu> / ★2,906 / Rust / MIT / crates.io `gitu` **0.43.0**。「Emacs の外の Git porcelain」＝**Magit クローン**。「Magit の core 機能を段階的に実装する」「以前の Magit ユーザに馴染むこと」を目標に掲げ、キーバインドは **Magit を模倣しつつ Vim ライク**。実装済み: staging/unstaging（file/hunk/**line**）、show、branching、committing（commit/amend/**fixup**）、fetch、log、pull/push（設定済み upstream/pushDefault へ）、rebase（elsewhere/abort/continue/**autosquash**/interactive）、reset（soft/mixed/hard）、revert、stash。`h` でヘルプメニュー、`general.always_show_help.enabled = true` で常時表示可。
  - **Kagi への示唆**: **Magit の操作語彙（transient menu = 「今押せるキーとその意味を常に画面に出す」）は、Kagi のキーボード駆動を設計するときの最良の参照**。`always_show_help` を設定で常時 ON にできるのは初学者と熟練者を同一 UI で両立させる巧い手。`fixup` / `autosquash` を一級の操作語彙に持つのは absorb 系機能への入口。
  - **難易度**: transient menu 相当 = M。
- **serie**: <https://github.com/lusingander/serie> / ★2,073 / Rust / MIT / crates.io `serie` **0.8.2**（2026-08-20）。
  - **仕組みが独特**: **ターミナルエミュレータの画像表示プロトコル（iTerm / Kitty / kitty-unicode）を使ってコミットグラフを画像としてレンダリングする**。オプション: `-n` 最大コミット数、`-p` 画像プロトコル、`-o` **コミット順序アルゴリズム（chrono / topo）**、`-g` **グラフ画像のセル幅（auto / double / single）**、`-s` **エッジのスタイル（rounded / angular）**。
  - Goals/Non-Goals を明記: 「リッチな `git log --graph` 体験をターミナルで」「**コミットグラフ中心の**リポジトリブラウジング」/ 非目標「フル機能の Git クライアント」「複雑な UI」「あらゆる端末で動くこと」。
  - 動機: 「`git log --graph` の出力はオプションを足しても読みづらい。ログを見るためだけに複雑なツールを学ぶのは面倒」。
  - **Kagi への示唆**: **「コミットグラフ中心」という Kagi と同一のポジショニングを宣言しているツール**。借用価値が高いのは ①**グラフのエッジスタイル（rounded / angular）とセル幅（double / single）をユーザ設定にする**——Kagi のグラフは既に canvas 描画なので設定として出すのは安価で、見た目の好みという最も分かれる部分をユーザに委ねられる。②**コミット順序を chrono / topo で切替可能にする**（Kagi のレーン安定化と組み合わせると挙動が大きく変わるので、明示的な設定にする価値が高い）。③Non-Goals を書いて「やらないこと」を製品として宣言する姿勢。
  - **難易度**: エッジスタイル/セル幅設定 = S、chrono/topo 切替 = S〜M。

#### diff 系（delta / difftastic / diffnav / mergiraf / git-absorb）

- **delta**: <https://github.com/dandavison/delta> / ★32,065 / Rust / MIT / crates.io `git-delta` **0.19.2**。「git / diff / grep / `rg --json` / **blame** 出力のための syntax-highlighting ページャ」。**Kagi への示唆**: **blame 出力にも syntax highlight を当てるという発想**は Kagi の blame/file history 面に足せる。`rg --json` を入力として受けられる設計（＝diff レンダラを汎用の「ハイライト付き行レンダラ」として切り出す）は、Kagi の diff レンダリング層を検索結果表示にも使い回す設計指針になる。難易度 S〜M。
- **difftastic**: <https://github.com/Wilfred/difftastic> / ★25,851 / Rust / MIT / crates.io **0.70.0**（2026-08-07）。「構文を理解する構造 diff」。**Kagi への示唆**: **「行 diff ではなく構文木 diff」を diff view の第 3 のモード（inline / split / structural）として持てると、AI が生成した大規模リファクタ diff（インデント変更や関数移動が混ざる）のレビュー体験が劇的に変わる**。これは AI native 化の中で最も効く diff 側の一手。MIT なので参照も可。難易度 L（tree-sitter 統合 + 構造 diff アルゴリズム）。ただし GitComet が tree-sitter を既に diff highlight に使っている（既存調査あり）ので、Kagi も tree-sitter を入れるなら構造 diff への道が開く。
- **diffnav**: <https://github.com/dlvhdr/diffnav> / ★1,548 / Go / MIT / 最終 push 2026-09-01。「**delta ベースの git diff ページャだが、GitHub 風のファイルツリーが付く**」。**Kagi への示唆**: 「diff ビューアに**ファイルツリー**を添える」という一点だけのツールが 1.5k★ を集めている＝**需要の実証**。Kagi の diff 画面のファイル一覧を（flat list でなく）ツリー表示にする案の裏付け（Zed の Tree View 切替と同じ結論）。難易度 S〜M。
- **mergiraf**: <https://codeberg.org/mergiraf/mergiraf>（GitHub ミラー: <https://github.com/qundao/mirror-mergiraf>）/ **GPLv3** / crates.io `mergiraf` **0.19.1**（2026-09-01）。出典: <https://mergiraf.org/> / LWN 解説 <https://lwn.net/Articles/1042355/>。
  - **仕組み**: **tree-sitter** で各言語を汎用構文木に変換（葉 = トークン、内部ノード = 言語構文）し、**言語非依存の木マッチングアルゴリズム**で conflict 解決を導く（言語固有の知識は薄く上に載せるだけ）。新しい言語は**完全に宣言的に**追加できる。`git merge` の merge driver として差し込めて `merge` / `revert` / `rebase` / `cherry-pick` を強化するほか、conflict 発生後に手動起動も可。
  - **設計原則が Kagi と一致**: 「**conflict を絨毯の下に隠さない**——構文認識のマージヒューリスティクスは楽観的すぎることがあるので、Mergiraf は慎重側に倒し、疑わしいケースでは conflict marker をファイルに残す」。「**全部自力で解決できた場合は `mergiraf review` コマンドでその仲裁作業をレビューするよう促す**」。「行ベースのマージで conflict が出なければ、その結果をそのまま返す（速いので）。**例外は行ベースマージが重複キーを作ってしまう場合**」。「対話利用に耐える速さ」。
  - **Kagi への示唆**: **GitComet の autosolve（既存調査あり）が「identical / single-side / whitespace / subchunk / regex」のテキスト的ヒューリスティクスなのに対し、mergiraf は構文木レベル**。Kagi が conflict 自動解決に踏み込むなら、取り込むべきは**機能ではなく 3 つの原則**: ①**楽観的に解けたと判断しない（疑わしければ marker を残す）** ②**自動解決したら必ず「レビューしてください」と促す**（Kagi なら conflict editor に「自動解決した箇所」をハイライトして開く）③**まず行ベースで試し、conflict が無ければそれを返す**（速い道を先に通す）。ライセンスが GPLv3 なのでコード転写は不可、概念のみ。難易度 M（原則の実装）/ L（構文木マージ本体）。
- **git-absorb**: <https://github.com/tummychow/git-absorb> / ★5,712 / Rust / **BSD-3-Clause**（＝ Kagi にとって最も緩いライセンスの absorb 実装）/ crates.io **0.9.0**。
  - **出典**: <https://github.com/tummychow/git-absorb>（README）
  - **仕組み**: `hg absorb`（Facebook）の Git 移植。「`git absorb` が**どのコミットが安全に変更できるか**と、**staged 変更のどれがそれらのコミットに属するか**を自動判定し、各変更に対する `fixup!` コミットを書く」。`--and-rebase` で fixup を自動統合、付けなければ **fixup コミットを残すので `git log` で人間が検証してから `git rebase -i --autosquash` で畳める**。「conflict なしに適用できない変更は未コミットのまま残る」。
  - **Kagi への示唆**: **jj/Sapling の absorb が「1 コマンドで完結」なのに対し、git-absorb は「fixup コミットを一旦作り、人間が検証してから畳む」二段構成**。これは **Kagi の plan → confirm → execute にそのまま写る最良の形**（plan で「hunk → 宛先コミット」表を出し、confirm 後に execute、verify で結果を見せる）。しかも実装は git2（統的リンクの libgit2 を使うと README にある）で BSD-3-Clause＝**Kagi の git2 単一 backend 方針と完全に整合し、参照も法的に容易**。**§3 で最優先候補に置く根拠がここ。**
  - **難易度**: M。
- **diff-so-fancy**: <https://github.com/so-fancy/diff-so-fancy> / ★18,086 / Perl / MIT。「diff を人間が読める形に」。Kagi は既に split view + syntax highlight を持つので追加の学びは薄い。難易度 —。

#### git-machete（TUI 面）

§2.1 の表参照。`git machete status` / `traverse` の逐次確認 UX が本体。

### 2.4 Sapling ISL / ReviewStack

§2.1 (d)(e) 参照。

### 2.5 AI 統合クライアント / AI エージェントと統合された git UI

#### AI コミットメッセージ生成ツール

| ツール | リポジトリ / ★ / 言語 / ライセンス | 仕組み | Kagi への示唆 |
|---|---|---|---|
| aicommits | <https://github.com/Nutlope/aicommits> / ★9,095 / TypeScript / MIT / push 2026-09-01 | staged diff を LLM に渡してメッセージ生成する CLI | この分野の需要の大きさの証明（9k★）。**Kagi は smart commit（ADR-0090/0099）で既に実装済み**なので機能面の学びは無い |
| opencommit | <https://github.com/di-sukharev/opencommit> / ★7,529 / JavaScript / MIT | 「最も機能豊富な GPT ラッパー。Claude / GPT 等で 1 秒でコミットメッセージ生成」 | 同上。**複数プロバイダ対応**という点は Kagi の CLI provider 方式（ADR-0099）と同じ結論 |
| GitComet | **既存調査あり** (`docs/research/gitcomet-comparison.md`) / ★795 / Rust / AGPL-3.0 | Claude Code / Codex / GitHub CLI 連携は Pro 機能 | 既存調査どおり。AGPL でコード転写不可 |
| Tower AI Commits | プロプライエタリ | 「組み込みまたは**カスタムのプロンプトプリセット**で生成」 | **プロンプトプリセットをユーザに開放する**発想を Kagi の smart commit に足す（難易度 S） |
| SourceGit AI | ★5,861 / C# / MIT | AI コミットメッセージ + **conventional commit ヘルパー**を併置 | 「AI が書いた本文 × 規約に沿った type/scope」の組み合わせ（難易度 S） |
| **ghstack `automsg`** | ★1,015 / Python / MIT | **本命**（下記） | 下記 |

**`ghstack automsg` — AI に「差分の差分」を渡すという発想**（出典: <https://github.com/ezyang/ghstack> README）:
`ghstack config automsg claude` / `automsg codex`（`--model gpt-5.4` 等でモデル指定可）を設定すると、ghstack は **「PR コンテキスト（ローカルのコミットメッセージと現在の PR description）を一時ファイルに書き、エージェントを『この更新の具体的な内容を要約せよ』というプロンプトで起動する。パッチの更新分はプロンプトに直接含める。**既存 PR については、パッチの更新分は PR 全体のパッチではなく、**以前に提出した PR のパッチに対する interdiff**である**」。明示的な `ghstack -m MESSAGE` は常に優先される。
- **Kagi への示唆（この節で最重要の 1 つ）**: **「AI に渡すコンテキストは『変更の全体』ではなく『前回提出分からの差分の差分（interdiff）』であるべき」**。Kagi は **pushed amend** と **PR review conversation** を既に持つので、**「amend 後の PR 更新コメントを、前回 push した版との interdiff から生成する」**が自然に作れる。生 diff を全部投げる素朴な AI 連携より、トークンも精度も明確に優れる。jj の `jj interdiff` が同じ計算をコアコマンドとして持っている（§2.1 (f)）ことも裏付け。**加えて「明示指定が常に AI を上書きする」という優先順位の設計は Kagi の AI 機能全体の原則にすべき。**
- **難易度**: M（interdiff 計算 S〜M + プロンプト経路）。

#### AI エージェントと統合された git UI

**Cursor（3.x, 2026）**
- 出典: <https://cursor.com/help/integrations/git> / <https://cursor.com/blog/agent-best-practices> / 補助的に各種レビュー記事。
- **仕組み**: ①標準の Source Control パネルの上に AI 機能を載せる。**staged 変更に対してコミット入力欄の sparkle アイコンを押すと、diff と repo 履歴からメッセージを生成**する。②**git worktree を自動生成・管理して並列エージェントを走らせる**。「各エージェントは自分の worktree で、隔離されたファイルと変更を持つので、互いを踏まずに編集・ビルド・テストできる」。エージェントの場所として **Worktree** を選ぶと、現在のブランチから新ブランチを作り、一時ディレクトリに worktree を作り、そこでエージェントを走らせ、終わったら**フル diff を見せて「Apply / Reject」の 2 ボタンを出す**。③3.0 の **Agents Window** は「多数のエージェントをローカル / worktree / クラウド / リモート SSH で並列に走らせる」ための専用サーフェス。**Agent Tabs**（複数チャットを並置/グリッド）。並列数は 8 まで。
- **既知の弱点**: 「エージェントがファイル内の最初の変更しか diff 表示せず、残りはハイライトなしで適用される」というバグ報告（<https://forum.cursor.com/t/not-all-agent-edits-are-shown-in-diff-view/152413>）。ベストプラクティス記事には「**各エージェント操作の後に `git diff` で検証せよ**」「`.cursorrules` で各ステップの `git diff` 検証を必須化せよ」と書かれている。
- **Kagi への示唆（最重要）**: **「エージェントの変更を人間が diff で裁定する」という一点が、AI 時代の git GUI の主戦場**。そして Cursor 側の弱点は**まさに diff 表示の信頼性**であり、「エージェントの操作を毎回 `git diff` で検証せよ」という運用回避策が推奨されている状況＝**信頼できる diff 提示と検証は Kagi の得意領域そのもの**。Kagi は worktree 管理・repo タブ・in-memory merge・oplog・conflict 予測を全部持っているので、**「並列エージェント worktree の裁定盤」**（worktree ごとのタブ、各 worktree の diff を並べて比較、Apply/Reject を plan→confirm→execute→verify→oplog に通す）は**新規機能ではなく既存資産の再配置**で到達できる。
- **難易度**: M〜L。

**Conductor**
- 出典: <https://conductor.build/>。「クラウドでコーディングエージェントのチームを走らせる」。**Conductor Cloud は隔離された microVM 上で動く**。ワークスペースリンクを共有してマルチプレイヤー（誰がアクティブか見える、追いたい作業をフォローできる、一緒にエージェントにプロンプトを出せる）。
- **Kagi への示唆**: 「エージェントのワークスペースを共有 URL で人間が覗く」というモデル。Kagi はローカル GUI なので**クラウド化は射程外**だが、「**ワークスペース（= worktree + ブランチ + セッション）を第一級の共有単位にする**」という概念は Kagi の workspace mode（ADR-0137）と repo タブの将来設計に効く。難易度 —（概念のみ）。

**Crystal（→ Nimbalyst）**
- <https://github.com/stravu/crystal> / ★3,113 / TypeScript / MIT / 最終 push 2026-02-26（改名）。「**複数の Codex / Claude Code セッションを並列 git worktree で走らせる。テストし、アプローチを比較する**」。
- **Kagi への示唆**: Cursor と同じ「worktree 並列 + アプローチ比較」パターンを、**専用ツールとして 3k★ 集めている**＝需要の独立した実証。「同じタスクを複数エージェントに解かせて**比較して勝者を選ぶ**」ワークフローには **「N 個の worktree の diff を横並びで比較する UI」** が必要で、これは既存 git GUI に一つも無い。**Kagi の split view と repo タブの延長で作れる空白地帯**。難易度 M〜L。

**vibe-kanban**
- <https://github.com/BloopAI/vibe-kanban> / ★27,994 / Rust / Apache-2.0 / **最終 push 2026-04-24、README 冒頭に「Vibe Kanban is sunsetting」と告知**（＝この形のプロダクトは一度失敗した、という事実自体が重要な情報）。
- **仕組み**: 「ソフトウェアエンジニアが時間の大半をコーディングエージェントの計画とレビューに費やす世界では、出荷量を増やす最も効果的な方法は**計画とレビューを速くすること**」。kanban issue で計画 → **ワークスペース（エージェントに branch + terminal + dev server を与える）**で実行 → **diff をレビューして inline コメントを残し、UI を離れずにエージェントへフィードバックを送る** → 組み込みブラウザ（devtools / inspect mode / device emulation）でプレビュー → **AI 生成の description で PR を作り、GitHub でレビューして merge**。10+ のエージェント（Claude Code / Codex / Gemini CLI / Copilot / Amp / Cursor / OpenCode / Droid / CCR / Qwen Code）を切替。
- **Kagi への示唆**: ①**「diff に inline コメントを付けてエージェントに送り返す」は Kagi の PR review conversation UI の資産がそのまま使える形**で、しかも「AI に対するコードレビュー」という新しい用途。**Kagi にとって最も距離が近い AI native 機能**かもしれない。②「ワークスペース = branch + terminal + dev server」という単位定義は、Kagi の埋め込みターミナル + workspace mode + worktree と一致。③**ただし sunsetting しているので、「kanban で計画する」層は本質ではなかった可能性がある**——Kagi は計画層に手を出さず、**diff レビューとフィードバックのループ**に集中すべき、という読みができる。
- **難易度**: inline コメント → エージェント送信 = M。

**GitButler の agent 統合** — §2.1 参照（`but` CLI の JSON 出力、MCP サーバ、agent skill、Rules、VC ベンチマーク）。

---

## 3. Kagi 取り込み候補（優先順）

「効果」は Kagi の芯（安全性・グラフ中心・人間に優しい・AI native）に対する寄与。難易度 S=数百行 / M=1 機能分 / L=アーキ変更を伴う。

| # | 提案 | 効果 | 難易度 | 依存 | 出典 |
|---|---|---|---|---|---|
| 1 | **absorb（未コミット hunk を「その行を最後に触った mutable 祖先」へ自動配分）を plan/confirm 付きで実装**。曖昧な hunk は working tree に残す。既定 dry-run + 確認、明示フラグで即適用。pushed/protected コミットは対象外 | AI が作った雑な変更を既存コミット群へ安全に畳める。Kagi にしかない「配分先を GUI で確認してから実行」形になる | M | 行→コミット帰属計算（blame/`log -L` 相当）、既存 plan/confirm | jj `absorb` <https://docs.jj-vcs.dev/latest/cli-reference/> / `sl absorb` <https://sapling-scm.com/docs/commands/absorb/> / **git-absorb (BSD-3)** <https://github.com/tummychow/git-absorb> |
| 2 | **oplog に「時点復元（op restore）」と「選択的取り消し（op revert）」を追加**。各エントリに全 ref + HEAD + stash + WIP のスナップショットを持たせる。実装は **イベント列 + 定期 checkpoint** で JSONL を維持 | Kagi の存在理由（安全性）の中核が「1 手戻す」から「任意時点へ戻す / 途中の 1 手だけ消す」へ格上げされる | L（時点復元のみなら M） | oplog スキーマ拡張 | jj op log <https://docs.jj-vcs.dev/latest/operation-log/> / git-branchless Architecture（event log + checkpoint 構想）<https://github.com/arxanas/git-branchless/wiki/Architecture> / GitButler oplog snapshot（既存調査あり） |
| 3 | **undo/危険操作の confirm に「実行後のグラフのプレビュー」を出す** | 「戻した結果どうなるか」を実行前に見せるのは既存 GUI が誰もやっていない。Kagi の in-memory 再計算能力の最良の使い道 | M | in-memory グラフ再計算、confirm モーダル | `sl undo --preview/--interactive` <https://sapling-scm.com/docs/commands/undo/> / `git undo -i` <https://github.com/arxanas/git-branchless/wiki/Command:-git-undo> |
| 4 | **全操作に JSON I/O を持つ CLI/IPC 面を用意し、coding agent 用の skill を同梱配布**。「`push --force` / `reset --hard` / `git clean` を持たない」ことをエージェントに対するガードレールとして売る | Kagi の唯一無二の性質（破壊的操作が存在しない）が、初めて AI 時代の商品価値に変換される | M | 既存 single-instance ソケット、operation pipeline | GitButler `but` CLI + agent skill + MCP <https://blog.gitbutler.com/but-cli> <https://docs.gitbutler.com/ai-agents/overview> / `av` agent plugins <https://github.com/aviator-co/agent-plugins> |
| 5 | **並列エージェント worktree の裁定盤**: worktree ごとにタブ、複数 worktree の diff を横並び比較、Apply/Reject を plan→confirm→execute→verify→oplog に通す | 業界の主戦場（Cursor / Crystal / vibe-kanban）に、Kagi の既存資産（worktree・repo タブ・in-memory merge・oplog）の再配置だけで参入できる | M〜L | worktree 管理、repo タブ、split view | Cursor <https://cursor.com/blog/agent-best-practices> / Crystal <https://github.com/stravu/crystal> / vibe-kanban <https://github.com/BloopAI/vibe-kanban> |
| 6 | **diff への inline コメント → エージェントへの送信** | Kagi の PR review conversation 資産の新用途。「AI に対するコードレビュー」で最も距離が近い AI native 機能 | M | PR review conversation UI | vibe-kanban <https://github.com/BloopAI/vibe-kanban> |
| 7 | **AI に渡すコンテキストを interdiff（前回提出版との差分の差分）にする**。pushed amend 後の PR 更新コメント生成に適用。明示指定は常に AI を上書き | トークン削減と精度向上を同時に達成。素朴な「全 diff を投げる」実装に対する明確な優位 | M | pushed amend、PR 連携 | ghstack `automsg` <https://github.com/ezyang/ghstack> / `jj interdiff` <https://docs.jj-vcs.dev/latest/cli-reference/> |
| 8 | **smartlog モード**: 未 push コミット + main + 重要ブランチ + landed マーカーだけを残し、main の長い直線を破線で圧縮するグラフフィルタプリセット。**squash merge 済みコミットに `x` + "Landed as YYY" + 着地先リンク** | Kagi の ghost connector が既に検出している情報を、人間に優しい形で提示する。グラフ中心という Kagi の芯を直接強化 | M | 既存 lane layout、ghost connector、`list_merged_prs` | Sapling smartlog <https://sapling-scm.com/docs/overview/smartlog/> / git-branchless smartlog |
| 9 | **oplog を一級のビューにし、行を選んで restore / preview を出す**（選択対象の種類で内容が変わる単一 preview ペイン、evolog 専用ビュー、ace jump） | Kagi の oplog は既にあるので「ビュー化」だけで体験が跳ねる。#2 の UI 面 | M | #2 | jjui <https://github.com/idursun/jjui> / GG (Apache-2.0) <https://github.com/gulbanana/gg> |
| 10 | **「これから実行する / 実行した git 相当コマンド」を plan / verify に表示** | 透明性 = Kagi の思想。同時に AI エージェントへの監査ログになる | M | plan/verify 表示 | Sublime Merge "Real Git" <https://www.sublimemerge.com/> / SourceGit "Git command logs" / Sapling ISL |
| 11 | **単発の履歴編集操作**（コミットを選んで squash / 任意の古いコミットに staged 変更を amend / cherry-pick を copy-paste メタファで） — interactive rebase の TODO 編集 UI は作らない | Kagi の「1 操作 = 1 パイプライン」に interactive rebase より自然に収まる。lazygit 81k★ の中核体験 | M | pushed amend、in-memory rebase | lazygit <https://github.com/jesseduffield/lazygit> / gitu（Magit 系）<https://github.com/altsem/gitu> |
| 12 | **スタック/ブランチ横断のコマンド実行**（各コミットを worktree で並列実行、**tree ID をキャッシュキー**に、グラフ行に緑/赤バッジ） | エージェントが作ったスタックを push 前に全コミット緑にできる。Kagi の worktree + ターミナル資産の活用 | M〜L | worktree、ターミナル | git-branchless `git test`（Apache-2.0）<https://github.com/arxanas/git-branchless/wiki/Command:-git-test> / `jj run` |
| 13 | **`undo --keep` 相当**（履歴は戻すが作業内容は pending changes として残す）と **read-only 操作を undo 履歴からスキップ**、**「ローカルだけ戻しリモートは戻さない」の明示** | 「amend を取り消したいがコードは失いたくない」という現実の要求に直接答える。undo の空振りが無くなる | M | oplog セマンティクス | `sl undo` <https://sapling-scm.com/docs/commands/undo/> |
| 14 | **conflict 自動解決の 3 原則**（①疑わしければ marker を残す ②自動解決したら必ずレビューを促し該当箇所をハイライトして conflict editor を開く ③まず行ベースで試して conflict が無ければそれを返す） | Kagi が AI/自動解決に踏み込むときの憲法。GitComet の autosolve（既存調査あり）に足りない「慎重さの作法」 | M | conflict editor | mergiraf <https://mergiraf.org/> <https://lwn.net/Articles/1042355/> / `jj converge`（CHANGELOG） |
| 15 | **merged 判定の根拠を UI に明示**（どのチェック（PR 照合 / git の merge 検出 / merge commit 解析 / リモート追跡ブランチ消滅）が一致したかを表示し、merge commit / PR への検証リンクを添える）+ **Archive**（削除の代わりに退避） | Kagi は検出ロジックを既に持つ。**根拠を見せてから消させる**のが安全性の完成形 | S〜M | ghost connector、`list_merged_prs`、branch cleanup | Tower 17 <https://www.git-tower.com/blog/tower-mac-17> / <https://www.git-tower.com/release-notes> |
| 16 | **グラフの表示設定を開放**: エッジスタイル（rounded / angular）、レーンのセル幅（double / single）、**コミット順序（chrono / topo）** | 好みが最も分かれる部分をユーザに委ねる。canvas 描画済みなので安価 | S | 既存 lane layout | serie <https://github.com/lusingander/serie> |
| 17 | **revset 風フィルタ語彙の最小セット**: `stack()` / `draft()` / `public()` / `paths.changed()` + **入力欄のオートコンプリートとシグネチャヘルプ** | 「今の自分に関係するコミット」を宣言的に切り出せる。#8 の基盤 | M（DSL 完全実装は L） | Navigator フィルタ | git-branchless Revsets <https://github.com/arxanas/git-branchless/wiki/Reference:-Revsets> / jjui の revset 補完 |
| 18 | **diff の第 3 モード = 構造（構文木）diff** | AI が生成した大規模リファクタ diff のレビュー体験が跳ねる。AI 時代の diff 側の最大の一手 | L | tree-sitter | difftastic (MIT) <https://github.com/Wilfred/difftastic> |
| 19 | **conflict marker の第 3 表示モード = 「base スナップショット + 各側の diff」**。長マーカー / 末尾改行欠落のエッジケースを marker parse のテストに追加 | 3-way 以上（octopus / criss-cross）への自然な拡張路。エッジケースは即座にテスト資産になる | M（表示のみ） | conflict editor | jj conflicts <https://docs.jj-vcs.dev/latest/conflicts/> |
| 20 | **repo の遅延アクティベーション**（インデックス/検索は先に済ませ、git 機能だけ後から有効化）+ **巨大リポジトリのベンチ公開**（時間 / メモリ / freeze / crash） | 多数 repo タブでの起動性能。「安全 かつ 速い」の主張に数字の裏付けが付く | M / S | repo タブ、背景スレッド | Zed <https://zed.dev/docs/git.html> / gitui のベンチ表 <https://github.com/gitui-org/gitui> |
| 21 | 小物まとめ: **hunk 上下端ドラッグでコンテキスト行伸縮**（Sublime Merge）/ **Image Diff の Swipe・Blend モード**（SourceGit）/ **diff ファイル一覧のツリー表示切替**（Zed, diffnav）/ **conventional commit ヘルパー**（SourceGit）/ **AI プロンプトのプリセット開放**（Tower）/ **blame への syntax highlight**（delta）/ **context based help（今押せるキーだけ出す）**（gitui, gitu）/ **PR スタック内ナビゲーション `shift+N/P`**（ReviewStack） | 個々は小さいが体験差が大きく、いずれも既存資産の上に乗る | 各 S〜M | — | 各項に併記 |
| 22 | **stacked branch の状態は「git config が正本、キャッシュは別」の二層に**（将来 stacked branch を持つ場合の設計決定） | Kagi が消えても壊れない・素の git で検査できる = Kagi の透明性の思想に合う | S（決定のみ） | — | Tower 17.1（git config）<https://www.git-tower.com/release-notes> / Graphite（独自 ref → SQLite）<https://graphite.com/blog/git-key-value> |
| 23 | **複合操作の逐次確認 UX と「やったこと / やらなかったこと + その理由」の報告様式** | Kagi の二段確認をブランチ横断操作へ拡張するテンプレート。verify 表示の目標水準 | M / S | operation pipeline | `git machete traverse` <https://github.com/VirtusLab/git-machete> / `av sync` の出力 <https://github.com/aviator-co/av> / `git town undo` <https://www.git-town.com/commands/undo> |
| 24 | **git-branchless wiki の「Git で困る 13 パターン」対応表を Kagi の受け入れテスト表に転用** | 安全機能の価値を「13 パターンのうち N 個が 1 クリック」と定量的に説明できる | S | — | <https://github.com/arxanas/git-branchless/wiki/Command:-git-undo> |
| 25 | **change ID 相当（rewrite を越えたコミット同一性）と evolog（同一 change の系譜をグラフ上で線でつなぐ）** | amend/rebase 前後をグラフで追える。Kagi のグラフ中心思想の最も遠く強い延長 | L | グラフ、oplog | jj glossary <https://docs.jj-vcs.dev/latest/glossary/> / jjui の evolog ビュー |

---

## 4. 取り込まないと判断したもの（理由付き）

| 項目 | 出典 | 取り込まない理由 |
|---|---|---|
| **virtual branch（単一 worktree に複数ブランチを同時適用、workspace commit）** | GitButler | 既存調査（`gitbutler-reuse-research.md`）の結論を堅持。HEAD を管理下に置き worktree を書き換える侵襲的モデルで、Kagi の「repo 無傷 / plan→confirm」と思想が真逆。加えて **FSL-1.1-MIT の Competing Use によりコード流用不可** |
| **working-copy-as-commit / first-class conflict の *データモデル*採用** | jj | 既存調査の結論どおり。Git の index を置き換える設計で、git2 単一 backend（ADR-0002）と排他。採用 = backend 総入れ替え。**conflict の *表示形式* と absorb / op log の *セマンティクス* だけを借りる**のが正解 |
| **"Nuke the working tree" / `git clean` / `reset --hard` / force push** | lazygit, SourceGit, Gitnuro, GitComet | **Kagi の存在理由に真正面から反する**。force-with-lease は既に実装済みでそれが上限。ここは決別点として明示的に保つ |
| **merge queue / AI Reviews（サーバ側機能）** | Graphite | ローカル GUI の射程外。ホスティング側サービスに依存し、Kagi のオフライン性・可搬性を損なう |
| **クラウド microVM でエージェントを走らせる / マルチプレイヤー共有ワークスペース** | Conductor | Kagi はローカルネイティブ GUI。インフラ運用を抱える判断は製品の性格を変える。「ワークスペースを第一級の単位にする」概念だけ借用 |
| **kanban で計画する層** | vibe-kanban | **当該プロダクト自身が sunsetting しており、計画層が本質でなかった可能性が高い**。Kagi は「diff レビューとエージェントへのフィードバックのループ」に集中すべき |
| **interactive rebase の TODO 編集 UI** | lazygit, SourceGit, Gitnuro, Tower | Kagi の「1 操作 = 1 plan/confirm/execute/verify/oplog」と粒度が合わない。**単発操作（#11）に分解する方が Kagi の思想に忠実**で、かつ lazygit 自身も単発操作を併設している |
| **Elm/Redux 中央 Store / 自前 text_input / mimalloc** | GitComet | **既存調査あり**（`gitcomet-comparison.md` で Study / Reject 判定済み）。gpui Entity 路線と非互換。再評価不要 |
| **conflict UX の再調査（GitKraken / Fork / SourceTree / GitHub Desktop / VS Code / JetBrains のマージエディタ）** | — | **既存調査あり**（`conflict-ux-gui-clients.md`, `conflict-ux-editors.md`, `conflict-ux-models.md`）。結論（ブランチ名ラベル / 内蔵 3-way / 多段粒度 / 解決状態常時可視 / 統一 Continue-Abort-Skip バナー）は既に採用済み。重複調査しない |
| **gix / 外部 git CLI への backend 切替** | jj, GitButler, GitComet, SourceGit, GitFiend | **既存調査あり**（Reject 済み）。git2 単一 backend + 外部 git 不要という可搬性は Kagi の資産。SourceGit が git 2.25.1+ 必須、GitComet が 2.50+ 必須であるのは移植性の負債 |
| **tig / Gitnuro / Sapling / git-spice / mergiraf のコード参照** | — | GPL-2.0 / GPL-3.0 / AGPL 系。ADR-0031 のライセンスゲートで転写不可。概念参照に限定（本文でもそう扱った） |
| **Aurora Editor の参照** | — | archived（最終 push 2025-08-26）。生きていない参照先を設計根拠にしない |

---

## 5. 未解決の疑問

1. **「Vincent」という git クライアントを特定できなかった**。2026-09-03 時点の web 検索では該当プロダクトが見つからず（GitFiend と混同されている可能性、あるいは非常に新しい / 別名の可能性）。**依頼元に正式名称か URL の確認が必要**。
2. **JetBrains の git UX（changelist による部分コミット、Local History）を一次資料で確認できていない**。「changelist = 変更を名前付きグループに分けて別々にコミットする」機能は GitButler の hunk assignment / Rules と同じ問題を 20 年前から解いており、**Kagi のコミット分割提案（#4 の Rules 相当）を設計するときの最重要参照**になるはず。要追加調査。
3. **absorb の「行 → 帰属コミット」計算を git2 でどう実装するか**が未確定。git-absorb は libgit2 を使っているので実装可能なのは確実だが、blame ベースか `log -L` ベースか、性能特性（巨大ファイル / 深い履歴）が不明。**#1 を着手する前に git-absorb のアルゴリズムを読む必要がある**（BSD-3-Clause なので参照は自由）。
4. **oplog の「イベント列 + checkpoint」方式で、Kagi の JSONL 形式を保ったまま時点復元が本当に成立するか**。git-branchless の著者自身が「専用 undo イベント型を入れた方がよい」「checkpoint は未実装」と述べている＝**参照実装が無い**。Kagi 側で設計する必要がある（ADR 案件）。
5. **`jj arrange`（対話的グラフ並べ替え）の実際の操作モデル**を確認できていない（CLI reference の 1 行説明と CHANGELOG のスクロール修正のみ）。Kagi がグラフ上での直接編集を検討するなら実機確認が必要。
6. **Graphite CLI がソース非公開化した経緯と時期**。`withgraphite/graphite-cli` が 404 になっているが、アーカイブ / 移転 / 削除のどれか不明。過去のソース（stack メタデータを `refs/branch-metadata/` に置く実装）が参照可能かどうかも未確認。
7. **GitButler の VC ベンチマーク（selective commit 22.4s / multi-amend 51.3s / split commit 42.1s）の測定条件**が不明（自社ブログのみ、リポジトリ規模・モデル・試行回数不明）。「エージェントの履歴整理にかかる時間とトークン」を Kagi の評価軸として採用するなら、**Kagi 側で独立に測定方法を定義する必要がある**。
8. **jjui / GG / Sapling ISL を実機で触っていない**（README と docs のみ）。特に #9（oplog ビュー）と #17（revset 補完）の具体的なインタラクション（キーバインド設計、補完のトリガ、エラー表示）は実機確認しないと設計に落とせない。
9. **Kagi の smart commit が既に「複数 CLI provider」に対応している（ADR-0099）ため、#7（interdiff をコンテキストに）を既存経路に足すだけで済むのか、プロンプト構築層の再設計が必要なのか**が未確認（本調査はリポジトリを読み取り専用で扱ったため、実装詳細に踏み込んでいない）。
