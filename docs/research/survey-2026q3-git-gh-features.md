# git 本体 と GitHub / gh CLI の新機能サーベイ

調査日: 2026-09-03 / 担当スライス: git 本体（2.40〜2.55）と GitHub / gh CLI

**調査環境（実測）**: ローカル `git version 2.50.1 (Apple Git-155)` / `gh version 2.97.0 (2026-07-31)`。
**最新版（一次情報で確認）**: git **2.55**（2026-06-29, https://github.com/git/git/releases/tag/v2.55.0）/ gh CLI **2.99.0**（2026-09-01, https://github.com/cli/cli/releases/tag/v2.99.0）。

GitHub API に関する記述の多くは、`gh api` / `gh api graphql` によるライブ・イントロスペクションで実地検証している（該当箇所に「実測」と記載）。

### git リリース日一覧（`gh api /repos/git/git/git/tags` で実測）

| ver | date | ver | date | ver | date |
|---|---|---|---|---|---|
| 2.34 | 2021-11-15 | 2.44 | 2024-02-23 | 2.51 | 2025-08-18 |
| 2.35 | 2022-01-24 | 2.45 | 2024-04-29 | 2.52 | 2025-11-17 |
| 2.36 | 2022-04-18 | 2.46 | 2024-07-29 | 2.53 | 2026-02-02 |
| 2.38 | 2022-10-02 | 2.47 | 2024-10-06 | 2.54 | 2026-04-20 |
| 2.41 | 2023-06-01 | 2.48 | 2025-01-10 | 2.55 | 2026-06-29 |
| 2.42 | 2023-08-21 | 2.49 | 2025-03-14 | | |
| 2.43 | 2023-11-20 | 2.50 | 2025-06-16 | | |

---

## 1. サマリ

Kagi に効く上位5点（すべて Kagi の既存機能とは重複しない新規要素）:

1. **`git history` (2.54, experimental)** — `drop`/`fixup`/`reword`/`split <commit>` を **`--dry-run` で「ref 更新プランのみ」出力**でき、hooks を一切実行せず、中断状態を持たない履歴改変プリミティブ。Kagi の `plan → confirm → preflight → execute → verify → oplog` に構造的に 1:1 対応する。最優先。
2. **`GET /repos/{o}/{r}/rules/branches/{branch}`（実効ルール取得, `repo` scope のみ・admin 不要）** — 全 23 種のルール型のうち **13 種は完全にローカルで検証できる**（`commit_message_pattern` / `commit_author_email_pattern` / `branch_name_pattern` / `max_file_size` / `file_extension_restriction` / `file_path_restriction` / `max_file_path_length` / `required_signatures` / `required_linear_history` / `non_fast_forward` / `creation` / `update` / `deletion`）。**1 度取得してキャッシュすれば、ネットワーク往復ゼロで「push が拒否される変更」を commit 前に止められる**。しかも「ブランチは存在しなくてもよい」ので**ブランチ作成前**の名前検証もできる。Kagi の preflight を「ローカル安全性」から「リモート契約の事前検証」へ拡張する唯一のエンドポイント。
3. **`git repo info -z` (2.52-2.54) と `git last-modified -z` (2.52)** — Kagi が現在おそらく複数コマンドや human 向け出力のパースで得ている情報（bare/shallow/object.format/references.format/commondir）を **NUL 区切りの機械可読形式で一発取得**。`last-modified` は Analyze の file-history を per-file `git log` の N 回呼びから 1 回のツリー走査に置換。
4. **`git log --graph-lane-limit=<n>` / `--graph-indent` / `log.graphIndent` (2.55)** — git 本体が Kagi と同じ「レーン爆発」「visual root の区別」問題に到達した。**upstream が採った設計（超過レーンを `~` で切る、根なしコミットをインデントで区別）は Kagi のレーン安定化アルゴリズムの検証材料**になる。
5. **`gh pr checkout --worktree PATH` (2.98) / `gh issue develop --checkout --worktree PATH` (2.99)** — gh CLI が**ネイティブに worktree を第一級市民として扱い始めた**。Kagi の worktree 管理と GitHub 連携の接続点が公式化された。加えて 2.99 は `gh pr merge --delete-branch` の linked-worktree 事故と `gh repo sync` の worktree 破損を修正済み（= Kagi が `gh` を呼ぶ場合の前提バージョンが上がる）。

---

## 2. 詳細

### 【git 本体】

#### G-1. `git history`（履歴改変, EXPERIMENTAL）
- **何か**: `drop` / `fixup` / `reword` / `split` の 4 サブコマンドで、特定コミットを狙って履歴を書き換える新 builtin。`rebase` より「意見の強い（opinionated）」単発操作。
- **出典**: 2.54 で `drop`/`reword`/`split` 導入（https://github.com/git/git/blob/master/Documentation/RelNotes/2.54.0.adoc「"git history" history rewriting (experimental) command has been added」/「"git history" learned the "split" subcommand」）、2.55 で `fixup` 追加（同 2.55.0.adoc「"git history" learned "fixup" command」）。仕様: https://github.com/git/git/blob/master/Documentation/git-history.adoc
- **仕組み**（doc 実読）:
  - `git history drop <commit> [--dry-run] [--update-refs=(branches|head)] [--empty=(drop|keep|abort)]`
  - `--dry-run`: 「**ref を一切更新せず、`git update-ref` が消費できる形式で ref 更新を印字する。必要な新オブジェクトはリポジトリに書き込まれるので、印字された ref 更新を後から適用するのは一般に安全**」。
  - `--update-refs=branches`（既定）: 元コミットの子孫を指す**すべてのローカルブランチ**を書き換え。`head` なら HEAD のみ。
  - **hooks を実行しない**（明記）。**bare repo で動く**（`fixup` のみ index を読むため例外）。
  - **設計上、コンフリクトし得る操作をサポートしない**（「history rewrites are not intended to be stateful operations」）。マージを含む履歴は非対応（`rebase --rebase-merges` を使う）。
- **Kagi への示唆**: 決定的。Kagi の `plan → confirm → preflight → execute → verify → oplog` は今まで自前で組み立てるしかなかったが、`--dry-run` の出力が**そのまま plan / preflight のデータ**になり、実行は `git update-ref --stdin` に流すだけ。しかも「中断状態を作らない」「hooks を撃たない」という性質は Kagi の安全性優先の思想と完全一致（rebase 中断状態の面倒を負わない）。`split` は Kagi にとって新しいユーザー価値（コミット分割 UI）。
- **難易度**: **M**（1 機能分）。ただし experimental かつ git 2.54+ 必須なので、バージョンゲートと fallback（rebase -i 相当）の設計が必要 → その分は L 寄り。

#### G-2. `git replay`（bare 対応バッチ rebase, EXPERIMENTAL）
- **何か**: worktree と index に触らずコミット列を別ベースへ replay。サーバサイド用途を想定して導入。
- **出典**: 導入 2.44（RelNotes 2.44.0.adoc「Introduce "git replay", a tool meant on the server side without working tree to recreate a history」）。以降 2.53「ref 更新を既定でトランザクションとして自分で行う」、2.54「空になったコミットを drop」「`revert` モード追加」「root commit まで replay 可能」。仕様: https://github.com/git/git/blob/master/Documentation/git-replay.adoc
- **仕組み**: `git replay ([--contained] --onto=<newbase> | --advance=<branch> | --revert=<branch>) [--ref=<ref>] [--ref-action=<mode>] <revision-range>`
  - 既定で**アトミックな ref トランザクション**（全部更新か全部失敗）。
  - `--ref-action=print` で自動更新せず、`git update-ref --stdin` に流せる更新コマンドを取得。
  - `--onto` は `rebase --update-refs` 相当で範囲内の複数ブランチを更新。`--contained` は範囲内コミットを指す全ブランチを更新。
  - `--revert=<branch>` は `git revert` 相当だが worktree を更新しない。
- **Kagi への示唆**: **worktree を汚さないバッチ rebase**。Kagi は複数 worktree / repo タブを持つので、「他 worktree のブランチを、そこを checkout せずに rebase する」が可能になる。`--ref-action=print` は G-1 と同じく preflight プリミティブ。`--revert` は「worktree をクリーンなまま revert コミットを積む」に使える（Kagi の二段確認と相性が良い）。
- **難易度**: **M**。

#### G-3. reftable ref backend
- **何か**: 大量 ref 環境向けのバイナリ ref ストレージ形式。loose ref ファイル + packed-refs を置換。
- **出典**: refs フレームワークへの統合が 2.45（RelNotes 2.45.0.adoc「Integrate the reftable code into the refs framework as a backend. With "git init --ref-format=reftable", hopefully it would be a lot more efficient to manage a repository with many references」）。**2.51 で「成熟した。Git 3.0 では新規リポジトリの既定形式になる」と宣言**（RelNotes 2.51.0.adoc「The reftable ref backend has matured enough; Git 3.0 will make it the default format in a newly created repositories by default」）。2.52 で整合性チェック強化（`kn/reftable-consistency-checks`）。2.54 で「ref backend がデータを置くディレクトリを指定可能に」。
- **仕組み**: `extensions.refStorage=reftable`（`git init --ref-format=reftable`）。`.git/refs` の代わりに `.git/reftable/` にテーブル群。`git repo info references.format` で現在の形式を取得可能（G-4 参照）。
- **Kagi への示唆**: **Kagi が `.git/refs` や `.git/packed-refs` を直接読んでいる箇所があれば Git 3.0 で全滅する**。これは機能追加ではなく**互換性リスク**。対応は「ref は必ず `git for-each-ref` / `git refs list` 経由で読む」に統一すること。逆に、reftable 環境では ref 走査が速いので Kagi のブランチ一覧更新レイテンシが改善する。
- **難易度**: **S**（既に libgit2/git CLI 経由なら 0。直読み箇所の掃除のみ）。ただし直読みしていた場合は M。

#### G-4. `git repo info` / `git repo structure`（新コマンド, EXPERIMENTAL）
- **何か**: リポジトリのメタデータと構造統計を機械可読形式で返す新 builtin。
- **出典**: `git repo` 導入 2.52（RelNotes 2.52.0.adoc「A new subcommand "git repo" gives users a way to grab various repository characteristics」「"git repo structure", a new command」「"git repo info" learns the short-hand option "-z" ...and learns to report the objects format」）、2.53「`--all` オプション」「`repo struct` も `-z`」「ODB 情報を追加」、2.54「`--keys` で既知キー一覧」「structure が各種最大値を報告」。仕様: https://github.com/git/git/blob/master/Documentation/git-repo.adoc
- **仕組み**（doc 実読）:
  - `git repo info [--format=(lines|nul) | -z] [--all | <key>...]`
  - `nul` 形式は「**値が決してクォートされない。他アプリケーションからのパースに適する**」と明記。`-z` はそのエイリアス。
  - キー: `layout.bare`, `layout.shallow`, `object.format`, `references.format`, `path.commondir.absolute`, `path.commondir.relative`, `path.gitdir.absolute`, `path.gitdir.relative`。`--keys` でキー一覧を列挙可（前方互換な feature detection ができる）。
  - `git repo structure` は ref 数（型別）、到達可能オブジェクト数/inflate サイズ/ディスクサイズ（型別）、型別最大オブジェクトを返す。
- **Kagi への示唆**: 二重に効く。(a) **repo タブを開くときの初期化パスが 1 コマンドに集約**され、`core.quotePath` のクォート解除ロジックが不要になる（`-z` は無クォート保証）。特に `references.format` を見れば reftable 環境を検出できる（G-3 の対策と直結）。(b) `git repo structure` は Kagi の Analyze に**新しい軸「リポジトリ健全性」**を追加できる（巨大 blob の特定、ref 爆発の検出）。既存の hotspots/coupling/ownership と重複しない。
- **難易度**: **S**（info の置換）/ **M**（structure を Analyze に組み込む）。

#### G-5. `git last-modified`（新コマンド, EXPERIMENTAL）
- **何か**: 各パス（ファイルおよびサブディレクトリ）を最後に変更したコミットを一括で出す新 builtin。
- **出典**: 2.52（RelNotes 2.52.0.adoc「A new command "git last-modified" has been added to show the closest ancestor commit that touched each path」）。仕様: https://github.com/git/git/blob/master/Documentation/git-last-modified.adoc
- **仕組み**: `git last-modified [--recursive] [--show-trees] [--max-depth=<depth>] [-z] [<revision-range>] [[--] <pathspec>...]`。リネームとモード変更も考慮する。`-z` で NUL 終端。既定 `--max-depth=0` はサブツリーに降りずに pathspec 一致パスのみ。
- **Kagi への示唆**: **これは「ファイルツリー画面に各行の最終更新コミットを出す」ための専用コマンド**。GitHub の repo ブラウザと同じ体験を、従来なら N ファイル × `git log -1 -- <path>` で実現するしかなかったものが 1 コマンドになる。Kagi の Analyze の file history とは別物（あちらは 1 ファイルの通史、こちらは全パスの最新点）。埋め込みエディタのファイルツリーに効く。
- **難易度**: **S**。

#### G-6. `git log --graph-lane-limit` / `--graph-indent`（グラフ描画）
- **何か**: `--graph` の出力レーン数上限と、visual root のインデント区別。
- **出典**: 2.55（RelNotes 2.55.0.adoc「The graph output from commands like "git log --graph" can now be limited to a specified number of lanes, preventing overly wide output in repositories with many branches」）。オプション仕様: https://github.com/git/git/blob/master/Documentation/rev-list-options.adoc（実読）
- **仕組み**（doc 実読）:
  - `--graph-lane-limit=<n>`: 「上限を超えたレーンは切り詰め記号 `~` で置換される。既定 0（無制限）、0 と負値は無制限として無視」。
  - `--graph-indent` / `--no-graph-indent`（既定有効）+ `log.graphIndent`: 「visual root（親を持たない、または親が表示されていないコミット）をインデントして、垂直に隣接するが無関係なコミットと区別する」。
- **Kagi への示唆**: Kagi はコミットグラフ中心の製品なので、**upstream が同じ問題に到達し、どう解いたかが直接の設計レビュー材料**。特に「親が表示範囲外のコミット（= ページング境界）を視覚的に区別する」は Kagi のグラフでも起きる問題で、upstream は「インデント」を答えにした。既存の ghost connector とは別の課題（あちらは squash merge 検出、こちらは表示範囲の境界表現）。**取り込むのは実装ではなく設計判断**。
- **難易度**: **S**（境界インデントの導入）。レーン上限は Kagi のレーン安定化と衝突しうるので慎重に。

#### G-7. `git maintenance`（背景メンテ）
- **何か**: `gc` を分解した背景メンテナンス機構。スケジューラ登録を含む。
- **出典**:
  - タスク一覧（ローカル `git help maintenance` 実測）: `commit-graph`, `prefetch`, `gc`, `loose-objects`, `incremental-repack`, `pack-refs`, `worktree-prune`。
  - 2.43「hourly 等のスケジュールがランダム分散」、2.46「1 個エラーでも他リポジトリを続行」、2.47「gc 以外のタスクも正しく background 化」「background タスクが credential helper で UI ブロックしないように」、2.50「loose-objects のバッチサイズ設定可」「reflog expire タスク追加」「gc のクリーンアップタスクを maintenance でも利用可能に」。
  - **2.52「`geometric` ストラテジ追加（全再構築を伴うタスクを避ける）」**（RelNotes 2.52.0.adoc）、**2.53「`is-needed` サブコマンド追加（メンテが必要かを判定）」**（RelNotes 2.53.0.adoc）、**2.54「`geometric` ストラテジが既定に」**（RelNotes 2.54.0.adoc）。
  - 2.51「ref packing 中にリポジトリロックを長時間保持しない改善」、2.55「background 時のロックファイルで多重起動を防止」。
- **Kagi への示唆**: **`git maintenance is-needed` (2.53) が要**。Kagi は GUI なので「勝手に重いメンテを走らせて UI を固める」ことが最悪の体験。`is-needed` を使えば「メンテが必要です。実行しますか？」という**Kagi の既存の plan→confirm パターンに乗せた提案 UI** が作れる（勝手に走らせない）。`geometric` が既定になったので実行コストも読める。2.55 のロック修正により Kagi が maintenance を叩いても多重起動事故が起きない。
- **難易度**: **S**（is-needed 判定 + 提案バナー）/ **M**（設定 UI 込み）。

