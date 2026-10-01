# ADR-0211: `git replay` / `git history` を plan パイプラインに載せる — 調査結果とバージョンゲート

- Status: **Proposed**（調査 + 設計のみ。実装は本 ADR を PM が読んでから別 issue に分割する。コードは 1 行も変えていない）
- Date: 2026-10-01
- Related: [#344](https://github.com/TomiXRM/kagi/issues/344)（親 #359）、ADR-0104（plan → confirm → preflight → execute → verify → oplog）、ADR-0052（merge / rebase 方向）、ADR-0131（sequencer は CLI 経由）、ADR-0177（oplog 例外表）、#340 / ADR-0208（worktree lock）、#356（署名 UX）
- 一次資料: git `master` の `Documentation/git-history.adoc`（249 行）、`git-replay.adoc`（237 行）、`RelNotes/2.53.0` / `2.54.0` / `2.55.0`。引用は全て 2026-10-01 取得の原文。
- 実測環境: この Mac の `git version 2.50.1 (Apple Git-155)`。`git history` は存在しないので **history の記述は文書引用のみ**、replay は実測。

## 0. アーキテクチャレビュー（5 点）

1. **SEARCH.** 既存の履歴改変は `crates/kagi-git/src/ops/rebase.rs`（`git rebase` にシェルアウト、`--rebase-merges`）と `conflicts.rs`（sequencer の continue / abort / skip）。git 子プロセスは全て `crates/kagi-git/src/cli.rs::run_git`（argv 配列、`HARDENING_ARGS`、`GIT_EDITOR=true`、`REPO_LOCAL_ENV` 除去）。`git --version` を読むのは計測コードだけ（`crates/kagi-git/src/benchmark/environment.rs:160` `git_version()` は生の文字列を report に載せるのみで、パースも機能判定もしない）で、**製品経路に版の検出は無い**。ref の一括更新（`update-ref --stdin`）を使う既存経路も無い。
2. **OWNER.** 履歴改変 = `ops/<feature>.rs` の `plan_/preflight_/execute_` 三つ組 + `Backend::run`。git の実行 = `cli.rs::run_git`。バージョンのような「プロセス寿命の環境事実」の置き場は `Backend`（`open` 時に確定する `ExecutionPolicy` と同じ層）。
3. **DECISION.** EXTEND: 新 crate / 新 manager は作らない。`cli.rs` に `GitVersion` の検出（1 回、`OnceLock`）、`Backend` に `git_features()`、`ops/replay.rs` と `ops/history.rs` に三つ組を足す。plan の中身は git の `--dry-run` / `--ref-action=print` 出力（`update <ref> <new> <old>` 行）をパースした `RefUpdate` の列で、既存 `OperationPlan` に `preview_refs`（新規 field、後述）として載せる。
4. **REFACTOR.** なし。既存 `rebase.rs` は残す（merge を含む履歴は今後も rebase の担当。§3）。
5. **ANTI-PATTERNS.** #3（UI 内 I/O）: バージョン検出は `Backend::open` の裏で 1 回、render では読むだけ。#5（flag 分岐）: 機能の有無は `GitFeatures` の値で UI が「出す / 出さない」を決めるだけで、実行経路に分岐を持ち込まない。#7（同じ問題を別名で）: replay の `--onto` と既存 rebase を「同じ rebase」と呼ばない — 前者は worktree に触らない ref 更新、後者は checkout 済み branch の作業（§3 の住み分け）。

## 1. `git replay` の実測（2.50.1、fixture 再現手順は付録 A）

| # | 確認したこと | 結果（2.50.1） | 根拠 |
|---|---|---|---|
| i | worktree / index / HEAD に触らないか | **触らない**。main worktree を dirty（`a.txt` 変更 + `u.txt` staged）にしたまま `git replay --onto main main..feat` を実行しても `git status --porcelain`・`.git/index` の hash・`HEAD`・`refs/heads/feat` の全てが実行前と同一 | 付録 A step 3 |
| ii | 出力が `git update-ref --stdin` に流せるか | **流せる**。stdout は `update refs/heads/feat <new> <old>` の 1 行（2.50.1 は常にこの形式で、ref は更新しない）。そのまま `git update-ref --stdin` に渡して exit 0、`feat` が新 commit に移った。新 commit オブジェクトは replay の時点で書かれている（`git cat-file -t <new>` = commit） | 付録 A step 4 |
| ii' | `<old>` が CAS になるか | **なる**。`<old>` を偽の SHA にして `update-ref --stdin` に流すと `fatal: cannot lock ref 'refs/heads/feat': is at <a> but expected <b>`、exit 128。plan 時の `<old>` を execute まで持ち回れば、その間に branch が動いた場合は git 自身が拒否する（preflight の一部を git が担保） | 付録 A step 5 |
| iii | 他 worktree で checkout 中の branch を rebase できるか | **できるが、その worktree の index が古くなる**。`feat` を `../wt-feat` で checkout 中に main 側から replay + update-ref → `wt-feat` の `HEAD` は symbolic なので新 commit を指す（`git -C wt-feat rev-parse HEAD` = 新 SHA）が、index と working tree は旧 tree のまま。`git -C wt-feat status --short` が `M a.txt` / `D c.txt`（staged）を示す = **ユーザーには「rebase 分を打ち消す変更を stage した」ように見える** | 付録 A step 4 |
| iv | merge を含む履歴 | `fatal: replaying merge commits is not supported yet!`、exit 128。状態ファイル（`.git/rebase-*` / `sequencer` 等）は作られない | 付録 A step 6 |
| v | conflict | stdout 空、stderr 空、exit 1。worktree は dirty のまま不変、状態ファイルなし（文書どおり: "There is no stderr output on conflicts", exit 1 = conflict, それ以外 = error） | 付録 A step 7 |
| vi | hooks | `post-rewrite` / `pre-commit` を実行可能で置いても呼ばれない（stderr に hook 出力なし） | 付録 A step 8 |
| vii | `--advance` / `--contained` | 動く。`--advance main main..tmp2` → `update refs/heads/main <new> <old>` | 付録 A step 9 |
| viii | `--ref-action=print` / `--revert` | **2.50.1 には無い**（`error: unrecognized argument: --ref-action=print`、`--revert` は usage エラー）。§2 参照 | 付録 A step 0 |

## 2. バージョンの事実（RelNotes 引用）

| 機能 | 入った版 | 引用 |
|---|---|---|
| `git replay --onto/--advance/--contained`、出力は常に update-ref 形式、ref は更新しない | 2.44 | （2.50.1 の usage: `(EXPERIMENTAL!) git replay ([--contained] --onto <newbase> \| --advance <branch>) <revision-range>...`） |
| **`git replay` が既定で ref を自分で更新する**（`--ref-action=update` 既定、`print` で従来動作） | **2.53** | 2.53.0: "git replay (experimental) learned to perform ref updates itself in a transaction by default, instead of emitting where each refs should point at and leaving the actual update to another command." |
| `git replay --revert=<branch>`、root まで replay | 2.54 | 2.54.0: "git replay (experimental) learns, in addition to "pick" and "replay", a new operating mode "revert"." / "git replay now supports replaying down to the root commit." |
| `git history drop / reword / split`、`--dry-run` | 2.54 | 2.54.0: "git history history rewriting (experimental) command has been added." / "git history learned the split subcommand." |
| `git history fixup` | 2.55 | 2.55.0: "git history learned fixup command." |

**設計上の落とし穴（2.53 の既定変更）**: 2.44–2.52 では replay は「印字するだけ」だが、2.53+ で **同じ argv が ref を直接更新する**。Kagi が `--ref-action=print` を付けずに replay を呼ぶと、confirm 前に ref が動く。逆に 2.52 以下に `--ref-action=print` を渡すと `unrecognized argument` で失敗する（実測 viii）。したがって replay は**必ず検出済みバージョンで argv を切り替える**か、`git -c replay.refAction=print replay …` を使う（`replay.refAction` は 2.53 の文書に "The default mode can be configured via the replay.refAction configuration variable" とあり、未知の config は旧版で無視される）。本 ADR は **両方**を採る: `-c replay.refAction=print` を常に付け、2.53+ ではさらに `--ref-action=print` を付け、verify で「stdout が非空」「対象 ref が実行前と同じ」を確認して、万一 ref が動いていたら `TerminationUnknown` 相当で oplog に記録する（黙って成功にしない）。この二重化はどちらか一方の前提が崩れても confirm 前に ref が動く事故を防ぐためで、2.53+ の実機で検証するまで解除しない。

## 3. 今すぐ replay でできること / `git history` が要ること

### 3.1 replay（2.44+、この環境で実測済み）

| ユーザー価値 | コマンド | plan の中身 | 制約 |
|---|---|---|---|
| **他 worktree で checkout 中の branch を、そこを checkout せずに rebase** | `git replay --onto <base> <base>..<branch>` | `update refs/heads/<branch> <new> <old>`（1 行以上。`--onto` は範囲内の複数 branch を同時に動かしうる） | merge を含むと exit 128（§1 iv）。conflict は exit 1 で何も残さない（§1 v）。**checkout 中の worktree の index が古くなる（§1 iii）→ §5** |
| 複数 branch をまとめて rebase | `--contained --onto` | 複数行 | 同上 |
| branch を前進させる（fast-forward 的な積み替え） | `--advance <branch> <range>` | 1 行 | 同上 |
| **revert commit を worktree を汚さず積む** | `--revert=<branch> <range>` | 1 行 | **2.54+**。2.50.1 では不可（§1 viii） |

### 3.2 `git history`（2.54+、文書引用のみ。この環境では未実測）

| sub | 引数（文書の synopsis） | Kagi での扱い | 注意 |
|---|---|---|---|
| `drop <commit>` | `[--dry-run] [--update-refs=(branches\|head)] [--empty=(drop\|keep\|abort)]` | 自動化可 | "The root commit cannot be dropped … Merge commits cannot be dropped either" / "If HEAD points at a commit that is to be rewritten, the index and working tree are updated to match the new HEAD. The command aborts before any references are updated in case local modifications would be overwritten." |
| `reword <commit>` | `[--dry-run] [--update-refs=…]` | **editor を起動する**（"This command will spawn an editor with the current message"）。Kagi は `GIT_EDITOR` に「用意した message file を書き出すスクリプト」を渡す必要がある（`cli.rs` は今 `GIT_EDITOR=true` 固定） | メッセージは Kagi の modal で入力し、editor には触らせない |
| `split <commit> [--] [<pathspec>...]` | `[--dry-run] [--update-refs=…]` | **対話的**（`add -p` 型の `Stage addition [y,n,q,a,d,p,?]?` プロンプト、文書の例）。pathspec で**ファイル単位**の分割なら非対話で成立しうるが、hunk 単位は stdin 駆動が必要 | "It is invalid to select either all or no hunks" |
| `fixup <commit>` | `[--dry-run] [--update-refs=…] [--reedit-message] [--empty=…]` | staged 変更を対象 commit に畳む。**index を読む**（bare 不可）。2.55+ | "Changes are applied … by performing a three-way merge between the HEAD commit, the target commit and the tree generated from staged changes" |

共通の性質（文書）:
- `--dry-run`: "Do not update any references, but instead print any ref updates in a format that can be consumed by git-update-ref. Necessary new objects will be written into the repository, so applying these printed ref updates is generally safe."
- hooks: "does not execute any githooks at the current point in time. This may change in the future."
- merge: "This command does not (yet) work with histories that contain merges. You should use git-rebase with the --rebase-merges flag instead."
- conflict: "the command does not support operations that can result in merge conflicts … history rewrites are not intended to be stateful operations."
- `--update-refs=branches`（既定）: "all local branches that point to commits which are descendants of the original commit will be rewritten."

### 3.3 住み分け

- merge を含む範囲 → 既存 `ops/rebase.rs`（`--rebase-merges`）のまま。replay / history の plan は範囲に merge があれば **blocker**（git の exit 128 を待たず、plan 時に `rev-list --merges <range>` で先に判定）。
- 「checkout 中の branch を rebase」→ 既存 rebase。「checkout していない branch」→ replay。同じ「rebase」の語を UI で使い分ける（Branch メニューの "Rebase onto…" は worktree 外の branch には replay を使い、card の title で `git replay` と明示）。

## 4. 決定

### 決定 1 — バージョンゲートは **(a) 隠す**（既定案）

- (b) `rebase -i` 相当の自前 fallback は作らない: 実装量が倍で、「中断状態を持たない」という本機能の価値そのものを失う。
- (c) replay で代替できる範囲（3.1）は **バージョンに応じて先に出す**。これは (a) と矛盾しない: 各機能に「必要な最小バージョン」を持たせ、満たさない機能だけを隠す。

| 機能 | 最小版 | 2.50.1（この Mac） |
|---|---|---|
| replay `--onto` / `--advance` / `--contained` | 2.44 | 出す |
| replay `--revert` | 2.54 | 隠す |
| history `drop` / `reword` / `split` | 2.54 | 隠す |
| history `fixup` | 2.55 | 隠す |

隠し方: メニュー項目を出さない。**理由を出す場所を 1 つだけ**持つ — Settings の「実験的機能」節（決定 3）に「この機能には git 2.54 以上が必要です（現在 2.50.1）」を表示する。無効ボタンの理由 footer（#354 の availability 契約）と同じ考え方で、「Kagi を入れたのに機能が無い」体験には**理由が見える**ことで応える。

### 決定 2 — `git --version` の検出は `kagi-git` に 1 回、`Backend` が保持

- `crates/kagi-git/src/cli.rs` に `pub struct GitVersion { major, minor, patch }` と `pub fn git_version() -> Option<GitVersion>`（`run_git(…, ["--version"])` を **プロセスで 1 回** `OnceLock` にキャッシュ。`git version 2.50.1 (Apple Git-155)` の `(…)` 以降は捨てる。パースは pure 関数 `parse_git_version(&str)` で unit test）。失敗（git が無い / 形式不明）は `None` = **全て隠す**（不明を「使える」扱いにしない）。
- `Backend` に `pub fn git_features(&self) -> GitFeatures { replay: bool, replay_revert: bool, history: bool, history_fixup: bool, replay_updates_refs_by_default: bool /* ≥2.53 */ }` を足し、`open_with_policy` で `git_version()` から計算して保持する。UI は `Backend` からしか読まない（`src/ui` に `git --version` を書かない）。
- 起動時 klog（新規契約行、既存行は不変）: `[kagi] git: version=2.50.1 replay=on revert=off history=off fixup=off`。

### 決定 3 — experimental の扱い: 設定で隠す（既定 OFF）

`git history` / `git replay` は両方とも文書に "THIS COMMAND IS EXPERIMENTAL. THE BEHAVIOR MAY CHANGE." とある。2.53 で replay の既定動作が変わった事実（§2）が示すとおり、次の版でも変わりうる。

- `settings.json` に `"experimental_history": "true"|"false"`（既定 `false`、`Settings::experimental_history()`、`settings::` の atomic、EN/JA toggle 1 つ。#772 の `terminal_auto_lock` と同型）。
- OFF のとき: 決定 1 の版判定に関わらず全て隠す。ON のとき: 版を満たす機能だけ出す。
- toggle の説明文に「git のバージョンによって使える機能が変わります。現在: git 2.50.1 — replay のみ」のように**検出結果を埋め込む**（決定 2 の値）。

### 決定 4 — plan / execute の形（ADR-0104 に載せる）

- **plan**: `git -c replay.refAction=print replay [--ref-action=print if ≥2.53] …` / `git history <sub> <commit> --dry-run …` を `run_git` で実行し、stdout の各行 `update <ref> <new> <old>` を pure な `kagi_domain::ref_update::parse_update_ref_lines(&str) -> Result<Vec<RefUpdate>, ParseError>` でパース（unit test）。`OperationPlan` に `preview_refs: Vec<RefUpdate>`（新規 field。既存 field の意味は変えない）として載せ、card は ref の一覧を表示。**件数が多いとき**（`--update-refs=branches` が子孫 branch を全部動かす、#344 §5）は既存の `IncludeCopy { sample, more }` と同じ折りたたみ（先頭 5 件 + 「+N more」）。**署名が失われる**（#356）: 範囲に署名付き commit があれば warning `PlanNote::Rewrite(RewriteNote::DropsSignatures { count })`（`git log --format=%G? <range>` で `G`/`U`/`X`/`Y`/`R`/`E` の commit を数える）。
- **preflight**: (1) `head_at_plan` 不変（既存 `preflight_check`）、(2) `preview_refs` の各 `<old>` が今も一致（`rev-parse`）、(3) 範囲に merge が無い（`rev-list --merges`）、(4) replay `--onto` の対象 branch が **他 worktree で checkout 中なら** その worktree が clean であること（§5）。
- **execute**: `git update-ref --stdin` に `preview_refs` を**そのまま**流す（`<old>` の CAS で二重に守られる、§1 ii'）。`--stdin` は 1 トランザクション。exit ≠ 0 は「何も動いていない」と読める（update-ref は all-or-nothing）。
- **verify**: 各 ref を `rev-parse` して `<new>` と一致。
- **oplog**: op 名 `replay-onto` / `replay-revert` / `history-drop` / `history-reword` / `history-split` / `history-fixup`。`Operation` enum に variant を足し `Backend::run` を通す（ADR-0177 の同期 UI 例外には**載せない**）。
- hooks は git 側が走らせない（§1 vi、history は文書）。Kagi は既存の `-c core.hooksPath=/dev/null` も併用する。

### 決定 5 — `split` は Phase を分ける

`split` は hunk 単位の対話（`add -p` 型）で、Kagi 側に hunk 選択 UI が要る（#344 §5 のとおり 1 機能分）。第 1 段は **pathspec によるファイル単位の split**（非対話で成立するかは 2.54 実機で確認が必要 — `--dry-run` と pathspec の組み合わせで editor / プロンプトが出ないことを検証してから）。hunk 単位は別 issue。

## 5. 未解決（実装 issue に持ち越す論点、実測で確定したもの）

1. **他 worktree の index が古くなる（§1 iii）**: ref だけ動かすと、その worktree の `git status` に「rebase を打ち消す staged 変更」が見える。案: (A) 対象 branch が他 worktree で checkout 中で **dirty なら blocker**、clean なら execute 直後に `git -C <wt> read-tree -m -u HEAD`（index と worktree を新 HEAD に合わせる。上書きが必要なら失敗する = 安全側）を verify の一部として行う。(B) 常に blocker にして「その worktree で rebase してください」と案内。**A は実機での検証が要る**（`read-tree -m -u` が clean な worktree で確実に成立するか、`.git/worktrees/<name>/index` の更新が `Backend` から見えるか）。#340 / ADR-0208 の lock（そこで作業中なら触らない）とも整合させる。
2. **2.53+ の実機検証**: `-c replay.refAction=print` と `--ref-action=print` の二重化が期待どおり「ref を動かさない」こと。CI の macOS runner の git 版を確認する（2.53 以上ならそこで検証できる）。
3. **`git history` の `--dry-run` 出力の実物**: 文書上は update-ref 形式だが、`reword` / `split` が `--dry-run` でも editor / プロンプトを出すのかは未確認。実装前に 2.54 の実機で `GIT_EDITOR=true` 下の挙動を取る。
4. **`--update-refs=branches` の列挙**: 既定で子孫 branch を全部動かす。plan では折りたたむ（決定 4）が、`head` に限定するオプションを UI に出すかは実装 issue で決める。

## 6. 却下した案

- **(b) `rebase -i` 相当の自前 fallback**: 実装量倍、中断状態を持ち込む、本機能の価値を失う。
- **版検出を `src/ui` で行う**: `src/ui` に git を触らせない不変条件（AGENTS.md）に反する。
- **版検出を毎回行う**: `git --version` は起動 1 回で十分。プロセス寿命中に git が入れ替わる事態は考慮しない（次回起動で反映）。
- **replay を version 判定なしで呼ぶ**: 2.53+ で confirm 前に ref が動く（§2）。
- **`update-ref` を使わず replay に ref を更新させる（2.53+ 既定）**: Kagi の confirm の外で書くことになる。plan と execute の間に人の確認を挟む本 pipeline の前提に反する。

## 付録 A — fixture 再現手順（2.50.1 で実行したもの）

```sh
# 0. 環境（開発者の設定を遮断）
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@e
git replay --ref-action=print --onto main main..feat   # 2.50.1: error: unrecognized argument: --ref-action=print
git replay --revert feat feat~1                        # 2.50.1: usage error（--onto/--advance のみ）

# 1. fixture: main(base, main1, main2) / feat(base, feat1, feat2)、feat を別 worktree で checkout
d=$(mktemp -d); cd "$d"; git init -q -b main repo && cd repo
c(){ echo "$2" > "$1"; git add -A; git commit -qm "$2"; }
c a.txt base; c a.txt main1; git branch feat HEAD~1
git checkout -q feat; c b.txt feat1; c b.txt feat2; git checkout -q main; c c.txt main2
git worktree add -q ../wt-feat feat

# 2. main worktree を dirty にする
echo dirty >> a.txt; echo untracked > u.txt; git add u.txt

# 3. (i) replay は何も触らない
bs=$(git status --porcelain); bi=$(shasum .git/index); bh=$(git rev-parse HEAD); bf=$(git rev-parse feat)
git replay --onto main main..feat > replay.out            # stdout: update refs/heads/feat <new> <old>
[ "$(git status --porcelain)" = "$bs" ] && [ "$(shasum .git/index)" = "$bi" ] \
  && [ "$(git rev-parse HEAD)" = "$bh" ] && [ "$(git rev-parse feat)" = "$bf" ] && echo "untouched"
git cat-file -t "$(awk '{print $3}' replay.out)"           # commit（オブジェクトは書かれている）

# 4. (ii)(iii) update-ref に流す → feat が動く、wt-feat の HEAD は追従、index は古い
git update-ref --stdin < replay.out
git -C ../wt-feat rev-parse --short HEAD; git rev-parse --short feat   # 同じ
git -C ../wt-feat status --short                                       # M  a.txt / D  c.txt （staged に見える）

# 5. (ii') <old> は CAS
printf 'update refs/heads/feat %s %s\n' "$(git rev-parse feat)" 0000000000000000000000000000000000000001 \
  | git update-ref --stdin                                # fatal: cannot lock ref … is at … but expected …

# 6. (iv) merge を含む範囲
git checkout -q -b topic main; c d.txt t1; git merge -q --no-ff -m merge-feat feat; c e.txt t2; git checkout -q main
git replay --onto main~1 main~1..topic                    # fatal: replaying merge commits is not supported yet! (exit 128)

# 7. (v) conflict
git checkout -q -b other main; echo other > b.txt; git add -A; git commit -qm other-b; git checkout -q main
echo dirty2 >> a.txt; bs=$(git status --porcelain)
git replay --onto other other..feat; echo "exit=$?"       # exit=1、stdout/stderr 空
[ "$(git status --porcelain)" = "$bs" ] && echo "worktree unchanged"; ls .git | grep -Ei 'rebase|sequencer' || echo "no state"

# 8. (vi) hooks は呼ばれない
printf '#!/bin/sh\necho HOOK-post-rewrite >&2\n' > .git/hooks/post-rewrite; chmod +x .git/hooks/post-rewrite
git replay --onto main main..feat 2>&1 >/dev/null          # 何も出ない

# 9. (vii) --advance / --contained
git checkout -q -b tmp2 main; c f.txt tmp2a; git checkout -q main
git replay --advance main main..tmp2                       # update refs/heads/main <new> <old>
git replay --contained --onto main main..feat              # update refs/heads/feat <new> <old>
```
