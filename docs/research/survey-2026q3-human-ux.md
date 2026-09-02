# 人間に優しい git クライアント UX と安全性パターン(外部サーベイ)

調査日: 2026-09-03 / 担当スライス: survey-human-ux(人間に優しい UX と安全性パターン)

> 前提: conflict UX は既存調査(`docs/research/conflict-ux-models.md`, `conflict-ux-gui-clients.md`, `conflict-ux-editors.md`)で扱い済みのため本書では深追いしない(§4 に理由付きで記載)。本書は **undo / diff 理解 / 学習コスト / a11y / 性能** に紙面を割く。
> Kagi 側の現状は本リポジトリを読み取り専用で確認した(`Cargo.lock`, `src/ui/`, `crates/kagi-git/src/oplog.rs`)。リポジトリのファイルは一切変更していない。

---

## 1. サマリ

- **AccessKit は既に Kagi の依存ツリーに入っている**(`Cargo.lock`: `accesskit 0.24.1` / `accesskit_macos 0.26.2` / `accesskit_unix 0.21.1`、gpui 経由)。GPUI は `_accessibility.rs` / `window/a11y.rs` で公式 API(`.id()` + `.role()` + `on_a11y_action()`)を提供済み。一方 Kagi 側の `.role(` 使用箇所は **0 件** → スクリーンリーダー対応は「フレームワーク待ち」ではなく **今日書ける差分**。最も費用対効果が高い未着手領域。
- **oplog を「見せて戻れる」パネルにするのが最大の未回収価値**。Kagi は `read_oplog_tail()`(`crates/kagi-git/src/oplog.rs:474`)と undo/redo tooltip プレビュー(ADR-0081)を持つが、`src/ui/` に oplog 一覧 UI が無い。jj の `op log` + `--at-op`、GitButler の `but oplog restore <SHA>` が完成形。
- **コマンドパレットはほぼ無料で作れる**。Kagi は既に静的 `COMMANDS` レジストリ(`src/ui/commands.rs`: `Command`, `command_state()`, `effective_keystroke()`, `shortcut_listing()`)を持つ。Sublime Merge の Cmd+P 相当は「この配列を検索 UI に露出するだけ」。
- **diff 理解の三点セットが未実装**: move detection(`--color-moved=zebra`)、intra-line/word diff、generated file / lockfile の自動折り畳み。特に lockfile 抑制は GitHub linguist の実装済みヒューリスティクス(ファイル名リスト + 「平均行長 > 110 で minified」)をそのまま移植できる。
- **git の `advice.*` は「約 40 個の人間向け解説文」の既製カタログ**。Kagi は git2(libgit2)なので `advice` が一切出力されない → 生エラーの翻訳文を自分で書く必要がある。`advice.adoc` を i18n キーの設計図として使えるのは大きなショートカット。

---

## 2. 詳細

### 2-A. 取り消しと安全性

#### jj (Jujutsu) — operation log / `jj undo` / `jj op restore` / `--at-op`
- 何か: 全リポジトリ変更操作を content-addressed な operation DAG として記録し、任意の時点へ巻き戻せる。
- 出典: https://docs.jj-vcs.dev/latest/operation-log/ (jj docs, latest / 2026-09 時点)
- 仕組み:
  - 各 operation が「その操作終了時点のリポジトリの姿」= **view オブジェクトのスナップショット**を持つ。view は「各 bookmark / tag / git ref がどこを指していたか」+ heads 集合 + 各 workspace の working-copy commit を含む。
  - operation は直前 operation への **親ポインタ**と metadata(timestamps / username / hostname / description)を持つ。
  - `jj undo` = 1 個ずつ取り消し。`jj op revert` = **最新でない特定 operation だけ**を打ち消す。`jj op restore` = リポジトリ全体を過去の姿に戻す。
  - `@` が現在 operation、`@-` が親、`@+` が子(revset ライクな演算子)。
  - `--at-op=<id>` で「その時点のリポジトリ」として任意コマンドを実行できる(read-only 用途が想定)。**「なぜこの状態になったのか」を後から再生できる**。
  - operation DAG により lock-free 並行実行が可能(divergent operations として検出、`jj st` / `jj log` が通知)。
- Kagi への示唆: Kagi の `OpLogEntry`(`timestamp` / `op` / `repo` / `before: StateSummary` / `outcome`)は **線形 JSONL で id も parents も無い**。ここに (a) エントリ ID、(b) 親 ID、(c) `description`/`hostname` を足すと「特定の 1 操作だけ revert」「N 個前へ restore」が表現可能になる。jj-reuse-research.md は「oplog metadata の参考にする」と結論しているが、**`--at-op` 相当の "その時点で見る" 機能は未検討** で、Kagi の graph 中心 UI と極めて相性が良い(「3 手前のグラフを見る」)。
- 難易度: エントリ ID/親付与 = S、restore-to-point = M、`--at-op` 相当のグラフ再生 = L