#### G-8. commit-graph と changed-path Bloom filter
- **何か**: コミットグラフのキャッシュと、パス限定 traversal を加速する Bloom filter。
- **出典**:
  - **2.51「`git log` の changed-path filter を、複数のリテラルパスを持つ pathspec でも使えるよう制限を撤廃」**（RelNotes 2.51.0.adoc）。
  - **2.52「`commitGraph.changedPaths` 設定で `git commit-graph` の `--changed-paths` を既定 ON にできる」**、**2.52「`git log dir/*` のようなワイルドカード付き pathspec でも Bloom filter を活用するようになった」**（RelNotes 2.52.0.adoc）。
  - 2.55「commit-graph からの tree lazy-load が、commit-graph 消失時にコミットオブジェクト読みへフォールバックしてロバストに」。2.54/2.55 でタイムスタンプ overflow バグ修正（2514年 / 2106年）。
  - 2.55「commit-graph を OFF にする影響のドキュメント整備」（`kh/doc-commit-graph`）。
- **仕組み**: `git commit-graph write --changed-paths` で Bloom filter を書く（または `commitGraph.changedPaths=true`）。以降 `git log -- <path>` がフィルタで大半のコミットをスキップ。
- **Kagi への示唆**: Kagi の Analyze（hotspots / coupling / ownership / file history）と**グラフのパス絞り込みが全部これに乗る**。特に 2.51/2.52 の緩和で「複数パス」「ワイルドカード」でも効くようになったので、Kagi の「複数ファイルを選んで履歴を見る」系 UI が実用速度に入る。**やるべきは「commit-graph が無い/古いリポジトリを検出して、書き込みを提案する」**（G-7 の is-needed と同じ提案パターン）。
- **難易度**: **S**（検出 + 提案）。

#### G-9. `core.fsmonitor` 内蔵デーモン
- **何か**: git 内蔵のファイルシステム監視デーモン。`git status` の worktree スキャンを省略。
- **出典**: **2.55「fsmonitor daemon が Linux 向けに実装された」**（RelNotes 2.55.0.adoc「The fsmonitor daemon has been implemented for Linux」）。それ以前は macOS/Windows のみ。macOS 側のバグ修正: 2.45（icase の corner case, `jh/fsmonitor-icase-corner-case-fix`）、2.47（submodule 絡みで無限ハング）、2.48（macOS の race で client が永久待機）。2.54 で `fsmonitor-watchman` サンプルフックの typo 修正。
- **仕組み**: `core.fsmonitor=true` で内蔵デーモン（`git fsmonitor--daemon`）を使う。
- **Kagi への示唆**: **Kagi は macOS と Linux の両対応なので、2.55 で初めて「両プラットフォームで内蔵 fsmonitor が使える」状態になった**。Kagi はおそらく自前の file watcher を持っているが、**`git status` 側のコストは fsmonitor でしか下がらない**（Kagi の watcher は「変更があった」を知るだけで、git 側の index refresh は依然フルスキャン）。大規模リポジトリでの status レイテンシに直接効く。ただし 2.45〜2.48 の macOS バグ履歴が示すように、**Kagi が勝手に有効化すべきではない**。「有効化を提案する」に留めるのが安全側。
- **難易度**: **S**（検出 + 提案）。有効化を Kagi 側から書く場合は preflight/oplog に載せる必要があるので M。

#### G-10. partial clone / sparse-checkout / sparse-index / `git backfill`
- **何か**: 巨大リポジトリ向けの部分取得群。
- **出典**:
  - sparse-index の各コマンド対応が 2.41〜2.46 で継続（2.41「`git write-tree`」「`git diff-files` が不要な展開を避ける」、2.42「`git worktree` が sparse index と協調」、2.43「`git check-attr`」、2.46「sparse checkout 外の worktree cruft を扱う」）。
  - **2.49「`git backfill` 導入。blobless clone 後の一括ダウンロード（1 blob ずつになりがちな問題への対策）」**（RelNotes 2.49.0.adoc）、**2.54「`git backfill` が revision と pathspec 引数を受け付ける」**（RelNotes 2.54.0.adoc）。
  - **2.52「`git sparse-checkout` に `clean` アクション追加（関心領域外の未使用 worktree ファイルを刈る）」**（RelNotes 2.52.0.adoc）。
  - 2.52「promisor-remote capability が `partialCloneFilter` 設定と `token` 値をサーバ側から伝達できるよう更新」、2.54「大オブジェクト promisor remote の auto filter ロジック」、2.55「`pack-objects --path-walk` が blobless/sparse フィルタと統合」。
- **Kagi への示唆**: Kagi は「安全性優先」を掲げるので、**partial clone 環境で「このファイルの blob はローカルに無い」状態を UI で正しく表現する**必要がある（黙ってネットワークフェッチして固まるのが最悪）。`git backfill --revision --pathspec` (2.54) は「今から見る範囲だけ先に落とす」ができるので、**Kagi の diff 表示前 prefetch** に使える。`sparse-checkout clean` (2.52) は破壊的操作なので、Kagi の二段確認 + ODB バックアップの対象として自然（`git clean` を意図的に持たない Kagi にとって、**「安全な clean」として提供できる唯一の候補**）。
- **難易度**: **M**（partial clone 状態の UI 表現）/ **S**（backfill prefetch）。

#### G-11. bundle-uri（clone 高速化）
- **何か**: サーバが bundle の URI を広告し、client がまず bundle を落として差分だけ fetch する clone 高速化機構。
- **出典**: **2.50「Bundle-URI 機能が bundle 内の通常ブランチ以外の ref もアンカーとして使い、clone 後の follow-up fetch を最適化するようになった」**（RelNotes 2.50.0.adoc）。2.51「debug 出力を fp へ」（`jm/bundle-uri-debug-output-to-fp`）、2.53「URI エントリを欠く不正な bundle-URI をクラッシュせず診断」（`sb/bundle-uri-without-uri`）。
- **仕組み**: `git clone --bundle-uri=<uri>` またはサーバ広告（`transfer.bundleURI`）。
- **Kagi への示唆**: 効果は clone 時のみで、Kagi の主戦場（既存リポジトリの操作）には効かない。**取り込み優先度は低い**が、2.53 の「不正 bundle-URI で crash しない」修正が示すように、**Kagi が clone をラップするなら bundle-uri 由来のエラーを握れる程度の認識は必要**。
- **難易度**: **S**（が、価値も小）。

#### G-12. diff 系: `--anchored`, `diff.algorithm`, `zdiff3`, `merge.conflictStyle`
- **何か**: diff アルゴリズム選択とコンフリクト表示形式。
- **出典**:
  - `zdiff3`（zealous diff3）: **2.35**（RelNotes 2.35.0.adoc「"Zealous diff3" style of merge conflict presentation has been added」, 2022-01-24）。`merge.conflictStyle=zdiff3`。
  - **2.54「`git diff --anchored=<text>` が最適化された」**（RelNotes 2.54.0.adoc）。
  - **2.53「`git blame` が `--diff-algorithm=<algo>` を学んだ」**（RelNotes 2.53.0.adoc）。
  - 2.44「`git merge-file` が `--diff-algorithm` を受ける」。2.53「`git apply` と `git diff` が新 whitespace エラークラス `incomplete-line` を学んだ」。2.53「`--quiet` は変更の有無だけ気にするので rename/copy 検出を無効化して高速化」。
  - 2.50「`--minimal` が最適化発動時に非最小出力になっていたのを是正」。2.55「Myers diff で共通 prefix/suffix 除去を勘案せず無駄に確保していたメモリを縮小」「`git diff` のオプションパーサが非負整数のみ取るオプションを認識」。
  - histogram を既定化する議論は 2.40〜2.55 のリリースノートに**変更として記載されていない**（既定は依然 Myers）。
- **仕組み**: `diff.algorithm=(myers|minimal|patience|histogram)`。`--anchored=<text>` は指定テキスト行を移動として扱わせないアンカー（patience 系）。
- **Kagi への示唆**: (a) **`git blame --diff-algorithm` (2.53) が Kagi の Analyze/ownership の精度に直接効く** — histogram にすると「移動したコードを別人の追加として誤帰属する」ケースが減る。ownership の質が上がるのはユーザーに見える改善。(b) `zdiff3` は Kagi の conflict editor が既に diff3 を持っているので、**選択肢として zdiff3 を追加する（既存 diff3 の隣に置く）だけ**。zdiff3 は共通行を conflict marker の外に出すので、コンフリクト領域が小さく読みやすくなる。既存の diff3 実装への追加なので重複ではない。
- **難易度**: **S**（blame の algorithm 指定 + zdiff3 選択肢）。

#### G-13. `git range-diff` と `git log --remerge-diff`
- **何か**: `range-diff` はパッチ列同士の差分。`--remerge-diff` はマージコミットの「機械的マージ結果と実際に記録された結果の差」= マージ時に人が手で入れた解決内容。
- **出典**:
  - `--remerge-diff` 導入: **2.36**（RelNotes 2.36.0.adoc「"git log --remerge-diff" shows the difference from mechanical merge result and the result that is actually recorded in a merge commit」, 2022-04-18）。バグ修正: 2.47（`rev-list | diff-tree -p --remerge-diff --stdin` の crash）、2.48（`log -p --remerge-diff --reverse` が完全に壊れていた, `js/log-remerge-keep-ancestry`）。
  - `range-diff`: **2.52「O(N*N) コスト行列のメモリ消費を制限する方法を学んだ」**（RelNotes 2.52.0.adoc）。2.52「`format-patch --range-diff=... --notes=...` が underlying range-diff に正しい `--notes` を渡していなかったのを修正」。2.55「range-diff が notes を取るという doc 修正」（`sp/doc-range-diff-takes-notes`）。
- **Kagi への示唆**: **`--remerge-diff` は Kagi のマージコミット表示の答え**。Kagi はグラフ中心なのでマージコミットをクリックする頻度が高いが、通常の diff ではマージコミットは「何も変わっていない」か「巨大なノイズ」になる。`--remerge-diff` は「**このマージで人間が実際に何を判断したか**」だけを見せる。Kagi の conflict editor で解決した内容の事後レビューにもなる（自分の解決を後から検証できる）。`range-diff` は「rebase 前後で本当に同じか」の検証 = **Kagi の rebase の verify フェーズで使える**（oplog の undo 判断材料）。
- **難易度**: **S**（remerge-diff をマージコミット表示に追加）/ **M**（rebase verify への range-diff 組込み）。

#### G-14. rerere と `git rebase --update-refs` / `--trailer`
- **何か**: `rerere` はコンフリクト解決の記録・再利用。`--update-refs` は stacked branch を rebase 時に一括追従させる。
- **出典**:
  - `rebase --update-refs` 導入: **2.38**（RelNotes 2.38.0.adoc「rebased range with "--update-refs" option」, 2022-10-02）。**2.55 でバグ修正: `rebase.instructionFormat` に `%d`(describe) が含まれると誤ってローカルブランチ HEAD を更新しようとしていた**（RelNotes 2.55.0.adoc, `ag/rebase-update-refs-limit-to-branches`）。
  - **2.54「`git rebase` が `--trailer` オプションを学び、interpret-trailers 機構を駆動する」**（RelNotes 2.54.0.adoc）。
  - rerere: 2.54 で strbuf ハンドリングの近代化（`jc/rerere-modern-strbuf-handling`）。機能追加は 2.40〜2.55 のリリースノートに記載なし（安定機能）。
- **Kagi への示唆**: (a) **`--update-refs` は Kagi が stacked branch を扱うなら必須**だが、2.55 未満では `rebase.instructionFormat` に `%d` があるとブランチを壊すバグがある → **Kagi は `--update-refs` を使うとき `rebase.instructionFormat` を自前で上書きして無害化するべき**（これは preflight の具体的なチェック項目になる）。(b) `rebase --trailer` (2.54) は **AI 生成コミットの `Co-Authored-By:` を rebase 越しに一貫して付与できる**（G-15 参照）。(c) **rerere は Kagi の conflict editor に載せる価値が高い**が既存機能の拡張。「同じコンフリクトを前も解決した」を検出して提案する形なら新規価値（既存 conflict editor に対する追加であり重複ではない）。
- **難易度**: **S**（instructionFormat 無害化 = preflight 1 項目）/ **M**（rerere 提案 UI）。

#### G-15. `git interpret-trailers` と trailer 操作（AI 生成コミットの帰属）
- **何か**: コミットメッセージの trailer（`Signed-off-by:`, `Co-Authored-By:` 等）を機械的に読み書きするコマンドと `--format` 側の trailer atom。
- **出典**: **2.54「`git rebase` が `--trailer` オプションを学び、interpret-trailers 機構を駆動する」**（RelNotes 2.54.0.adoc）。2.54 で doc 整備（`kh/doc-interpret-trailers-1`）。`git log --format=%(trailers:...)` は既存機能。
- **仕組み**:
  - 読み: `git log --format='%(trailers:key=Co-authored-by,valueonly=true)'` で trailer 値だけ抽出。`--format=%(trailers:only=true,unfold=true)` で折返し解除。
  - 書き: `git interpret-trailers --trailer "Co-authored-by: X <x@y>" --in-place <file>`。`trailer.<token>.*` 設定で正規化ルールを定義可。
  - `git rebase --trailer` (2.54) で rebase 中の全コミットに trailer を付与。
- **Kagi への示唆**: **AI native の中核**。実際に流通している AI 帰属 trailer（実測・後述 GH-13）は `Co-authored-by: Copilot <copilot@github.com>`。Kagi は `%(trailers:key=Co-authored-by)` で**グラフ上のコミットに「AI 関与」バッジを出せる**。既存の Analyze/ownership は author/committer ベースなので、**trailer ベースの「AI 共著率」は完全に新しい軸**。さらに `interpret-trailers` を使えば Kagi の commit UI で「AI 支援を受けた」チェックボックス → trailer 自動付与ができる（手打ちさせない）。`rebase --trailer` で後付け一括付与も可能。
- **難易度**: **S**（読み取り + バッジ）/ **M**（Analyze への AI 帰属軸追加 + commit UI での付与）。

#### G-16. `git worktree` 周辺の全改善
- **何か**: Kagi の中核である worktree の周辺整備。
- **出典**（機能と修正を分離して列挙）:
  - `worktree add --orphan`: **2.42**（RelNotes 2.42.0.adoc「'git worktree add' learned how to create a worktree based on an orphaned branch with `--orphan`」, 2023-08-21）。
  - `worktree list --porcelain -z`: **2.36**（RelNotes 2.36.0.adoc「"git worktree list --porcelain" did not c-quote pathnames and lock reasons with unsafe bytes correctly, which is worked around by introducing NUL terminated output format with "-z"」, 2022-04-18）。**= `-z` 無しの porcelain はパスに危険バイトがあると壊れる。Kagi は必ず `-z` を使うべき。**
  - `worktree move` / `worktree repair` / `worktree lock --reason` / `worktree prune --expire`: ローカル `git worktree -h` 実測（2.50.1）で全て存在を確認。
  - `extensions.worktreeConfig`: 2.20 で導入（本サーベイ範囲外）。**2.42「`config.worktree` の値はリポジトリ毎なのにプロセス毎のシングルトングローバル変数に保持されていた。再帰 grep のように複数リポジトリを同時に触る操作では正しくない」修正**（RelNotes 2.42.0.adoc, `vd/worktree-config-is-per-repository`）。
  - 2.41「別 worktree で checkout 済みのブランチで作業するのを止める仕組みを複数サブコマンドに追加」「別 worktree で checkout されている場合のメッセージ改善」「`git fsck` が他 worktree の index も検査」。
  - 2.42「ブランチ X が別 worktree で bisect / rebase 中なのに『checked out』と言っていたのを『in use』に改めた」「`git worktree` が sparse index 機能とより良く動作」。
  - 2.53「`git worktree list` が非 ASCII パスの表示カラム数を誤カウントして整列が崩れていたのを修正」（`pw/worktree-list-display-width-fix`）。
  - 2.54「`git worktree [list|prune]` の `--expire` のヘルプと doc を改善」「worktree サブシステムの API 整理」「`git for-each-repo` を secondary worktree から起動すると期待通り動かなかったのを修正」（`ds/for-each-repo-w-worktree`）「`merge-file --object-id` が linked worktree で BUG を踏んでいたのを修正」（`mr/merge-file-object-id-worktree-fix`）。
  - **2.55「`.git/info/exclude` は同一リポジトリに紐づく worktree 間で共有されるという事実をドキュメント化」**（RelNotes 2.55.0.adoc）。「`ar/receive-pack-worktree-env`（worktree がクリーンなときだけ worktree を更新）」。
