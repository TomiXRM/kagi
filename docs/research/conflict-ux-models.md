# Conflict モデル比較調査(jj / GitButler / Git CLI)— kagi 実装基盤の観点

- 調査日: 2026-06-13 / 調査者: UX research subagent
- スコープ: **conflict のデータモデルと表現(UI ではなく)**。jj の first-class conflict、GitButler の virtual-branch / conflicted commit、Git CLI の merge/rebase/cherry-pick/revert conflict 状態。
- 前提(既存研究と非重複): `docs/research/jj-reuse-research.md`(`Merge<T>` separability・流用区分)、`docs/research/gitbutler-reuse-research.md`(FSL ゲート・oplog)、`docs/adr/0005`(conflict 取り扱い)、`docs/adr/0031`(流用ポリシー)、`docs/adr/0032`/`0033`。本書は **conflict の内部データモデルと Git CLI の状態表現** に踏み込み、kagi の git2 0.21 実装基盤に落とす。
- 表記: 不確実な箇所は **推測**。URL はインラインで明示。

---

## 0. なぜ「conflict モデル」を比較するのか

3 システムは「conflict とは何か」のデータモデルが根本的に異なる:

- **Git CLI**: conflict は **一過性の例外状態**。index の stage 1/2/3 + worktree のマーカー + `.git/` 下の状態ファイルで表現し、解決するまで作業が止まる(operation が pause する)。commit には conflict を記録できない。
- **jj**: conflict は **commit content の一部**(first-class)。commit object が「複数 tree の列」を内包し、解決しないまま rebase が成功する。
- **GitButler**: jj の思想を Git 上に移植。**synthetic root tree を持つ特殊な conflicted commit** を書き、rebase を常に成功させる。

kagi は git2 0.21 単一 backend(ADR-0002)であり、実装基盤は完全に **Git CLI のモデル**。jj / GitButler は「将来 Conflict Mode を設計する際の思想ソース」として読む。

---

## 1. システム別ルーブリック

| 観点 | Git CLI | jj (Jujutsu) | GitButler |
|---|---|---|---|
| **conflict とは(データモデル)** | index の **stage 1/2/3 エントリ** + worktree のマーカーファイル。commit には保存不可 | **commit content**。`Merge<T>` = 奇数個の tree を「正項・負項」交互に持つ列(`A+(C-B)` が 3-way) | **synthetic root tree** を持つ特殊 commit。各 side を subtree として格納 + auto-resolution subtree + 特別 commit header |
| **追跡単位** | **path 単位**(index entry が path×stage)。worktree ファイル単位でマーカー | **tree/path 単位**。conflict は `MergedTree` 内の per-path `Merge<TreeValue>` | **commit 単位**(conflicted フラグ付き commit)+ 内部は per-path tree |
| **部分解決の永続化** | **不可**(index に書き戻すまで揮発、解決途中は worktree のみ)。`git add` で stage 2 に確定 → stage 1/3 が消える | **可**。次の snapshot で worktree 状態が自動取り込み。部分的に直しても commit に保存され、残りは conflict のまま残る | **可**。edit mode で直した分を保存、commit が conflicted のまま部分更新できる(推測: 内部 tree を再構成) |
| **undo** | `ORIG_HEAD` / reflog による手動巻き戻し。abort コマンド | **op log**(content-addressed operation DAG)。`jj op undo` で任意操作を巻き戻し、conflict 解決自体も undo 可 | **oplog**(snapshot = Git tree)。操作前 snapshot へ復帰 |
| **進行中 operation の表現** | `.git/` 下の状態ファイル(`MERGE_HEAD` 等)+ `git_repository_state` enum | **存在しない**。conflict は commit に入り operation は即完了。「進行中」概念がない | rebase は常に完走。**進行中状態を持たない**(conflicted commit が残るだけ) |
| **sequencing(continue/abort/skip)** | `--continue` / `--abort` / `--skip` が operation ごとに存在 | 不要。`jj resolve`(任意タイミング)+ `jj squash`/`amend` のみ。abort = `jj op undo` | 不要。「Resolve Conflicts」ボタン → save。abort = oplog 巻き戻し |
| **rename/delete** | rename は merge.renames で検出。modify/delete conflict は stage 2 or 3 の片方欠落で表現 | tree merge で per-path に統一的に表現(`Merge<Option<TreeValue>>` で削除を None として扱う) | per-path tree。jj 同様(推測) |
| **binary** | merge driver なしならマーカー不可、stage 1/2/3 を残し手動選択 | materialize 不可な値は side 全体提示(推測) | 同様(推測) |