#### git-branchless — `git undo`(対話的タイムトラベル)
- 何か: commit graph への操作(commit / amend / merge / rebase / checkout / ブランチの rename・move・delete)を undo できる CLI。
- 出典: https://github.com/arxanas/git-branchless/wiki/Command:-git-undo (v0.1.0 以降、working copy 対応は v0.4.0 以降)
- 仕組み:
  - `-i/--interactive` で **矢印キーで過去の commit graph 状態を前後に移動**し、Enter で確定。
  - 確定前に「実行される inverse events の一覧」を提示して y/N 確認:
    ```
    Will apply these actions:
    1. Hide commit 8d4738cd new message
    Confirm? [yN] y
    Applied 1 inverse event.
    ```
  - working copy の undo は「snapshot が取られている場合のみ」。**untracked ファイルは undo 不可**と明記。
  - Tower の "Surviving with Git" 17 シナリオに対して「どのコマンドで解決するか / git-branchless で計画があるか」の**対応表**を wiki に持つ(#1 Discarding All Local Changes in a File → `git restore`、#9 Recovering Deleted Commits → `git undo` 等)。
- Kagi への示唆: 2 点。(1) **「実行される逆操作の一覧を出してから確認」**は Kagi の `plan → confirm` と同型で、undo にも同じ形式を適用すべき(現状は tooltip プレビューのみ)。(2) **「できないことを明示する表」**は極めて誠実な UX。Kagi の undo は ODB blob バックアップで discard を戻せるが、untracked 削除や push 済み更新は戻せない。この境界を UI に書くだけで信頼が上がる。
- 難易度: 逆操作一覧の確認ダイアログ = S、undo 可否マトリクスの UI 化 = S

#### Sapling — `sl undo` / `sl redo` / `--preview` / `--interactive`
- 何か: 「直前のローカルコマンド」を取り消す。ローカルコマンド = checkout 中のコミットを変えた / ローカルコミットの内容を変えた / ローカル bookmark を変えたもの。
- 出典: https://sapling-scm.com/docs/commands/undo/
- 仕組み:
  - 連続実行可能、`--step N` で N 個まとめて。read-only コマンドと非ローカルコマンドは**自動スキップ**される。
  - `--keep` で「working copy の状態は保ったまま」commit/amend を取り消す → 変更が pending changes として残る。
  - **`--preview` で「undo 後の smartlog がどう見えるか」をグラフで表示**、`--interactive` は undo 履歴を前後に歩ける対話版。
  - hybrid コマンド(local + remote)ではローカル側だけ undo し、リモートは触らない(明示)。
- Kagi への示唆: **`--keep` 相当のセマンティクス**は「amend を取り消すが編集は失いたくない」という現実の要望に直答する。Kagi の undo は現状 all-or-nothing。また `--preview` の「undo 後のグラフ」は Kagi のコミットグラフ描画をそのまま再利用でき、Kagi の最大の差別化(グラフ)と undo(第二の差別化)を掛け算できる。
- 難易度: `--keep` 相当 = M、undo 後グラフのプレビュー = M

#### GitButler — `but oplog` / on-demand named snapshot / 単一エントリ化された破壊操作
- 何か: 操作履歴 + 任意時点への復元。**uncommitted changes を含む全 state** が operation に保存される。
- 出典: https://docs.gitbutler.com/commands/but-oplog / https://docs.gitbutler.com/commands/but-discard (2026-07-30 更新)
- 仕組み:
  - `but oplog list` はデフォルト直近 20 件。`--since <SHA>` で起点指定、`-s/--snapshot` で**手動スナップショットだけ**に絞れる。
  - `but oplog snapshot -m "<message>"` = **名前付きの手動セーブポイント**。「いつでも known good state に戻れる」ことを目的と明記。
  - `but oplog restore <OPLOG_SHA>` で復元。`but redo` = 直前の undo をやり直す。
  - `but discard` の doc に明記: 「**The entire operation is recorded as a single oplog entry, so it can be undone with `but undo`**」。branch / commit / committed file / uncommitted file / uncommitted hunk の粒度で discard 可能だが、それでも 1 エントリ。
- Kagi への示唆:
  - **名前付きスナップショット**は Kagi の stash とは意味が違う(stash は working copy を「退避して消す」、snapshot は「今の状態を消さずに刻む」)。危険な rebase の前に 1 クリックで打てるセーブポイントは、Kagi の安全性ブランドに直結する。
  - **「複合操作 = 1 oplog エントリ」の不変条件**は Kagi の branch cleanup(複数ブランチ削除)などで既に問題になり得る箇所(`src/ui/branch_cleanup.rs` は per-branch failure を集約している)。1 エントリ = 1 undo 単位を規約にすると undo の意味が壊れない。
  - `-s/--snapshot` フィルタ = 「機械が刻んだ履歴」と「人が意図して刻んだ履歴」を分ける発想。oplog パネルの見やすさを一撃で解決する。
- 難易度: 名前付きスナップショット = M、1操作=1エントリの規約徹底 = S(監査作業)

#### lazygit — reflog 由来の `z` undo / `Z` redo
- 何か: TUI クライアントの undo。**reflog を読んで「直前の git コマンドを打ち消す git コマンド」を決定**する。
- 出典: https://github.com/jesseduffield/lazygit/blob/master/docs/keybindings/Keybindings_en.md
- 仕組み: キーバインド表に明記 —
  - `z` Undo: "The reflog will be used to determine what git command to run to undo the last git command. **This does not include changes to the working tree; only commits are taken into consideration.**"
  - `Z` Redo: 同じく reflog 由来。
  - conflict 編集中の `z` は「直前の conflict 解決を undo」という**モード固有の undo**。
  - Reflog 専用パネルを持ち、`<space>` でその commit を detached HEAD で checkout できる = **reflog を人間に見せる UX の最小実装**。
- Kagi への示唆: lazygit の弱点(working tree を戻せない)が **まさに Kagi の oplog + ODB blob backup が既に解決している点**。ここは「Kagi は lazygit/Sublime Merge/Fork より一段強い undo を持つ」と主張できる差別化ポイントなので、UI で可視化する価値が高い。逆に `z` 一発 + モード固有 undo という**発見性の高さ**は真似すべき。
- 難易度: グローバル undo キーバインド + モード固有 undo = S

#### Sublime Merge — reflog を歩く Repository ▶ Undo
- 何か: reflog を前後に歩いて commit / reset 等を undo/redo する。
- 出典: https://www.sublimemerge.com/ (製品ページ) / https://www.sublimemerge.com/docs/ (2026-09 時点で undo 専用 doc ページは無く、機能はアプリメニュー `Repository ▶ Undo`)
- 仕組み: reflog エントリを 1 ステップ単位で往復。
- Kagi への示唆: reflog ベースは「commit graph だけ」の限界がある(lazygit と同じ)。**Kagi は oplog を持つので reflog UI を作る必要は無い** — ただし「reflog にはこう記録されている」を oplog エントリの詳細ビューに併記すると、CLI に落ちたときの接続点になる(Sublime Merge の "Real Git" 思想)。
- 難易度: oplog 詳細に reflog 対応行を併記 = S

#### 危険操作の前に自動退避 — `rebase.autoStash` / `merge.autoStash`
- 何か: dirty worktree でも rebase/merge を実行できるよう、**操作開始前に一時 stash エントリを自動作成し、終了後に適用**する。
- 出典: https://github.com/git/git/blob/master/Documentation/config/rebase.adoc / `.../config/merge.adoc`(git master, 2026-09 取得)
  - 原文: "When set to `true`, automatically create a temporary stash entry before the operation begins, and apply it after the operation ends. This means that you can run merge on a dirty worktree. **However, use with care: the final stash application after a successful merge might result in non-trivial conflicts.**" 両方 default `false`。
- 仕組み: `--autostash` / `--no-autostash` でコマンド単位に上書き可能。
- Kagi への示唆: Kagi は `autostash` 相当が grep で 0 件。現状は preflight が dirty worktree を blocker として**拒否**していると推測される([推測]、blocker 文言は未確認)。git 自身が「便利だが最後の適用で conflict が起きうる」と警告している通り、**暗黙の autostash は安全性ブランドに反する**。取るべき形は「preflight が dirty を検出 → 『先に退避しますか?(退避内容: N ファイル)』を confirm に出す → 実行 → 復帰時の conflict も oplog に載せる」= **明示 autostash**。Kagi の plan/confirm/preflight パイプラインにそのまま乗る。
- 難易度: M

#### JetBrains Local History — VCS 非依存の自動セーブポイント
- 何か: IDE 側が「個人用バージョン管理」としてファイル変更・リファクタ・テスト実行などのイベント時点を自動記録し、ラベル付け・復元できる。
- 出典: https://www.jetbrains.com/help/idea/local-history.html (build 2723 / 2026-09-02 生成)
- Kagi への示唆: Kagi は **埋め込みエディタ(workspace mode)を持つ**ため、「エディタで編集した内容が commit 前に消える」事故が構造的に起こり得る。Local History 的な「エディタ保存時点の自動 ODB blob スナップショット」は、既にある discard 時 ODB backup の仕組みを流用できる。ただし過剰機能のリスクもあり、優先度は低め。
- 難易度: M〜L
- 判断: **候補に載せるが優先度低**(Kagi の役割は git クライアントで、エディタは補助)

#### dry-run の可用性(git が実際に提供しているもの)
- 出典: https://git-scm.com/docs/git-push(`-n | --dry-run`)、`.../git-clean.adoc`(`-n`, `-i` 対話モード)
- 仕組み: `git push --dry-run` は実際の送信をせずに ref 更新の結果を報告。`git clean -n` は削除予定ファイルを列挙、`-i` は対話選択。`git rm --dry-run`、`git apply --check` も同様。**rebase / merge には dry-run が無い**(だから GUI 側が影響範囲プレビューを自作する必要がある)。
- Kagi への示唆: Kagi は `git clean` を実装しない方針なので `-n` は不要。しかし **push の confirm 画面に `--dry-run` 相当の ref 更新一覧(`<ref>: <old>..<new>`, forced か否か)を出す**のは既存 force-with-lease 実装と地続きで、二段確認の情報量を上げる。rebase/merge に dry-run が無いことは「Kagi の影響範囲プレビューが代替物として価値を持つ」根拠になる。
- 難易度: push の ref 更新プレビュー = S

---

### 2-B. 差分と履歴の理解

#### difftastic — 構文認識(structural)diff
- 何か: tree-sitter で両側をパースし、**構文単位で**差分を出す構造的 diff。
- 出典: https://github.com/Wilfred/difftastic (README) / https://difftastic.wilfred.me.uk/git.html
- 仕組み:
  - 30 言語超をサポート。**拡張子が未知なら行指向 diff + 単語ハイライトにフォールバック**。
  - git 統合は `diff.external=difft` または `GIT_EXTERNAL_DIFF` 環境変数。`git diff` 以外のサブコマンドでは `--ext-diff` が必要。git ≤ 2.43.1 は external diff + 権限変更でクラッシュしうる(→ `difftool` 経由を推奨)。
  - **既知の限界(README に明記)**: 変更が多いファイルでスケールが悪くメモリを大量消費。side-by-side 表示が混乱を招く場合がある。クラッシュ修正リリースが頻繁。
  - **非目標**: patch 生成をしない(出力は人間向け)。AST merge もしない(→ mergiraf)。
- Kagi への示唆: Kagi は既に tree-sitter を持つ(`gpui-component` の `tree-sitter-languages` feature、`SyntaxHighlighter` を diff で使用中: `src/ui/diff_view.rs:314`)。**フル構造 diff は難易度 L + difftastic 自身が認めるスケーリング問題を輸入することになる**。より現実的な中間解は「行指向 diff を保ったまま、変更行ペアに対して tree-sitter トークン境界で intra-line ハイライト」= 難易度 M で 8 割の価値。
- 難易度: intra-line トークン diff = M / フル構造 diff = L(非推奨)

#### `--color-moved` — move detection
- 何か: 移動したコード行を追加/削除とは別の色で表示する。
- 出典: https://github.com/git/git/blob/master/Documentation/diff-options.adoc(git master, 2026-09 取得)
- 仕組み(原文の mode 定義):
  - `plain`: ある場所で追加され別の場所で削除された行すべてを色分け。「移動を検出はするが、**permutation 無しに移動されたのかをレビューで判断するには有用でない**」
  - `blocks`: **英数 20 文字以上**の移動ブロックを貪欲に検出。隣接ブロックは区別されない。
  - `zebra`: `blocks` と同じ検出 + `(old|new)Moved` と `(old|new)MovedAlternative` を交互に使い、**色の切り替わりで新しいブロックの開始を示す**。
  - `dimmed-zebra`: 隣接ブロックの境界行だけを「興味あり」とし、残りを減光。
  - オプション無し = `no`、mode 無しで指定 = `zebra`。
  - `--color-moved-ws=<mode>,...` で move 検出時の空白無視を制御(`ignore-space-at-eol` / `ignore-space-change` / `ignore-all-space` / `allow-indentation-change`)。
- Kagi への示唆: `allow-indentation-change` が特に効く(if でラップして丸ごとインデント、という最頻出パターンを「移動」と認識できる)。Kagi の diff は行 kind が `Added/Removed/Context` の 3 値(`src/ui/diff_split.rs:149-157`)なので、**`Moved{ block_id }` を第 4 の kind として足す**設計になる。zebra の「交互色でブロック境界を示す」は色覚配慮の観点で問題があるので、Kagi では**色ではなく左端の縦バーの太さ/破線**でブロック境界を出すべき(§2-D 参照)。libgit2 に move detection API は無いため、検出アルゴリズム(20 英数文字以上の貪欲マッチ)は自前実装。
- 難易度: M

#### `--word-diff` / intra-line diff
- 何か: 行内の変化した語だけを強調する。
- 出典: https://github.com/git/git/blob/master/Documentation/diff-options.adoc(`--word-diff[=<mode>]`, `--word-diff-regex=<regex>`)
- Kagi への示唆: Kagi は grep で `word_diff` / intra-line ハイライトの実装が 0 件。**タイポ 1 文字の修正と行の全面書き換えが同じ「赤 1 行 + 緑 1 行」に見える**のは認知負荷の主要因。`--word-diff-regex` の思想(言語ごとに「語」の定義を差し替える)は、tree-sitter トークンを語境界に使う実装と同じ発想。
- 難易度: M(`--color-moved` と同じ diff row 拡張の上に乗るので、セットで実装するとコストが下がる)

#### `git log --remerge-diff`
- 何か: マージコミットについて「機械的なマージ結果」と「実際に記録された結果」の差分を表示 = **人間が conflict 解決時に手で入れた変更だけ**が見える。
- 出典: git 2.36.0 リリースノート(https://github.com/git/git/blob/master/Documentation/RelNotes/2.36.0.adoc)— "git log --remerge-diff shows the difference from mechanical merge result and the result that is actually recorded in a merge commit."(git 2.36 = 2022-04)
- Kagi への示唆: Kagi は **既に in-memory merge を持つ**(ADR-0005、conflict editor の基盤)。ということは「機械的マージ結果」を計算する能力が既にある = **remerge-diff は既存部品の組み合わせで作れる**。マージコミットを選択したときに「自動マージでは済まなかった部分」だけを見せられるのは、コミットグラフ中心 UI にとって非常に強い。Kagi は grep で `remerge` 0 件。
- 難易度: M(in-memory merge が既にあるため。無ければ L)

#### `git range-diff`
- 何か: 2 つのコミット列(rebase 前/後、force-push 前/後)を比較し、「どのコミットが対応し、そのうち何が変わったか」を出す。
- 出典: https://git-scm.com/docs/git-range-diff
- Kagi への示唆: **これは Kagi の存在理由に直結する**。Kagi は force-with-lease を実装済み・`push --force` を排除済み。その上で「force-with-lease push の confirm 画面に range-diff を出す」= 「リモートの N コミットがこう置き換わります。中身の差はこれだけです」と言える。**force push の二段確認に、初めて意味のある情報量が入る**。amend(pushed amend 含む)や rebase の confirm でも同じ。Kagi は grep で `range_diff` 0 件。
- 難易度: M(表示は既存の commit list + diff コンポーネントの組み合わせ。対応付けアルゴリズムは patch-id ベースで自前実装)

#### blame UX — `--ignore-rev` / `.git-blame-ignore-revs` / inline blame
- 何か: 「一括フォーマット」など意味のないコミットを blame から除外する / 現在行の blame をエディタ行末に出す。
- 出典:
  - https://git-scm.com/docs/git-blame — `--ignore-rev <rev>` / `--ignore-revs-file <file>`
  - https://github.com/git/git/blob/master/Documentation/config/blame.adoc —
    - `blame.ignoreRevsFile`: 「1 行 1 個の非省略オブジェクト名。空白と `#` 始まりのコメントは無視。複数回指定可。空のファイル名で除外リストをリセット。**コマンドラインの `--ignore-revs-file` より先に処理される**」
    - `blame.markUnblamableLines`: 無視 revision が変えた行で**他コミットに帰属できなかった**ものを `*` で印
    - `blame.markIgnoredLines`: 無視 revision が変えた行で**他コミットに帰属できた**ものを `?` で印
- 仕組み: 慣習として `.git-blame-ignore-revs` をリポジトリ直下に置き、`git config blame.ignoreRevsFile .git-blame-ignore-revs` で有効化。
- Kagi への示唆:
  1. Kagi に blame UI は事実上無い(`src/ui/` で "blame" は `mod.rs` に 1 箇所のみ、`file_history.rs` はコミット単位の履歴)。
  2. `.git-blame-ignore-revs` を**自動検出して「N 件の revision を無視中」と UI に出す**のは、他クライアントがしばしば落とす部分。libgit2 の `git_blame_options` は `oldest_commit`/`newest_commit` はあるが **ignore-revs 相当が無い**([推測]— git2 0.21 の API 未確認)ので、実装は「無視 rev の親へ blame を再帰的に付け替える」自前ロジックか、`gh`/`git` サブプロセス委譲になる。
  3. `markUnblamableLines`/`markIgnoredLines` の `*` / `?` は **色に依存しない状態表示**の良例(§2-D)。
- 難易度: 基本 blame = M、ignore-revs 対応 = M、inline blame = M

#### inline blame の実装例 — GitLens "Current Line Blame"
- 何か: アクティブ行の行末に、控えめでテーマ対応の注釈を出す(作者 / コミット日 / コミットメッセージ)。
- 出典: https://help.gitkraken.com/gitlens/gitlens-features/ (2026-08-12 更新)
- Kagi への示唆: Kagi は埋め込みエディタを持つので同じ場所に置ける。「デフォルトで作者・日付・メッセージ、可視性は設定可能」という**控えめさの既定値**をそのまま採用してよい。
- 難易度: M

#### 大きい diff の扱い — generated file / lockfile 判定
- 何か: 生成物・ロックファイルを diff でデフォルト折り畳み / 言語統計から除外する。
- 出典:
  - 属性による明示指定: https://docs.github.com/en/repositories/working-with-files/managing-files/customizing-how-changed-files-appear-on-github — `.gitattributes` に `search/index.json linguist-generated` と書くと「言語統計から無視され、**diff でデフォルト非表示**」。`bootstrap.min.css -linguist-generated` で解除。
  - ヒューリスティクス実装: https://github.com/github-linguist/linguist/blob/main/lib/linguist/generated.rb
- 仕組み(`generated.rb` の実際の判定、抜粋):
  - **ファイル名一致**: `Cargo.lock`, `Cargo.toml.orig`, `package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`, `bun.lock`, `composer.lock`, `deno.lock`, `flake.lock`(Nix), `pipenv.lock`, `pixi.lock`, `terraform.lock`, bazel lock, `Manifest.toml`(Julia), `node_modules/`, `Godeps/`, gradle/maven wrapper, `.secrets.baseline`
  - **先頭行のマーカー**: Go は「先頭 40 行のいずれかが `^// Code generated .*`」。protobuf は「先頭 3 行のいずれかが `Generated by the protocol buffer compiler.  DO NOT EDIT!` を含む」
  - **minified**: 拡張子が `.js`/`.css` のときだけ判定し、**平均行長 > 110 文字**なら minified
  - **source map**: 末尾 2 行に `sourceMappingURL` / `sourceURL` があるか、`.map` そのもの
  - その他: Xcode / IntelliJ / CocoaPods / Carthage / `.designer.cs` / `.feature.cs` / ANTLR / Racc / JFlex / sorbet `.rbi` / sqlx query / Unity `.meta` など数十種
- Kagi への示唆: Kagi は grep でこの種の抑制が 0 件(`generated` の hit は全て無関係のコメント)。**この判定表は移植コストがほぼゼロで効果が最大**の項目。実装形は 2 段:
  1. `.gitattributes` の `linguist-generated` を読む(明示指定を尊重。git2 に `git_attr_get` があるので安い)
  2. フォールバックでファイル名リスト + 「平均行長 > 110」+ 先頭 40 行の `Code generated` / `DO NOT EDIT` マーカー
  UI は「折り畳み + `生成ファイル` バッジ + 展開ボタン + ファイル一覧の末尾へソート」。
- 難易度: S

#### 大きい diff の扱い — `-diff` 属性と core.bigFileThreshold
- 出典: https://git-scm.com/docs/gitattributes(`diff` 属性: unset すると「binary 扱い」で diff テキストを生成しない / `diff=<driver>` で funcname パターン等を切り替え)
- Kagi への示唆: `.gitattributes` の `-diff` を尊重すれば、リポジトリ側が既に「見せなくていい」と宣言しているファイルを無料で抑制できる。`diff=<driver>` の funcname パターンは **hunk ヘッダに関数名を出す**機能で、Kagi の diff hunk ヘッダを賢くできる(既に tree-sitter があるので tree-sitter 側で解いてもよい)。
- 難易度: `-diff` 尊重 = S、hunk ヘッダの関数名 = M

#### コードレビュー UX — viewed 管理と進捗バー
- 何か: PR の Files changed でファイル単位に「Viewed」チェックを付けると折り畳まれ、ヘッダの進捗バーに反映される。
- 出典: https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request
  - 原文: "After reviewing a file, mark it as **Viewed** to collapse it and track your progress." / "The **progress bar** in the pull request header shows how many files you've viewed." / "**If the file changes after you view the file, it will be unmarked as viewed.**"
- Kagi への示唆: Kagi は PR 一覧・PR merge・review conversation・PR conflict preview を既に持つ。**viewed 状態は「レビュー再開時にどこまで見たか」を人間の記憶から外部化する**単純で効果の大きい機能。決定的なのは「**ファイルが変わったら viewed が自動解除される**」という不変条件で、blob SHA を保存しておけば実現できる。保存先は `$HOME/.kagi/` 配下のローカル状態(GitHub API には viewed 状態を書く公式手段が無い、という点は [推測])。
- 難易度: S〜M

#### コードレビュー UX — suggested change
- 出典: https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/incorporating-feedback-in-your-pull-request — 「suggested changes を PR に直接適用できる」「**複数の suggestion を 1 コミットにまとめられる**(batch)」「スコープ外のフィードバックは issue にして追跡」
- Kagi への示唆: Kagi の review conversation は既にある。suggestion の**適用**側(受け取った suggestion を hunk として working tree に当てる)は Kagi の hunk 適用基盤に乗る。「複数まとめて 1 コミット」は Kagi の plan/confirm と相性が良い。ただし GitHub 依存機能なので優先度は中。
- 難易度: M

---

### 2-C. 学習コストと導線

#### git 自身の `advice.*` — 既製の「人間向け解説文カタログ」
- 何か: 新規ユーザ向けのオプションのヘルプメッセージ群を制御する config 名前空間。
- 出典: https://github.com/git/git/blob/master/Documentation/config/advice.adoc(git master, 2026-09 取得)
- 仕組み: 原文冒頭 — 「これらは human users を助けることを意図しているので stderr に出力される。**git をサブプロセスとして実行するツールがこれを邪魔だと感じるなら `GIT_ADVICE=0` で全 advice を抑制できる**」。個別に `false` を設定して黙らせられる。
- カタログの実例(約 40 項目、原文より):
  `addEmbeddedRepo`(repo の中に repo を add した)/ `addEmptyPathspec` / `addIgnoredFile` / `amWorkDir` / `ambiguousFetchRefspec` / `checkoutAmbiguousRemoteBranchName` / `commitBeforeMerge`(ローカル変更の上書きを避けて merge を拒否)/ `detachedHead`(detached HEAD になった、後からブランチを作る方法)/ `diverging`(fast-forward できない)/ `fetchRemoteHEADWarn` / `fetchShowForcedUpdates` / `forceDeleteBranch`(未マージブランチを force なしで消そうとした)/ `ignoredHook`(hook が実行可能でないので無視)/ `implicitIdentity` / `mergeConflict` / `nestedTag` / `pushAlreadyExists` / `pushFetchFirst` / `pushNeedsForce` / `pushNonFFCurrent` / `pushNonFFMatching` / `pushRefNeedsUpdate` / `pushRepoLooksLikeRef` / `pushUnqualifiedRefname` / `pushUpdateRejected`(上記 push 系を一括制御)/ `rebaseTodoError` / `refSyntax` / `resetNoRefresh` / `resolveConflict` / `rmHints` / `sequencerInUse` / `skippedCherryPicks` / `sparseIndexExpanded` / `statusAheadBehind` ほか
- Kagi への示唆: **これは Kagi にとって設計図そのもの**。
  - Kagi は git2(libgit2)なので **`advice.*` は一切出力されない**。つまり「fast-forward できません」に相当する人間向け説明は、Kagi が自分で書かなければ存在しない。
  - `advice.adoc` の項目名は「git が 20 年かけて発見した、人間が詰まる地点の一覧」= **i18n キーの命名と網羅性チェックリストにそのまま使える**。Kagi は EN/JA を実装済みなので、`error.advice.diverging` 等のキー体系を advice に揃えると、抜けが機械的に検出できる。
  - 逆向きの示唆: 埋め込みターミナルや `gh` 経由の呼び出しでは advice が**出てしまう**。Kagi の UI が同じ内容を自前で説明するなら、サブプロセス側では `GIT_ADVICE=0` を設定して二重表示を避けるべき。
- 難易度: advice カタログのキー化 + 翻訳 = M(項目数が多いだけで各項目は小)、`GIT_ADVICE=0` の設定 = S

#### 「実行される git コマンドを見せる」 — Sublime Merge の "Real Git"
- 何か: GUI 操作が実行している git コマンドをそのまま見せ、CLI と行き来できる。
- 出典: https://www.sublimemerge.com/ — 原文: "**When you're using Sublime Merge, you're using Git. View the exact Git commands you're using, and seamlessly transition between the command line and Sublime Merge.**"
- Kagi への示唆: **Kagi は libgit2 なので「実行した git コマンド」が原理的に存在しない**。ここは誠実さが問われる分岐点で、選択肢は 2 つ:
  - (a) **等価コマンド**を出す: 「この操作は `git push --force-with-lease=main:abc123 origin main` に相当します」。plan にコマンド文字列フィールドを足す。CLI へのブリッジと学習効果の両方が得られる。「相当します(equivalent)」と明記すれば嘘にならない。
  - (b) 何も出さない。
  → (a) を推す。Kagi は既に埋め込みターミナルを持つので、**等価コマンドをターミナルにコピーできる**ところまで行くと「GUI で学んで CLI に持っていく」導線が完成する。
- 難易度: M(全 plan 種に等価コマンド文字列を追加する横断作業)

#### コマンドパレット
- 何か: 全機能をタイプで検索して起動する単一入口。
- 出典:
  - Sublime Merge: https://www.sublimemerge.com/docs/getting_started — 「コマンドパレットは Sublime Merge の膨大なコマンド群への素早い入口。**Ctrl+P (macOS は Cmd+P)** で開き、コマンド名の一部をタイプして絞り込む」
  - コマンド定義形式: https://www.sublimemerge.com/docs/command_palette — `.sublime-commands`(JSON 配列。各要素は `caption` / `command` / 任意の `args`)。**同じ `command` に異なる `args` を与えて別エントリにできる**(例: `checkout_branch` に `{"local_refs": false, "remote_refs": true}` を渡して "Checkout Remote Branch…" を作る)。
  - キーバインド形式: https://www.sublimemerge.com/docs/key_bindings — `.sublime-keymap`(`keys` 配列で**連続キー入力**も表現、`context` で状況限定、`primary` = Windows/Linux の Ctrl / macOS の ⌘)。パレット自体をキーに割り当てることも可能(`{"keys": ["super+shift+b"], "command": "show_command_palette", "args": {"command": "create_branch"}}`)。
- Kagi への示唆: **Kagi は既に必要な部品を全部持っている**。`src/ui/commands.rs`(2047 行)に静的 `COMMANDS` 配列、`Command { id, label, ... }`、`command_state(app, id) -> CommandState`(有効/無効の判定)、`effective_keystroke(id)`、`display_keystroke()`、`shortcut_listing()` が既にある。コマンドパレットは:
  - この配列を fuzzy 検索してリスト表示
  - `command_state()` で無効なものをグレーアウト(**かつ「なぜ無効か」を出せる** — ここが Sublime Merge を超えられる点。Kagi は blocker 文言を持っている)
  - `effective_keystroke()` を右端に表示してキーバインドを学習させる
  これだけ。既存 `register_keybindings`(`KeyBinding::new` は全リポジトリで 18 箇所のみ)と併せて、**undo の発見性問題も同時に解決する**(パレットで "undo" と打てば見つかる)。
- 難易度: S〜M

#### Gitless — 「git の概念そのものが misfit」という研究由来の設計
- 何か: git 互換だが概念モデルを組み替えた VCS。MIT の概念設計研究から生まれた。
- 出典:
  - https://gitless.com/
  - Santiago Perez De Rosso, Daniel Jackson, "Purposes, concepts, misfits, and a redesign of git", OOPSLA / SPLASH 2016, 2016-10-19. DOI: 10.1145/2983990.2984018(Crossref で著者・所属 MIT・日付を確認)
- 仕組み(gitless.com の主張):
  - **Simple commit workflow**: staging area を露出しない。track/untrack でコミット対象を制御し、tracked ファイルの変更はデフォルトでコミットされる。
  - **Independent branches**: 「Gitless のブランチは作業中の変更を含む」ので、**未コミット変更の衝突を気にせずブランチを切り替えられる**。
  - **Friendly CLI**: コマンドが良いフィードバックを返し、次に何をすべきかを示す。
  - git 互換なのでいつでも git に戻れる。
- Kagi への示唆:
  - 「ブランチが作業中の変更を含む」は GitButler の virtual branches / jj の working-copy commit と同じ方向。Kagi は git2 単一 backend 方針(ADR-0002)なのでモデル置換は不可 → **UX 層だけで近似する**: ブランチ切り替え時に「未コミット変更をこのブランチに紐づけて退避 → 戻ったら復元」(= 明示 autostash のブランチ単位版)。
  - 「次に何をすべきかを示す」は Kagi の blocker 文言の書き方に直結: blocker は**禁止の宣言ではなく次の行動の提示**であるべき(「dirty worktree です」ではなく「先に commit / stash / 退避のどれかをします」)。
  - この論文は「git の学習困難さは慣れの問題ではなく概念設計の misfit」という**査読済みの根拠**を与える。Kagi の設計思想を文書化するときに引ける一次資料。
- 難易度: blocker 文言の行動指向への書き換え = S(ただし全 blocker の横断監査)

#### エラーメッセージ翻訳の必要性の証拠 — "Oh Shit, Git!?!"
- 出典: https://ohshitgit.com/
- 原文: 「Git は難しい: 失敗するのは簡単で、直し方を見つけるのは不可能に近い。**git のドキュメントには鶏と卵の問題がある — 問題を直すために知る必要のあるものの名前を既に知っていない限り、抜け出す方法を検索できない**」
- Kagi への示唆: この「名前を知らないと検索できない」問題は **GUI が構造的に解ける**(症状から入れる)。Kagi の undo / oplog パネルは「`git reflog` という名前を知らなくても時間を巻き戻せる」入口そのもの。UI 文言は git 用語ではなく**症状の言葉**で書き、括弧で git 用語を併記する形(「取り消す(reflog / reset)」)が学習と操作を両立する。
- 難易度: S(文言方針)

#### キーボード駆動 / モード固有ヘルプ — lazygit / gitui
- 出典: https://github.com/jesseduffield/lazygit/blob/master/docs/keybindings/Keybindings_en.md / https://github.com/extrawurst/gitui (README)
- 仕組み: lazygit のキーバインド表は**パネル別に章立て**され、各キーに「Action」と「Info(何が起きるか)」の 2 列を持つ。conflict モードの `z` のように**同じキーが文脈で意味を変える**。
- Kagi への示唆: Kagi の `shortcut_listing()` は `(label, keystroke)` のフラットな組で、**「何が起きるか」の説明列と「どのモードで有効か」が無い**。conflict editor / workspace mode / graph が別モードとして存在するので、モード別のキーヘルプ(`?` で開くオーバーレイ)は認知負荷を直接下げる。既存 `Command` 構造体に `info` フィールドを足すだけ。
- 難易度: S

---

### 2-D. アクセシビリティと国際化

#### GPUI のスクリーンリーダー対応 — **実現可能。API は存在し、依存も既に入っている**
- 何か: GPUI は AccessKit と統合し、programmatic accessibility を提供する。
- 出典:
  - https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_accessibility.rs(GPUI 公式の a11y ガイド doc、2026-09 取得)
  - 関連ファイル: `crates/gpui/src/window/a11y.rs`、`crates/gpui/src/window/a11y/debug.rs`、`crates/gpui/examples/a11y.rs`、`crates/gpui/Cargo.toml`(`accesskit.workspace = true`)、`crates/gpui_macos/Cargo.toml`(`accesskit` + `accesskit_macos`)
  - Kagi 側の裏付け: `Cargo.lock` に `accesskit 0.24.1` / `accesskit_macos 0.26.2` / `accesskit_unix 0.21.1` / `accesskit_windows 0.33.1` / `accesskit_consumer 0.36.0, 0.37.0` / `accesskit_atspi_common 0.18.1`。gpui は `git+https://github.com/zed-industries/zed#90b3aa0b3bd3b453775b11a386907c7ac9acd997`。
- 仕組み(GPUI ガイドの内容):
  - GPUI がフレームを描くとき **UI ツリーを歩いて `GlobalElementId` を持つノードを見つけ、支援技術に通知**する。`GlobalElementId` は祖先の非 `None` な `id` を合成して作られる。
  - 通知されるには **`role` が非 `None` である必要**がある。`div().id(...).role(Role::Button)` の形。**role の無いノードは支援技術に報告されない**(= 段階的に付けていける)。
  - **フレームを跨いで同じ global ID のノードは「同じノード」**と見なされる。ID が変わると「1 つ削除 + 1 つ追加」と解釈され、スクリーンリーダーが不要な読み上げをする(原文: "This can be very disorienting for users")。つまり **ID を制御することで「意味のある変化かどうか」を制御できる**。
  - `text!` マクロは ID を自動導出するが、**導出元は「そのマクロ呼び出しのソースコード上の位置」**。ループ内で 1 回書いた `text!` は全要素が同じ ID になり、リリースビルドでは**一部ノードが黙って落ちる**。回避は `text!(todo).with_id(index)` / `text(id = index, todo)` / `div().id(index).child(text!(todo))`。ID を持たせたくない場合は `Text::new_inaccessible`。
  - アクション: `div().on_a11y_action(AccessibleAction::Increment, |_extra, _window, _cx| { ... })`。**`on_click()` を付けると `AccessibleAction::Click` ハンドラが自動登録される**。`AccessibleAction` は `accesskit::Action` の re-export で、GPUI の `Action` トレイトとは無関係。
  - カスタム `Element` は `Element::a11y_role()` と `Element::a11y_synthetic_children(&mut self, prepaint, builder: &mut A11ySubtreeBuilder)` で「1 要素を複数ノードに見せる」ことができる(例: `Role::TextInput` の中に `Role::TextRun` を作り、`set_character_lengths` と `set_text_selection` でキャレット位置を伝える)。synthetic children は **prepaint 後**に追加されるので「画面に見えている範囲だけ」を判断できる。
- Kagi の現状: `src/ 配下で `.role(` / `on_a11y_action` / `a11y_role` / `Role::` の使用は **0 件**。`text!(` も 0 件。
- Kagi への示唆(見立て):
  - **技術的障壁は無い。作業量の問題だけ。** 依存は既に入っており、API は安定した公式ドキュメント付き。
  - ただし **`uniform_list` / `virtual_list` を 47 箇所使っている**(`src/` grep)ため、仮想スクロールと a11y の相性が焦点になる。`uniform_list` は「表示範囲のサブセットだけ render」する(https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/uniform_list.rs の doc comment: "will only render the visible subset of items")。つまり **スクリーンリーダーには「見えている行だけ」しか見えない**。正しい対処は親コンテナに `Role::List` + item 総数を伝えるプロパティを載せ、各行には**インデックス由来の安定 ID**を付けること(コミットグラフの行なら commit OID を ID にするのが自然で、フレーム跨ぎの同一性も正しくなる)。
  - 着手順序の推奨: (1) modal / confirm ダイアログ(**破壊的操作の二段確認が読み上げられないのは安全性の欠陥**)→ (2) ボタン・チェックボックス等の操作要素 → (3) コミットリスト・ファイルリスト(`Role::List` + 行 ID) → (4) diff ビュー(`a11y_synthetic_children` で行を `TextRun` として出す)。
  - **落とし穴を先に共有すべき**: ループ内 `text!` の ID 衝突でノードが黙って消える件は、Kagi のリスト系 UI で確実に踏む。
- 難易度: modal のみ = S、操作要素全体 = M、リスト + diff = L(段階的に切れる)

#### GPUI の reduce motion — **API はあるが OS 設定に配線されていない**
- 出典:
  - `crates/gpui/src/app.rs`: `App::reduce_motion(&self) -> bool` / `App::set_reduce_motion(&mut self, bool)`(後者は変化時に `refresh_windows()`)
  - `crates/gpui/src/elements/animation.rs`: `AnimationExt` の doc — 「このトレイト経由で描かれるアニメーションは自動的に `App::reduce_motion` を尊重する。設定されている場合、要素は静的状態(oneshot は終了状態、繰り返しは開始状態)で描かれ、**アニメーションフレームはスケジュールされない**」。spring アニメーションも `cx.reduce_motion()` を見て即 settle。テスト `test_spring_animation_respects_reduced_motion` あり。
- 確認したこと: `accessibilityDisplayShouldReduceMotion` / `should_reduce_motion` は zed リポジトリのコード検索で **0 件** → **GPUI は OS の「視差効果を減らす」設定を読んでいない**。`reduce_motion` は単なるアプリ内フラグ。
- Kagi への示唆: Kagi が macOS の `NSWorkspace.shared.accessibilityDisplayShouldReduceMotion` を読んで `cx.set_reduce_motion(true)` を呼ぶだけで、アニメーション全体が自動的に静的化する。**これは数十行**。`openlogi-learnings.md` にある `AnyClass::get(c"...")` パターン(クラス不在でも panic せず degrade)がそのまま使える。Linux 側は `org.gnome.desktop.interface enable-animations` 等の相当設定([推測]、未確認)。
- 難易度: S

#### GPUI の高コントラスト
- 確認したこと: zed のコード検索で `increase_contrast` / `high_contrast` は **0 件**。GPUI にハイコントラスト API は無い。
- Kagi への示唆: Kagi はテーマシステム(Apple HIG 準拠)を持つので、**高コントラストテーマを 1 つ追加する**のが唯一の道。GitHub も同じ解決をしている(下記)。OS の `accessibilityDisplayShouldIncreaseContrast` を読んで自動選択するところまでやると良い。
- 難易度: M(テーマ 1 本追加 + コントラスト比の検証)

#### 色以外での区別 — WCAG 1.4.1 と GitHub の色覚テーマ
- 出典:
  - https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html — SC 1.4.1 Use of Color (Level A): 「色が、情報を伝える・動作を示す・応答を促す・視覚要素を区別する**唯一の**視覚的手段になっていないこと」「色以外に形や文字などの情報を使う」
  - https://docs.github.com/en/get-started/accessibility/managing-your-theme-settings — 「低視力なら前景と背景のコントラストが強い**高コントラストテーマ**が有用。色覚特性があるなら**ライト/ダークの colorblind テーマ**が有用」。GitHub は Protanopia & Deuteranopia 用(赤緑を橙青へ置換)と Tritanopia 用(青黄)を別テーマとして提供。
  - git 側の色非依存マーカーの例: `blame.markUnblamableLines` は `*`、`blame.markIgnoredLines` は `?`(前掲 `config/blame.adoc`)
- Kagi の現状(コードで確認):
  - diff 行は **`+`/`-`/` ` の sigil を表示文字列に保持している**(`src/ui/diff_view.rs:289-302` のコメント: "The sigil (+/-/ ) at position 0 of each `text` is kept in the display string but excluded from the highlighted region")。split view も同じ `DiffRow::Line { text }` を描く(`src/ui/diff_split.rs:170-176`)。→ **diff 行については SC 1.4.1 を既に満たしている**。
  - 一方 **色のみに依存している疑いが強い箇所**: コミットグラフのレーン色(構造上、色でレーンを区別している)、`change_added` / `change_deleted` の文字色、diffstat バー、ブランチ/状態バッジ、ahead/behind 表示。
- Kagi への示唆:
  1. `--color-moved` を実装するなら **zebra の「交互色」方式は採らない**。git 自身の zebra は色の切り替わりでブロック境界を示すので、色覚特性がある人には情報が消える。Kagi は左端の縦バー(太線/破線の切り替え)か行頭マーカーで境界を出すべき。
  2. コミットグラフのレーンは色 + **線種(実線/破線)や太さ**の二重符号化を検討。ghost connector が既に「線種で意味を持たせる」前例になっている。
  3. 色覚テーマを 1〜2 本追加(GitHub と同じ橙/青の置換)。
- 難易度: 色覚テーマ = M、グラフの二重符号化 = M、move 表示の色非依存設計 = S(設計判断のみ)

#### 非 ASCII パス — `core.quotePath`
- 出典: https://github.com/git/git/blob/master/Documentation/config/core.adoc — 原文: 「パスを出力するコマンド(`ls-files`, `diff` 等)は、パス名中の "unusual" な文字をダブルクォートで囲み、C のエスケープでバックスラッシュエスケープする(`\t` 等)、**あるいは 0x80 より大きいバイト**(UTF-8 の "micro" なら 8 進 `\302\265`)。この変数を false にすると 0x80 超のバイトは unusual と見なされない」
- Kagi への示唆: Kagi は git2 なのでこの config の影響は基本的に受けない(libgit2 は生のバイト列を返す)が、**埋め込みターミナルや `gh` サブプロセス経由の出力をパースする箇所では `\302\265` 形式のエスケープが出てくる**。日本語ファイル名が化ける典型経路。サブプロセス呼び出し側で `-c core.quotePath=false` を指定するのが正しい。Kagi は日本語 UI を持つ = 日本語パスのユーザが多いはずで、ここは実利がある。
- 難易度: S

#### CJK 幅 / Han unification
- Kagi の現状(`Cargo.lock` で確認): `cosmic-text 0.19.0` が **`git+https://github.com/TomiXRM/cosmic-text?branch=fix-han-unification-0.19`** という **Kagi 作者自身のフォーク**。`unicode-width 0.2.2`、`unicode-bidi 0.3.18`、`unicode-bidi-mirroring 0.4.0`、`rustybuzz 0.20.1` が依存ツリーにある。
- Kagi への示唆: Han unification(同一コードポイントを日中韓で異なる字形で描く問題)は**既に踏んで自前フォークで対処済み**。従って CJK の残り課題は「幅」側:
  - `unicode-width` は既にあるので、**等幅前提の列揃え**(diff の行番号カラム、`format!("{:5}", n)` のようなパディング: `src/ui/diff_split.rs:164`)は CJK・絵文字混在で崩れる。`format!("{:5}")` は char 数ではなく Display 幅を見ないため、日本語コミットメッセージの列揃えが崩れる。ここは `unicode_width::UnicodeWidthStr::width()` に置き換える。
  - 絵文字(特に ZWJ シーケンス、異体字セレクタ)は `unicode-width` でも近似にしかならない。GUI なので **本質的な解は「文字数で揃えず、レイアウト(flex/固定 px)で揃える」**。
  - **フォークの維持コスト**が中長期リスク。upstream cosmic-text への PR 状況を追う必要がある([未確認])。
- 難易度: 幅計算の修正 = S、フォーク解消 = 外部依存

#### RTL / bidi と Trojan Source(安全性としての bidi)
- 出典:
  - https://trojansource.codes/ — 「Unicode 制御文字を使ってエンコーディングレベルでソースコードのトークンを並べ替える。視覚的に並べ替えられたトークンは、意味的には正しいが**論理順序で提示されるロジックとは乖離した**ロジックを表示できる。**コンパイラとインタプリタは論理順序に従い、視覚順序には従わない**」「これらの敵対的エンコーディングは**視覚的な痕跡を一切生じない**」
  - https://blog.rust-lang.org/2021/11/01/cve-2021-42574.html — CVE-2021-42574。Rust Security Response WG: 「bidirectional override コードポイントを含むソースコードでは、**レビューされたコードとコンパイルされたコードが異なる**場合がある」。rustc の欠陥ではないが予防的措置を取った。
  - GitHub は bidi 制御文字を含むファイルに警告を表示する(2021 年〜)。
- Kagi への示唆: **これは「安全性優先の git クライアント」として最も筋の通った a11y/i18n 由来の安全機能**。Kagi は `unicode-bidi` を既に依存ツリーに持つ。実装:
  1. diff 表示時に bidi 制御文字(`U+202A`〜`U+202E`, `U+2066`〜`U+2069`, `U+200E/200F`)と不可視文字(`U+200B` ZWSP, `U+00AD` SHY, `U+2060` WJ)を検出
  2. 検出したら **hunk ヘッダに警告バッジ**を出し、当該文字を可視プレースホルダ(`⟨RLO⟩` 等)で描く
  3. 同種の危険: 混在スクリプト(ラテン文字に見えるキリル文字 `а`)= homoglyph。CVE-2021-42694 の側面。
  - **これを PR review / conflict editor / diff の全経路で効かせられるのは、diff レンダラを自前で持つ Kagi の強み**。GitHub は Web だから同じことをしている。Sublime Merge / Fork / GitKraken がこれをやっているという証拠は見つからなかった([未確認] = 差別化余地)。
- 難易度: M
- RTL レイアウト(UI 全体を右→左に反転)は別問題。GPUI に RTL レイアウト機構がある証拠は見つからず([未確認])。Kagi の i18n は EN/JA のみなので**現時点では対象外で正しい**。

#### i18n の実務
- Kagi の現状: EN/JA 実装済み。`docs/adr/0129-appendix-templates.md` に「`WorktreeValidationError::Other(s)` の English-only 透過(**未キー化**)」が Phase 2 で対応予定として記録されている。
- 他クライアントの多言語対応の質(確認できた範囲):
  - git 本体のドキュメントは EN / FR / JA / PT-BR / RU / SV / UK / ZH-HANS に翻訳されている(https://git-scm.com/docs/git-blame のページ内言語リスト)。**つまり git 用語の公式な日本語訳が存在する** → Kagi の JA 訳はこれに揃えるべき(独自訳を作らない)。
  - difftastic はマニュアルを EN / zh-CN で提供(README のバッジ)。
- Kagi への示唆:
  1. **git 公式 JA ドキュメントの用語を訳語の正典にする**。`fast-forward` / `detached HEAD` / `rebase` などをどう訳す(あるいは訳さない)かの決定を、公式訳に委ねると議論が終わる。
  2. `advice.*` カタログ(§2-C)を i18n キーの網羅性チェックリストにする。
  3. 未キー化エラーの検出は CI で機械化できる(「`Other(String)` 経路に入った文字列を全部集めて未キーを落とす」)。
- 難易度: 用語整備 = S、未キー化の網羅 = M

---

### 2-E. 性能と体感

#### 巨大リポジトリ — git 側の既製最適化
- 出典:
  - https://git-scm.com/docs/scalar — 「Scalar は大規模リポジトリで git を最適化するリポジトリ管理ツール。**高度な git 設定を構成し、バックグラウンドでリポジトリを保守し、ネットワーク越しに送られるデータを削減する**ことで性能を改善する」。`scalar clone` は既定で commit と tree オブジェクトのみ clone(partial clone)、**sparse-checkout を有効化しトップレベルのファイルだけを配置**、background maintenance を構成。`scalar run (all|config|commit-graph|fetch|loose-objects|pack-files)`。
  - https://git-scm.com/docs/git-commit-graph(commit-graph ファイル: コミットの親と generation number をキャッシュして走査を高速化)
  - https://git-scm.com/docs/git-fsmonitor--daemon(ファイルシステム監視デーモンで `git status` の走査を回避)
- Kagi への示唆:
  - **Kagi がやるべきことは「これらが有効かを検出して、無効なら有効化を提案する」**。自前で高速化アルゴリズムを書くのではなく、リポジトリを開いたときに「commit-graph が無い / 古い」「fsmonitor が未設定」を検出して 1 クリックで直せるようにするのが最短。libgit2 は commit-graph を読める([推測] — git2 0.21 での対応可否は未確認)。
  - `scalar` は sparse-checkout + partial clone を前提とする = **Kagi の worktree 管理 / repo タブと組み合わせると「linux kernel を開いても軽い」を実現できる**。ただし sparse-checkout は「見えないファイルがある」状態を作るので、安全性優先の Kagi では**その事実を UI に常時表示する**必要がある(「sparse: 3/8 ディレクトリを展開中」)。
  - jj の `default_index/` は generation number をキャッシュして revset の範囲クエリを高速化している(既存 `jj-reuse-research.md` の観点 6)。Kagi が自前 lane layout を持つなら、**generation number 相当を自前 index に持つ**のが「巨大リポジトリでグラフを即描く」の核。
- 難易度: 設定検出 + 提案 = S、generation number index = L

#### 仮想スクロール — GPUI `uniform_list`
- 出典: https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/uniform_list.rs
- 仕組み(doc comment 原文):
  - 「均一な高さの要素のスクロールリスト。**taffy のフルレイアウトシステムを使わず、最初の要素だけを測って残りは全部その測定値で 1 列に配置**する。フルレイアウトシステムよりずっと速いが、均一な高さの要素にしか使えない」
  - 「`overflow-y: hidden` と固定(または最大)高のコンテナに描かれると、**見えているサブセットだけを render** する」
  - `item_to_measure_index`(どの要素で測るか)、`ListSizingBehavior`、`ScrollHandle` を持つ。
- Kagi の現状: `src/` で `uniform_list` / `virtual_list` を 47 箇所使用。**仮想スクロールは既に効いている。**
- Kagi への示唆: 性能側の残課題は uniform_list ではなく **(a) データ供給側**(コミット列挙・diff 計算)と **(b) a11y**(前述: 見えている行しかアクセシビリティツリーに出ない)。
- 難易度: —(既存)

#### 「待たせない」設計 — Sapling ISL のコマンドキュー
- 出典: https://sapling-scm.com/docs/addons/isl/
- 仕組み(原文):
  - 「`sl status` 等がバックグラウンドで自動的に走ってデータを取得するので、UI は常に最新」
  - 「**UI を操作するとコマンドが自動的にキューに積まれる。前のコマンドが実行中またはキュー待ちの間も、追加の操作を続けられる**。これは CLI でコマンドを繋げるのに似ている(`sl pull && sl rebase main && sl goto main`)。**CLI の `&&` と同様に、途中のコマンドが失敗するか merge conflict に当たれば、以降のキュー済みコマンドは全てキャンセルされる**」
  - 「**You are here** インジケータで現在のコミットを示す」、コミットのドラッグ&ドロップで rebase。
- Kagi への示唆: **これは Kagi の `plan → confirm → preflight → execute → verify → oplog` パイプラインと真正面から緊張する**。安易な楽観的 UI(実行前に結果を描く)は Kagi の安全性ブランドを壊す。取るべき形:
  - 楽観的 **表示** はしない(preflight を飛ばすことになる)
  - **キューは採る**: 実行中に次の操作を受け付け、`&&` セマンティクス(前が失敗したら以降を全キャンセル)で直列化する。これは安全性を損なわず体感を改善する唯一の道。かつ「キャンセルされた操作の一覧」を出せるので誠実。
  - 「You are here」相当は Kagi のグラフに既にあるはず([未確認])。
  - `sl status` の背景自動実行 = Kagi の watcher/reload と同種。
- 難易度: コマンドキュー = M

#### 段階的ロードと進捗の粒度
- 出典: git は `--progress` / `blame --progress`(前掲 `git-blame.adoc` の SYNOPSIS)、`advice.fetchShowForcedUpdates` は「forced update の計算に時間がかかっているとき」に通知、`advice.resetNoRefresh` は「reset の index refresh が 2 秒超のとき `--no-refresh` を教える」、`advice.statusAheadBehind` は「ahead/behind の計算が想定より長いとき」に通知(前掲 `config/advice.adoc`)。
- Kagi への示唆: **git 自身が「2 秒」という閾値を持ち、遅いときにユーザに理由と回避策を伝えている**のは強い前例。Kagi も「2 秒を超えたら『ahead/behind を計算中(大きいリポジトリでは時間がかかります)』+ スキップ選択肢」という形にできる。無言のスピナーより情報量が桁違い。
- 難易度: S

#### Sublime Merge の性能主張(参考)
- 出典: https://www.sublimemerge.com/ — 「軽快なクロスプラットフォーム GUI ツールキット、比類ないシンタックスハイライトエンジン、**カスタムの高性能 git 読み取りライブラリ**で、Sublime Merge は性能の基準を作る」/ 「Stage Files, Hunks and Lines with no waiting」
- Kagi への示唆: Sublime Merge は libgit2 でも git サブプロセスでもなく**自前の git 読み取り実装**を持つ。Kagi は git2 単一 backend 方針(ADR-0002)なので、性能で勝つには **libgit2 の呼び出しパターン最適化 + Kagi 側のキャッシュ**(既に `src/ui/diff_cache.rs` がある)で戦うことになる。

---

## 3. Kagi 取り込み候補(優先順)

| # | 提案 | 効果 | 難易度 | 依存 | 出典 |
|---|---|---|---|---|---|
| 1 | **generated file / lockfile の自動折り畳み** — `.gitattributes` の `linguist-generated` / `-diff` を尊重 + linguist ヒューリスティクス(ファイル名リスト、平均行長>110 で minified、先頭 40 行の `Code generated` / `DO NOT EDIT`)を移植 | 大きい PR の diff で認知負荷が即座に下がる。実装は判定表の移植だけ | **S** | 既存 diff renderer / git2 `git_attr_get` | [linguist generated.rb](https://github.com/github-linguist/linguist/blob/main/lib/linguist/generated.rb) / [GitHub docs](https://docs.github.com/en/repositories/working-with-files/managing-files/customizing-how-changed-files-appear-on-github) / [gitattributes.adoc](https://git-scm.com/docs/gitattributes) |
| 2 | **コマンドパレット** — 既存 `COMMANDS` 配列を fuzzy 検索 UI に露出。無効コマンドは blocker 理由付きでグレーアウト、右端に `effective_keystroke()` | 全機能の発見性 + undo の発見性 + キーバインド学習を一撃で解決。部品は全部ある | **S〜M** | `src/ui/commands.rs`(既存) | [Sublime Merge getting_started](https://www.sublimemerge.com/docs/getting_started) / [command_palette](https://www.sublimemerge.com/docs/command_palette) |
| 3 | **OS の reduce motion を `cx.set_reduce_motion()` に配線** | 数十行でアプリ全体のアニメーションが自動的に静的化。`AnimationExt` が自動で尊重する | **S** | GPUI `App::set_reduce_motion`(既存)/ `NSWorkspace` FFI | [gpui/src/app.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/app.rs) / [animation.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/animation.rs) |
| 4 | **modal / confirm ダイアログに a11y role を付ける** — `.id()` + `.role()` + `on_a11y_action` | **破壊的操作の二段確認が読み上げられない**のは安全性の欠陥。a11y の最初の一歩として最も価値が高い | **S** | AccessKit(依存済)/ GPUI a11y API | [gpui/src/_accessibility.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_accessibility.rs) |
| 5 | **oplog エントリに ID と親を付ける** — 現 `OpLogEntry` は `timestamp`/`op`/`repo`/`before`/`outcome` のみ | 「N 手前へ restore」「特定 1 操作だけ revert」の前提。スキーマ変更なので**早いほど安い** | **S** | `crates/kagi-git/src/oplog.rs` | [jj operation-log](https://docs.jj-vcs.dev/latest/operation-log/) / [but oplog](https://docs.gitbutler.com/commands/but-oplog) |
| 6 | **oplog パネル**(操作履歴一覧 + 詳細 + restore-to-point) — `read_oplog_tail()` は既にある | Kagi 最大の差別化(全操作が oplog を通る)が UI に出ていない。`git reflog` の名前を知らなくても時間を戻せる入口 | **M** | #5 | [jj operation-log](https://docs.jj-vcs.dev/latest/operation-log/) / [but oplog restore](https://docs.gitbutler.com/commands/but-oplog) / [ohshitgit](https://ohshitgit.com/) |
| 7 | **force-with-lease push / rebase / amend の confirm に `range-diff` を出す** | 「リモートの N コミットがこう置き換わる。中身の差はこれだけ」。**Kagi の存在理由(force を安全にする)に直結** | **M** | 既存 commit list + diff コンポーネント | [git-range-diff](https://git-scm.com/docs/git-range-diff) |
| 8 | **`advice.*` カタログの翻訳と in-app 説明文** — 約 40 項目を i18n キー化(`error.advice.diverging` 等)。併せてサブプロセス側で `GIT_ADVICE=0` | git2 では advice が出ないので、人間向け説明は Kagi が書く以外に存在しない。git 20 年分の「詰まる地点リスト」が既製で手に入る | **M** | 既存 i18n(EN/JA) | [config/advice.adoc](https://github.com/git/git/blob/master/Documentation/config/advice.adoc) |
| 9 | **intra-line / word diff**(tree-sitter トークン境界で行内ハイライト) | 「1 文字修正」と「行の全面書き換え」が同じ見た目、という最頻出の認知負荷を解消 | **M** | 既存 `SyntaxHighlighter`(diff で使用中) | [diff-options.adoc `--word-diff`](https://github.com/git/git/blob/master/Documentation/diff-options.adoc) / [difftastic](https://github.com/Wilfred/difftastic) |
| 10 | **move detection**(`DiffLineKind::Moved{block}` を追加、20 英数文字以上の貪欲マッチ、`allow-indentation-change` 相当) | リファクタ PR が「巨大な赤 + 巨大な緑」でなくなる。**ブロック境界は色でなく縦バーの線種で示す**(色覚配慮) | **M** | #9 と同じ diff row 拡張(セットで安い) | [diff-options.adoc `--color-moved`](https://github.com/git/git/blob/master/Documentation/diff-options.adoc) |
| 11 | **明示 autostash** — preflight が dirty を検出したら「先に退避しますか?(N ファイル)」を confirm に出し、復帰時の conflict も oplog に載せる | git 自身が「便利だが復帰時に conflict しうる」と警告している操作を、**暗黙にせず可視化する**。既存 plan/confirm/preflight にそのまま乗る | **M** | 既存 stash 実装 | [config/rebase.adoc](https://github.com/git/git/blob/master/Documentation/config/rebase.adoc) / [config/merge.adoc](https://github.com/git/git/blob/master/Documentation/config/merge.adoc) |
| 12 | **bidi / 不可視文字 / homoglyph の警告**(Trojan Source 対策)— diff / PR review / conflict editor の全経路で | **安全性優先ブランドに最も筋の通った未実装機能**。他の GUI クライアントがやっている証拠が無い = 差別化 | **M** | `unicode-bidi`(依存済) | [trojansource.codes](https://trojansource.codes/) / [Rust CVE-2021-42574](https://blog.rust-lang.org/2021/11/01/cve-2021-42574.html) |
| 13 | **`--remerge-diff` 相当** — マージコミットで「機械マージ結果 vs 実際の記録」= 人が手で入れた変更だけを見せる | コミットグラフ中心 UI で「このマージで何が起きたか」が初めて分かる。**in-memory merge(ADR-0005)が既にあるので実装可能** | **M** | 既存 in-memory merge | [git 2.36.0 RelNotes](https://github.com/git/git/blob/master/Documentation/RelNotes/2.36.0.adoc) |
| 14 | **PR review の viewed 管理 + 進捗バー**(ファイル blob SHA を保存し、**変わったら自動で unviewed に戻す**) | レビュー再開時に「どこまで見たか」を記憶から外部化。既存 PR 機能に小さく乗る | **S〜M** | 既存 PR 一覧 / diff | [GitHub reviewing docs](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request) |
| 15 | **名前付きスナップショット**(`but oplog snapshot -m` 相当)+ 「1 複合操作 = 1 oplog エントリ」の規約徹底 | 危険な rebase の前に 1 クリックのセーブポイント。stash とは意味が違う(消さずに刻む)。1エントリ規約は undo の意味を壊さないための不変条件 | **M** | #5 / #6 | [but oplog snapshot](https://docs.gitbutler.com/commands/but-oplog) / [but discard](https://docs.gitbutler.com/commands/but-discard) |
| 16 | **undo の逆操作一覧を confirm に出す + 「できないこと」の明示** | git-branchless の `Will apply these actions: ... Confirm? [yN]` 形式。Kagi の plan/confirm と同型。untracked 削除など戻せない境界を書くだけで信頼が上がる | **S** | 既存 undo(ADR-0081)| [git-branchless git undo](https://github.com/arxanas/git-branchless/wiki/Command:-git-undo) |
| 17 | **blame UI + `.git-blame-ignore-revs` の自動検出**(「N 件の revision を無視中」表示、`*`/`?` の色非依存マーカー) | Kagi に blame が実質無い。ignore-revs を尊重するクライアントは少なく、一括フォーマットで blame が死ぬ問題に直答 | **M** | 埋め込みエディタ | [git-blame.adoc](https://git-scm.com/docs/git-blame) / [config/blame.adoc](https://github.com/git/git/blob/master/Documentation/config/blame.adoc) |
| 18 | **inline blame**(埋め込みエディタの行末に作者/日付/メッセージ、可視性は設定可能) | GitLens の既定値をそのまま採用できる。埋め込みエディタを持つ Kagi の強みを活かす | **M** | #17 | [GitLens Current Line Blame](https://help.gitkraken.com/gitlens/gitlens-features/) |
| 19 | **等価 git コマンドの提示**(plan に「この操作は `git push --force-with-lease=...` に相当します」を持たせ、埋め込みターミナルへコピー) | libgit2 なので「実行したコマンド」は無いが「等価」なら誠実に出せる。GUI で学んで CLI に持っていく導線 | **M** | 全 plan 種への横断追加 | [Sublime Merge "Real Git"](https://www.sublimemerge.com/) |
| 20 | **コマンドキュー**(実行中に次の操作を受け付け、`&&` セマンティクス = 前が失敗したら以降を全キャンセル) | **楽観的 UI を採らずに**体感を改善する唯一の道(楽観表示は preflight を飛ばすので Kagi では不可)。キャンセルされた操作を列挙できるので誠実 | **M** | 既存 blocking_ops | [Sapling ISL](https://sapling-scm.com/docs/addons/isl/) |
| 21 | **色覚テーマ + 高コントラストテーマ**(GitHub と同じ赤緑→橙青の置換)。グラフのレーンは色 + 線種の二重符号化 | GPUI に高コントラスト API は無いのでテーマで解決するのが唯一の道。SC 1.4.1 Level A | **M** | 既存テーマシステム | [WCAG 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html) / [GitHub theme settings](https://docs.github.com/en/get-started/accessibility/managing-your-theme-settings) |
| 22 | **リスト系 UI の a11y**(`uniform_list` の親に `Role::List` + item 総数、各行に commit OID 由来の安定 ID) | 仮想スクロールは「見えている行だけ」を render するので、対処しないとスクリーンリーダーに全体が見えない。ID をフレーム跨ぎで安定させると余計な読み上げも消える | **M〜L** | #4 / GPUI a11y | [gpui/_accessibility.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_accessibility.rs) / [uniform_list.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/uniform_list.rs) |
| 23 | **`unicode-width` による列揃えの修正** — `format!("{:5}", n)` 系のパディングを Display 幅ベースに | 日本語コミットメッセージ・絵文字混在で列が崩れる。`unicode-width 0.2.2` は既に依存ツリーにある | **S** | `unicode-width`(依存済) | UAX#11 / `Cargo.lock` |
| 24 | **サブプロセス呼び出しに `-c core.quotePath=false`** | 日本語パスが `\302\265` 形式に化ける典型経路を封じる。JA UI を持つ Kagi では実利がある | **S** | 埋め込みターミナル / `gh` 経路 | [config/core.adoc](https://github.com/git/git/blob/master/Documentation/config/core.adoc) |
| 25 | **リポジトリ健全性の検出と提案**(commit-graph が無い/古い、fsmonitor 未設定を検出して 1 クリックで直す) | 巨大リポジトリ対策は自前高速化より「git 既製の最適化を有効にする」が最短 | **S** | git2 config API | [scalar.adoc](https://git-scm.com/docs/scalar) / [git-commit-graph.adoc](https://git-scm.com/docs/git-commit-graph) / [git-fsmonitor--daemon.adoc](https://git-scm.com/docs/git-fsmonitor--daemon) |
| 26 | **遅い処理に理由と回避策を出す**(2 秒超で「ahead/behind を計算中(大きいリポジトリでは時間がかかります)」+ スキップ) | git 自身が `advice.statusAheadBehind` / `advice.resetNoRefresh` で 2 秒閾値の前例を作っている。無言のスピナーより情報量が桁違い | **S** | — | [config/advice.adoc](https://github.com/git/git/blob/master/Documentation/config/advice.adoc) |
| 27 | **blocker 文言を「禁止の宣言」から「次の行動の提示」へ横断書き換え** + モード別キーヘルプ(`Command` に `info` 列を追加) | Gitless の "commands will give you good feedback and help you figure out what to do next"。文言方針なので安いが効果は全画面に及ぶ | **S** | 既存 blocker / `shortcut_listing()` | [gitless.com](https://gitless.com/) / [DOI 10.1145/2983990.2984018](https://doi.org/10.1145/2983990.2984018) / [lazygit keybindings](https://github.com/jesseduffield/lazygit/blob/master/docs/keybindings/Keybindings_en.md) |
| 28 | **undo の `--keep` 相当**(amend/commit の undo で working copy の編集を保持し pending changes として残す) | 「amend を取り消したいが編集は失いたくない」に直答。現状の undo は all-or-nothing | **M** | #5 / #6 | [sl undo](https://sapling-scm.com/docs/commands/undo/) |
| 29 | **undo 後のグラフプレビュー**(`sl undo --preview` 相当) | Kagi の最大の差別化(グラフ)と undo を掛け算できる。描画部品は既にある | **M** | #6 / 既存グラフ | [sl undo](https://sapling-scm.com/docs/commands/undo/) |
| 30 | **oplog 詳細に対応する reflog 行を併記** | CLI に落ちたときの接続点。Sublime Merge の "Real Git" 思想を oplog 側で実現 | **S** | #6 | [Sublime Merge](https://www.sublimemerge.com/) |

---

## 4. 取り込まないと判断したもの(理由付き)

| 対象 | 判断 | 理由 |
|---|---|---|
| **conflict UX 全般**(mergiraf の AST merge、3-way/diff3 表示、hunk 単位解決) | **本書では扱わない** | `docs/research/conflict-ux-models.md` / `conflict-ux-gui-clients.md` / `conflict-ux-editors.md` で既に調査済み。Kagi は専用 conflict editor(hunk 単位の ours/theirs/manual、diff3、abort/continue/skip)を実装済み。mergiraf(https://mergiraf.org/introduction.html)は syntax-aware merge driver として存在し difftastic 自身が推薦しているが、**merge driver は `.gitattributes` 経由で git の外側に置くもの**で、Kagi の in-memory merge を置き換える性質のもの。既存調査の範囲として扱う。 |
| **difftastic 相当のフル構造 diff**(tree-sitter で AST 差分) | **Reject(現時点)** | difftastic 自身が README で「変更が多いファイルでスケールが悪くメモリを大量消費」「side-by-side 表示が混乱を招く場合がある」「クラッシュ修正リリースが頻繁」と認めている。加えて patch を生成できない設計なので、Kagi の hunk staging(行/hunk 単位のステージング)と**根本的に非互換**。#9(intra-line トークン diff)で 8 割の価値を難易度 M で取る方が合理的。 |
| **`--color-moved=zebra` の交互色をそのまま採用** | **Reject** | git の zebra は「色の切り替わり」でブロック境界を示すため、色覚特性がある人には情報が完全に消える。WCAG 1.4.1 違反。move detection 自体は採る(#10)が、境界は縦バーの線種で示す。 |
| **暗黙の autostash**(`rebase.autoStash=true` を既定にする) | **Reject** | git 自身が「use with care: the final stash application after a successful rebase might result in non-trivial conflicts」と警告し default を `false` にしている。ユーザに黙って stash を作って黙って戻すのは、Kagi の「全書き込み操作が plan→confirm を通る」原則に反する。#11 の明示 autostash として採る。 |
| **楽観的 UI**(操作結果を実行前に描く) | **Reject** | preflight を飛ばすことになり、Kagi の安全性パイプラインを無効化する。体感改善は #20 のコマンドキュー(`&&` セマンティクス)で取る。 |
| **reflog を歩く undo UI**(Sublime Merge / lazygit 方式) | **Reject** | lazygit の doc が明記する通り reflog ベースの undo は「working tree の変更を含まず、commit だけを考慮する」。Kagi は oplog + ODB blob backup で**すでにそれより強い**。reflog UI を作ると弱い undo を並置してユーザを混乱させる。ただし #30 として oplog 詳細に reflog 対応行を併記するのは有益。 |
| **jj / GitButler のコード流用**(op store の protobuf、content-addressed object store) | **Reject** | 既存 `jj-reuse-research.md` の結論(jj-lib は gix 依存、Kagi は git2 単一 backend、protobuf + 独自 object store は MVP に過剰)を踏襲。本書は**概念採用**のみを提案する(#5 の ID/親付与は JSONL のまま実現可能)。 |
| **RTL レイアウト**(UI 全体の右→左反転) | **Reject(現時点)** | Kagi の i18n は EN/JA のみ。GPUI に RTL レイアウト機構がある証拠は見つからなかった([未確認])。一方、**bidi 制御文字の検出・警告は別問題で採る**(#12)— これは RTL 言語対応ではなくセキュリティ機能。 |
| **JetBrains Local History 相当**(エディタ保存時点の自動スナップショット) | **保留(優先度低)** | Kagi は埋め込みエディタを持つので価値はあるが、Kagi の役割は git クライアントであってエディタではない。既に discard 時 ODB backup の仕組みがあるので将来安く作れる。#1〜#30 を先に。 |
| **`git clean -n` の dry-run 表示** | **N/A** | Kagi は `git clean` を実装しない方針。`push --dry-run` 相当の ref 更新プレビューは #7 に含む。 |
| **sparse-checkout / partial clone を既定にする**(scalar 方式) | **保留** | 巨大リポジトリでは有効だが「見えないファイルがある」状態を作る = 安全性優先の Kagi では常時 UI 表示が必須で、設計負債になりやすい。#25(既製最適化の検出と提案)を先に取り、sparse は明示的なオプトインとして後回し。 |
| **自前の高性能 git 読み取りライブラリ**(Sublime Merge 方式) | **Reject** | ADR-0002(git2 単一 backend)に反する。性能は libgit2 呼び出しパターンの最適化 + 既存 `diff_cache.rs` + #25 で戦う。 |

---

## 5. 未解決の疑問

1. **git2 0.21 の blame API に ignore-revs 相当があるか**(#17 の実装形が「自前の再帰的付け替え」か「オプション一発」かで難易度が S〜M 変わる)。`git_blame_options` に `oldest_commit`/`newest_commit` はあるが ignore-revs は未確認。
2. **git2 0.21 が commit-graph を読むか**(#25 で「有効化を提案する」意味があるのは Kagi 自身も恩恵を受ける場合)。libgit2 の commit-graph 対応状況は未確認。
3. **`uniform_list` と AccessKit の実際の噛み合い**。GPUI の a11y ツリー構築が「render された要素」だけを歩くのは doc から明らかだが、`Role::List` の親に「実際の item 総数」を伝える公式手段(accesskit の `set_size` / `set_position_in_set` 相当)が GPUI 経由で使えるかは未確認。`crates/gpui/src/window/a11y.rs` の実装を読む必要がある。
4. **Kagi の preflight が dirty worktree をどう扱っているか**(#11 の出発点)。blocker として拒否しているのか、そもそも許しているのかを `src/git/` の preflight 実装で確認していない。
5. **`TomiXRM/cosmic-text` フォーク(`fix-han-unification-0.19`)の upstream 状況**。フォーク維持コストが中長期リスク。upstream PR の有無・見込みは未確認。
6. **他 GUI クライアントの bidi / Trojan Source 対応**。Sublime Merge / Fork / GitKraken / GitHub Desktop がこれをやっている証拠は見つからなかったが、「やっていない」ことの積極的証明もできていない(#12 の差別化主張の強度に関わる)。
7. **Linux 側の reduce motion / high contrast 設定の読み方**(#3 / #21)。`org.gnome.desktop.interface enable-animations` 等が候補だが、GNOME 以外の DE を含めた妥当な検出方法は未確認。
8. **GitHub API に PR の viewed 状態を書く公式手段があるか**(#14)。無いという前提でローカル保存を提案したが未確認。無ければ Kagi 独自のローカル状態になり、他デバイスと同期しない旨を UI に書く必要がある。
9. **`.claude/worktrees/agent-*/src/ui/oplog_panel.rs` の存在**。本調査中、エージェント作業ツリーに `oplog_panel.rs` が存在することを確認した(main には無い)。#6 が並行して進行している可能性があり、Main で重複を確認すべき。