- **Kagi への示唆**: 3 点が実務的。(a) **`worktree list --porcelain` は必ず `-z` 付き**（2.36 の理由が「危険バイトで壊れる」なので、これはバグ回避ではなく正しさの問題）。(b) **`.git/info/exclude` が worktree 間で共有される (2.55 doc)** — Kagi が worktree ごとに ignore を出す UI があると、ユーザーは「この worktree だけ」と誤解する。**UI で「全 worktree に影響します」と明示すべき**（Kagi の安全性思想に直結）。(c) `worktree add --orphan` (2.42) は「既存履歴と無関係な worktree」を作る手段で、Kagi の worktree 作成 UI に**まだ無い選択肢**（既存は「ブランチごとに開く」）。ドキュメント/リリースノート系の変更は取り込み対象外。
- **難易度**: **S**（`-z` 統一、exclude 共有の警告文、`--orphan` オプション追加）。

#### G-17. `git switch` / `git restore` の成熟
- **何か**: `git checkout` の役割分割後継コマンド。
- **出典**: **2.51「`git switch` と `git restore` は experimental ではないと宣言された」**（RelNotes 2.51.0.adoc「"git switch" and "git restore" are declared to be no longer experimental」, 2025-08-18）。2.41「`git restore` で `--staged` と `--worktree` が両方指定されたときコンフリクトと非互換とマーク」（`ak/restore-both-incompatible-with-conflicts`）。
- **仕組み**: `git switch <branch>`（ブランチ切替のみ）、`git restore [--staged] [--worktree] <path>`（ファイル内容復元のみ）。
- **Kagi への示唆**: **Kagi が内部で `git checkout` を呼んでいる箇所を `switch`/`restore` に分けるべき理由が、2.51 で「experimental でない」と公式に確定した**。意味は大きい: `checkout` は「ブランチ切替」と「ファイル破棄」が同じコマンドで、**引数の解釈ミスで意図せずファイルを破棄しうる**（`git checkout <ambiguous>` 問題）。`switch`/`restore` はこの曖昧さが構造的に無い。**`push --force` / `reset --hard` / `git clean` をコードベースに持たない Kagi の思想からすれば、`git checkout` も同じ理由で追放対象**。これは機能追加ではなく安全性の内部改善。
- **難易度**: **S**（呼び出し置換）。ただし `git checkout -m` の挙動（下記）だけは switch/restore に完全対応がないので個別確認が必要。
- 関連: **2.55「`git checkout -m another-branch` はコンフリクト解決の機会を 1 度しか与えなかったが、ローカル変更を保存する stash を作るようになった」**（RelNotes 2.55.0.adoc）。これは Kagi の「ブランチ切替時にローカル変更がある」ケースの安全性を上げる。

#### G-18. `git for-each-ref` / `git refs` の新機能
- **何か**: ref 列挙の新オプションと新フロントエンド。
- **出典**:
  - **2.51「`git for-each-ref` が `--start-after` オプションを学んだ。出力をページングしたいアプリケーションを助けるため」**（RelNotes 2.51.0.adoc）。
  - **2.52「`git refs` の `list` サブコマンドが `git for-each-ref` のフロントエンドとして機能する」**、**2.52「`git show-ref --exists` と同じ働きをする `git refs exists` コマンドが追加された」**（RelNotes 2.52.0.adoc）。
  - 2.53「一部の ref backend は annotated tag のオブジェクト名だけでなく tag が指すオブジェクト名も保持できる。この情報を扱うコードを整理」。
- **Kagi への示唆**: **`--start-after` (2.51) は「アプリケーションが出力をページングするため」に作られた = Kagi のようなクライアントが名指しされた機能**。ref が数万あるリポジトリでブランチ一覧を仮想スクロールする際、全 ref を取ってからメモリでページングする必要がなくなる（reftable 環境と組み合わせると特に効く）。`git refs exists` (2.52) は「このブランチは存在するか」の判定に `for-each-ref` の全走査やエラー握りを使わずに済む → **Kagi の preflight で「push 先の ref が既にあるか」を軽量に確認できる**。
- **難易度**: **S**。

#### G-19. `git notes` / `git bisect` / squash 相当の公式機能
- **何か**: 補助的な履歴メタデータと二分探索、および `git absorb` 相当の有無。
- **出典**:
  - `git bisect`: **2.52「`git bisect` のヘルプテキストと man page の整合性を取った」**、**2.52「`git bisect` が `git bisect help` と `git bisect unknown` に正しく反応していなかったのを修正」**（`rz/bisect-help-unknown`）、**2.55「`git bisect` が選択された用語（old/new 等）を出力でより一貫して使うようになった」**（`jr/bisect-custom-terms-in-output`）。機能面の追加は 2.40〜2.55 の範囲で無い（`bisect run` は既存）。
  - `git notes`: 2.40〜2.55 のリリースノートに `git notes` 自体の機能追加の記載は**無い**。関連は 2.52 の `format-patch --range-diff --notes` 修正と 2.55 の range-diff notes doc 修正のみ。
  - **`git absorb` 相当の公式機能: `git history fixup <commit>` (2.55) がこれに最も近い**（G-1 参照）。ただし `git absorb` のような「staged 変更を自動で適切なコミットに振り分ける」自動判定は**無い**。`git history fixup <commit>` は対象コミットを明示指定する。`--empty=(drop|keep|abort)` で fixup により空になったコミットの扱いを制御。
- **Kagi への示唆**: `bisect` は Kagi にとって**まだ無い機能領域**で、GUI の価値が非常に高い（「良い/悪い」を押すだけで犯人コミットに到達し、グラフ上で探索範囲が縮んでいくのが見える）。ただし 2.55 の「用語の一貫性」修正が示すように、`old/new` カスタム用語のパースは注意が必要。`git notes` は取り込み優先度が低い（ユーザー母数が小さく、リモート同期が煩雑）。**`git history fixup` を「absorb 風 UI」に包むのは Kagi の付加価値になりうる**（Kagi はグラフを持っているので「どのコミットに吸収させるか」を視覚的に選べる = CLI の `git absorb` の自動判定より人間に優しい）。
- **難易度**: **M**（bisect の GUI モード）/ **M**（absorb 風 fixup UI）。

#### G-20. 署名: SSH 署名 (`gpg.format=ssh`) と検証
- **何か**: GPG に加えて SSH 公開鍵でコミット/タグ/push-cert を署名。
- **出典**: 導入 **2.34**（RelNotes 2.34.0.adoc「In addition to GnuPG, ssh public crypto can be used for object and push-cert signing. Note that this feature cannot be used with ssh-keygen from OpenSSH 8.7, whose support for it is broken. Avoid using it unless you update to OpenSSH 8.8」, 2021-11-15）。
  - **2.54「長い昔に GPG 署名されたコミットの署名は、署名に使われた鍵が期限切れになった後も有効であるべきだが、警告色の赤で表示していた」修正**（RelNotes 2.54.0.adoc）。
  - fast-import 側の署名扱いが 2.51〜2.54 で大幅整備: 2.51「commit object の署名を fast-import stream へ export / import する方法の整理」、2.52「`fast-import` が `--signed-commits=<how>`（fast-export 相当）を学んだ」「署名付き tag を扱えるようになった」、2.53「`--signed-commits=strip-if-invalid`（無効な暗号署名をオブジェクトから落とす）」、2.54「replay により署名が無効化されたコミットについて、署名を付け直すオプション」「fast-import での署名付き commit/tag の扱いをより設定可能に」。
  - 2.53「`git replay` が `gpgsig` と同様に `gpgsig-sha256` 拡張ヘッダを結果コミットから省くのを忘れていたのを修正」（`pw/replay-exclude-gpgsig-fix`）。
- **仕組み**: `gpg.format=ssh` + `user.signingkey=<ssh pubkey or key file>` + `gpg.ssh.allowedSignersFile` で検証。`git verify-commit` / `git log --show-signature` / `%(signature:...)` フォーマット atom。
- **Kagi への示唆**: (a) **2.54 の「期限切れ鍵で署名された古いコミットを赤で出さない」修正は、Kagi がそのまま踏む UX バグ**。Kagi が署名状態を色で出しているなら、「鍵の期限切れ」と「署名が無効」を区別しなければならない（前者は過去のコミットについては問題ではない）。これは git 本体が実際に間違えて直した箇所なので、Kagi も同じ間違いをしている可能性が高い。(b) SSH 署名は GPG より導入障壁が圧倒的に低く、**Kagi が「署名付きコミットを作る」を勧めるなら SSH 署名が現実解**。(c) `git history` / `git replay` は署名を無効化する（署名は書き換え後のコミットには引き継げない）ので、**Kagi の履歴改変 UI は「署名が失われます」を二段確認に含めるべき**（2.53/2.54 の一連の修正はまさにこの問題への対処）。
- **難易度**: **S**（署名状態の色分け是正、履歴改変時の署名喪失警告）。

#### G-21. 大きいリポジトリ向け: cruft pack と `git gc --cruft`
- **何か**: 到達不能オブジェクトを loose に展開せず「cruft pack」に集める仕組み。
- **出典**: **2.41「cruft pack の使用（到達不能オブジェクトを loose object file に展開する代わり）は以前から効率的な選択肢として提供されていたが、既定になり、実験的機能ではなくなった」**（RelNotes 2.41.0.adoc, 2023-06-01）。2.42「到達不能オブジェクトを cruft pack に」、2.43「オブジェクトを cruft pack へ」「**`git repack` が `--max-cruft-size` オプションを学び、cruft pack が無限に成長するのを防ぐ**」、2.50「**`git repack` が `--combine-cruft-below-size` オプションを学び、cruft pack の結合方法を制御**」、2.51「`pack-objects` が midx から cruft pack 内のオブジェクトを指さないように」。
- **Kagi への示唆**: **Kagi の discard 時の ODB blob バックアップと直接関係する**。Kagi がバックアップした blob は到達不能オブジェクトなので、`gc` で cruft pack に入り、やがて `gc.pruneExpire`（既定 2 週間）で消える。**Kagi は「バックアップの有効期限」をユーザーに正しく伝えるべきで、その期限は cruft pack の expire 設定に依存する**。さらに `--max-cruft-size` / `--combine-cruft-below-size` により cruft pack の挙動が可変になったので、Kagi が「復元可能期間」を表示するなら実際の設定を読む必要がある。これは新機能の取り込みではなく**既存機能の正確性の問題**。
- **難易度**: **S**（バックアップ有効期限の正確な表示）。

#### G-22. `scalar`
- **何か**: 巨大リポジトリ向けの推奨設定を一括適用する同梱ツール。
- **出典**: 2.53「`make strip` が `git` に加えて `scalar` も strip するようになった」（RelNotes 2.53.0.adoc）、2.53「`ds/doc-scalar-config`（scalar の設定に関する doc 修正）」。**2.40〜2.55 の範囲で scalar 自体の機能追加はリリースノートに記載されていない**（成熟・維持フェーズ）。
- **仕組み**: `scalar clone` / `scalar register` が partial clone + sparse-checkout + fsmonitor + commit-graph + maintenance を一括で有効化。
- **Kagi への示唆**: **Kagi が scalar を呼ぶべきではない**。scalar は「複数の設定を勝手に変える」ツールで、Kagi の「全書き込み操作が plan → confirm を通る」原則と正面衝突する。**ただし scalar が有効にする設定の一覧は、Kagi が個別に「これを有効化しますか？」と提案する項目リストとして最良の参考**（fsmonitor / commit-graph / maintenance = G-7,8,9 と一致）。**取り込まない判断**（§4 参照）。
- **難易度**: —

#### G-23. 設定ベース hooks（`hook.<name>.*`）と reference-transaction hook
- **何か**: フックを `.git/hooks/` のファイルではなく**設定ファイルで定義**し、1 イベントに複数フックを走らせる機構。
- **出典**: **2.54「Hook コマンドを（場合によっては集中管理された）設定ファイルで定義でき、同一 hook イベントに対して複数を実行できるようになった」**（RelNotes 2.54.0.adoc）。**2.55「設定システム経由で定義された hook スクリプトを並列実行するよう設定できる」**（RelNotes 2.55.0.adoc）。設定キー仕様: https://github.com/git/git/blob/master/Documentation/config/hook.adoc （実読）
  - **2.54「reference-transaction hook が、参照のロックを取る前の "preparing" フェーズでも起動されるようになった」**（RelNotes 2.54.0.adoc）。
- **仕組み**（doc 実読）: `hook.<friendly-name>.command`（実行パス or シェル one-liner）、`hook.<friendly-name>.event`（複数指定可、multi-valued）、`hook.<friendly-name>.enabled`（既定 true、system/global で定義されたフックをリポジトリ単位で無効化できる）、`hook.<friendly-name>.parallel`（既定 false、**1 つでも true でないフックがあればそのイベントの全フックが逐次実行**）、`hook.<event>.enabled`（イベント単位の一括 on/off）、`hook.<event>.jobs`。`<friendly-name>` に既知イベント名を使うのは fatal error。
- **Kagi への示唆**: **二重に効く。** (a) **`hook.<event>.enabled=false` は Kagi にとって決定的**。Kagi はフックが GUI を固めたり予期せぬ副作用を出す問題を抱えるが、これまで「フックを一時的に切る」には `--no-verify` を渡すしかなかった（それはユーザーの意図を曲げる）。設定単位で切れるなら**Kagi の「フックを無効にして実行」を plan に明示して confirm を取る**という筋の通った実装ができる。(b) **reference-transaction hook の "preparing" フェーズ (2.54) は、ロック取得前に呼ばれる = Kagi の preflight を git 側から観測できる唯一の点**。Kagi 自身が preflight を持っているので必須ではないが、「Kagi 外（CLI や他ツール）で行われた ref 変更を Kagi の oplog に取り込む」ためのフックポイントとして使える（= **Kagi 外の操作を oplog に反映する道筋**、現在の oplog は Kagi 経由の操作しか記録できないはず）。
- **難易度**: **S**（hook 無効化の設定利用）/ **L**（reference-transaction hook 経由で外部操作を oplog に取り込む = アーキ変更）。

#### G-24. `git status status.compareBranches` とその他 UI 系
- **何か**: `git status` が現在ブランチと他ブランチの比較を出す設定。
- **出典**: **2.54「`git status` が、`status.compareBranches` 設定に列挙された various other branches と現在ブランチの比較を表示するようになった」**（RelNotes 2.54.0.adoc）。
- **Kagi への示唆**: Kagi はグラフを持つので `git status` の出力自体は不要だが、**「現在ブランチと、ユーザーが指定した複数の基準ブランチとの ahead/behind を常時見せる」という UX 提案そのものが有用**。Kagi のブランチペインに「main に対して +3/-12」を常時出す（現在は solo 表示があるが ahead/behind の常時多対比較は別）。実装は `git rev-list --count --left-right` で足りるので、この設定自体を読む必要はない。
- **難易度**: **S**。