---

## 2. Git CLI conflict モデル(kagi 実装基盤 — 精密版)

### 2.1 index stage エントリ(1=base / 2=ours / 3=theirs)

conflict 時、index は同一 path について **最大 3 つの stage エントリ**を持つ([git-merge](https://git-scm.com/docs/git-merge)、[Index in git2](https://docs.rs/git2/latest/git2/struct.Index.html)):

- **stage 1 = common ancestor(base / merge base)**
- **stage 2 = "ours"(HEAD 側)**
- **stage 3 = "theirs"(MERGE_HEAD 側)**

通常の clean entry は stage 0。conflict が残る限り stage 0 は存在せず、stage 1/2/3 が並ぶ。`git add <path>` すると stage 1/2/3 が消えて stage 0 が作られる = 解決確定。modify/delete conflict は欠落 side の stage が無い(例: theirs で削除 → stage 3 欠落)。

### 2.2 ours/theirs の意味反転(rebase で SWAP する)— 最重要落とし穴

**merge**: ours = 現在いるブランチ(HEAD)、theirs = 取り込む側(MERGE_HEAD)。

**rebase / cherry-pick / pull --rebase**: 意味が **反転**する([git-checkout docs](https://git-scm.com/docs/git-checkout)、[riptutorial](https://riptutorial.com/git/example/1422/rebase--ours-and-theirs--local-and-remote)):

- rebase は最初に HEAD を **onto(rebase 先)** に move する。よって作業中の各コミットは「third-party の変更」として onto の上に適用される。
- 結果: **`--ours` = rebase 先(onto, 取り込み先の正史)**、**`--theirs` = 自分が rebase しているコミット(自分の作業)**。
- `-Xours` / `-Xtheirs` の意味も merge と rebase で逆になる。

> kagi 実装注意: conflict UI で「ours / theirs」をそのまま見せると、rebase 中ユーザーは確実に混乱する。**進行中 operation 種別(`git_repository_state`)を見て、ラベルを「rebase 先 (onto) / あなたの変更 (commit X)」のように文脈名に翻訳すべき**。生の ours/theirs を出さない。これは kagi の差別化ポイントになりうる。

### 2.3 状態ファイルの所在(`.git/` 下)

| operation | マーカー ref | state dir | abort 基準 ref |
|---|---|---|---|
| merge | `.git/MERGE_HEAD`(+ `MERGE_MSG`, `MERGE_MODE`) | — | `ORIG_HEAD` |
| cherry-pick | `.git/CHERRY_PICK_HEAD` | `.git/sequencer/`(複数 pick 時 todo/done) | `ORIG_HEAD` |
| revert | `.git/REVERT_HEAD` | `.git/sequencer/` | `ORIG_HEAD` |
| rebase(merge backend / 既定) | — | **`.git/rebase-merge/`**(`git-rebase-todo`, `done`, `onto`, `head-name`, `orig-head`) | `ORIG_HEAD`, `rebase-merge/orig-head` |
| rebase(apply backend / `am`) | — | **`.git/rebase-apply/`** | 同上 |

出典: [git-rebase docs](https://git-scm.com/docs/git-rebase)(「The two backends keep their state in different directories under .git/」)、[lazygit issue #5184](https://github.com/jesseduffield/lazygit/issues/5184)(rebase-merge 半書き込みの破損例)。`ORIG_HEAD` は HEAD を大きく動かす操作(am/merge/rebase/reset)が操作前 HEAD を記録する([Peter Eisentraut](http://peter.eisentraut.org/blog/2022/04/21/git-rebase-and-ORIG_HEAD))。

### 2.4 conflict marker style(merge / diff3 / zdiff3)

`--conflict=<style>` / `merge.conflictStyle` で worktree マーカー形式が変わる([git-checkout docs](https://git-scm.com/docs/git-checkout)):

- **merge(既定)**: 2-way。`<<<<<<< HEAD` / `=======` / `>>>>>>> branch`。base を見せない。
- **diff3**: base 領域を `||||||| merged common ancestor` で挿入。**3-way の全体像が見える**(なぜ衝突したか理解しやすい)。
- **zdiff3**: diff3 + zealous(両 side で共通な行を conflict 領域外へ追い出す)。マーカー内のノイズが減る。Git 2.35+。

> kagi 実装注意: 3-way 解決 UI(ADR-0005 の v1.0)では **zdiff3 相当**を内部生成すると base 文脈が出せて UX が良い。git2 の `MergeFileOptions::style_zdiff3(true)` で取得可能(2.6 節)。

### 2.5 rerere(reuse recorded resolution)

[git-rerere docs](https://git-scm.com/docs/git-rerere):

- **記録対象**: 衝突状態(preimage = マーカー入りファイル)と手動解決後(postimage)。
- **保存先**: `.git/rr-cache/<preimage hash>/`(`preimage` / `postimage` / メタ)。
- **再生**: 新たな衝突で preimage が一致したら、preimage/postimage/今回の衝突の 3-way を実行し、clean に通れば worktree に解決を書く(`git add`/`commit` は依然手動。`rerere.autoupdate` で stage 自動化)。
- **既定 off**。`rerere.enabled true` で有効。`gc.rerereResolved`(60日)/`gc.rerereUnresolved`(15日)で剪定。`rebase --abort`/`am --abort` は rerere メタを自動 clear。

> kagi 実装注意: rerere は **kagi 独自の「同一衝突の解決を覚える」機能を後から自前実装するなら設計言語**になる。libgit2 は rerere を直接 API 化していない(推測: 高確度)。kagi が rerere を使うなら `.git/rr-cache` を `git` サブプロセス経由で触るか、自前で preimage hash → resolution map を持つ。MVP 不要。

### 2.6 git2 0.21(libgit2 binding)が露出するもの — 実装基盤

kagi の `Cargo.toml`: `git2 = "0.21"`。以下が直接使える(出典: [git2 Index](https://docs.rs/git2/latest/git2/struct.Index.html)、[git2 Repository](https://docs.rs/git2/latest/git2/struct.Repository.html)、[git2-rs/src/merge.rs](https://github.com/rust-lang/git2-rs/blob/master/src/merge.rs)):

**(a) conflict 検出・列挙(index)**
- `Index::has_conflicts() -> bool`
- `Index::conflicts() -> Result<IndexConflicts>` — `git_index_conflict_iterator` のラッパ。各要素 `IndexConflict { ancestor: Option<IndexEntry>, our: Option<IndexEntry>, their: Option<IndexEntry> }`。**`ancestor`=stage1 / `our`=stage2 / `their`=stage3**。modify/delete は欠落 side が `None`。
- `Index::get_path(path, stage)` / `add_conflict` / `remove_conflict`(`git_index_conflict_get/add/remove`)。
- `IndexEntry::stage` で stage 番号(0/1/2/3)を判別。

**(b) 進行中 operation の種別**
- `Repository::state() -> RepositoryState`(`git_repository_state`)。variant(推測: libgit2 と一致): `Clean`, `Merge`, `Revert`, `RevertSequence`, `CherryPick`, `CherryPickSequence`, `Bisect`, `Rebase`, `RebaseInteractive`, `RebaseMerge`, `ApplyMailbox`, `ApplyMailboxOrRebase`。**この値で 2.2 の ours/theirs ラベル翻訳を駆動できる**。
- `Repository::cleanup_state()` — merge/revert/cherry-pick の途中メタを除去(`git_repository_state_cleanup`)。abort 経路の一部。

**(c) in-memory merge(無傷 dry-run — kagi の既存資産)**
- `Repository::merge_commits(our, their, opts) -> Index` / `merge_trees(ancestor, our, their, opts) -> Index` — **worktree を触らず** conflict を index に出す。ADR-0005 の「事前予測」の核。
- `MergeOptions`: `file_favor(FileFavor)`(`FileFavor::Normal/Ours/Theirs/Union` — auto-resolve 戦略)、`fail_on_conflict`、`skip_reuc`、`minimal` 等。

**(d) file-level merge(3-way マーカー生成)**
- `Index::merge_file_from_index(ancestor, ours, theirs, Option<&MergeFileOptions>) -> MergeFileResult`(推測: git2 0.21 のメソッド名/可用性は実装時に要確認)、または `MergeFileOptions` 経由。
- `MergeFileOptions`: `ancestor_label/our_label/their_label`(マーカーのラベル文字列)、**`style_standard(bool)` / `style_diff3(bool)` / `style_zdiff3(bool)`**、`marker_size`、`favor`、`ignore_whitespace*`、`patience`/`minimal`、`accept_conflicts`。
- `MergeFileResult`: `is_automergeable() -> bool`、`path()`、`mode()`、`content() -> &[u8]`(**マーカー入りの解決対象テキスト**)。

> 要するに kagi は git2 だけで「(1) conflict を無傷予測(merge_commits)」「(2) conflict path と 3 stage を列挙(conflicts())」「(3) diff3/zdiff3 マーカーをメモリ生成(MergeFileOptions::style_zdiff3 + merge_file)」「(4) operation 種別を取得(state())」が全て可能。**3-way 解決 UI の実装基盤は揃っている**。rerere と `.git/rebase-merge/todo` の細部だけは libgit2 が薄く、必要なら `git` サブプロセス併用。

---

## 3. jj first-class conflict モデル(思想ソース)

出典: [jj technical/conflicts.md](https://github.com/jj-vcs/jj/blob/main/docs/technical/conflicts.md)、[jj conflicts(user)](https://docs.jj-vcs.dev/latest/conflicts/)、[DeepWiki tree merging](https://deepwiki.com/jj-vcs/jj/2.4-tree-merging-and-conflicts)。

### 3.1 データモデル: `Merge<T>` = tree の交互列

- conflict は **commit の中身**。commit は「奇数個の tree の列」を持ち、`A+(C-B)+(E-D)` のように **正項(added)・負項(removed)** を交互に並べる。
- 通常の 3-way merge = `A+(C-B)`(A=base、B/C=両 side)。`resolved()` は要素 1 個。
- per-path に `Merge<Option<TreeValue>>`(削除 = `None`)。tree merge は再帰的、変更が無い sub-tree は辿らない(materialize は on-demand)。

### 3.2 自動簡約と伝播(rebase が成功する理由)

- **`Merge::simplify()`**: 打ち消し合う項を除去。conflicted commit `C+(B-A)` を D に rebase すると `D+(B-A)` に簡約され、中間状態 C が消える → **解決しないまま rebase が成功**し再帰的複雑化を防ぐ。
- **same-change rule**: 全 side が同一変更なら自動解決(Git/hg 同様、理論上は lossy だが実用優先)。

### 3.3 materialization(マーカー)とワークフロー

- checkout/edit/diff のときだけ worktree にマーカーを **materialize**。
- 既定は **diff スタイルマーカー**(`ui.conflict-marker-style` で変更可):
  ```
  <<<<<<< conflict 1 of 1
  %%%%%%% diff from base to side A   ( ' ' 文脈 / '-' 削除 / '+' 追加 )
  +++++++ snapshot of side B
  >>>>>>> conflict 1 of 1 ends
  ```
  `%%%%%%%` = base へ適用する diff、`+++++++` = side のスナップショット。N-way(>2)を表現できる(git-style は 2-way 限定)。
- **部分解決の永続化**: 解決コマンド不要。次 snapshot が worktree を取り込み、直した分が commit に保存され残りは conflict のまま。`jj resolve` は merge tool 起動の補助。
- **undo**: `jj op undo` が operation DAG を辿り、conflict 解決を含む任意操作を巻き戻す。continue/abort/skip という sequencing 自体が存在しない。

---

## 4. GitButler conflict / virtual-branch モデル(思想ソース)

出典: [Fearless Rebasing(blog)](https://blog.gitbutler.com/fearless-rebasing)、[Rebasing and Conflicts(docs)](https://docs.gitbutler.com/features/virtual-branches/merging)、[Representation of conflicts(discussion #11564)](https://github.com/gitbutlerapp/gitbutler/discussions/11564)。

### 4.1 conflicted commit のデータモデル

- jj の思想を Git object 上に移植: rebase は **常に成功**し、衝突するコミットは「conflicted」とマークして次へ進む(applyable な hunk だけ worktree に部分適用)。
- conflicted commit の中身(discussion #11564): **synthetic root tree** に複数 subtree(各 conflict side)+ auto-resolution 用 subtree を格納し、「この root tree は特別扱い」と示す **extra commit header** を付ける。→ 標準 `git checkout` ではこの commit を正しく展開できない。
- 提案中の新案: auto-resolution(マーカー入り)tree を root に置き、各 side tree への map を ref or sqlite に外出し → 標準 Git 互換にする(推測: 実装状況は流動的)。

### 4.2 virtual branch との関係

- 複数 stack を単一 worktree に同時適用し、統合は **workspace commit(octopus merge)**(`gitbutler-reuse-research.md` §1 既述)。conflicted commit は各 stack 内に並列存在できる。
- 解決 = 「Resolve Conflicts」ボタン → 他の並列 branch を worktree から退避し、その commit のマーカーだけを checkout(**edit mode**)→ 直して save → 上位コミットを自動 rebase。
- abort = oplog(snapshot = Git tree)巻き戻し(`gitbutler-reuse-research.md` §4 既述)。

### 4.3 ライセンス再掲(本書は concept-only)

- **GitButler = FSL-1.1-MIT**。kagi は Git GUI = FSL "Competing Use" に該当する蓋然性が高く、**コード流用不可・概念のみ**(ADR-0031 ゲート、ADR-0033)。本節は思想記述に限定し、コード片を一切転写しない。

---

## 5. kagi への示唆(Conflict Mode 設計)

### 5.1 jj から借りられるもの(backend 変更なしで可能)

1. **「途中まで直した状態を失わせない」UX 原則**。Git index は部分解決を永続化できない(`git add` で stage 1/3 が消える)が、kagi は **解決中ファイルを kagi 側 oplog snapshot(ADR-0019/0011, `src/git/oplog.rs`)に退避**すれば「途中で別操作 → 戻ると解決途中が残る」体験を git2 のまま近似できる。jj の op log 思想の部分採用。
2. **3-way を常に見せる materialization**。jj の diff スタイルは N-way 向けで kagi(2-way 中心)には過剰。kagi は git2 の **zdiff3**(`MergeFileOptions::style_zdiff3`)で「base 文脈つき 2-way」を出すのが現実解。base region を畳んで表示する UI が良い。
3. **operation 種別に応じたラベル翻訳**(2.2)。jj は ours/theirs を使わず常に「commit X の変更」と呼ぶ。kagi も `Repository::state()` を見て **rebase 中は「ours」を「rebase 先」/「theirs」を「あなたのコミット X」** と翻訳する。これは git2 だけで実装でき、kagi の安全志向と差別化に直結。

### 5.2 backend を変えないと非現実的なもの(= 借りない)

1. **conflict を commit に保存する(first-class conflict)**。jj/GitButler は object model を拡張(`Merge<T>` の tree 列 / synthetic root tree + 特別 header)している。git2 0.21 の素の commit はこれを表現できず、特別 header を付ければ **標準 Git ツールと相互運用不能**になる(GitButler が直面した問題, #11564)。kagi の「無傷・標準互換」方針(ADR-0023/0005)と衝突 → **Reject**。
2. **rebase を常に成功させる(fearless rebase)**。上記 first-class conflict 前提なので同じ理由で不可。kagi は Git CLI モデルどおり「conflict で pause、continue/abort/skip を安全に提供」が筋。ADR-0005 の段階導入(MVP は回避・案内、v0.2 で ours/theirs 選択、v1.0 で 3-way 編集)を維持。
3. **`Merge<T>` の N-way 一般化**。kagi の in-memory merge は 2-way+base で十分(git2 の 3-way API がそのまま使える)。N-way は jj backend 同等の再設計が必要 → **Study only**(`jj-reuse-research.md` の結論と一致)。

### 5.3 結論(設計判断)

- kagi の Conflict Mode は **Git CLI モデル上に構築**する。実装基盤は git2 0.21 で十分(2.6 節: conflicts iterator / state / merge_commits / zdiff3 marker)。
- jj から借りるのは **「部分解決を失わせない」「3-way 文脈を見せる」「ours/theirs を文脈名に翻訳する」** の 3 概念のみ(いずれも git2 のまま実装可)。
- GitButler / jj の first-class conflict(commit に conflict を埋める)は **標準互換性を壊すため採用しない**。これは ADR-0005/0023/0031/0032/0033 と整合する。

---

## 6. ライセンス caveat(再確認)

- **GitButler = FSL-1.1-MIT**: Competing Use 該当の蓋然性が高く **コード流用不可・concept only**。本書は思想記述のみ。2 年経過 MIT 転換コードの例外は運用負荷ゆえ原則使わない(ADR-0031)。
- **jj = Apache-2.0**: コード流用は法的に可だが **gix(gitoxide)前提**で git2 世界へ移植コスト大。`Merge<T>` 等の概念は portable だが、採用時は ADR-0031 の Reimplement 区分(原典を見て転写せず kagi 型で再実装)に従う。
- **Git 本体 = GPLv2**: コード転写不可。仕様/挙動の参照のみ。kagi は libgit2(GPLv2 + linking exception)を git2 経由で利用しており、本書の Git CLI 記述は仕様参照に限る。

---

## 7. 確認できなかった/要実装時検証

- git2 **0.21 ちょうど**での `merge_file_from_index` / `style_zdiff3` の正確なメソッド名・可用性(master ソース基準で記述。実装時に 0.21 の docs.rs で要確認)。**推測**箇所。
- `RepositoryState` enum の 0.21 における正確な variant 列挙(libgit2 と一致と仮定)。**推測**。
- libgit2 が rerere / `.git/rebase-merge/todo` を API 化していないことの最終確認(現状: 薄い/無いと判断。必要なら `git` サブプロセス)。**推測(高確度)**。
- GitButler の新 conflict 表現(root=auto-resolution tree + 外部 map)の実装完了状況(discussion 段階)。**推測**。

---

### 参照 URL 一覧

- jj: <https://github.com/jj-vcs/jj/blob/main/docs/technical/conflicts.md> / <https://docs.jj-vcs.dev/latest/conflicts/> / <https://deepwiki.com/jj-vcs/jj/2.4-tree-merging-and-conflicts>
- GitButler: <https://blog.gitbutler.com/fearless-rebasing> / <https://docs.gitbutler.com/features/virtual-branches/merging> / <https://github.com/gitbutlerapp/gitbutler/discussions/11564>
- Git CLI: <https://git-scm.com/docs/git-rebase> / <https://git-scm.com/docs/git-checkout> / <https://git-scm.com/docs/git-rerere> / <https://git-scm.com/docs/git-merge> / <http://peter.eisentraut.org/blog/2022/04/21/git-rebase-and-ORIG_HEAD> / <https://riptutorial.com/git/example/1422/rebase--ours-and-theirs--local-and-remote>
- git2-rs: <https://docs.rs/git2/latest/git2/struct.Index.html> / <https://docs.rs/git2/latest/git2/struct.Repository.html> / <https://github.com/rust-lang/git2-rs/blob/master/src/merge.rs>