#### G-25. その他の Kagi 関連 git 変更（まとめ）
- **`git stash` の interchange format と import/export**: **2.51「stash エントリの交換形式が定義され、import/export する `git stash` サブコマンドが追加された」**（RelNotes 2.51.0.adoc）。2.54 で completion 対応。→ **Kagi の stash 機能に「stash を別マシン/別 worktree へ移送する」という新軸**。stash は本来ローカル専用だったので、これは新規価値。難易度 **M**。
- **`stash.index` 設定**: **2.52「`stash.index` 設定で `git stash pop/apply` が `--index` 付きで起動されたかのように振る舞わせられる」**（RelNotes 2.52.0.adoc）。→ Kagi の stash 適用 UI で「index も復元」チェックボックスの既定値をユーザー設定から読める。難易度 **S**。
- **`git rev-list --maximal-only`**: **2.54「`git rev-list` と friends が `--maximal-only` を学び、他のコミットから到達可能でないコミットのみを表示」**（RelNotes 2.54.0.adoc）。→ Kagi のグラフで「tip だけ」を高速に得る（レーン割当の初期化に使える）。難易度 **S**。
- **`git rev-list --max-count-oldest`**: **2.55「`git rev-list`（および `git log` family）が新しい `--max-count-oldest` を学び、範囲内の最も古い N 件を選ぶ」**（RelNotes 2.55.0.adoc）。→ Kagi のグラフの逆方向ページング（履歴の根に向かうスクロール）が 1 コマンドで可能。難易度 **S**。
- **`git rev-list` の NUL 区切り機械可読出力**: **2.50「`git rev-list` が各フィールドを NUL で区切る機械可読出力形式を学んだ」**（RelNotes 2.50.0.adoc）。→ **Kagi の Rust 側パーサから改行/クォートの曖昧さを排除できる**。難易度 **S**（が、パーサ全体に効く）。
- **`git blame --porcelain` の未帰属行**: **2.50「`git blame --porcelain` モードが unblamable な行と ignored commit に帰属された行について言及するようになった」**（RelNotes 2.50.0.adoc）。→ Kagi の blame 表示で「帰属不能」「ignore-rev により無視」を区別表示できる（現状はおそらく無区別）。難易度 **S**。
- **`git add -p` の UX 改善群**: 2.52「hunk を 'selected' にしてから split すると分割片が全部 selected になっていたのを、全部 'undecided' に変更（より良い体験）」「`git add -p` に 'P'ipe コマンド表示」、2.54「現在の hunk の状態を表示」「既に処理済みのファイルを再訪できる新モード」、2.55/2.52「'q'(uit) が無意味な作業をせず抜ける」。→ **Kagi は既に hunk 単位の staging UI を持つが、「split 後は undecided に戻す」「処理済みファイルを再訪できる」は Kagi の staging UI がまだ間違えている可能性の高い挙動**。upstream が実際に「より良い end-user experience」として直した点なので、Kagi も追随すべき。難易度 **S**。
- **`git format-rev`（新 builtin）**: **2.55「1 行につき 1 つの revision 式、または実行テキスト中のコミットオブジェクト名を pretty format するための新 builtin `git format-rev` が導入された」**（RelNotes 2.55.0.adoc）。→ **Kagi のコミットメッセージ本文中の SHA を自動リンク化 / 短縮表示するのに直接使える**（現状は自前で SHA を正規表現検出しているはず）。難易度 **S**。
- **`git url-parse`（新サブコマンド）**: **2.55「内部の URL パースロジックが新サブコマンド `git url-parse` 経由でアクセス可能になった」**（RelNotes 2.55.0.adoc）。→ Kagi が remote URL から owner/repo を抽出する自前ロジック（SSH/HTTPS/scp-like 形式の全パターン）を git 本体の実装に置換できる。**GitHub 連携の owner/repo 推定は自前パースだとバグの温床**なのでこれは実利がある。難易度 **S**。
- **`git push` の remote group**: **2.55「`git push` が push 先の "remote group" 名を取れるようになり、`git fetch` と同様に複数箇所へ push する」**（RelNotes 2.55.0.adoc）。→ Kagi の push UI に「複数リモートへ同時 push」。ただし**force-with-lease との組み合わせで一部成功/一部失敗が起きうるので、Kagi の verify/oplog 設計に partial failure の表現が必要**。難易度 **M**。
- **sideband のターミナル制御シーケンス無効化**: **2.55「リモートと通信中に sideband 経由で来るターミナル制御シーケンスは、ANSI カラーエスケープを除いて既定で無効化された」**（RelNotes 2.55.0.adoc）。2.55「sideband メッセージの表示方法を調整してターミナルを刺激しないように」。→ **これはセキュリティ修正の性質**。Kagi は埋め込みターミナルを持つので、**リモートからのメッセージを表示する経路で同じ対策が必要**（悪意あるリモートがエスケープシーケンスを送って Kagi の表示を偽装する攻撃）。難易度 **S**（が、セキュリティ上必須）。
- **`git remote` の重複名検出**: 2.51「`git remote` が互いに重なる remote 名（例: "outer" と "outer/inner"）を検出するようになった」。→ Kagi の remote 管理 UI で事前警告。難易度 **S**。
- **`git cat-file --batch` の `mailmap` インラインコマンド**: **2.55（RelNotes 2.55.0.adoc）**。`--batch` セッション中に mailmap の使用を切り替えられる。→ Kagi が長命な `cat-file --batch` プロセスを持つなら（性能上そうすべき）、mailmap 適用/非適用を 1 プロセスで切り替えられる。難易度 **S**。
- **`git config list --type=<X>`**: **2.54「`git config list` が `--type=<X>` オプションで特定の型として解釈された値を表示するよう教えられた」**（RelNotes 2.54.0.adoc）。→ Kagi の設定読み取りで bool/int/path の正規化を git に任せられる（`true`/`yes`/`on` の揺れを自前で処理しない）。難易度 **S**。
- **パス値設定の `:(optional)` 接頭辞**: **2.52「パス名を値に取る設定変数（例: `blame.ignorerevsfile`）は値の前に `:(optional)` を付けることで optional とマークできる」**（RelNotes 2.52.0.adoc）。→ Kagi が `blame.ignorerevsfile` を扱うとき、ファイル不在でエラーにしない設定を提案できる。難易度 **S**。
- **`git init` の既定ブランチ名**: **2.52「他に設定されていない `git init` は Git 3.0 以降 'master' ではなく 'main' を初期ブランチとして使うと宣言」**（RelNotes 2.52.0.adoc）。**Symlink symref も Git 3.0 で消える**（同）。→ **Kagi の互換性リスク**。Kagi が `master` を仮定している箇所と、symlink symref を読む箇所があれば Git 3.0 で壊れる。難易度 **S**（監査）。
- **Rust**: **2.55「Rust サポートが既定で有効（オプトアウト可）。Git 3.0 では Rust が必須になる」**（RelNotes 2.55.0.adoc）、2.52「3 つのライブラリアーカイブ（git / reftable / xdiff）を単一の `libgit.a` に統合。これは後の Rust への FFI 作業を助ける」、2.55「xdiff コードベースを Rust で動くよう準備」。→ **Kagi は Rust 製なので、git 本体が `libgit.a` 単一化 + Rust FFI に向かっているのは中期的に極めて大きい**。将来「git のサブプロセス起動」ではなく「git のコードに直接 FFI」が現実的な選択肢になる。現時点では[推測]の域だが、**Kagi のアーキテクチャ判断（libgit2 依存 vs CLI 呼び出し vs 将来の libgit FFI）に影響する情報**。難易度 **L**（将来の選択肢として）。

---

### 【GitHub / gh CLI】

#### GH-1. Repository Rulesets の「実効ルール」取得（push 前検証）
- **何か**: 特定ブランチに実際に適用されるルールの一覧を、リポジトリ/organization の両レベルから解決済みの形で返す REST エンドポイント。
- **出典**: `GET /repos/{owner}/{repo}/rules/branches/{branch}`（https://docs.github.com/en/rest/repos/rules）。**本調査で `gh api` により実測**。
- **仕組み**（**実測レスポンス**、`microsoft/vscode` の `main`）:
  ```
  {"type":"deletion","ruleset_id":5351760,"ruleset_source_type":"Organization","ruleset_source":"microsoft","parameters":null}
  {"type":"non_fast_forward","ruleset_id":5351760,"ruleset_source_type":"Organization","ruleset_source":"microsoft","parameters":null}
  {"type":"pull_request","ruleset_id":5351760,"ruleset_source_type":"Organization","ruleset_source":"microsoft",
   "parameters":{"required_approving_review_count":1,"dismiss_stale_reviews_on_push":false,
                 "required_reviewers":[],"require_code_owner_review":true,"require_last_push_approval":true,
                 "required_review_thread_resolution":false,
                 "require_extra_approval_for_unattributed_changes":true,
                 "allowed_merge_methods":["merge","squash","rebase"]}}
  {"type":"deletion","ruleset_id":7988222,"ruleset_source_type":"Repository","ruleset_source":"microsoft/vscode","parameters":null}
  {"type":"copilot_code_review","ruleset_id":7988222,"ruleset_source_type":"Repository","ruleset_source":"microsoft/vscode",
   "parameters":{"review_on_push":false,"review_draft_pull_requests":true}}
  ```
  他リポジトリでの実測: `rust-lang/rust@master` → `["creation","update","deletion","pull_request","non_fast_forward"]`、`kubernetes/kubernetes@master` → `[]`。
  - **仕様書の記述（`docs.github.com/en/rest/repos/rules.md` 実読）**: 「指定ブランチに適用される**すべての active ルール**を返す。**ブランチは存在しなくてもよい。その名前のブランチに適用されるであろうルールが返る**。**設定されたレベル（リポジトリ / organization）に関わらず、適用される全 active ルールが返る**。enforcement が `evaluate` または `disabled` のルールセットのルールは返らない。」
  - **「ブランチは存在しなくてもよい」が決定的** — Kagi は**ブランチを作る前に**「その名前で作れるか」「その名前は `branch_name_pattern` に適合するか」を検証できる。
  - **重要**: このエンドポイントは**`repo` scope のみで読める**（admin 権限不要）。本調査のトークン scope は `admin:public_key, gist, read:org, repo, workflow` で、admin なしで上記が取れた。旧 `GET /repos/{o}/{r}/branches/{b}/protection` は admin 相当が必要で、しかも「org レベル ruleset の効果」が見えない。
  - `gh ruleset check <branch>` / `gh ruleset list` / `gh ruleset view` が CLI ラッパ（ローカル `gh ruleset --help` 実測、gh 2.97.0）。

- **ルール型の完全な一覧（仕様書実読、全 23 型）** — Kagi の preflight にとっての価値で 3 群に分類:

  **(A) ローカルで commit 前に完全検証できる群 ← Kagi にとって最大の発見**
  | type | parameters | Kagi での検証タイミング |
  |---|---|---|
  | `commit_message_pattern` | `operator`(`starts_with`/`ends_with`/`contains`/`regex`), `pattern`, `negate`, `name` | **コミットメッセージ入力中にライブ検証** |
  | `commit_author_email_pattern` | 同上 | コミット前（`user.email` を検証） |
  | `committer_email_pattern` | 同上 | コミット前 |
  | `branch_name_pattern` | 同上 | **ブランチ作成ダイアログでライブ検証**（ブランチ不存在でも取得可） |
  | `tag_name_pattern` | 同上 | タグ作成ダイアログ |
  | `max_file_size` | `max_file_size`（MB 単位。**Git LFS には適用されない**と明記） | **staging 時にファイルサイズを検証** |
  | `max_file_path_length` | `max_file_path_length`（文字数） | staging 時 |
  | `file_path_restriction` | `restricted_file_paths`（配列） | staging 時 |
  | `file_extension_restriction` | `restricted_file_extensions`（配列） | staging 時 |
  | `required_linear_history` | なし（マージコミットの push を禁止） | **マージ方式選択時**（merge commit を作らせない） |
  | `required_signatures` | なし（**検証済み署名が必須**） | **コミット前に署名設定の有無を検証** |
  | `non_fast_forward` | なし（force push を禁止） | push 前 |
  | `creation` / `update` / `deletion` | `update` のみ `update_allows_fetch_and_merge` | ブランチ作成/更新/削除の前 |

  **(B) サーバ状態が必要だが merge 前に判定できる群**
  | type | 主要 parameters |
  |---|---|
  | `pull_request` | `required_approving_review_count`, `dismiss_stale_reviews_on_push`, `require_code_owner_review`, `require_last_push_approval`, `required_review_thread_resolution`, **`require_extra_approval_for_unattributed_changes`**, **`allowed_merge_methods`**, `required_reviewers`（reviewer + file pattern の組） |
  | `required_status_checks` | `required_status_checks[]`(`context`, `integration_id`), `strict_required_status_checks_policy`, `do_not_enforce_on_create` |
  | `merge_queue` | `merge_method`(`MERGE`/`SQUASH`/`REBASE`), `grouping_strategy`(`ALLGREEN`/`HEADGREEN`), `min_entries_to_merge`, `max_entries_to_merge`, `max_entries_to_build`, `min_entries_to_merge_wait_minutes`, `check_response_timeout_minutes` |
  | `required_deployments` | `required_deployment_environments[]` |
  | `workflows` | `workflows[]`(`path`, `ref`, `repository_id`, `sha`), `do_not_enforce_on_create` |
  | `code_scanning` | `code_scanning_tools[]`(`tool`, `alerts_threshold`, `security_alerts_threshold`) |
  | `copilot_code_review` | `review_on_push`, `review_draft_pull_requests` |
  | `license_compliance_scanning` | なし |

  **(C) bypass 権限（`GET /repos/{o}/{r}/rulesets` 側で取得、仕様書実読）**
  - `bypass_actors[]`: `actor_type`(`Integration`/`OrganizationAdmin`/`RepositoryRole`/`Team`/`DeployKey`/`User`), `actor_id`, `bypass_mode`(`always`/`pull_request`/`exempt`)
  - **`current_user_can_bypass`: `always` / `pull_requests_only` / `never` / `exempt`** ← **これが `gh pr merge --admin` を出すか否かの判断根拠**
  - `enforcement`: `disabled` / `active` / `evaluate`（`evaluate` は Enterprise のみのテストモード）

- **Kagi への示唆**: **Kagi の preflight を「ローカルの安全性」から「リモートの契約」へ拡張する唯一の手段**。分類 (A) が特に効く:
  - **(A) 群は 1 度取得してキャッシュすれば、以降は完全にローカルで検証できる** — つまり**ネットワーク往復なしに「push が拒否される変更」を commit 前に止められる**。これは Kagi の「plan → confirm → preflight」に理想的に嵌る。特に:
    - `commit_message_pattern` → コミットメッセージ入力欄でのライブバリデーション（Conventional Commits を強制している org で絶大）
    - `max_file_size` / `file_extension_restriction` / `file_path_restriction` → **staging 時に「このファイルは push できません」**（現状は push して拒否されるまで分からない。しかも一度コミットしてしまうと履歴から除去する必要があり、それは Kagi が最も苦手を助けたい作業）
    - `required_signatures` → **コミット前に「このブランチは署名が必須ですが署名設定がありません」**（G-20 の SSH 署名設定案内につながる）
    - `branch_name_pattern` → ブランチ作成ダイアログでのライブ検証（ブランチ不存在でも取得できるのが効く）
    - `required_linear_history` → **マージ方式の選択肢から merge commit を除外**
  - `non_fast_forward` があれば → **Kagi は force-with-lease すら拒否すべき**（現状 Kagi は force-with-lease を実装済みだが、「そもそもリモートが non-fast-forward を禁止している」ことは push を試すまで分からない。事前に知れば「このブランチでは強制更新が禁止されています」と plan フェーズで言える）。
  - **`allowed_merge_methods` → merge/squash/rebase の選択肢を、許可されていないものはグレーアウト**。`merge_queue.merge_method` があれば「キュー経由のマージ方式は固定です」と表示。
  - **`current_user_can_bypass=never` なら `--admin` ボタンをそもそも出さない**。`always`/`exempt` のときだけ出し、GH-1 の (A)(B) から「踏み越えるルール」を列挙して二段確認（→ 推奨 #12）。
  - `merge_queue` ルールがあれば → **「このブランチは merge queue 経由でしかマージできません」を merge ボタン押下前に表示**（GH-8 のキュー状態表示と直結）。
- **難易度**: **M**。エンドポイント 1〜2 本 + ルール型 23 種のパーサ + preflight/staging/commit UI への配線。org/repo 両ソースのマージは API 側が済ませてくれている。**(A) 群だけ先に実装するなら S〜M**。

#### GH-2. `require_extra_approval_for_unattributed_changes`（AI 帰属ルール）
- **何か**: Copilot が「人に帰属しない」形で開いた PR に対し、設定した承認数 +1 を要求するルールセット設定。**新規/既存ルールセットで既定 ON**。
- **出典**: 上記 GH-1 の実測レスポンスに `"require_extra_approval_for_unattributed_changes":true` として実在を確認。仕様: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets
- **仕組み**: 「Require an additional approval for unattributed Copilot pull requests」。例: 1 承認を要求するルールセットは、未帰属の AI 変更に対しては write 権限者 2 承認を要求する。設計理由は「1 承認要求は通常『書いた人 + 承認した人』の 2 人が関与することを意味するが、Copilot が人の代理ではなく自身の app identity で PR を開くとその前提が崩れる（例: グループスレッドのような共有コンテキストからプロンプトした場合）」。「最後に push した人以外の承認も要求している場合、少なくとも 1 承認は最後の push をカバーし、かつ Copilot 以外から来なければならない」。
- **Kagi への示唆**: **AI native の観点で最も重要な単一の発見**。GitHub が「AI が作った変更は人間のレビュー要件が違う」を**ルールセットとして製品化した**という事実は、Kagi の「AI native かつ人間に優しい」の設計指針そのもの。Kagi は GH-1 でこのフラグを読めるので、**「この PR は AI 帰属なので追加承認が必要です」を PR 画面に明示できる**。さらに G-15（trailer）と組み合わせると、**Kagi 側で「このコミット群は AI 共著 → マージには追加承認が必要」を push 前に予告できる**。GitHub 側の判定は app identity ベース、Kagi 側は trailer ベースなので完全一致はしないが、**「AI 関与度をグラフ上で可視化する」という Kagi 固有の価値**につながる。
- **難易度**: **S**（GH-1 に含まれるフラグの表示）/ **M**（trailer と組み合わせた push 前予告）。

#### GH-3. `copilot_code_review` ルールセットルール
- **何か**: Copilot を自動レビュアーとして走らせることをルールセットで強制する新しいルール型。
- **出典**: **本調査で `gh api /repos/cli/cli/rules/branches/trunk` および `/repos/microsoft/vscode/rules/branches/main` により実測**:
  - `cli/cli@trunk`: `{"type":"copilot_code_review","parameters":["review_draft_pull_requests","review_on_push"]}`
  - `microsoft/vscode@main`: `{"type":"copilot_code_review","parameters":{"review_on_push":false,"review_draft_pull_requests":true}}`
- **仕組み**: `parameters.review_on_push`（push ごとに再レビューするか）、`parameters.review_draft_pull_requests`（draft PR もレビューするか）。ルールセットに属するので org/repo の両レベルで設定可、`ruleset_source_type` で由来が分かる。
- **Kagi への示唆**: **Kagi が「AI レビューが自動で走る/走らない」をローカルから予測できる**。「この PR を draft から ready にすると Copilot レビューが走ります」「この push で Copilot が再レビューします」を Kagi の push/PR 画面で予告できる。Kagi は既に PR review conversation を表示するが、**AI レビューコメントを人間のレビューコメントと視覚的に区別する**のは新機能（GH-13 の actor 識別と組合せ）。
- **難易度**: **S**（GH-1 のレスポンス解釈のみ）。

#### GH-4. sub-issues（階層 issue）
- **何か**: issue に親子関係を持たせる機能。
- **出典**:
  - REST API サポート: **2024-12-12**（https://github.blog/changelog/2024-12-12-github-issues-projects-close-issue-as-a-duplicate-rest-api-for-sub-issues-and-more/ 「You can now use the REST API to view, add, remove, and reprioritize sub-issues」）。
  - GA（issue types / advanced search と同時）: **2025-04**（https://github.com/orgs/community/discussions/154148 「Following our public preview in January, we're thrilled to announce the general availability of sub-issues, issue types, advanced search, and increased item limits」）。
  - **上限: 親 issue あたり sub-issue 100 件（旧 50）**（上記 changelog の「Increased limits」セクション）。
- **仕組み**（**実測で確認**）:
  - REST: `GET /repos/{owner}/{repo}/issues/{issue_number}/sub_issues` → 本調査で `gh api /repos/cli/cli/issues/14136/sub_issues` が 200 `[]` を返すことを確認。
  - GraphQL mutation: **`addSubIssue`, `removeSubIssue`, `reprioritizeSubIssue`**（`gh api graphql` で Mutation 型をイントロスペクションして実在を確認）。
  - **`gh issue view --json` のフィールドに `subIssues`, `subIssuesSummary`, `parent` が存在**（ローカル `gh issue view --help` 実測、gh 2.97.0）。
  - **`gh issue create --parent <number|URL>`** で作成時に親を指定可（ローカル `gh issue create --help` 実測）。
- **Kagi への示唆**: **Kagi が「AI に渡して議論できる context」を作るための最良の構造**。`subIssuesSummary`（完了数/総数）を持つ階層 issue は、そのまま「作業の木」として AI に渡せる。Kagi は既に PR 一覧を持つが issue 階層は持たない。**ブランチ ↔ issue ↔ sub-issue の三者を結びつけると、「このブランチはどの作業項目のどの部分か」が Kagi 上で完結する**（GH-6 の `gh issue develop` が接着剤）。
- **難易度**: **M**。

#### GH-5. issue types と issue dependencies（blocked-by / blocking）
- **何か**: organization 単位で定義する issue の型（Bug/Feature/Task 等）と、issue 間の依存関係。
- **出典**:
  - issue types REST API: **2025-03-18**（https://github.blog/changelog/2025-03-18-github-issues-projects-rest-api-support-for-issue-types/ 「Issue types can now be managed using the REST API」）。GA は sub-issues と同時の 2025-04。**上限: org あたり 25 型（旧 10）**（2024-12-12 changelog）。
  - dependencies: **`gh issue view --json` に `blockedBy`, `blocking` フィールドが存在**、**`gh issue create --blocked-by numbers` / `--blocking numbers`** が実在（ローカル `gh issue --help` 実測、gh 2.97.0）。
- **仕組み**（実測）:
  - REST: `POST/PATCH/DELETE /orgs/{org}/issue-types`（org レベルの型管理）。issue 作成/更新時に型を指定。
  - GraphQL mutation: **`createIssueType`, `updateIssueType`, `deleteIssueType`, `updateIssueIssueType`**（イントロスペクションで実在確認）。GraphQL field: `issueType`。
  - `gh issue create --type <name>` / `gh issue list --type <name>`（実測）。
- **Kagi への示唆**: issue types は Kagi のグラフ/PR 画面で**ブランチに「Bug」「Feature」バッジ**を出す材料（issue 経由）。より重要なのは **dependencies (`blockedBy`/`blocking`)** — Kagi は「グラフ」を描くのが得意なので、**issue の依存グラフを commit graph と同じ描画エンジンで出す**という固有の強みが立つ。既存の Analyze（コード側の coupling）と対になる「作業側の coupling」。
- **難易度**: **M**（issue 依存グラフの描画は既存のレーンアルゴリズムを再利用できるので L ではない）。

#### GH-6. `gh issue develop`（issue → ブランチ）と worktree 対応
- **何か**: issue から linked branch を作る。**2.99 で worktree に checkout できるようになった**。
- **出典**:
  - 基本仕様: ローカル `gh issue develop --help` 実測（gh 2.97.0）。`-b/--base`, `--branch-repo`, `-c/--checkout`, `-l/--list`, `-n/--name`。
  - **worktree 対応: gh 2.99.0（2026-09-01）**「`gh issue develop` can now create a linked branch and check it out in a new Git worktree, leaving your current working copy unchanged: `gh issue develop 123 --checkout --worktree /path/to/wt-feature`」（https://github.com/cli/cli/releases/tag/v2.99.0、実装 PR #14136 by @sergiou87）。同リリースで「reject non-empty worktree targets before creating a branch」（#14244）という安全性修正も入っている。
- **仕組み**: GitHub 側に issue↔branch の linked-branch 関係が記録される（`--list` で参照可）。`--branch-repo` で fork 側にブランチを作れる。
- **Kagi への示唆**: **Kagi の worktree 管理と GitHub issue が公式に接続された**。Kagi の worktree 作成 UI に「issue から作る」を追加すれば、`gh issue develop --checkout --worktree <Kagi が管理するパス>` 一発で「issue に紐づいた worktree」ができる。**しかも「現在の作業コピーを変更しない」= Kagi の安全性思想と完全一致**。既存の「ブランチごとに worktree を開く」の上位版（issue リンクが付く）。ただし gh 2.99+ が必須。
- **難易度**: **S**（gh 呼び出し 1 本追加）。issue 選択 UI を作るなら M。

#### GH-7. `gh pr checkout --worktree`
- **何か**: PR をワークツリーに checkout。
- **出典**: **gh 2.98.0（2026-08-20）**「Users can now checkout a pull request into a git worktree by using the new `--worktree PATH` flag in `gh pr checkout`: `gh pr checkout 12 --worktree ../wt-feature`」（https://github.com/cli/cli/releases/tag/v2.98.0、実装 PR #13946 by @tidy-dev）。
- **関連する worktree 安全性修正（gh 2.99.0）**:
  - 「fix(pr merge): safely handle `--delete-branch` with linked worktrees」（#14007）
  - 「fix(repo sync): prevent linked-worktree corruption」（#14060）
  - 「fix(repo sync): explain when the target branch is checked out in another worktree」（#14076）
- **Kagi への示唆**: **Kagi が `gh` を呼ぶ前提バージョンは 2.99 以上にすべき**という具体的な根拠。2.99 未満では `gh pr merge --delete-branch` が linked worktree を壊しうる（Kagi は worktree を多用するので直撃する）。**これは Kagi のドキュメント/起動時チェックに書くべき事項**。機能面では「PR を worktree で開く」が Kagi の repo タブ + worktree 管理と綺麗に噛み合う（PR レビューのために現在の作業を退避しなくて済む）。
- **難易度**: **S**（呼び出し + バージョンゲート）。

#### GH-8. merge queue
- **何か**: PR を順番待ち行列に入れ、逐次的に base へ取り込む機構。
- **出典**: **`gh api graphql` によるスキーマイントロスペクションで実測**:
  - `MergeQueue` フィールド: `configuration`, `entries`, `id`, `nextEntryEstimatedTimeToMerge`, `repository`, `resourcePath`, `url`
  - `MergeQueueEntry` フィールド: `baseCommit`, `enqueuedAt`, `enqueuer`, `estimatedTimeToMerge`, `headCommit`, `id`, `jump`, `mergeQueue`, `position`, `pullRequest`, `solo`, `state`
  - `MergeQueueEntryState` enum: `QUEUED`, `AWAITING_CHECKS`, `MERGEABLE`, `UNMERGEABLE`, `LOCKED`
  - Mutation: **`enqueuePullRequest`, `dequeuePullRequest`**（実在確認）
- **gh CLI 側**: **`gh pr merge` に `--queue` フラグは存在しない**（gh 2.97.0 の `gh pr merge --help` 実測: `--admin`, `--author-email`, `--auto`, `--body`, `--body-file`, `--delete-branch`, `--disable-auto`, `--match-head-commit`, `--merge`, `--rebase`, `--squash`, `--subject` のみ）。→ **Kagi が merge queue を扱うなら `gh api graphql` を直接叩く必要がある**。
- **Kagi への示唆**: **`position` と `estimatedTimeToMerge` が取れるのが決定的**。Kagi の PR 画面に「キュー 3 番目、推定 12 分」を出せる。`state` の `UNMERGEABLE` は「キューに入れたが壊れた」= ユーザーが最も知りたい状態。`nextEntryEstimatedTimeToMerge` はリポジトリ全体の混雑度。**Kagi は既に PR merge を持つが、merge queue 経路は別のライフサイクル**（merge ボタンを押した後も「終わっていない」）なので、既存機能と重複しない。`jump` / `solo` フィールドは「優先割り込み」「単独ビルド」を示すので、`--admin` 相当の破壊的操作として Kagi の二段確認に載せるべき。
- **難易度**: **M**（GraphQL クエリ + PR 画面のキュー状態表示 + ポーリング）。

#### GH-9. PR review threads の GraphQL（diff 上への重畳）
- **何か**: PR のレビュースレッドを行単位の位置情報付きで取得・解決する API。
- **出典**: **`gh api graphql` イントロスペクションで実測**:
  - `PullRequestReviewThread` フィールド: `comments`, `diffSide`, `id`, `isCollapsed`, `isOutdated`, `isResolved`, `line`, `originalLine`, `originalStartLine`, `path`, `pullRequest`, `repository`, `resolvedBy`, `startDiffSide`, `startLine`, `subjectType`, `viewerCanReply`, `viewerCanResolve`, `viewerCanUnresolve`
  - Mutation: **`resolveReviewThread`, `unresolveReviewThread`**（実在確認）
- **仕組み**: `path` + `line`/`startLine` + `diffSide`/`startDiffSide`（LEFT/RIGHT）で diff 上の正確な範囲が決まる。`isOutdated` は「その後の push でこの行が動いた」を示す。`originalLine`/`originalStartLine` は元の位置。`subjectType` は LINE / FILE の区別。`viewerCan*` で権限を事前に知れる。
- **Kagi への示唆**: **Kagi は既に PR review conversation を表示しているが、`diffSide` + `startLine`/`line` があれば Kagi の split view diff に直接重畳できる**（会話リストとしてではなく、diff の行の隣に）。`isOutdated` は「この指摘はもう古い（コードが動いた）」を視覚的に落とすのに必須で、これが無いと古い指摘がノイズになる。`viewerCanResolve` を見てから解決ボタンを出せば「押せるのに失敗する」を避けられる（GH-1 と同じ思想）。**既存の conversation 表示に対する diff 重畳は明確な機能追加**。
- **難易度**: **M**（既存の PR データ取得を GraphQL 化 + diff レンダラへの重畳）。

#### GH-10. suggested changes（```suggestion）
- **何か**: レビューコメント本文に ```suggestion フェンスを書くと、GitHub UI が「適用」ボタンを出す機能。
- **出典**: https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/incorporating-feedback-in-your-pull-request
- **仕組み**: **提案の「生成」は純粋にコメント本文の markdown 規約**（`\`\`\`suggestion` フェンス内に置換後の行を書く）。→ **Kagi から提案を作るのは REST の `POST /repos/{o}/{r}/pulls/{n}/comments` に `path` + `line`/`start_line` + `side` とフェンス付き body を渡すだけで可能**。
  - 一方「**提案の適用**」は、本調査時点で GraphQL Mutation のイントロスペクション結果に該当する mutation が見つからなかった（`apply*` / `*Suggestion` 系の mutation 名は Mutation 型のフィールド一覧に存在しない）。**適用は Web UI 側の機能である可能性が高い**[推測]。
  - 実務的な代替: Kagi は**ローカルに作業コピーを持っている**ので、提案フェンスをパースして**ローカルに直接パッチ適用 → コミット**すればよい。GitHub 側の「適用」を経由する必要がない。
- **Kagi への示唆**: **これは Kagi が Web UI より優位に立てる箇所**。Web の「適用」は GitHub 上に commit を作るが、Kagi なら「提案をローカルに適用 → Kagi の既存の hunk staging UI で確認 → amend または新規コミット」ができる。しかも Kagi の oplog / ODB バックアップの保護下に入る。**AI レビューが提案を大量に出す時代に、「提案を一括でローカル適用して人間が hunk 単位で選別する」は Kagi の conflict editor / staging UI の資産をそのまま活かせる**。
- **難易度**: **M**（提案フェンスのパース + ローカル適用 + 既存 staging UI への接続）。**Kagi 固有価値が最も高い項目の 1 つ**。

#### GH-11. `gh pr checks` と statusCheckRollup
- **何か**: PR の CI チェック状態。
- **出典**: ローカル `gh pr checks --help` / `gh pr view --help` 実測（gh 2.97.0）。
- **仕組み**（実測）:
  - `gh pr checks [<number>|<url>|<branch>] [--fail-fast] [-i/--interval int] [--json fields] [--required] [--watch] [-w/--web]`
  - **`--json` フィールド: `bucket`, `completedAt`, `description`, `event`, `link`, `name`, `startedAt`, `state`, `workflow`**
  - `--required` で必須チェックのみ。`--watch --interval N --fail-fast` で完了までポーリング（最初の失敗で抜ける）。
  - 裏の GraphQL は `PullRequest.statusCheckRollup`（`gh pr view --json statusCheckRollup` として公開されている）。`mergeStateStatus` enum は**実測で `DIRTY`, `UNKNOWN`, `BLOCKED`, `BEHIND`, `UNSTABLE`, `HAS_HOOKS`, `CLEAN`**。
- **Kagi への示唆**: `bucket` フィールドが便利（pass/fail/pending/skipping の粗い分類が API 側で済んでいる）。**`--watch --fail-fast` を Kagi のバックグラウンドジョブとして動かせば、「push した PR の CI が落ちた」を Kagi のネイティブ通知で出せる**（現状 Kagi は PR 一覧を持つが CI の能動通知は無い）。`mergeStateStatus` の `BEHIND`（base が進んでいる）/ `DIRTY`（コンフリクト）/ `BLOCKED`（ルール未達）/ `UNSTABLE`（必須でないチェックが失敗）の 4 値は**それぞれ Kagi が提示すべきアクションが違う**（rebase する / conflict editor を開く / 承認を待つ / そのままマージ可）。既存の PR conflict preview は `DIRTY` の場合だけを扱っているはずなので、他 3 値への対応は新規。
- **難易度**: **S**（状態表示の 4 分岐）/ **M**（バックグラウンド watch + 通知）。

#### GH-12. auto-merge / draft PR / `--admin`
- **何か**: 条件が揃ったら自動マージ、draft 状態の切替、要件を無視した管理者マージ。
- **出典**: **Mutation イントロスペクションで実測**: `enablePullRequestAutoMerge`, `disablePullRequestAutoMerge`, `convertPullRequestToDraft`。`markPullRequestReadyForReview` も存在（`gh pr ready` が対応）。`gh pr view --json` に **`autoMergeRequest`** フィールドあり（実測）。`gh pr merge --auto` / `--disable-auto` / `--admin` / `--match-head-commit SHA`（実測）。
- **Kagi への示唆**: **`--admin` は Kagi の思想上「最も危険な GitHub 操作」**。`push --force` / `reset --hard` を持たない Kagi が `--admin` merge を無条件に提供するのは一貫性を欠く。**提供するなら二段確認 + GH-1 で「どのルールを踏み越えるか」を具体的に列挙して見せるべき**（「required_approving_review_count=2 を無視します」「require_code_owner_review を無視します」）。これが Kagi らしい `--admin` の実装。
  逆に **`--auto`（auto-merge）は Kagi が積極的に推すべき安全な代替**: 「今すぐ強引にマージ」ではなく「要件が揃ったら自動でマージ」。`--match-head-commit SHA` は **push 後に他人が push していたらマージを拒否する = force-with-lease と同じ思想の PR 版**。Kagi が既に force-with-lease を実装しているなら、**`--match-head-commit` は同じ設計原則の自然な延長で、必ず付けるべき**。
- **難易度**: **S**（`--match-head-commit` の常時付与、`--admin` の警告詳細化）。

#### GH-13. Copilot coding agent と `gh agent-task`
- **何か**: GitHub 上で自律的にコードを書き PR を出す AI エージェントと、その CLI。
- **出典**: **ローカル `gh agent-task --help` 実測（gh 2.97.0、preview）**。ドキュメント: https://gh.io/copilot-cli（`gh copilot` 経由）。
- **仕組み**（**実測**）:
  - `gh agent-task create [<task description>] [-b/--base <branch>] [-a/--custom-agent <name>] [--follow] [-F/--from-file <file>] [-R/--repo]`
    - エイリアス: `gh agent-tasks`, `gh agent`, `gh agents`
    - **`--custom-agent my-agent` は `.github/agents/my-agent.md` で定義されたカスタムエージェントを使う**（実測の例示より）
    - `-F -` で stdin からタスク記述を読める
  - `gh agent-task list [--json fields] [-L/--limit int] [-w/--web]`
  - `gh agent-task view [<session-id>|<pr-number>|<pr-url>|<pr-branch>] [--follow] [--log] [--json fields] [-w/--web]`
  - **`--json` フィールド（list/view 共通、実測）: `completedAt`, `createdAt`, `id`, `name`, `pullRequestNumber`, `pullRequestState`, `pullRequestTitle`, `pullRequestUrl`, `repository`, `state`, `updatedAt`, `user`**
  - タスクの識別子: PR 番号 / session ID (UUID) / URL（`https://github.com/OWNER/REPO/pull/123/agent-sessions/<uuid>`）
- **AI 由来のコミット/PR/ブランチの識別方法**（調査で確定した具体的フィールド）:
  - **ブランチ接頭辞 `copilot/`** — 「Copilot agent は `copilot/` で始まるブランチのみ作成・push できる」。2025-10-16 の changelog でランダム名から意味のある名前（例 `copilot/add-theme-switcher`）に改善（https://github.blog/changelog/2025-10-16-copilot-coding-agent-uses-better-branch-names-and-pull-request-titles/）。
  - **PR author が `app/github-copilot`**（GitHub Copilot bot account）。`gh pr list --app` フラグで GitHub App author による絞り込みが可能（ローカル `gh issue list --help` に `--app string  Filter by GitHub App author` を実測。`gh pr list` も同様）。
  - **assignee `copilot-swe-agent[bot]`** — issue を Copilot に割り当てる際の login（`POST /repos/OWNER/REPO/issues/N/assignees` に `{"assignees":["copilot-swe-agent[bot]"], "agent_assignment":{"target_repo":..., "base_branch":..., "custom_instructions":..., "custom_agent":...}}`、出典 https://github.com/orgs/community/discussions/173575）。
  - **コミット trailer `Co-authored-by: Copilot <copilot@github.com>`** — VS Code 1.110 で Copilot を co-author として付与する設定が追加（https://github.com/microsoft/vscode/issues/314311）。
  - **`gh agent-task view <pr-branch>`** でブランチ名からセッションを引ける（実測: ARGUMENTS に `<pr-branch>` が列挙されている）。
  - 実務的な列挙: `gh pr list --search "head:copilot/" --state all`。
- **Kagi への示唆**: **これが「AI native な Kagi」の具体的な形**。二方向:
  - **(a) 可視化（読み）**: Kagi のコミットグラフ上で `copilot/` ブランチ、`app/github-copilot` 作成 PR、`Co-authored-by: Copilot` trailer 付きコミットに**AI バッジ**を出す。Kagi は既にレーン安定化・solo 表示・ghost connector という「グラフの意味づけ」の資産があるので、**「AI レーン」を人間のレーンと視覚的に区別する**のは Kagi 固有の表現になりうる。`gh agent-task view --log` でセッションログが取れるので、**AI が作ったコミットをクリックすると「なぜこう変更したか」のセッションログが読める**という体験は他のどの Git GUI にも無い。
  - **(b) 投下（書き）**: Kagi から `gh agent-task create` を叩く。**`--base <branch>` があるので「今グラフで選んでいるコミット/ブランチを base に AI タスクを投げる」ができる**。`--custom-agent`（`.github/agents/*.md`）を Kagi が読んで選択 UI に出せる。`-F -` で stdin を受けるので、**Kagi が「選択した diff / コンフリクト / Analyze 結果」を構造化して AI に渡す**のが素直に実装できる。これは「Kagi 上のコンテキストを AI に渡す」の最短経路。
- **難易度**: **M**（AI バッジ + agent-task 一覧/作成）/ **L**（セッションログとグラフの統合、AI レーンの視覚設計）。
- **注意**: `gh agent-task` は **preview**（「subject to change without notice」と明記）。Kagi は API 変更に備えた薄いラッパにすべき。

#### GH-14. `gh skill`（agent skills の配布機構）
- **何か**: GitHub リポジトリから「agent skill」を検索・インストール・公開する CLI。
- **出典**: **ローカル `gh skill --help` 実測（gh 2.97.0、preview）**。エイリアス `gh skills`。
- **仕組み**（実測）:
  - `gh skill search <query>` / `install <owner/repo> <skill-name>` / `list` / `preview <owner/repo> <skill>` / `publish` / `update [--all]`
  - 例示: `gh skill install github/awesome-copilot documentation-writer`
  - gh 2.99.0 で「fix(skills): install Codex user skills to `~/.agents/skills`」（#14154）、「Honor `PI_CODING_AGENT_DIR` for Pi user skills」（#14260）→ **複数の coding agent 実装（Copilot / Codex / Pi）に対して共通のスキル配布先が整備されつつある**。
- **Kagi への示唆**: **「Kagi 用の skill を配布する」という将来の配布チャネル**。Kagi が MCP / skill として「Kagi のグラフ・oplog・conflict 状態を AI に渡す」能力を公開するなら、`gh skill publish` が既存の配布路になる。現時点では preview なので**取り込みではなく監視対象**。ただし「`~/.agents/skills` が複数エージェント共通の置き場になりつつある」という事実は、Kagi が AI 連携を設計する際の設置場所の指針になる。
- **難易度**: **L**（Kagi 側の AI 公開インターフェース設計を伴う）。現時点では**取り込まない**（§4）。

#### GH-15. `gh` の新サブコマンド全体棚卸し（2.97.0 実測 + 2.98/2.99 差分）
- **出典**: **ローカル `gh --help` の全出力を実測（gh 2.97.0）**。差分は 2.98.0 / 2.99.0 のリリースノート。
- **CORE**: `auth`, `browse`, `codespace`, **`discussion`（preview）**, `gist`, `issue`, `org`, `pr`, `project`, `release`, `repo`, **`skill`（preview）**
- **GITHUB ACTIONS**: `cache`, `run`, `workflow`
- **ADDITIONAL**: **`agent-task`（preview）**, `alias`, `api`, `attestation`, `completion`, `config`, **`copilot`（preview）**, `extension`, `gpg-key`, `label`, `licenses`, `preview`, `ruleset`, `search`, `secret`, `ssh-key`, `status`, `variable`
- **EXTENSION（本環境にインストール済み）**: **`stack`（`github/gh-stack` v0.1.0）** → GH-16
- 個別:
  - **`gh discussion`（preview）**: `create`, `list`, `comment`, `edit`, `view`（実測）。→ Kagi には不要（Kagi はコードの GUI）。**取り込まない**。
  - **`gh copilot`（preview）**: 「Runs the GitHub Copilot CLI」。`gh` が Copilot CLI を PATH から探すか、なければ `~/.local/share/gh/copilot` にダウンロードして実行（実測のヘルプ本文）。例: `gh copilot -p "Summarize this week's commits" --allow-tool 'shell(git)'`。→ **`--allow-tool 'shell(git)'` という粒度のツール許可が既にある**。Kagi が「AI に git を触らせる」を実装するなら、この許可モデル（明示的な allow-list）が既存の参照実装になる。
  - **`gh ruleset`**: `check`, `list`, `view`。エイリアス `gh rs`（実測）。→ GH-1。
  - **`gh attestation`**: artifact attestation（SLSA provenance）の検証。`gh attestation verify` でビルド来歴を検証。→ **コミット署名（G-20）とは別レイヤ**（あちらは「誰が書いたか」、こちらは「どのビルドが作ったか」）。Kagi はソースコードの GUI なので artifact attestation の関与は薄い。**取り込まない**（§4）。
  - **`gh cache`**: Actions のキャッシュ管理。Kagi の関心外。
  - **`gh workflow` / `gh run`**: Actions の定義と実行。→ Kagi では GH-11（`gh pr checks`）で足りる。`gh run` の `--log` でログを取れるが、Kagi が CI ログビューアになるのはスコープ外[推測]。
  - **`gh project`**: Projects v2（GraphQL のみ、REST サポートなし）。`gh project item-add`, `item-edit`, `field-list`, `view --format json` 等。gh 2.98.0 で「Fix project item-add output for non-TTY」（#14056）。**Mutation イントロスペクション実測**: `addProjectV2DraftIssue`, `convertProjectV2DraftIssueItemToIssue`, `updateProjectV2DraftIssue`, `updateProjectV2ItemFieldValue` が実在。→ Kagi の関心は「コード ↔ 作業項目」なので、**issue 階層（GH-4/GH-5）で足りる。Projects v2 は取り込まない**（§4）。
  - **`gh search issues --search-type semantic|hybrid`**: **gh 2.98.0（2026-08-20）で追加**（https://github.com/cli/cli/releases/tag/v2.98.0、実装 PR #14006）。基盤の semantic search は 2026-04-02 に GA（https://github.blog/changelog/2026-04-02-improved-search-for-github-issues-is-now-generally-available/）。→ **「この変更に関係する issue を自然言語で探す」が API 一本でできる**。Kagi の「コミット/diff を選んで関連 issue を探す」に直結（AI に渡す context の自動収集）。難易度 **S**。
  - **`gh issue create/edit/comment --attach` / `gh pr create/edit/comment --attach`**: **gh 2.99.0（2026-09-01）**。「repeatable `--attach` flag uploads local images and videos」。body 内に該当ローカルパスの参照があればアップロード URL に置換、なければ末尾に追記。1 回の呼び出しで最大 50 ファイル（#14289）。出典: https://gh.io/gh-attach および https://github.blog/changelog/2026-09-01-github-cli-media-in-issues-pull-requests-and-comments/ → **Kagi は画像レンダリングを既に持つので、「Kagi のグラフ/diff のスクリーンショットを PR コメントに添付する」が実装できる**。難易度 **S**。
  - **`gh` の coding-agent 検出**: gh 2.99.0「Use text-only spinner output when `gh` is invoked by a coding agent」（#14191）。gh 2.98.0「Set `GH_EXTENSION=1` when gh invokes an extension」（#14072）。→ **`gh` は呼び出し元がエージェントかを検出して出力を変える**。Kagi が `gh` を subprocess で呼ぶなら、**Kagi も同様の環境変数を立てて `gh` の出力を安定化させるべき**（スピナーの ANSI 制御文字が Kagi のパーサを壊さないように）。これは実装上の具体的な注意点。難易度 **S**。

#### GH-16. stacked PR の公式サポート — `github/gh-stack`
- **何か**: **GitHub 公式の stacked PR 拡張**。ローカルのブランチ鎖を GitHub 上の「stack」として表現する。
- **出典**: **本環境にインストール済みであることを実測**: `gh extension list` → `gh stack  github/gh-stack  v0.1.0`。ドキュメント: **https://gh.io/stacks**（ヘルプ本文に記載）、フィードバック: https://gh.io/stacks-feedback。以下のサブコマンドは**ローカル `gh stack --help` の実測**。
- **仕組み**（実測）:
  - **Stack management**: `init [branches...]`, `add <branch>`, `checkout <stack#|PR#|PR URL|branch>`, `modify`（対話的な再構成）, `unstack`（ローカルと GitHub の両方から stack を削除）, `view`
  - **Remote operations**: **`link`**, `merge`（stack をまとめてマージ）, `push`, `rebase`, `submit`, `sync`
  - **Navigation**: `bottom`, `down`, `switch`, `top`, `trunk`, `up`
  - **`gh stack link` が決定的**（ヘルプ本文の実測）: 「**このコマンドは gh-stack のローカル追跡状態に依存しない。jj / Sapling / ghstack / git-town のような外部ツールでブランチを管理しつつ、ローカルの stack 追跡を採用せずに GitHub stacked PR を使いたいユーザー向けに設計されている**」。引数は stack 順（bottom→top）で branch 名 / PR 番号 / PR URL を混在可。「Branch 引数は PR 作成/検索の前に自動で remote に push される」「PR が未 stack なら新規 stack を作成、一部が既存 stack にあれば既存 stack を更新（既存 PR は決して削除されない）」「既存 stack を伸ばすショートカットとして、第 1 引数に stack 番号（**GitHub の stack UI に表示される番号**）を渡せる」。
  - `gh stack submit`（実測）: 1) 全ブランチを remote に push → 2) 対象ブランチの PR を新規作成 → 3) 既存 PR の base branch を更新 → 4) **GitHub 上に stack を作成/更新**。対話モードでは単一画面エディタで各 PR のタイトル/本文/draft 状態を編集して Ctrl+S で一括送信。`--auto` で自動生成タイトル（この場合は既定 draft、`--open` で ready）。`Ctrl+B` で「既存の open PR を stack にリンク」。
- **結論**: **2026-09 時点で GitHub は stacked PR を公式にサポートしている**。サーバ側に「stack」という第一級の概念があり（stack 番号、stack UI）、`github/gh-stack` が公式クライアント。サードパーティ（Graphite / spr / ghstack / git-town）は**もはや「GitHub に stack 概念が無いことを回避する」ツールではなく、「ローカル管理の流派」に位置が変わった** — `gh stack link` がそれらとの明示的な相互運用パスとして用意されている。
- **Kagi への示唆**: **極めて大きい。** Kagi はコミットグラフ中心なので、**ブランチ鎖の視覚化はもともと最も得意な領域**。そこに「サーバ側の stack」という正規の表現が来たので:
  - Kagi のグラフに **stack を第一級のオーバーレイとして描く**（既存の ghost connector / solo 表示と同じ層の機能）。
  - **`gh stack link` が「ローカル追跡状態を要求しない」のが Kagi にとって決定的** — Kagi は自前でブランチ鎖を把握しているので、gh-stack のローカル状態ファイルを採用せずに、**Kagi が把握した鎖をそのまま `gh stack link <b1> <b2> <b3>` に渡して GitHub 側の stack を作れる**。これは「Kagi が stack のローカル真実源になる」という筋の通った設計。
  - `git rebase --update-refs`（G-14）と `git replay --contained`（G-2）が stack の rebase を支える下層プリミティブ。**Kagi は「stack を rebase する」を `--update-refs` の 2.55 バグ（`instructionFormat` に `%d`）を回避しつつ実装できる**。
  - `gh stack merge` は複数 PR の一括マージなので、**Kagi の二段確認の対象**（1 操作で複数ブランチが動く）。
- **難易度**: **L**（グラフへの stack オーバーレイ + rebase 連携）。ただし `gh stack link` の呼び出しだけなら **S**。
- **注意**: `v0.1.0` = 非常に若い。API/CLI 変更を想定した実装が必要。

#### GH-17. CODEOWNERS
- **何か**: パスごとのレビュー担当者定義。
- **出典**: **`gh api /repos/cli/cli/codeowners/errors` を実測 → `{"errors":[]}` を返す**（https://docs.github.com/en/rest/repos/repos#list-codeowners-errors）。GraphQL 側の PR フィールドは `reviewRequests`, `latestReviews`, `reviewDecision`（`gh pr view --json` に実測で存在）。GH-1 の `pull_request.require_code_owner_review` で「CODEOWNERS レビューが必須か」が分かる。
- **仕組み**: **API は「CODEOWNERS の構文エラー一覧」しか返さない。「このパスの owner は誰か」を返すエンドポイントは存在しない**。→ **Kagi が「このファイルを触ると誰のレビューが必要か」を出すには、ローカルの CODEOWNERS ファイルを自前でパースする必要がある**（`.github/CODEOWNERS` / `CODEOWNERS` / `docs/CODEOWNERS`、gitignore 風のパターン、後勝ちルール）。`codeowners/errors` は「Kagi が自前パースする前に、GitHub 側が構文エラーと見なしていないか」の確認に使える。
- **Kagi への示唆**: **Kagi の Analyze には既に ownership がある（コミット履歴ベースの実質的な所有者）。CODEOWNERS は「宣言された所有者」**。この 2 つを並べると **「宣言と実態の乖離」= 極めて価値の高い分析**になる（「このディレクトリは CODEOWNERS では A チームだが、実際は B が 80% 書いている」）。これは既存の ownership 機能の拡張であり、他のどの Git GUI にも無い分析軸。さらに staging 画面で「この変更は @team-x のレビューが必要」を出せる（push 前に分かる）。
- **難易度**: **M**（CODEOWNERS パーサ + 既存 ownership との突き合わせ）。**Kagi 固有価値が高い**。

#### GH-18. `gh pr diff` と diff の取得経路
- **何か**: PR の diff 取得。
- **出典**: ローカル `gh pr diff --help` 実測（gh 2.97.0）。
- **仕組み**（実測）: `gh pr diff [--patch] [--name-only] [--color always|never|auto] [-e/--exclude patterns] [--allow-escape-sequences] [-w/--web]`
  - **`--allow-escape-sequences` フラグが存在する = 既定ではターミナルエスケープシーケンスを出さない**。これは G-25 の git 2.55 の sideband 対策と**同じ脅威モデル**（リモート由来の文字列でターミナル表示を偽装される）。
  - `-e/--exclude patterns` で glob 除外（lock ファイルや生成物を落とせる）。
  - `--patch` で patch 形式（`git am` に流せる）、無指定は diff 形式。
  - 生 API: `Accept: application/vnd.github.v3.diff` / `.patch` を `gh api` に渡す。
- **Kagi への示唆**: Kagi は既に diff レンダラを持つので `gh pr diff` の出力そのものは不要 — **`git fetch` して両端の SHA（`gh pr view --json baseRefOid,headRefOid` で取れる、実測）でローカル diff を出すべき**（そのほうが Kagi の split view / syntax highlight / hunk 操作が全部使える）。**取り込むべきは 2 点**: (a) `-e/--exclude` の発想（生成物を diff から落とす。Kagi の diff に「ノイズファイルを畳む」機能として）、(b) **`--allow-escape-sequences` が既定オフである理由** — Kagi の diff/PR 本文レンダラも同じ対策が必要。
- **難易度**: **S**。

#### GH-19. Codespaces / devcontainer と worktree の関係
- **何か**: クラウド開発環境と、ローカル worktree の対応関係。
- **出典**: ローカル `gh --help` に `codespace` が CORE COMMANDS として存在（実測）。gh 2.98.0 でセキュリティ修正: 「binds the local forwarded port to all available network interfaces by default」→ 修正、`gh codespace ports forward` 利用者は 2.98.0 への更新を推奨（GHSA-vfhh-p7hm-pxfh、https://github.com/cli/cli/security/advisories/GHSA-vfhh-p7hm-pxfh）。
- **結論**: **Codespaces とローカル worktree の間に、GitHub が提供する対応関係・同期機構は存在しない**（本調査で該当する API / gh サブコマンドを確認できなかった）。Codespace は「リモートのコンテナ 1 個 = リポジトリの 1 チェックアウト」で、ローカルの linked worktree とは無関係な概念。devcontainer.json は環境定義であって worktree とは直交する。
- **Kagi への示唆**: **取り込まない**（§4）。Kagi は macOS/Linux のネイティブアプリで、その価値は「ローカルの worktree を安全に扱う」ことにある。Codespaces 連携は別製品の領域。ただし**セキュリティ情報として**: Kagi が `gh codespace ports forward` を呼ぶことは無いはずだが、**Kagi が何らかのポートを開く実装（single-instance ソケット等）を持つなら、GHSA-vfhh-p7hm-pxfh と同じ間違い（全インターフェースへの bind）をしていないか確認する価値がある**。
- **難易度**: —

#### GH-20. GitHub MCP server
- **何か**: GitHub のデータを MCP（Model Context Protocol）経由で AI に渡すサーバ。
- **出典**: 本調査では `gh` に MCP 関連サブコマンドは**存在しない**（`gh --help` 実測に該当項目なし）。`gh copilot` が Copilot CLI を起動する形（`--allow-tool 'shell(git)'` のようなツール許可モデルを持つ、GH-15 実測）。**独立した GitHub MCP server の一次情報を本調査の範囲では確定できなかった** → 「未確認」。
- **Kagi への示唆**: Kagi が AI に GitHub context を渡す手段としては、**MCP を待つよりも `gh` の `--json` 出力を直接使うほうが確実**（GH-4/5/8/9/11/13 で列挙した `--json` フィールドと GraphQL は全て実測済みで安定している）。Kagi 自身が MCP server として「Kagi のグラフ・oplog・conflict 状態」を公開する側になる設計のほうが Kagi 固有の価値が出る[推測]。
- **難易度**: **L**（Kagi が MCP server 側になる場合）。§5 の未解決の疑問へ。

---

## 3. Kagi 取り込み候補（優先順）

| # | 提案 | 効果 | 難易度 | 依存 | 出典 |
|---|---|---|---|---|---|
| 1 | **`git history` (drop/fixup/reword/split) を `--dry-run` → confirm → `update-ref --stdin` で実装**。plan の中身に「更新される ref の一覧」をそのまま表示 | 履歴改変が Kagi の安全モデルに完全に乗る。中断状態なし・hooks 非実行。`split` は新規ユーザー価値 | M | git 2.54+（`fixup` は 2.55+）。バージョン検出と rebase fallback | RelNotes 2.54/2.55、git-history.adoc |
| 2 | **ruleset の (A) 群をローカル検証エンジンとして実装**（`GET /rules/branches/{branch}` を 1 度取得してキャッシュ → 以降ネットワーク往復なし）: コミットメッセージ入力欄で `commit_message_pattern` をライブ検証、staging 時に `max_file_size` / `file_extension_restriction` / `file_path_restriction` / `max_file_path_length` を検証、ブランチ作成ダイアログで `branch_name_pattern` を検証（**ブランチ不存在でも取得可**）、`required_signatures` で署名未設定を警告、`required_linear_history` でマージ方式から merge commit を除外 | **「push が拒否される変更」を commit 前に止める**。コミット後に気づくと履歴からの除去が必要で、それは Kagi が最も助けたい作業。ネットワーク往復ゼロで検証できるのが決定的 | S〜M | `repo` scope のみ（admin 不要、実測）。既存の staging / commit / branch 作成 UI | 実測レスポンス + rules.md 実読（§GH-1 分類 A）|
| 2b | **ruleset の (B) 群を PR/merge の preflight に**: `non_fast_forward` で強制更新を plan 段階で拒否、`allowed_merge_methods` で merge メソッドをグレーアウト、`pull_request.*`（承認数 / CODEOWNERS / 未解決スレッド / last-push 承認 / **未帰属 AI 変更の追加承認**）を merge 前に表示、`merge_queue` があれば「キュー経由のみ」を明示、`required_status_checks` の `context` 一覧を CI 表示に突き合わせ | 「押せるのに失敗する」を「押せない / なぜ押せないかが分かる」に変える | M | #2 と同じ 1 リクエスト。#11（merge queue）、#24（`mergeStateStatus`）と統合 | 実測レスポンス + rules.md 実読（§GH-1 分類 B）|
| 3 | **AI 帰属の可視化**: `copilot/` ブランチ / `app/github-copilot` PR author / `Co-authored-by: Copilot <copilot@github.com>` trailer をグラフ上でバッジ表示。`%(trailers:key=Co-authored-by)` で読む | 「AI native な Git GUI」の中核。Kagi のグラフ資産（レーン・solo・ghost connector）に AI 軸が乗る | M | `git log --format=%(trailers:...)`（既存機能）、`gh pr list --app`（実測） | §GH-13、G-15、changelog 2025-10-16 |
| 4 | **`gh agent-task create/list/view` を Kagi から駆動**。`--base` に「グラフで選択中のブランチ」、`-F -` で「選択した diff / conflict / Analyze 結果」を渡す。`--custom-agent` は `.github/agents/*.md` を読んで選択 UI に | Kagi 上のコンテキストを AI に渡す最短経路。`view --log` でセッションログをコミットに紐づけると他 GUI に無い体験 | M | gh 2.97+（preview なので薄いラッパ）。#3 と併用 | ローカル `gh agent-task --help` 実測 |
| 5 | **`git repo info -z` で repo 初期化を 1 コマンドに集約**し、`references.format` で reftable を検出。ref 読み取りを `for-each-ref` / `refs list` に統一（`.git/refs` 直読みを撤去） | Git 3.0（reftable 既定）への互換性確保 + クォート解除ロジックの削除 | S | git 2.52+（`--all` は 2.53+、`--keys` は 2.54+）。`--keys` で feature detection 可 | git-repo.adoc 実読、RelNotes 2.51/2.52 |
| 6 | **`git last-modified -z` でファイルツリーの「最終更新コミット」列を実装** | per-file `git log` の N 回呼びが 1 回のツリー走査に。埋め込みエディタのファイルツリーに直接効く | S | git 2.52+ | git-last-modified.adoc 実読 |
| 7 | **PR review thread を diff に重畳**。`PullRequestReviewThread` の `path`/`line`/`startLine`/`diffSide` で split view に配置、`isOutdated` を薄く落とす、`viewerCanResolve` を見て解決ボタンを出す | 既存の conversation 表示（リスト）を diff 上の指摘に格上げ。AI レビューが大量にコメントする時代に効く | M | GraphQL への移行。既存の split view diff レンダラ | 実測（§GH-9） |
| 8 | **suggested changes をローカル適用**。```suggestion フェンスをパースして作業コピーに適用 → 既存の hunk staging UI で選別 → commit | **Web UI より優位に立てる箇所**。oplog / ODB バックアップの保護下で AI 提案を選別できる | M | 既存の hunk staging UI、conflict editor の資産 | §GH-10（適用 mutation は不在を確認） |
| 9 | **CODEOWNERS を自前パースし、Analyze の ownership と突き合わせる**（宣言 vs 実態の乖離）。staging 画面で「@team-x のレビューが必要」を予告 | 既存 ownership の拡張。他の Git GUI に無い分析軸。API は owner を返さないので自前パースが必須 | M | 既存 Analyze/ownership。`codeowners/errors` で構文検証 | 実測（§GH-17） |
| 10 | **`git log --remerge-diff` をマージコミット表示に採用** | マージコミットで「人間が実際に何を判断したか」だけが見える。conflict editor の解決内容の事後レビューにもなる | S | git 2.36+（バグ修正は 2.48+ なので実質 2.48+ 推奨） | RelNotes 2.36/2.47/2.48 |
| 11 | **merge queue の状態を PR 画面に表示**（`position`, `estimatedTimeToMerge`, `state`, `nextEntryEstimatedTimeToMerge`）。`enqueuePullRequest` / `dequeuePullRequest` を提供。`jump`/`solo` は二段確認 | merge ボタン後の「終わっていない」ライフサイクルを可視化。既存 PR merge と重複しない別経路 | M | `gh pr merge --queue` は存在しないので `gh api graphql` を直接。ポーリング | 実測（§GH-8） |
| 12 | **`gh pr merge` に常に `--match-head-commit <SHA>` を付ける**。**`current_user_can_bypass` が `never` なら `--admin` ボタンを出さない**。出す場合は GH-1 の (A)(B) から「踏み越える具体的なルール名とパラメータ」を列挙して二段確認 | force-with-lease と同じ設計原則の PR 版。「マージ確認中に他人が push」を構造的に防ぐ。`--admin` の一貫性回復（`push --force` を持たない製品が無条件 admin merge を出すのは矛盾）| S | #2 / #2b（GH-1）。`GET /repos/{o}/{r}/rulesets` の `current_user_can_bypass`（enum: `always`/`pull_requests_only`/`never`/`exempt`）| 実測（§GH-12）、rules.md 実読（§GH-1 分類 C）|
| 13 | **`gh issue develop --checkout --worktree` / `gh pr checkout --worktree` を worktree 作成 UI に統合**。同時に**`gh` の必須バージョンを 2.99+ に上げる** | issue/PR に紐づく worktree が 1 操作で作れる。2.99 未満の `pr merge --delete-branch` による worktree 破損を回避 | S | gh 2.99+（2.98 で pr checkout、2.99 で issue develop） | gh 2.98.0 / 2.99.0 リリースノート |
| 14 | **`git maintenance is-needed` + commit-graph / fsmonitor の検出 → 「有効化しますか？」の提案 UI**（勝手に実行しない） | GUI が固まる最大要因（重いメンテ、巨大 worktree の status）に、Kagi の plan→confirm パターンで対処 | S | git 2.53+（`is-needed`）。fsmonitor は Linux は 2.55+ | RelNotes 2.52/2.53/2.54/2.55、`git help maintenance` 実測 |
| 15 | **stack オーバーレイをグラフに描き、`gh stack link` で GitHub 側の stack を作る**（Kagi が鎖のローカル真実源になる） | Kagi の最も得意な領域（ブランチ鎖の視覚化）にサーバ側の正規表現が来た。`link` はローカル追跡状態を要求しないので Kagi が真実源になれる | L（`link` 呼び出しのみなら S） | `github/gh-stack` v0.1.0（若い）。#16（`--update-refs`）| 実測 `gh stack --help`、https://gh.io/stacks |
| 16 | **`rebase --update-refs` 使用時に `rebase.instructionFormat` を無害化**（`%d` があるとブランチを壊す 2.55 未満のバグ）。preflight の 1 項目に | 実在するデータ損失バグの回避。stack rebase の前提 | S | git 2.55 で修正済みだが Kagi は古い git も相手にする | RelNotes 2.55.0（`ag/rebase-update-refs-limit-to-branches`）|
| 17 | **署名 UI の是正**: 「鍵の期限切れ」と「署名が無効」を区別（期限切れ鍵で署名された古いコミットを警告色にしない）。履歴改変の二段確認に「署名が失われます」を追加 | git 本体が実際に間違えて 2.54 で直した UX バグ。Kagi も同じ間違いをしている可能性が高い | S | 既存の署名表示。#1（`git history` は署名を無効化する） | RelNotes 2.54.0、2.53.0（`pw/replay-exclude-gpgsig-fix`）|
| 18 | **ターミナルエスケープシーケンスの無害化**: リモート由来の文字列（sideband メッセージ、PR 本文、review コメント）を表示する全経路 | **セキュリティ**。git 2.55 と `gh pr diff` の両方が既定で無効化した脅威。Kagi は埋め込みターミナルを持つので直撃する | S | 既存の Markdown レンダラ、埋め込みターミナル | RelNotes 2.55.0、`gh pr diff --allow-escape-sequences` 実測 |
| 19 | **`blame --diff-algorithm=histogram` を Analyze/ownership に採用**。`--porcelain` の unblamable / ignored-commit 帰属行を区別表示 | 「移動したコードを別人の追加と誤帰属する」を減らす → ownership の質が上がる（ユーザーに見える改善）| S | git 2.53+（blame の algorithm）、2.50+（porcelain の未帰属表示）| RelNotes 2.53.0、2.50.0 |
| 20 | **`--graph-indent` の設計を採用**: 親が表示範囲外のコミット（ページング境界の visual root）をインデントで区別 | Kagi のグラフでも起きる問題に upstream が出した答え。既存 ghost connector とは別課題 | S | 既存のグラフ描画。レーン上限はレーン安定化と衝突しうるので慎重に | rev-list-options.adoc 実読（`--graph-lane-limit` / `--graph-indent` / `log.graphIndent`）|
| 21 | **`hook.<event>.enabled=false` でフックを設定単位で無効化**（`--no-verify` でユーザーの意図を曲げるのをやめる）。plan に「フックを無効化します」を明示 | 「フックが GUI を固める / 予期せぬ副作用」に筋の通った対処。`--no-verify` は plan に書けない（意図の改変）が設定なら書ける | S | git 2.54+ | Documentation/config/hook.adoc 実読、RelNotes 2.54.0/2.55.0 |
| 22 | **`gh search issues --search-type semantic`** で「選択中のコミット/diff に関連する issue」を自然言語検索 | AI に渡す context の自動収集。1 API で済む | S | gh 2.98+ | gh 2.98.0 リリースノート、changelog 2026-04-02 |
| 23 | **sub-issues + issue dependencies のグラフ描画**（`subIssues`, `subIssuesSummary`, `parent`, `blockedBy`, `blocking`）。既存のレーンアルゴリズムを再利用 | 「作業側の依存グラフ」を commit graph と同じエンジンで描く = Kagi 固有の強み。AI に渡す作業の木になる | M | GraphQL / `gh issue view --json`（全フィールド実測済み）。既存グラフ描画 | 実測（§GH-4, §GH-5）、changelog 2024-12-12 / 2025-03-18 |
| 24 | **`mergeStateStatus` の 4 状態に応じたアクション提示**（`BEHIND`→rebase / `DIRTY`→conflict editor / `BLOCKED`→承認待ち / `UNSTABLE`→非必須チェック失敗）。`gh pr checks --watch --fail-fast` をバックグラウンドジョブにして CI 失敗を通知 | 既存の PR conflict preview は `DIRTY` のみを扱う。他 3 状態への対応は新規。CI の能動通知も新規 | S / M | 実測の enum（`DIRTY, UNKNOWN, BLOCKED, BEHIND, UNSTABLE, HAS_HOOKS, CLEAN`）| 実測（§GH-11）|
| 25 | **`git url-parse` で remote URL → owner/repo の自前パースを置換** | SSH / HTTPS / scp-like の全パターンを git 本体の実装に委譲。GitHub 連携の owner/repo 推定はバグの温床 | S | git 2.55+（fallback に自前パースを残す）| RelNotes 2.55.0 |
| 26 | **`git format-rev` でコミットメッセージ本文中の SHA を自動リンク/短縮** | 自前の SHA 正規表現検出を git 本体に委譲 | S | git 2.55+ | RelNotes 2.55.0 |
| 27 | **`git stash export/import` で stash の移送**（別マシン / 別 worktree へ）。`stash.index` を stash 適用 UI の既定値に | stash は本来ローカル専用だったので、移送は新規価値。Kagi の repo タブ / worktree 間で効く | M / S | git 2.51+（export/import）、2.52+（`stash.index`）| RelNotes 2.51.0、2.52.0 |
| 28 | **`git repo structure` を Analyze の新軸「リポジトリ健全性」に**（巨大 blob の特定、ref 爆発の検出、型別ディスクサイズ）。**cruft pack の expire 設定を読んで discard バックアップの有効期限を正確に表示** | 既存の hotspots/coupling/ownership と重複しない新軸。バックアップ期限は既存機能の正確性の問題 | M / S | git 2.52+（structure）。`gc.pruneExpire` / `--max-cruft-size` / `--combine-cruft-below-size` の読み取り | RelNotes 2.52/2.53/2.54、2.41/2.43/2.50（cruft）|
| 29 | **`git add -p` の 2 つの UX 修正を staging UI に追随**: (a) hunk を選択後 split したら分割片は「未決定」に戻す、(b) 処理済みファイルを再訪できる | upstream が「より良い end-user experience」として実際に直した挙動。Kagi の staging UI が同じ間違いをしている可能性が高い | S | 既存の hunk staging UI | RelNotes 2.52.0、2.54.0 |
| 30 | **`worktree list --porcelain` を必ず `-z` 付きに**。`.git/info/exclude` が worktree 間で共有される旨を ignore UI に明示。`worktree add --orphan` を選択肢に追加 | `-z` 無しはパスに危険バイトがあると壊れる（正しさの問題）。exclude 共有はユーザーが必ず誤解する箇所 | S | git 2.36+（`-z`）、2.42+（`--orphan`）| RelNotes 2.36.0、2.55.0（exclude doc）、2.42.0 |
| 31 | **`gh pr create/comment --attach` で Kagi のグラフ/diff スクリーンショットを PR に添付** | Kagi は画像レンダリングを既に持つ。「Kagi で見た状態」をそのまま議論に持ち込める | S | gh 2.99+（1 回 50 ファイルまで）| gh 2.99.0 リリースノート、https://gh.io/gh-attach |
| 32 | **`gh` 呼び出し時の出力安定化**: coding-agent 検出用の環境変数を立ててスピナー等の ANSI 制御を抑止 | `gh` は呼び出し元がエージェントかを検出して出力を変える（2.99）。Kagi のパーサが ANSI で壊れるのを防ぐ | S | gh 2.99+ | gh 2.99.0 #14191、2.98.0 #14072（`GH_EXTENSION=1`）|
| 33 | **`git for-each-ref --start-after` で ref 一覧をサーバ側ページング**。`git refs exists` を preflight の ref 存在確認に | ref が数万あるリポジトリでブランチ一覧の仮想スクロールが軽くなる（reftable と組合せると特に）| S | git 2.51+（`--start-after`）、2.52+（`refs exists`）| RelNotes 2.51.0、2.52.0 |
| 34 | **`git replay --onto --ref-action=print` で「worktree を汚さないバッチ rebase」**（他 worktree のブランチをそこを checkout せずに rebase）。`--revert` で worktree クリーンなまま revert | Kagi は複数 worktree / repo タブを持つのでこれが効く。`--ref-action=print` は #1 と同じ preflight プリミティブ | M | git 2.44+（実用は 2.54+: revert / root / empty-drop）。experimental | git-replay.adoc 実読、RelNotes 2.44/2.53/2.54 |
| 35 | **`rev-list` の NUL 区切り機械可読出力に移行**（`--maximal-only` で tip 抽出、`--max-count-oldest` で逆方向ページング） | Rust 側パーサから改行/クォートの曖昧さを排除。逆方向スクロールが 1 コマンドに | S | git 2.50+（NUL 出力）、2.54+（`--maximal-only`）、2.55+（`--max-count-oldest`）| RelNotes 2.50.0、2.54.0、2.55.0 |
| 36 | **`zdiff3` を conflict editor の表示形式の選択肢に追加**（既存 diff3 の隣）。`sparse-checkout clean` を「安全な clean」として二段確認 + ODB バックアップ付きで提供 | zdiff3 は共通行を marker 外に出すのでコンフリクト領域が小さく読める。`git clean` を持たない Kagi にとって「安全な clean」は唯一の候補 | S / M | git 2.35+（zdiff3）、2.52+（sparse-checkout clean）。既存の二段確認 + ODB バックアップ | RelNotes 2.35.0、2.52.0 |

---

## 4. 取り込まないと判断したもの（理由付き）

| 項目 | 理由 |
|---|---|
| **`scalar` の呼び出し** | scalar は「複数の設定を一括で勝手に変える」ツールで、Kagi の「全書き込み操作が plan → confirm を通る」原則と正面衝突する。**ただし scalar が有効化する設定の一覧（fsmonitor / commit-graph / maintenance / partial clone / sparse-checkout）は、Kagi が個別に提案すべき項目リストとして最良の参考**であり、その形（#14）で取り込む。 |
| **bundle-uri（clone 高速化）** | 効果が clone 時のみで、Kagi の主戦場（既存リポジトリの操作）に効かない。ただし 2.53 の「不正 bundle-URI で crash しない」修正が示すように、Kagi が clone をラップするなら bundle-uri 由来のエラーを握る程度の認識は必要。 |
| **`git notes`** | 2.40〜2.55 の範囲で機能追加が無い（維持フェーズ）。ユーザー母数が小さく、リモート同期（`refs/notes/*` の fetch/push refspec を自分で設定する必要がある）が煩雑で、GUI で提供しても混乱を招く可能性が高い。 |
| **Projects v2 API / `gh project`** | Kagi の関心は「コード ↔ 作業項目」。それは issue 階層と issue dependencies（#23）で十分に表現できる。Projects v2 は「ビュー・カスタムフィールド・ロードマップ」という**プロジェクト管理ツールの領域**で、Git GUI が持つべき機能ではない。GraphQL のみで REST が無く、`ProjectV2Item` のフィールド値操作（`updateProjectV2ItemFieldValue`）はスキーマが動的でクライアント実装コストが高い。 |
| **`gh attestation`（artifact attestation / SLSA provenance）** | コミット署名（誰が書いたか）とは別レイヤの「どのビルドがこの artifact を作ったか」の証明。Kagi はソースコードの GUI であって artifact / release の GUI ではない。 |
| **`gh cache` / `gh workflow` / `gh run`（CI ログビューア化）** | PR の CI 状態は `gh pr checks`（#24）で足りる。Actions のキャッシュ管理・ワークフロー定義編集・実行ログの閲覧まで持つと Kagi が「GitHub の GUI」になってしまい、「コミットグラフ中心の Git クライアント」という製品の輪郭が崩れる。 |
| **`gh discussion`** | Discussions はコードと結びつかないコミュニケーション機能。Kagi の PR review conversation とは目的が違う。 |
| **Codespaces / devcontainer 連携** | **本調査で「Codespaces とローカル worktree の間に GitHub 提供の対応関係・同期機構は存在しない」ことを確認**した。Codespace は「リモートコンテナ 1 個 = 1 チェックアウト」で linked worktree とは無関係な概念。Kagi の価値は「ローカルの worktree を安全に扱う」ことなので、別製品の領域。 |
| **`gh skill` による Kagi skill の配布（現時点）** | preview であり、かつ Kagi 側の「AI に何を公開するか」のインターフェース設計が先。ただし**「`~/.agents/skills` が Copilot / Codex / Pi 共通の置き場になりつつある」という事実は監視すべき**（gh 2.99 の #14154 / #14260）。 |
| **`git log --graph-lane-limit` のレーン上限そのもの** | Kagi は独自のレーン安定化アルゴリズムを持ち、レーン数を機械的に切って `~` に置換するのはレーンの連続性（Kagi の中核価値）を壊す。**取り込むのは実装ではなく `--graph-indent` の設計判断のみ**（#20）。 |
| **suggested changes の GitHub 側「適用」** | Mutation イントロスペクションで該当 mutation が見つからず、Web UI 専用機能である可能性が高い[推測]。そもそも Kagi はローカルに作業コピーを持つので、**GitHub 側の適用を経由せずローカルに直接適用するほうが優れている**（#8）。 |
| **`gh pr diff` の出力を Kagi の diff 表示に使う** | Kagi は自前の split view / syntax highlight / hunk 操作を持つ。`baseRefOid` / `headRefOid`（実測でフィールド存在確認）を取ってローカルで diff すべき。`gh pr diff` から取り込むのは `-e/--exclude` の発想と `--allow-escape-sequences` が既定オフである理由（#18）だけ。 |
| **histogram diff の既定化** | 2.40〜2.55 のリリースノートに「既定を histogram にする」変更は記載されていない（既定は依然 Myers）。**Kagi が独自に既定を変えるべきではない**（`git diff` の出力と Kagi の表示が食い違うのは最悪）。blame の ownership 計算という**内部用途に限って** histogram を使う（#19）。 |
| **`git bisect` の GUI 化（今回は見送り）** | 価値は高い（Kagi のグラフ上で探索範囲が縮むのが見えるのは強い体験）が、2.55 の「カスタム用語（old/new）の出力一貫性」修正が示すようにパースが不安定で、かつ bisect は**中断状態を持つステートフルな操作**。Kagi の oplog / undo との統合設計が別途必要なので、#1〜#36 より後の独立トピックとする。 |
| **`git absorb` の自動振り分け** | 公式には存在しない（`git history fixup` は対象コミットを明示指定する）。ただし **Kagi はグラフを持つので「どのコミットに吸収させるか」を視覚的に選ばせるほうが、CLI の自動判定より人間に優しい** — これは #1 の `fixup` の UI として実現する。 |

---

## 5. 未解決の疑問

1. **`git history` の experimental 度合い** — 2.54 で導入・2.55 で `fixup` 追加という速いペースだが、「THE BEHAVIOR MAY CHANGE」が明記されている。Kagi が主要な履歴改変経路をこれに賭けるのは早いか？ 最低限、rebase 経路との二重実装が必要か、それとも `--dry-run` の出力形式（`update-ref --stdin` 互換）だけは安定と見なせるか。**判断材料**: `--dry-run` の出力が `git update-ref --stdin` の入力形式である以上、その形式は git の安定 API なので**出力形式は変わりにくい**[推測]。変わるのはサブコマンドの追加とオプション名か。
2. **`GET /rules/branches/{branch}` に旧 branch protection が含まれるか（部分的に解消）** — 仕様書は「**設定されたレベル（例: リポジトリ or organization）に関わらず、適用される全 active ルールが返る**」とだけ述べており、**「classic branch protection rule」を含むと明言していない**。実測で `kubernetes/kubernetes@master` が `[]` を返したのは (a) ruleset を使っておらず classic branch protection のみ、(b) そもそも保護なし、のどちらかで、本調査では切り分けられなかった（当該リポジトリの admin 権限がないため `branches/{b}/protection` と比較できない）。**残る確認事項**: classic branch protection のみのリポジトリで、このエンドポイントが空を返すか protection 相当のルールを返すか。→ **これが #2 / #2b の適用範囲（「ruleset を使っている org でのみ有効」なのか「全リポジトリで有効」なのか）を決める**。**実務的な安全側の設計**: 「ルールが返らなかった = 制約なし」と解釈してはならない。Kagi は空レスポンスを「**不明**」として扱い、既存の（ルール取得前の）保守的な確認フローを維持すべき。これは実装上の確定した指針にできる。
3. **`require_extra_approval_for_unattributed_changes` の判定基準** — GitHub 側は「app identity で PR を開いたか」で判定する。Kagi 側は trailer（`Co-authored-by: Copilot`）で判定する。**この 2 つはどれだけ乖離するか？** 人間が Copilot 支援で書いて自分の identity で push した PR は GitHub 的には「帰属済み」だが trailer は付いている。Kagi はどちらを「AI 関与」として見せるべきか。→ **両方を別ラベルで見せる**のが正解か（「AI 支援」vs「AI 作成」）。
4. **`git repo info` のキー安定性** — `--keys` で列挙できるので前方互換な feature detection は可能だが、`references.format` のような重要キーが将来リネームされないか。experimental 表記があるので Kagi は `--keys` を使った動的検出を前提にすべきか、それとも固定キー + fallback で十分か。
5. **merge queue のポーリングコスト** — `position` と `estimatedTimeToMerge` はリアルタイム性が価値なので頻繁に取りたいが、GraphQL の rate limit（point 計算）とデスクトップアプリの常時ポーリングは相性が悪い。**webhook を受けられないデスクトップアプリで、どの間隔が現実的か？** `gh pr checks --watch --interval` の既定 10 秒が参考になるが、merge queue 全体の混雑度まで取ると重い。
6. **`github/gh-stack` v0.1.0 の安定性と、サーバ側 stack の API** — `gh stack` は CLI として提供されているが、**サーバ側の stack を GraphQL / REST で直接読み書きできるのか**を本調査では確認できなかった（Mutation 一覧に stack 系の mutation は見当たらなかった）。Kagi が `gh stack` の subprocess 呼び出しに依存するのは、`gh` と拡張の両方のバージョンに縛られるので望ましくない。**gh.io/stacks のドキュメントに API が記載されているかの確認が必要**。
7. **GitHub MCP server の実在と機能** — 本調査では一次情報を確定できなかった（`gh` にサブコマンドは無い）。`gh copilot` の `--allow-tool 'shell(git)'` というツール許可モデルが存在することは実測できたので、**「AI に git を触らせる際の許可粒度」の設計は既に前例がある**。Kagi が MCP server 側になる（Kagi のグラフ・oplog・conflict 状態を公開する）場合、この allow-list モデルを踏襲すべきか。
8. **Rust FFI（Git 3.0）の Kagi への影響** — git 2.55 で「Rust サポートが既定有効、Git 3.0 で必須」、2.52 で「libgit.a に統合（後の Rust FFI 作業を助ける）」。**Kagi は Rust 製なので、将来「git のサブプロセス起動」から「libgit への直接 FFI」に移れる可能性がある**。これはアーキテクチャの根本判断（現在の libgit2 依存 / CLI 呼び出しの比率）に影響するが、Git 3.0 のリリース時期と FFI の公開範囲が不明。**Kagi が今から取るべき備えは「git CLI の呼び出しを一箇所に集約しておく」ことか**[推測]。
9. **Git 3.0 の破壊的変更の全リスト** — 本調査で確認できたのは 4 件: reftable が新規リポジトリの既定（2.51 宣言）、`git init` の既定ブランチが `main`（2.52 宣言）、symlink symref の消滅（2.52 宣言）、Rust が必須（2.55 宣言）。**`git whatchanged` の廃止予定（2.51）と `core.commentChar=auto` の deprecated（2.52）も含まれる。Kagi はこれら全てについて依存の有無を監査すべきだが、Git 3.0 の完全な破壊的変更リストは本調査時点で未公開。**
