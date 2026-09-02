# worktree manager エコシステム サーベイ — Kagi の worktree 機能の「上」に積むもの

調査日: 2026-09-03 / 担当スライス: worktree manager エコシステム
調査者: SurveyWorktree

---

## 0. Kagi の現状 worktree 実装(コード実測)

提案の重複を避けるため、まず現状を確定させた。読んだもの: `crates/kagi-git/src/ops/worktree.rs`、
`crates/kagi-git/src/ops/branch.rs`、`src/ui/worktree_menu.rs`、`src/ui/operations/worktree.rs`、
`crates/kagi-git/src/snapshot.rs`、`crates/kagi-domain/src/refs.rs`、
ADR-0025 / 0054 / 0103 / 0128 / 0147。

### あるもの

| 機能 | 実装位置 | 備考 |
|---|---|---|
| worktree 作成(新規 branch) | `plan_create_worktree` / `execute_create_worktree` | git2 `Repository::worktree()`。ADR-0025 |
| worktree 作成(既存 branch を紐づけ) | `plan_open_worktree_for_branch` / `execute_open_worktree_for_branch` | ADR-0054。既に別 worktree で checkout 済みなら**そのパスを案内して作成しない** |
| 既存 checkout の検出 | `branch_checked_out_worktree_path` | main worktree + `repo.worktrees()` を走査 |
| デフォルトパス | `default_worktree_path` | `../<repo名>-worktrees/<safe_branch>`(`/` をサニタイズ) |
| パス検証 | `validate_worktree_path_keyed` | 空 / 既存 / 親 dir 不在 を blocker。`WorktreePathError::{Empty,Exists}` として i18n |
| unlock | `plan_unlock_worktree` / `execute_unlock_worktree` + `worktree_menu.rs` | サイドバー右クリック。lock 理由を warning として提示 |
| WORKTREES サイドバーセクション | `collect_worktrees`(snapshot.rs) | main worktree は ✓ |
| worktree ごとの WIP 行(グラフ内) | `WorktreeWip { staged, unstaged, untracked }` + `render_wip_row` | ADR-0103。**全 worktree の未 commit 状態を 1 グラフに同時表示**(業界的にこれは Kagi 固有) |
| worktree タブ + レーン色一致 | `RepoTab.is_worktree` / `wt_color_idx` | 🌲 マーカー。WIP 行と同じ色 |
| multi-HEAD バッジ | `build_badge_map` | 他 worktree で checkout 中の branch tip に 🌲 |
| worktree digest を preflight で再検証 | `WorkingTreeStatus::digest()` / ADR-0147 | 分類(staged/unstaged/untracked/conflicted)のハッシュ |
| branch 削除時の worktree 巻き取り | `branch.rs:775` `prune(Some(&mut opts))` + `WorktreeCheckout` | locked worktree / dirty worktree を blocker、clean なら worktree を消してから branch 削除 |
| Branch Cleanup ペイン | ADR-0128、`src/ui/branch_cleanup.rs` | `FullyMerged` / `SquashMergedLikely` / `MergedThenGrown` / `stale` 分類 + 一括削除 + oplog undo |

### ないもの(=このサーベイの提案対象)

- **standalone の worktree 削除**。branch 削除の副作用としてしか消えない。`WorktreeAction` enum は `Unlock` のみ
- **lock**(`--reason` 付き)。unlock だけあって非対称。`WorktreeRecovery::Unlock` は「戻すなら CLI で `git worktree lock`」と案内している
- **prune**(明示操作)/ **prunable 検出** / **repair** / **move**
- **セットアップ自動化**(post-create フック、依存インストール)
- **gitignore されたファイルの持ち込み**(`.env` 等)。`copy` も `symlink` も無い
- **削除前フック**(サービス停止・DB teardown)
- **ディスク使用量表示**、**マージ済み worktree の検出と一括削除**
- **worktree ごとの ahead/behind**(WIP 行は staged/unstaged/untracked の件数のみ)
- **埋め込みターミナル / 埋め込みエディタの worktree 紐付け**(タブはあるがターミナル cwd の固定・per-worktree 環境変数は無い)
- **ポート衝突対策**(`CONDUCTOR_PORT` 相当)
- **PR から worktree を起こす**(PR 一覧はあるが worktree 化の導線が無い)
- `--detach` / `--orphan` / `--guess-remote` / `worktree.useRelativePaths` 相当の選択肢

---

## 1. サマリ

Kagi に効く上位5点(いずれも「作成後〜削除まで」の実運用摩擦で、Kagi は今そこが空白):

1. **`.worktreeinclude` は既にデファクト規格になっている。** Claude Code、Conductor、VS Code(`git.worktreeIncludeFiles`)、worktree-link(`.worktreelinks`)が同じ「gitignore 構文で ignored ファイルだけをコピー」の形に収束した。Kagi が独自形式を作る理由はゼロで、**`.worktreeinclude` を読むだけで最大の摩擦(`.env` が無い)が消える**。難易度 S。
2. **post-create / pre-remove フックは全ツールが持っている唯一の共通機能。** phantom `postCreate.commands`/`preDelete.commands`、gwq `setup_commands`、wtp `hooks.post_create`、Conductor `scripts.setup`/`scripts.archive`、VSCode 拡張 `postCreateCmd`/`preRemoveCmd`、Zed `create_worktree` タスクフック。**Kagi にこれが無いのは機能表で唯一目立つ空白**。ただし gwq v0.1.0 のセキュリティ修正(リポジトリ同梱設定からの任意コード実行)が示すとおり、**Kagi の安全性原則では trust prompt が必須**。難易度 M。
3. **standalone の worktree 削除が無いのは製品の穴。** JetBrains 2026.2、VS Code、Zed、GitLens、全 CLI が持っている。Kagi は「branch を消すと worktree も消える」しかなく、**逆(worktree だけ消して branch を残す)ができない**。`phantom delete --keep-branch` / `wtp remove --with-branch` の二択モデルがそのまま使える。plan→confirm と ODB backup を通せば Kagi らしい差別化になる。難易度 S〜M。
4. **ポート衝突は「worktree で並列開発」の最後の壁で、Conductor と Uzi が実際に解いている。** Conductor は workspace ごとに**10 ポートを予約**して `CONDUCTOR_PORT`..`+9` として渡す。Uzi は `uzi.yaml` の `portRange: 3000-3010` から割り当て、`devCommand` の `$PORT` に注入する。Kagi は埋め込みターミナルを持っているので、**`KAGI_PORT` を worktree ごとに払い出すだけ**で同じ価値が出る。難易度 M。
5. **Kagi の ADR-0103(全 worktree の WIP を 1 グラフに同時表示)は業界で誰もやっていない。** gwq `status --watch`、Uzi `ls -w`、Conductor サイドバー、vibe-kanban ボードは全て「別画面のリスト」。**Kagi のグラフ内表示を ahead/behind とディスク使用量まで拡張すれば、これは競合が真似しにくい柱になる**。難易度 M。

---

## 2. 詳細

### 2.1 git 本体(基準線)

- **何か**: `git worktree` サブコマンド群。Kagi が乗っている土台。
- **出典**: <https://git-scm.com/docs/git-worktree>(manual last updated in **2.54.0**、2026-04-20。2.55.0 は変更なし)
- **仕組み**: サブコマンドは `add` / `list` / `lock` / `move` / `prune` / `remove` / `repair` / `unlock` の 8 つ。
  主要フラグ:
  - `add`: `-f/--force`(二重指定で locked も対象)、`-b/-B <new-branch>`、`-d/--detach`、`--checkout`/`--no-checkout`(sparse-checkout 用)、`--guess-remote`(config: `worktree.guessRemote`)、`--track`/`--no-track`、`--lock [--reason <string>]`(add + lock のレース回避)、`--orphan`(空 index + unborn branch)、`--relative-paths`(config: `worktree.useRelativePaths`)、`-q`
  - `list`: `-v`、`--porcelain [-z]`(**フォーマット安定を保証、スクリプト向け**)。出力に `bare` / revision / branch(または detached HEAD)/ `locked` / `prunable` の注記が付く
  - `prune`: `-n/--dry-run`、`-v`、`--expire <time>`(config: `gc.worktreePruneExpire`)
  - `remove`: `-f`(unclean 用、二重指定で locked)。**clean な worktree しか消せない。main worktree は消せない**
  - `repair [<path>...]`: main worktree 移動後にリンクを張り直す。逆方向(linked worktree 側の移動)も worktree 内で `repair` すれば直る
  - `move`: **main worktree と submodule を含む worktree は移動不可**
  - worktree 識別子は「パスの最終要素がユニークならそれだけで指せる」
  - ref の共有規則: 疑似 ref(`HEAD` 等)は per-worktree、`refs/` 配下は共有、ただし **`refs/bisect` / `refs/worktree` / `refs/rewritten` は非共有**。他 worktree の ref は `main-worktree/HEAD` / `worktrees/foo/HEAD` でアクセス可
  - per-worktree 設定は `extensions.worktreeConfig true` + `git config --worktree`。`core.worktree` は絶対に共有してはいけない、`core.bare=true` と `core.sparseCheckout` も共有すべきでない
  - lock は `$GIT_DIR/worktrees/<name>/locked` というプレーンテキストファイル(理由が中身)
- **Kagi への示唆**: Kagi は `lock` / `move` / `prune` / `repair` / `--detach` / `--orphan` / `--guess-remote` / `--relative-paths` を一切露出していない。**`list --porcelain` の `prunable` 注記は「孤児 worktree の検出」をタダで提供する**ので、サイドバーに出さない理由がない。`refs/worktree` が非共有という事実は、per-worktree の一時 ref を Kagi の oplog スコープに使える余地を示す。
- **難易度**: S(各操作は既存 plan/execute 三点セットのコピー)

### 2.2 Claude Code `--worktree`(AI ネイティブ側のデファクト)

- **何か**: Claude Code 本体の worktree 機能。`claude --worktree <name>` / `-w`。
- **出典**: <https://code.claude.com/docs/en/worktrees>(バージョン参照が本文に多数: v2.1.198 / 205 / 206 / 208 / 210 / 211 / 212 / 233 / 239 / 246)
- **仕組み**:
  - **配置規約**: `.claude/worktrees/<name>/`(リポジトリルート基準)、branch 名は `worktree-<name>`。名前省略時は `bright-running-fox` 形式で自動生成。`.claude/worktrees/` を `.gitignore` に入れる運用を推奨
  - **base branch**: 設定 `worktree.baseRef` に `"fresh"`(デフォルト、リモートのデフォルトブランチ)/ `"head"`(ローカル HEAD)。**ブランチ名は指定できない**。`"fresh"` では 24h 以上 fetch していなければ 5 秒上限で fetch し、失敗時はローカルキャッシュにフォールバック
  - **PR から worktree**: `claude --worktree "#1234"` / GitHub PR URL / GitLab MR URL。`.claude/worktrees/pr-<number>` に作る。fetch 先は origin のホストで分岐(github.com → `pull/<n>/head`、gitlab.com → `merge-requests/<n>/head`、それ以外は前者→後者の順で試行)
  - **ignored ファイル**: `.worktreeinclude`(リポジトリルート、**gitignore 構文**)。「パターンに一致 **かつ** gitignore されている」ファイルのみコピー。`**/` パターンが丸ごと ignore されたディレクトリの中に届かない既知の癖あり(`vendor/**/config.json` のようにディレクトリ名を明示せよ)
  - **後片付け**: 対話セッション終了時に「変更/未追跡ファイル/新規コミット」を検査 → clean かつ無名なら worktree と branch を自動削除、名前付きなら確認、作業が残っていれば keep/remove を確認。**`-p`(非対話)は掃除しない**
  - **定期スイープ**: subagent / background セッションの worktree を `cleanupPeriodDays` 経過後に削除。作業が残っていれば残す。**Claude Code が作った worktree には git メタデータにマーカーを書き、マーカーの無い worktree(ユーザーが `git worktree add` したもの)はスイープしない**(v2.1.246 でこの検査が入る前は誤削除の可能性があった)
  - **lock を並行削除ガードに使う**: agent 実行中は `git worktree lock` を張り、終了時に外す。**プロセスが死んだセッションのロックはスイープが解放するが、ユーザーが自分で張ったロックは絶対に解放しない**
  - **隔離の強制**(4 種のチェック): main checkout を対象とする `Edit`/`Write`/`NotebookEdit` を拒否 / cwd が main checkout に解決される Bash・PowerShell・Monitor コマンドを拒否 / `git -C`・`--git-dir`・`GIT_DIR`・`GIT_WORK_TREE`・`cd` 経由の git リダイレクトを拒否 / worktree 内に留まると検証できないコマンド形(brace expansion、非クォート heredoc)を拒否(**この最後のチェックは無効化できない**)
  - **subagent の worktree 隔離**: `.claude/agents/*.md` の frontmatter に `isolation: worktree`
  - **共有されるもの**: `.git` ディレクトリ / project スコープのプラグイン(v2.1.200+)/ **権限承認**(worktree 内で「今後聞かない」を選ぶと main checkout の `.claude/settings.local.json` に保存され、worktree 削除後も残る。v2.1.211 より前は worktree 内に保存されて失われていた)
  - **非 git VCS**: `WorktreeCreate` / `WorktreeRemove` フックで git ロジックを丸ごと差し替え可能(`.worktreeinclude` は処理されなくなる)
- **Kagi への示唆**: 3 点が直接効く。(a) **`.worktreeinclude` を読む**のはコストがほぼゼロで、Claude Code / Conductor / VS Code のユーザーの既存設定がそのまま効く。(b) **「作った主体をメタデータで区別し、自分が作ったものだけ掃除する」**という設計は、Kagi が worktree 一括削除を提供するときの安全弁としてそのまま採用すべき。(c) **lock を「実行中の保護」として使う**発想 — Kagi は unlock しか持たないが、`git worktree lock --reason "kagi: agent running"` を Kagi が張れるなら、埋め込みターミナルでプロセスが走っている worktree を削除から守れる。
- **難易度**: (a) S / (b) M / (c) S

### 2.3 Conductor(Mac app、Claude Code / Codex / Cursor / OpenCode を worktree ごとに並走)

- **何か**: worktree = 「workspace」として、agent の作業単位・レビュー単位・PR 単位を一致させた Mac アプリ。
- **出典**: <https://www.conductor.build/docs/concepts/git-worktrees>、<https://www.conductor.build/docs/reference/scripts>、
  <https://www.conductor.build/docs/reference/files-to-copy>、<https://www.conductor.build/docs/reference/environment-variables>、
  <https://www.conductor.build/docs/concepts/workspaces-and-branches>、<https://www.conductor.build/docs/concepts/workflow>
- **仕組み**:
  - **配置規約**: `~/conductor/workspaces/<repo name>/<workspace name>`。workspace 名は都市名風の自動生成(`warsaw-v2`, `tokyo`)。branch とは別軸で、UI では branch 名が主・workspace ディレクトリ名が副として表示される
  - **base**: リポジトリ設定の base branch(例 `origin/main`)。**作成前に origin から fetch する**ので、ローカル checkout が遅れていても workspace は常に最新から始まる
  - **セットアップ自動化**: `.conductor/settings.toml` の `[scripts]` テーブル。
    ```toml
    "$schema" = "https://conductor.build/schemas/settings.repo.schema.json"
    [scripts]
    setup   = "pnpm install"
    run     = "pnpm dev --port $CONDUCTOR_PORT"
    archive = "./script/workspace-archive.sh"
    run_mode = "concurrent"
    ```
    - `scripts.setup`: workspace 作成後(git が tracked ファイルを checkout した後)
    - `scripts.run`: Run ボタン。**名前付き複数定義**が可能 — `[scripts.run.web]` / `[scripts.run.worker]` / `[scripts.run.test]`、各々 `command` / `args` / `options.cwd` / `default` / `icon`(Lucide アイコン名)/ `hide` / `available_in = ["local","cloud"]`
    - `scripts.archive`: archive の**前**。workspace ディレクトリ外のリソース掃除用(例: `rm -rf "$HOME/Library/Application Support/com.example.app.dev.$CONDUCTOR_WORKSPACE_NAME"`)
    - `scripts.run_mode`: `concurrent` / `nonconcurrent`(**固定ポート・単一 DB・単一 Docker スタックのように共有リソースが 1 つしかないプロジェクト用**)
    - プロセス停止は `SIGHUP` → 200ms 待つ → `SIGKILL`。**`&` でのバックグラウンド化を避け `concurrently` を使えと明記**(バックグラウンドプロセスがポートを掴んだまま残るため)
    - run スクリプトは**非対話シェル**で走る
  - **ignored ファイル共有**: 「Files to copy」。解決順は (1) リポジトリルートの **`.worktreeinclude`**(あれば勝つ、設定 UI は read-only プレビューになる)→ (2) リポジトリ設定の `file_include_globs`(`.conductor/settings.toml`)→ (3) デフォルト `.env*`。**gitignore されているファイルのみコピー対象**(tracked は既にあるので不要、非 ignored の untracked は対象外)。`node_modules` / `.next` / `dist` / `target` のコピーは「遅くなる・stale が持ち込まれる」ので setup script でやれと明記
  - **ポート衝突**: **`CONDUCTOR_PORT` から始まる 10 ポートを local workspace に割り当てる**。cloud workspace には割り当てない
  - **環境変数**: `CONDUCTOR_WORKSPACE_NAME` / `CONDUCTOR_WORKSPACE_PATH` / `CONDUCTOR_ROOT_PATH` / `CONDUCTOR_DEFAULT_BRANCH` / `CONDUCTOR_BASE_DIR` / `CONDUCTOR_PORT` / `CONDUCTOR_IS_LOCAL` / `CONDUCTOR_API_URL` / `CONDUCTOR_API_TOKEN` / `CONDUCTOR_API_KEY` / `CONDUCTOR_SESSION_ID`。カスタムは `[environment_variables]` + `.local` / `.cloud` サブテーブル
  - **DB 衝突**: 「アプリがローカル状態を持つなら setup script で workspace ごとにリソースを作れ」+ `CONDUCTOR_WORKSPACE_NAME` を app identifier / データディレクトリ / Application Support フォルダ / 開発用アイコンの名前に混ぜる、という指針
  - **root checkout の追随**: 「Conductor の fetch は remote view を更新するが root checkout の branch は動かさない」ので setup script に
    `git -C "$CONDUCTOR_ROOT_PATH" fetch --prune origin && git -C "$CONDUCTOR_ROOT_PATH" pull --ff-only || true` を入れよと案内
  - **横断状態一覧**: Threads/workspace サイドバー、Diff Viewer(`Cmd+Shift+D`)、**Checks タブ**(git status / CI / deployments / comments / todos)
  - **後片付け**: archive(サイドバーから消える)+ History ペインから**チャット履歴込みで復元可能**
  - **隔離の性質**: 「workspace isolation is development isolation, **not a security boundary**」と明記。agent はユーザー権限で走る
  - **設定の共有**: `.conductor/settings.toml` を commit してチームで共有、`.conductor/settings.local.toml` はマシンローカル(シークレット用)
  - **`.context` フォルダ**: gitignore された workspace メモ/handover 置き場
- **Kagi への示唆**: **これが Kagi の worktree 機能のロードマップとして最も参考になる一次情報**。特に (a) `setup` / `run` / `archive` の 3 フック分類、(b) `run_mode: nonconcurrent`(共有リソースがあるプロジェクトを正しく諦める逃げ道)、(c) 10 ポート予約、(d) `.worktreeinclude` を `file_include_globs` より優先する解決順、(e) `SIGHUP`→200ms→`SIGKILL` のプロセス停止契約、(f) 「workspace 名を app identifier に混ぜる」という DB/状態衝突の解法指針。Kagi は埋め込みターミナルを持つので `run` 相当は自然に置ける。
- **難易度**: フック 3 種 M / ポート割り当て M / `.worktreeinclude` S

### 2.4 Zed(GPUI 製、Kagi と同じ土台)

- **何か**: エディタ本体の worktree ピッカー + Parallel Agents の worktree 隔離。
- **出典**: <https://zed.dev/docs/git>(Git Worktrees 節)、<https://zed.dev/docs/ai/parallel-agents>(Worktree Isolation 節)、
  <https://zed.dev/docs/tasks>(hooks)、
  既知の課題: <https://github.com/zed-industries/zed/discussions/53807>、<https://github.com/zed-industries/zed/discussions/54553>、
  <https://github.com/zed-industries/zed/issues/54026>、<https://github.com/zed-industries/zed/issues/58103>、
  <https://github.com/zed-industries/zed/issues/54598>、<https://github.com/zed-industries/zed/issues/55714>
- **仕組み**:
  - **配置規約**: 設定 `git.worktree_directory`。**デフォルトは リポジトリ working dir 基準の `../worktrees`**(Kagi の `../<repo>-worktrees/<branch>` とほぼ同型)
  - **worktree ピッカー**: タイトルバーの project picker の右隣、または `git: worktree` アクション。できること: 現ブランチ or デフォルトブランチから新規 linked worktree 作成 / 名前を打つか自動命名 / 既存 worktree に現ワークスペースを切り替え / **既存 worktree を新ウィンドウで開く** / **開いていない linked worktree を削除**
  - **⚠ 設計上の重要な選択: 新規 worktree は detached HEAD で作られる。** 「同じ branch が複数 worktree に入る事故を防ぐため」。切り替えた後に branch picker で branch を作る/checkout する。**選んだ branch が他 worktree で checkout 済みなら、別を選ぶまで detached HEAD のまま留まる**
  - **セットアップ自動化**: タスクの `hooks` フィールドに `create_worktree`。`ZED_WORKTREE_ROOT`(新 worktree)と `ZED_MAIN_GIT_WORKTREE`(元リポジトリ working dir)が環境変数として渡る。タスクの通常フィールド(`cwd` / `env` / `reveal` / `hide`)がそのまま使える:
    ```json
    { "label": "copy .env into new worktree",
      "command": "cp",
      "args": ["$ZED_MAIN_GIT_WORKTREE/.env", "$ZED_WORKTREE_ROOT/.env"],
      "hooks": ["create_worktree"], "reveal": "no_focus", "hide": "on_success" }
    ```
  - **multi-root**: プロジェクトが複数の git リポジトリを含む場合、**ピッカーからの新規作成でリポジトリごとに linked worktree を作る**。非 git フォルダはそのまま新ワークスペースに含める
  - **横断状態一覧**: Threads Sidebar でプロジェクト単位にグループ化。**linked worktree で走っているスレッドは main worktree と同じプロジェクトの下に並ぶ**
  - **後片付け**: 「linked worktree で走っていたスレッドを Thread History に移すと、他にそれを使うアクティブスレッドが無ければ **worktree の git 状態を保存して disk から消す**。History から復元すると worktree も復元される」。永久削除すると worktree データも掃除される
  - **既知の弱点(Kagi の差別化余地)**: ステータスバーが branch 名しか出さず**どの worktree にいるか分からない**(#53807)/ bare リポジトリレイアウトでピッカーが worktree を列挙できない(#54553)/ 「新ウィンドウで開く」が現ウィンドウで開いてしまう(#54026)/ 削除ボタンが 1 回目で無反応・2 回目でエラーなのに実は成功している(#58103)/ `create_worktree` フックが worktree 切り替えでも発火する(#54598)、逆に作成時に発火しない(#55714)
- **Kagi への示唆**: 4 点。(a) **detached HEAD デフォルト**は、Kagi の「worktree は必ず新規 branch とセット」(ADR-0025)と正反対の解法。Kagi の方が「後で branch 名を決められない」不便があるので、**`--detach` オプションを追加する価値がある**。(b) `git.worktree_directory` の 1 設定キーだけで配置を決める簡潔さは、Kagi のモーダル毎回入力より優れている — **デフォルトパスを設定化すべき**。(c) `ZED_WORKTREE_ROOT` / `ZED_MAIN_GIT_WORKTREE` の 2 変数だけでフックの 8 割が書けるという実証。(d) **#53807 と #58103 は Kagi が既に勝っている**(🌲 マーカー + レーン色 + タブ色で「どの worktree にいるか」が常に見え、全操作が plan→confirm→verify を通るので「無反応→エラー→実は成功」が構造的に起きない)。ここは Kagi の宣伝材料。
- **難易度**: (a) S / (b) S / (c) M

### 2.5 VS Code 本体

- **何か**: 2025-07 リリースで worktree サポートを本体に取り込み、2026 時点で `Migrate Worktree Changes` まで持つ。
- **出典**: <https://github.com/microsoft/vscode-docs/blob/main/docs/sourcecontrol/branches-worktrees.md>(DateApproved: **9/2/2026**)、
  <https://code.visualstudio.com/docs/sourcecontrol/branches-worktrees>
  課題: <https://github.com/microsoft/vscode/issues/318526>(Workspace-level Worktree Support)、
  <https://github.com/microsoft/vscode/issues/311858>(multi-worktree ワークフロー改善: ターミナル/エディタ/agent のグループ化)、
  <https://github.com/microsoft/vscode/issues/313526>(1.118.1 で "Close Other Repositories" が worktree に効かない)、
  <https://github.com/microsoft/vscode/issues/315699>(vscode-agents の worktree 体験を本体へ)
- **仕組み**:
  - **作成**: Source Control Repositories ビュー → リポジトリ選択 → More Actions (...) → Worktrees > Create Worktree。branch と location をプロンプトで
  - **ignored ファイル共有**: 設定 **`git.worktreeIncludeFiles`**(glob 配列)。**「パターンに一致 かつ `.gitignore` に載っている」ファイルのみコピー**(Conductor / Claude Code と同一のセマンティクス)。
    ```json
    "git.worktreeIncludeFiles": [".env", "node_modules/**"]
    ```
    公式ドキュメントが **`node_modules` のコピーを一般的な用途として明記**している(「依存を再インストールせずすぐ作業を始められる」)。agent 用 worktree には agent が安全に触れるものだけ入れよという注記付き
  - **自動検出**: `git.detectWorktrees`(デフォルト off。on にするとリポジトリを走査して既存 worktree を Repositories ビューに出す)、`git.detectWorktreesLimit`(**デフォルト 50**)
  - **開く**: 右クリック → `Open Worktree in New Window` / `Open Worktree in Current Window`、またはコマンドパレット
  - **⭐ 横断機能: `Compare with Workspace` と `Migrate Worktree Changes`。** worktree 内の変更ファイルを右クリックして現ワークスペースと side-by-side 比較、レビュー後に **`Migrate Worktree Changes` で worktree の全変更を現ワークスペースにマージ**
  - **後片付け**: ドキュメントには削除操作の明示的な節が無い(**未確認** — Worktrees サブメニュー内にあると推測されるが公式ドキュメントに記載なし)
  - **セットアップ自動化 / ポート衝突 / DB 衝突**: **対処なし**(公式ドキュメントに記載なし)
- **Kagi への示唆**: **`Migrate Worktree Changes` / `Compare with Workspace` が最大の学び**。Kagi は「全 worktree の WIP を 1 グラフに表示」までやっているのに、**その WIP 行から他 worktree の変更を diff したり自分側に取り込む導線が無い**。ADR-0103 のデータ(各 worktree の staged/unstaged/untracked)は既にあるので、WIP 行クリックで「切り替える」以外に「この worktree の変更を diff で見る」を足すのは自然な拡張。`git.detectWorktreesLimit=50` は「worktree は 50 個規模まで増える」という業界の想定値の目安。
- **難易度**: cross-worktree diff M / migrate L(index 越しの適用になるので plan→preflight の設計が重い)

### 2.6 JetBrains IntelliJ IDEA 2026.2

- **何か**: 2026.2 でネイティブ worktree サポート(Git tool window の Worktrees タブ)。
- **出典**: <https://www.jetbrains.com/help/idea/use-git-worktrees.html>(IntelliJ IDEA **2026.2** Help、ページ更新 2026-06-29、build 2026-09-02)。
  関連 issue: <https://youtrack.jetbrains.com/issue/IDEA-143404/Support-git-worktree-feature>(IJPL-112226)、
  <https://youtrack.jetbrains.com/projects/IDEA/issues/IDEA-386301>(Native Git Worktree Management: UI, Visual Indicators, and Shared Indexing)
- **仕組み**:
  - **UI**: Git tool window(`Alt+9`)の Worktrees タブ(worktree が 2 個以上あればデフォルトで出る)、または メインメニュー Git | New Worktree / Git | Worktrees
  - **作成**: New Worktree ダイアログ = From branch(source branch、または New Branch オプション)/ Project name / Location。**作成後は別プロジェクトとして開く**
  - **⚠ 注意書き: worktree を現プロジェクトのディレクトリ内に作るな**(`Projects/mainProject/linkedWorktree`)。「IntelliJ IDEA がそれを multi-root プロジェクトと誤認して worktree 統合が壊れる」
  - **切り替え**: Worktrees タブでダブルクリック。**IDE 外で作った worktree も一覧に出る**。Recent Projects / project widget にも並ぶ
  - **状態表示**: **デフォルトでは何も出さない。** 特定の状態に入ったときだけインジケータが出る — `Locked`(外部で lock された)/ `Prunable`(ディレクトリが手で消された)。**dirty / ahead-behind は出さない**
  - **削除**: Worktrees タブで選択 → Delete。「削除前に全変更を commit したか確認せよ」という注意書きのみ
  - **prune**: Worktrees タブの Prune ボタン。「Prunable マークの内部レコードを消して branch を解放する」
  - **セットアップ自動化 / ignored ファイル共有 / ポート衝突 / DB 衝突**: **対処なし**(ヘルプに記載なし)
  - **既知の罠(トラブルシューティング節)**: `.idea/workspace.xml` を commit していると `ProjectId` が重複して IDE が両ディレクトリを同一プロジェクトと誤認する → worktree 側の `workspace.xml` を削除 → `.gitignore` に入れよ
- **Kagi への示唆**: 2 点。(a) **`Locked` / `Prunable` だけをバッジで出す**という割り切りは、Kagi のサイドバー WORKTREES セクションにそのまま採れる(`git worktree list --porcelain` の注記からタダで取れる)。しかも Kagi は既に dirty 件数を出しているので**上位互換になる**。(b) 「worktree をプロジェクト内に作ると誤認する」は Kagi にも同型のリスク — Kagi の `validate_worktree_path_keyed` は「repo 内パス」を blocker にしている(ADR-0025)ので既に防いでいる。**ここは Kagi が JetBrains より堅い**。
- **難易度**: Locked/Prunable バッジ S / Prune 操作 S

### 2.7 GitLens(VS Code 拡張、GitKraken)

- **何か**: Worktrees ビュー。GitLens Pro 機能。
- **出典**: <https://help.gitkraken.com/gitlens/gl-worktrees/>(Last updated: **August 2025**)、
  <https://help.gitkraken.com/gitlens/gitlens-features/>(Last updated: June 2025)
- **仕組み**: 「Worktrees ビューで worktree の作成・表示・管理ができる」「Worktrees 設定でワークフローに合わせてカスタマイズできる」。**公式ヘルプの記述はこの 2 文だけで、設定キー名・配置規約・フック・削除の詳細は記載なし**(設定は `/gitlens/gitlens-settings/#worktrees-view-settings` にリンクされているが、当該アンカーの内容は本調査では**未確認**)。
- **Kagi への示唆**: **GitLens は「業界最大手の VS Code Git 拡張が worktree について公式ドキュメントを 2 文しか書いていない」という事実そのものが示唆**。worktree の GUI 体験は業界的にまだ薄く、Kagi が本気でやれば取れる領域。なお ADR-0103 が引用している GitLens #5311(「見えるが操作できない」罠)の教訓は既に Kagi が回避済み。
- **難易度**: —(参照のみ)

### 2.8 git-worktree-manager(VS Code 拡張、jackiotyu)

- **何か**: VS Code の worktree 専用拡張。Marketplace / Open VSX 公開、GitHub star 279 / fork 24(2026-09-03 時点)、603 commits。
- **出典**: <https://raw.githubusercontent.com/jackiotyu/git-worktree-manager/main/README.md>、
  <https://marketplace.visualstudio.com/items?itemName=jackiotyu.git-worktree-manager>
- **仕組み**: 要件 **git >= 2.40**。設定キー(実名):
  - `git-worktree-manager.treeView.toSCM` — Source Control ビューに worktree を出す
  - `git-worktree-manager.treeView.worktreeDescriptionTemplate` — ツリー各行の説明文テンプレート。変数 `$FULL_PATH` / `$BASE_NAME` / `$RELATIVE_PATH` / **`$LAST_COMMIT`(最終コミットの相対時刻、例 "3 weeks ago")**。例 `"$RELATIVE_PATH · $LAST_COMMIT"`。**「stale な worktree を一目で見つけるのに便利」と明記**
  - `git-worktree-manager.treeView.worktreeLabelTemplate` — ラベル(太字)テンプレート。変数 `$REF_NAME` / `$BASE_NAME` / `$FULL_PATH` / `$RELATIVE_PATH` / `$LAST_COMMIT`。例 `"$BASE_NAME ⇄ $REF_NAME"`
  - `git-worktree-manager.worktreeCopyPatterns` — 新規 worktree にコピーするファイル/ディレクトリ。例 `[".env.local", "config/*.json"]`
  - `git-worktree-manager.worktreeCopyIgnores` — **上記に一致してもコピーしない除外パターン**。例 `["node_modules/**", "dist/**"]`
  - `git-worktree-manager.postCreateCmd` — 作成後コマンド。例 `"pnpm install"`
  - `git-worktree-manager.preRemoveCmd` — **削除前コマンド**。「worktree ディレクトリ内で実行。失敗またはキャンセルされたら削除を中止する」。例 `"pnpm run worktree:teardown-db"`(**per-worktree DB スキーマの撤去・サービス停止が用途として明記されている**)
  - `terminal.external.windowsExec` / `terminal.external.osxExec` — 外部ターミナル(iTerm.app 等)
  - その他機能: `Ctrl+Shift+R` で worktree マネージャ起動 / **worktree を VS Code workspace に追加**(複数 branch を並べて作業)/ **Favorites(お気に入り登録)**/ Copy Untracked Files / i18n(EN / 簡体字 / 繁体字 / **日本語**)
- **Kagi への示唆**: **`worktreeCopyPatterns` + `worktreeCopyIgnores` の二段構え**は `.worktreeinclude` 単独より柔軟(`config/*.json` を入れつつ `node_modules/**` を弾ける)。**`$LAST_COMMIT` を一覧に出して stale を見つけさせる**発想は、Kagi の Branch Cleanup(ADR-0128)が既に持つ stale 概念(90 日閾値)と worktree 側で対応させられる。**`preRemoveCmd` が失敗したら削除を中止する**契約は Kagi の plan/preflight 思想と完全に一致する。Favorites は worktree が増えたときの UX として Kagi のサイドバーに直接効く。
- **難易度**: copy/ignore 二段 S / `$LAST_COMMIT` 列 S / pre-remove フック M

### 2.9 phantom(TypeScript / `@phantompane/cli`)

- **何か**: worktree 専用 CLI。star 212 / fork 16。Homebrew `brew install phantom` / npm。**Linux + macOS のみ公式サポート(ネイティブ Windows 非対応、WSL 推奨)**。
- **出典**: <https://raw.githubusercontent.com/phantompane/phantom/main/README.md>、
  <https://raw.githubusercontent.com/phantompane/phantom/main/docs/configuration.md>、
  <https://raw.githubusercontent.com/phantompane/phantom/main/docs/commands.md>
- **仕組み**:
  - **配置規約**: デフォルト **`.git/phantom/worktrees/<name>`**(=`.git` の中。`.gitignore` 不要になる巧い選択)。`worktreesDirectory` で変更可(相対パスはリポジトリルート基準、絶対パスはそのまま)。`directoryNameSeparator` で `feature/test` → `feature-test` に平坦化(**branch 名は変えない、ディレクトリ名だけ**)
  - **設定の二層**: プロジェクト `phantom.config.json`(リポジトリルート、commit してチーム共有)+ ユーザー `phantom preferences`(**`git config --global` の `phantom.*` 名前空間に保存**)。**preferences が config より優先**
  - **セットアップ自動化**:
    ```json
    { "worktreesDirectory": "../phantom-worktrees",
      "directoryNameSeparator": "-",
      "postCreate": { "copyFiles": [".env", ".env.local", "config/local.json"],
                      "commands": ["pnpm install", "pnpm build"] },
      "preDelete": { "commands": ["docker compose down"] } }
    ```
    - `postCreate.copyFiles`: **glob 対応**(node-glob)。`dot: true` 相当なので `*.env` が `.env` に一致。ディレクトリは除外(ファイルのみ)、`.git/**` は無視、重複は自動 dedupe、一致 0 件は無言スキップ。`--copy-file` で CLI から上書き可
    - `postCreate.commands`: 新 worktree のディレクトリで**順次実行、最初の失敗で停止**、出力はリアルタイム
    - `preDelete.commands`: 削除対象の worktree 内で順次実行、**失敗したら worktree を削除しない**。用途として `docker compose down` が例示されている
  - **ignored ファイル共有**: copy のみ(symlink なし)
  - **後片付け**: `phantom delete <name...>`。`--force`(未 commit 変更ごと)/ **`--keep-branch`**(worktree だけ消して branch を残す)/ `--current`(自分がいる worktree)/ `--fzf`。**`keepBranch` は preferences のキー**で、CLI と MCP の両方が参照する。孤児 prune / ディスク使用量表示: **対処なし**
  - **セッション紐付け**: tmux 統合が一級市民 — `--tmux`/`-t`(新 window)/ `--tmux-vertical`(`--tmux-v`)/ `--tmux-horizontal`。`phantom shell <name> --tmux` で既存 worktree も開ける。エディタは `phantom preferences set editor "code --reuse-window"`(`phantom.editor`)→ `phantom edit <name> [file]`、フォールバックは `$EDITOR`。AI は `phantom preferences set ai claude` / `"codex --full-auto"` → `phantom ai <name>`
  - **worktree 内の環境変数**: `PHANTOM=1` / `PHANTOM_NAME` / `PHANTOM_PATH`
  - **横断状態一覧**: `phantom list`(status 付き)/ `--fzf` / `--names`(スクリプト向け)。**watch なし、ahead-behind なし**
  - **GitHub 連携**: `phantom github checkout` — **PR や issue から直接 worktree を起こす**
  - **MCP サーバ**: `phantom mcp`。AI エージェントが自律的に worktree を作って並列開発する想定。README の例プロンプト: 「Express と Hono の hello world を各々の worktree に作り、それぞれ別 URL で起動できるようにせよ」
  - **プロジェクトレジストリ**: `phantom project add/list/remove` + **ghq 連携**(`ghqDiscovery` preferences、デフォルト true。ghq 未インストールならエラーなしで native registry のみ、ghq があって discovery が失敗したら partial success + stderr に警告、`--json` は valid な version 2 catalog を維持)
  - **ポート衝突 / DB 衝突**: `preDelete` で `docker compose down` する例のみ。**ポート割り当ての仕組みは無し**
- **Kagi への示唆**: 4 点。(a) **`.git/phantom/worktrees/` に置く**という選択は「`.gitignore` を汚さない」「リポジトリ外に散らからない」の両立で、Kagi の `../<repo>-worktrees/` より優れている可能性がある(ただし `.git` 内なので `du` での可視化や外部ツールからの発見性は落ちる)。(b) **`--keep-branch` / branch も消す の二択**は Kagi の worktree 削除 UI にそのまま必要。(c) **`preDelete` が失敗したら削除しない**契約(git-worktree-manager と同じ)。(d) **PR / issue から worktree** は Kagi の GitHub PR 一覧に足す導線が既にある。
- **難易度**: (a) 設定として S / (b) S / (c) M / (d) M

### 2.10 gwq(Go、d-kuro)

- **何か**: fuzzy finder ベースの worktree マネージャ。star 469 / fork 21。「parallel AI coding workflows に最適」を前面に。Homebrew / `go install`。要 git 2.5+。
- **出典**: <https://raw.githubusercontent.com/d-kuro/gwq/main/README.md>、
  <https://raw.githubusercontent.com/d-kuro/gwq/main/docs/release-notes/v0.1.0.md>
- **仕組み**:
  - **配置規約**: `worktree.basedir`(デフォルト **`~/worktrees`**)+ `naming.template`(デフォルト **`{{.Host}}/{{.Owner}}/{{.Repository}}/{{.Branch}}`**)→ `~/worktrees/github.com/user/myapp/feature-auth`。`naming.sanitize_chars = { "/" = "-", ":" = "-" }`。**URL 階層で名前衝突を防ぎ、どのリポジトリのものか文脈を保つ**。`[[repository_settings]]` の `basedir` でリポジトリ別に上書き可(`./worktrees`)
  - **レジストリ不要**: 「basedir をファイルシステム走査するだけ。別レジストリを維持しない」。git リポジトリ外なら basedir 内の全 worktree、リポジトリ内なら現リポジトリのみ(`-g` で全部)
  - **設定の二層**: グローバル `~/.config/gwq/config.toml` + ローカル `.gwq.toml`(カレントディレクトリ)。ローカルが優先。`repository_settings` は `repository` フィールドをキーにマージ(同一リポジトリならローカルが完全上書き、別リポジトリなら両方残す)
  - **セットアップ自動化**:
    ```toml
    [[repository_settings]]
    repository = "~/src/myproject"
    copy_files = ["templates/.env.example", "config/*.json"]
    setup_commands = ["npm install", 'echo "{{.Branch}}" > .worktree-branch']
    basedir = "./worktrees"
    ```
    - `setup_commands` は Go `text/template` でレンダリングしてから **POSIX `sh -c`** で実行。変数: `{{.Host}}` / `{{.Owner}}` / `{{.Repository}}` / `{{.Branch}}`(生、未サニタイズ)/ `{{.Hash}}` / `{{.Path}}`(絶対パス)。`sh -c` 経由なので `~` / `&&` / パイプが使える → **値にスペースやメタ文字が入りうるので自前でクォートせよと明記**(特に `{{.Path}}`)。未知のキー(`{{.Foo}}`)はそのコマンドをスキップして stderr にエラー(空文字に静かに展開しない)
  - **🔒 セキュリティ(v0.1.0 の破壊的変更、Kagi に最も効く一次情報)**: 「カレントディレクトリの任意の `.gwq.toml` が全サブコマンドでグローバル設定にマージされる」という**権限昇格の経路**を修正。敵性リポジトリが `repository_settings.setup_commands` を仕込んだ `.gwq.toml` を同梱すれば、次の `gwq add` で任意コードが走った。新挙動:
    - ローカル `.gwq.toml` は明示承認まで **untrusted**。`(絶対パス, SHA-256)` ペアごとに初回プロンプト、`~/.config/gwq/trusted_configs.json`(mode `0600`、atomic rename、symlink ガード)に direnv 方式で永続化
    - 内容が変わったら承認が無効化されて再プロンプト
    - **表示する内容の制御バイト(C0/C1, CR, DEL)を `\xHH` にエスケープ**(敵性 config が ANSI シーケンスで `[y/N]` プロンプトを偽装するのを防ぐ)。4 KiB で切り詰め
    - プロンプトと警告は **stderr**(stdout をプロトコルに使う `gwq cd` / completion を壊さないため)
    - 非 TTY(CI・スクリプト・パイプ)では**絶対にマージしない**、stderr 警告のみ。非通常ファイル(ディレクトリ・FIFO)もスキップ。trust store のパスが symlink なら書き込みを拒否
    - 承認の取り消しは `trusted_configs.json` を編集/削除(`gwq config trust/untrust` サブコマンドは**このリリースには入っていない**)
  - **ignored ファイル共有**: `copy_files`(glob)。symlink: **なし**
  - **後片付け**: `gwq remove` — `-f`(force)/ `-b`(branch も削除)/ `--force-delete-branch`(未マージ branch を強制)/ `-g` / **`--dry-run`(削除プレビュー)**、パターン指定・対話選択あり。`gwq prune`(削除済み worktree 情報の掃除)。**マージ済み worktree の自動検出・ディスク使用量表示: 対処なし**。削除前フック: **なし**
  - **横断状態一覧**: **`gwq status` が業界で最も充実**。`--watch`/`-w`(自動リフレッシュ)/ `--filter changed` / `--sort activity` / `-v` / `-g` / **`--json` / `--csv`**。「全 worktree の git status・変更・アクティビティを一覧」
  - **セッション紐付け**: **`gwq tmux` サブコマンド群** — `gwq tmux list` / `gwq tmux run "npm run dev"` / `gwq tmux run --id dev-server "npm run dev"` / `gwq tmux attach dev-server` / `gwq tmux kill dev-server`。「長時間プロセスを永続 tmux セッションで管理」
  - **シェル統合**: completion スクリプトが shell wrapper も提供。`cd.launch_shell = false` にすると `gwq cd` / `gwq add -s` が**新シェルを起動せず現シェルの cwd を変える**。`cd.auto_cd_on_add = true` で `gwq add` 後に常に cd。**PowerShell は未対応**
  - **その他設定**: `worktree.auto_mkdir` / `finder.preview` / `ui.icons` / `ui.tilde_home`
  - **ポート衝突 / DB 衝突**: **対処なし**
- **Kagi への示唆**: 3 点、うち 1 つは最重要。(a) **🔒 v0.1.0 の trust prompt は Kagi が post-create フックを実装するときの必須要件そのもの。** Kagi は safety-first を製品の存在理由にしているので、「リポジトリ同梱の設定ファイルから任意コマンドが走る」を無警告で許すのは製品原則違反になる。gwq が実装した (絶対パス, SHA-256) ペアでの信頼、内容変更で再確認、制御バイトのエスケープ、非 TTY では絶対に実行しない、の 4 点をそのまま採るべき。**Kagi は GUI なのでプロンプトを confirm モーダルとして出せる分、CLI より有利**。(b) `naming.template` によるパス生成の**テンプレート化**は、Kagi の固定 `../<repo>-worktrees/<branch>` より表現力が高い。(c) `status --watch --json --csv` の**機械可読出力**は、Kagi の headless テスト(klog 契約行)と相性が良い。
- **難易度**: (a) trust モデル M(ただし省略不可)/ (b) S / (c) S

### 2.11 wtp(Worktree Plus、Go、satococoa)

- **何か**: 「git-worktree の代わりに wtp を使う理由」を 4 つの摩擦で説明する worktree CLI。Homebrew `satococoa/tap/wtp` / `go install`。要 git 2.17+。Linux(x86_64/ARM64)+ macOS(Apple Silicon)。
- **出典**: <https://raw.githubusercontent.com/satococoa/wtp/main/README.md>
- **仕組み**:
  - **配置規約**: `.wtp.yml` の `defaults.base_dir`(デフォルト **`../worktrees`**)。`wtp add feature/auth` → `../worktrees/feature/auth`(**`/` を保ってネストする**)
  - **セットアップ自動化**: `.wtp.yml` の `hooks.post_create` が**型付きステップの配列**という、調査した中で最も構造化された形:
    ```yaml
    version: "1.0"
    defaults:
      base_dir: "../worktrees"
    hooks:
      post_create:
        - type: copy
          from: ".env"       # 常に MAIN worktree 基準。gitignored でも可
          to: ".env"         # 新 worktree 基準。省略時は from と同じ
        - type: copy
          from: ".claude"    # ディレクトリも可(.cursor/ 等)
        - type: symlink
          from: ".bin"       # MAIN worktree 基準(または絶対)
          to: ".bin"
        - type: command
          command: "npm install"
          env: { NODE_ENV: "development" }
        - type: command
          command: "make db:setup"
          work_dir: "."
    ```
    - **`copy` / `symlink` / `command` の 3 型を同じ配列で順序付きに書ける**のが要点。`from` が常に main worktree 基準なので、`wtp add` をどの worktree から実行しても結果が同じ
    - `symlink` の用途として **`.bin` / `.cache` / `node_modules`** が明記されている
  - **後片付け**: `wtp remove <name>` / `--force`(dirty でも)/ **`wtp remove --with-branch <name>`(branch がマージ済みのときだけ)** / `--with-branch --force-branch`(強制)。「worktree を消して branch を消し忘れる → 孤児 branch が溜まる」を**「1 コマンドで atomic に両方消す」**と位置づけている。孤児 prune / ディスク使用量: **対処なし**。削除前フック: **なし**
  - **リモート branch の扱い**: ローカルに無ければ**自動でリモート branch を track**。複数リモートに同名があれば「`origin, upstream` に存在する。欲しい方のローカル branch を作ってから再実行せよ」と**具体的な手順付きでエラー**にする(自動で選ばない)
  - **その他**: `wtp cd feature/auth`(tab 補完付き)/ `wtp cd @` または `wtp cd` で main worktree に戻る / `wtp exec <name> -- <cmd>` / `wtp add --exec "npm test"`(フック後に実行、TTY があれば対話コマンドも可)/ `wtp add --quiet`(作成した絶対パスのみ出力、スクリプト向け)
  - **一覧**: `wtp list` は PATH / BRANCH / HEAD の 3 列。main worktree は `@ (main worktree)*`。**dirty / ahead-behind / watch: なし**
  - **ポート衝突 / DB 衝突 / セッション紐付け**: **対処なし**(`type: command` で `make db:setup` する例のみ)
- **Kagi への示唆**: **`copy` / `symlink` / `command` を型付きステップの順序付き配列にする設計が、Kagi にとって最良の形。** 理由: Kagi の全操作は plan → confirm → preflight → execute → verify を通る。**型付きステップなら plan の時点で「何が起きるか」を正確に列挙できる**(「`.env` を copy」「`.bin` を symlink」「`npm install` を実行」)。文字列コマンドの羅列(gwq / phantom / Conductor)では plan に「シェルコマンドを 3 つ実行」としか書けない。さらに **`copy` と `symlink` は git 操作ではないので Kagi 自身が実装でき、`command` だけを trust prompt の対象にできる** — これは gwq の trust モデルと組み合わせると、「安全なステップは無確認、コマンド実行だけ確認」という Kagi らしい粒度になる。`--with-branch` が「マージ済みのときだけ」なのも ADR-0128 の分類と噛み合う。
- **難易度**: 型付きステップ M(Kagi の plan 表示との相性が良いので実装は素直)

### 2.12 wt(Go、raisedadead)— bare リポジトリ + TUI + oh-my-zsh 風フックシステム

- **何か**: bare リポジトリワークフロー前提の worktree マネージャ。CLI + lazygit 風 TUI。Homebrew `raisedadead/tap/wt`(**`git-wt` も入るので `git wt` がサブコマンドになる**)/ `go install`。要 git 2.20+、`gh`(任意)、`zoxide`(任意)。
- **出典**: <https://raw.githubusercontent.com/raisedadead/wt/main/README.md>
- **仕組み**:
  - **配置規約**: bare レイアウトを `wt clone owner/repo` で作る:
    ```
    project/
    ├── .bare/            # 共有リポジトリ
    ├── main/
    ├── feature-auth/
    └── fix-42-login/
    ```
    設定 `worktree_root = "~/DEV/worktrees"` / `branch_template = "{{type}}/{{number}}-{{slug}}"`
  - **設定**: 階層 TOML、優先順位 `runtime flag → .wt.toml(repo) → ~/.config/wt/config.toml(global) → defaults`。`wt config show` が**実効設定とその出所を表示する**
  - **セットアップ自動化**: **oh-my-zsh 風の名前付きフックシステム**。イベントは `pre_create` / `post_add` / `post_clone`。
    ```toml
    [hooks]
    post_clone = ["zoxide", "gh-default"]
    post_add   = ["zoxide", "direnv"]
    ```
    - **同梱フック**: `zoxide`(worktree を `z` に登録)/ `gh-default`(`gh` の default repo 設定)/ `direnv`(**新 worktree の `.envrc` を自動 allow**)/ `github-issue`(pre_create: issue メタデータを取って branch 名を提案)/ `github-pr`(pre_create: PR の実 branch を使う)
    - 管理: `wt hooks list` / `enable <name>` / `disable <name>` / `show <name>`
    - **カスタムフック**は `~/.config/wt/hooks/custom/` にスクリプトを置き、コメントでメタデータ宣言:
      ```bash
      # @name: my-setup
      # @description: Project-specific setup
      # @events: post_add
      cd "$WT_PATH" || exit 0
      npm install
      cp .env.example .env
      ```
    - 環境変数: `WT_PATH` / `WT_BRANCH` / `WT_PROJECT_ROOT` / `WT_DEFAULT_BRANCH` / `WT_WORKFLOW` / `WT_ISSUE` / `WT_PR`
    - **hook helper protocol**: pre_create フックが wt に**逆方向で branch 名やメタデータを提案できる**
    - タイムアウト: `hook_timeout = 30`、`--hook-timeout` で上書き、**`--no-hooks` で全スキップ**
  - **workflows**: branch 命名規約 + フック実行のセット。`--feature`/`-f`(`feat/{slug}`)/ `--bugfix`/`-b`(`fix/{slug}`)/ `--pr-review`(PR 自身の branch)/ `--workflow <w>`(TOML 定義のカスタム)。`--issue <n>` / `--pr <n>` で GitHub メタデータをフックに渡す(`wt add --bugfix --issue 42` → `fix/42-login-timeout`)
  - **後片付け**: `wt delete [branch...]`(worktree と branch)/ **`wt prune`(削除済みリモート branch・マージ済み branch の worktree を消す)**、`--merged` でマージ済みも対象、`--dry-run` でプレビュー。**`wt repair`(リポジトリ移動後の worktree パス修復)**。ディスク使用量: **対処なし**
  - **TUI**: worktree リスト(browse / `/` フィルタ / `space` 複数選択)+ 詳細パネル(**info / diff / log の 3 タブ**)+ 単キー操作(`enter` switch / `n` new / `N` workflow / `d` delete / `p` prune / `f` fetch)+ 確認ダイアログ
  - **一覧**: `wt list` は「status 付きテーブル」。`--path`(パスのみ)。**全コマンドが `--json`** — 一貫したエンベロープ `{ "success": bool, "command": str, "data": {...}, "error": null }`
  - **セッション紐付け**: completion に shell wrapper が同梱され `wt switch` が自動で cd。zoxide フックで `z auth` / `z main` で飛べる
  - **ポート衝突 / DB 衝突 / ignored ファイルの symlink 共有**: **対処なし**(カスタムフックで `cp .env.example .env` する例のみ)
- **Kagi への示唆**: 3 点。(a) **名前付き同梱フック + カスタムフックの二層**は、Kagi が「よくあるセットアップ」を無設定で提供する道を示す(`direnv` の `.envrc` auto-allow、`zoxide` 登録は特に汎用)。(b) **`wt config show` が実効設定と出所を出す**のは、Kagi の設定が複数層(グローバル / リポジトリ / モーダル入力)になったときに必須。(c) **`wt prune --merged` は「マージ済み worktree の一括削除」の直接の先例** — ADR-0128 の branch 分類ロジックが既にあるので、Kagi はこれを worktree に投影するだけでよい。(d) `--json` エンベロープの一貫性は Kagi の klog 契約行の設計思想と同じ。
- **難易度**: (a) M / (b) S / (c) M

### 2.13 worktree-link / `wtl`(Rust、km-tr)— symlink 専業

- **何か**: 「main worktree から新 worktree へ glob パターンで symlink を張る」だけの単機能 CLI。Rust 製。Homebrew `km-tr/tap/worktree-link`。
- **出典**: <https://raw.githubusercontent.com/km-tr/worktree-link/main/README.md>
- **仕組み**:
  - **設定ファイル `.worktreelinks`**(リポジトリルート、**gitignore 互換 glob**):
    ```gitignore
    # Dependencies
    node_modules
    # Environment variables
    .env
    .env.*
    # Build artifacts and caches
    .next/
    tmp/
    dist/
    # IDE settings
    .idea/
    .vscode/settings.json
    # Monorepo packages
    packages/*/node_modules
    ```
  - **オプション**: `-s/--source`(main worktree、デフォルトは `git worktree list` から自動検出)/ `-t/--target`(デフォルト `.`)/ `-c/--config`(デフォルト `<SOURCE>/.worktreelinks`)/ **`-n/--dry-run`** / `-f/--force` / `-v` / **`--unlink`** / `--no-ignore`
  - **挙動**:
    - **ディレクトリに一致したら、中のファイルを個別にリンクせずディレクトリ丸ごとを 1 本の symlink にする**(`node_modules` など)
    - **symlink は絶対パスで作る** → worktree の移動に耐える
    - 安全性: **`.git/` は常に除外** / 既存のファイル・symlink・ディレクトリは `--force` 無しでは絶対に上書きしない(`--force` ではディレクトリを再帰削除する)/ **`--unlink` は source ディレクトリを指す symlink だけを消す**
  - **プラットフォーム**: Unix symlink API(`#[cfg(unix)]`)。**ネイティブ Windows 非対応**。macOS でのみテスト済み、Linux は動くはずだが定期テストなし
- **Kagi への示唆**: **`.worktreelinks` は `.worktreeinclude` の symlink 版**。両方を読む価値がある(copy = `.worktreeinclude`、symlink = `.worktreelinks`)。特に (a) **ディレクトリは丸ごと 1 本の symlink**(中身を個別リンクしない)、(b) **絶対パスで張って worktree 移動に耐える**、(c) **`--unlink` が source を指す symlink だけを消す**(=後片付けが安全に戻せる)、(d) **既存ファイルは force なしで絶対に上書きしない** の 4 点は、Kagi が symlink ステップを実装するときの仕様としてそのまま使える。(d) は Kagi の「破壊操作を提供しない」原則と一致する。Rust 製なので実装参考としても読みやすい。
- **難易度**: symlink ステップ S〜M(Kagi は Rust なので `std::os::unix::fs::symlink` 直呼びで済む)

### 2.14 git-worktree.nvim(polarmutex 版 / ThePrimeagen 版)

- **何か**: Neovim の worktree ラッパ。「create / switch / delete をラップするだけ」と自称。要 neovim >= 0.9 + plenary.nvim、任意で telescope.nvim。LuaRocks 配布(`version = "^2"`)。
- **出典**: <https://raw.githubusercontent.com/polarmutex/git-worktree.nvim/main/README.md>
- **仕組み**:
  - API は 3 つ: `create_worktree(path, branch, upstream)` / `switch_worktree(path)` / `delete_worktree(path)`。パスは git root からの相対 or 絶対
  - **フック**: `require("git-worktree.hooks")` の `Hooks.register(Hooks.type.SWITCH, fn)` / `Hooks.register(Hooks.type.DELETE, fn)`。SWITCH のコールバックは `(path, prev_path)` を受ける。**builtins は一切デフォルト登録されない**(`Hooks.builtins.update_current_buffer_on_switch` を明示登録する必要がある)
    ```lua
    Hooks.register(Hooks.type.SWITCH, function (path, prev_path)
      vim.notify("Moved from " .. prev_path .. " to " .. path)
      update_on_switch(path, prev_path)
    end)
    ```
  - 設定は `vim.g.git_worktree` テーブル。UI は telescope 拡張(`require('telescope').load_extension('git_worktree')`)
  - ログは `git-worktree-nvim.log`(neovim cache path)。レベルは `vim.g.git_worktree_log_level` または環境変数 `GIT_WORKTREE_NVIM_LOG`
  - 既知の罠(README の Troubleshooting): **`gh` CLI で作ったリポジトリは `remote.origin.fetch` が `+refs/heads/*:refs/remotes/origin/*` になっていないことがあり、upstream が正しく設定されず pull/push が壊れる**
  - **配置規約 / セットアップ自動化(copy/symlink)/ 後片付け(prune・ディスク)/ 横断状態一覧 / ポート・DB 衝突: すべて対処なし**(フックで自分で書く前提)
- **Kagi への示唆**: 2 点。(a) **`SWITCH` フックが `(path, prev_path)` を渡す**のは Kagi の repo タブ切り替え(worktree WIP 行クリック → `open_repository`)に対応するイベントで、Kagi も「worktree を切り替えた」を通知できると外部連携(埋め込みターミナルの cwd 追随、埋め込みエディタのバッファ更新)が素直になる。(b) **`remote.origin.fetch` の refspec が壊れていると upstream が付かない**という罠は、Kagi の worktree 作成が `--track` 相当をやるなら preflight で検査すべき項目。
- **難易度**: (a) S(内部イベントとして既にありそう)/ (b) S

### 2.15 twm(Rust、vinnymeller)— tmux ワークスペースマネージャ

- **何か**: worktree ツールではなく tmux workspace マネージャ。**「worktree branch を window で開く機能は入れない」と明言している**点が示唆的。
- **出典**: <https://raw.githubusercontent.com/vinnymeller/twm/master/README.md>(main ブランチは存在せず master)
- **仕組み**:
  - ワークスペース定義 = 設定のパターンに一致するディレクトリ。**無設定なら `.git` ファイル/フォルダ または `.twm.yaml` を含むディレクトリ**(=worktree もそのまま拾える)
  - 設定: `$XDG_CONFIG_HOME/twm/twm.yaml`(+ `twm.schema.json` を自動生成して yaml-language-server で補完・検証)。ローカルレイアウトは `.twm.yaml`。`TWM_CONFIG_FILE` で上書き
  - **セッション内に渡す環境変数**: `TWM=1` / `TWM_ROOT`(ワークスペースルート)/ `TWM_TYPE`(設定の workspace definition の `name`)/ `TWM_NAME`(作成時のセッション名)。「**1 つの共通セットアップスクリプトを `TWM_TYPE` で分岐させる**」使い方を README が推奨
  - `-e`(既存セッションを選んでアタッチ)/ `-g`/`-G`(既存セッションと同じ group で新セッション)/ `-d`(アタッチしない)/ `-l`(グローバル layout を選ぶ)/ `-p <path>`(任意パスをワークスペースとして開く)/ `-n <name>` / **`-N`(生成されるセッション名を stdout に出す)** / `-c <command>` / `-s <SEARCH_PATHS>` / `-D <DEPTH>`
  - **⚠ 「入れない機能」として明記されているもの**: 「worktree branch を window で開く。layout でやれ。worktree があるワークスペースを検出してスクリプトを走らせろ。**それが組み込みより常に柔軟だ**」/ 「git リポジトリを新ワークスペースに clone する」/ 「セッションを選んで kill する fuzzy finder」。設計哲学: 「**シェルスクリプトで *well* できることは、たぶんそうすべき**」
  - **ignored ファイル共有 / 後片付け / 横断状態一覧 / ポート・DB 衝突**: 対処なし(スコープ外)
- **Kagi への示唆**: **「入れない」判断の一次情報として価値がある。** twm の作者は「worktree を window に並べる」を明示的に拒否し、layout + スクリプトに委ねた。Kagi も「tmux/zellij セッション管理そのもの」を実装すべきではない — Kagi は埋め込みターミナルを持っているので、**外部マルチプレクサとの統合ではなく、自分のターミナルタブを worktree に紐づける方が筋**。`TWM_TYPE` で分岐する 1 本の共通セットアップスクリプト、という発想は Kagi のフック環境変数設計(`KAGI_WORKTREE_*`)に効く。
- **難易度**: —(方針の裏付け)

### 2.16 sesh(Go、joshmedeski)— tmux セッションマネージャ

- **何か**: zoxide 連携の tmux セッションマネージャ。worktree は「ルートへ飛ぶ」文脈で扱う。
- **出典**: <https://raw.githubusercontent.com/joshmedeski/sesh/main/README.md>
- **仕組み**:
  - **worktree 関連**: 機能リストに「**Root session navigation — jump to the root of a git worktree or repository**」。セッション名は「git repo / git remote / ディレクトリ名」から自動生成
  - `sesh.toml` の `[[session]]` / `[[wildcard]]` / `[default_session]`:
    - `startup_command`(セッション作成時に実行。**`--command/-c` を使うと startup script は走らない**)、`preview_command`(`{}` がセッションパスに置換)、`disable_startup_command = true` で個別無効化
    - `[[wildcard]] pattern = "~/c/work/*"` で glob 一致するプロジェクト全体に設定を当てる(**複数パターンが一致したら config 順で最初が勝つ**)
    - `alias` / `alias_auto_connect`(エイリアスを打ち切った瞬間に接続。**「オプトインなのは、頻繁に飛ぶ数個だけ価値があるから」**)。エイリアスは大文字小文字を無視して一意、重複は起動時エラー
    - `[[name_substitution]]` で `find`/`replace`(`regex = true` で Go 正規表現 + `$1` キャプチャ)。**ルールは順に適用され、各ルールが前の結果を見る**。tmux セッション名に使えない `.` `:` は正規化、スペースは `_`
    - `sort_order`(デフォルト `tmux` → `config` → `tmuxinator` → `zoxide`)
    - `[tui]` の `preview` / `preview_width`(%)/ `preview_min_width`(デフォルト 100。**これより狭い端末では分割せず、広くなったら自動で戻る**)/ `preview_border`(`line`/`thick`/`double`/`none`)/ `show_windows`(セッション内 window 名を dim で併記、入らない分は `+N`)/ `window_name_format`(任意の tmux format、`#{?#{pane_title},#{pane_title},#{window_name}}` のような条件式が使える)/ `show_icons` + per-session `icon`(nerd font でも emoji でも)
    - `cache = true`(実験的。`$XDG_CACHE_HOME/sesh/sessions.gob`、**stale-while-revalidate で TTL 5 秒**。`sesh connect` 後は自動更新。tmux hooks `session-created` / `session-closed` で `sesh cache refresh`)
  - `sesh picker -q work/`(フィルタを打った状態で開く)/ `#` 先頭で番号ジャンプ(1–9、可視リストに追随して再採番)
  - **配置規約 / セットアップ自動化(copy/symlink)/ 後片付け / 横断状態一覧(git 状態)/ ポート・DB 衝突**: 対処なし(スコープ外)
- **Kagi への示唆**: 2 点。(a) **`preview_min_width` = 「狭い端末では分割をやめ、広くなったら自分で戻る」**という応答性の作り方は、Kagi の GPUI パネル(worktree 一覧 + プレビュー)にそのまま使える発想。(b) **`alias_auto_connect` をオプトインにした理由の説明**(「頻繁に飛ぶ数個だけ価値がある」)は、Kagi が worktree Favorites を入れるときの粒度の指針。
- **難易度**: —(UX 参考)

### 2.17 Uzi(Go、devflowinc)— ポート管理を持つ agent 並列実行

- **何か**: 複数 AI エージェントを worktree + tmux で並列実行し、**ポートを自動割り当てして各エージェントの dev server を同時に起動する** CLI。`go install github.com/devflowinc/uzi@latest`。
- **出典**: <https://raw.githubusercontent.com/devflowinc/uzi/main/README.md>、<https://www.uzi.sh>、
  ホワイトペーパー <https://cdn.trieve.ai/uzi-whitepaper.pdf>
- **仕組み**:
  - **設定 `uzi.yaml`**(プロジェクトルート)— 全部で 2 キーしかない:
    ```yaml
    devCommand: cd astrobits && yarn && yarn dev --port $PORT
    portRange: 3000-3010
    ```
    - `devCommand`: **`$PORT` がプレースホルダ**。Next.js/Vite は `npm run dev -- --port $PORT`、Django は `python manage.py runserver 0.0.0.0:$PORT`
    - **「`devCommand` にはセットアップ手順(`npm install` / `pip install`)を全部含めること。各エージェントは自分の worktree で自分の依存を持つ隔離環境で走るから」と明記** — つまり Uzi は copy/symlink をせず、毎回インストールさせる
    - `portRange`: `start-end`
  - **配置規約**: `~/.local/share/uzi` にデータを置く(`uzi reset` が「`~/.local/share/uzi` の全データを消す」と警告)。worktree のパス自体は**未確認**
  - **エージェント起動**: `uzi prompt --agents claude:3,codex:2 "<task>"`(`agent:count[,agent:count...]`、`random` で名前をランダム生成)。`--agents=claude:2,aider:2,cursor:1` のように異種混在も可
  - **⭐ `uzi auto`**: 「全エージェントセッションを監視して**プロンプトを自動処理する**」— trust プロンプトに Enter を自動送出、継続確認を処理、Ctrl+C までバックグラウンド動作
  - **横断状態一覧**: `uzi ls` / **`uzi ls -w`(1 秒ごとにリフレッシュ)**。列は `AGENT` / `MODEL` / `STATUS` / **`DIFF`(`+0/-0` 形式)** / **`ADDR`(`http://localhost:3003` — 割り当てられたポートの URL)** / `PROMPT`
  - `uzi run "<cmd>"`(全エージェントで実行、`--delete` で実行後に tmux window を消す)/ `uzi broadcast "<msg>"`(全エージェントにメッセージ)/ `uzi kill <name>` / `uzi kill all`
  - **取り込み**: `uzi checkpoint <agent-name> "<commit msg>"` — 「commit してエージェントの worktree の変更を現ブランチに **rebase** する」
  - **ignored ファイル共有 / 削除前フック / ディスク使用量 / マージ済み検出**: **対処なし**
- **Kagi への示唆**: 2 点。(a) **`portRange` + `$PORT` プレースホルダは、Conductor の 10 ポート予約より軽量な解**で、Kagi の埋め込みターミナルに `KAGI_PORT` を注入する形なら設定 1 行(`port_range = "3000-3010"`)で済む。(b) **`uzi ls` の `ADDR` 列(worktree ごとの localhost URL)**は「今どの worktree のアプリがどのポートで動いているか」を可視化する最小の形で、Kagi の WIP 行やサイドバーに出せば実用価値が高い。(c) **`uzi auto`(確認プロンプトの自動 Enter)は Kagi が絶対に真似すべきでないもの** — Kagi の二段確認は製品の存在理由であり、自動承認はそれを無効化する。
- **難易度**: (a) M / (b) S(ポート割り当てができた後なら)

### 2.18 claude-squad / `cs`(Go、smtg-ai)

- **何か**: tmux + worktree で Claude Code / Codex / Gemini / Aider を並列管理する TUI。AGPL-3.0。Homebrew `brew install claude-squad`。要 tmux + `gh`。
- **出典**: <https://raw.githubusercontent.com/smtg-ai/claude-squad/main/README.md>、<https://smtg-ai.github.io/claude-squad/>
- **仕組み**:
  - **How It Works**(README が明記): (1) tmux で各エージェントに隔離ターミナルセッション、(2) **git worktrees でコードベースを隔離し、各セッションが自分の branch で作業**、(3) ナビゲーション用 TUI
  - **設定**: `~/.claude-squad/config.json`(`cs debug` で正確なパスが出る)。**profiles** 配列で名前付きプログラム設定を複数定義し、セッション作成時に `←`/`→` で切り替え:
    ```json
    { "default_program": "claude",
      "profiles": [ { "name": "claude", "program": "claude" },
                    { "name": "codex",  "program": "codex" },
                    { "name": "aider",  "program": "aider --model ollama_chat/gemma3:1b" } ] }
    ```
  - **キーバインド**: `n` 新規セッション / `N` プロンプト付き新規 / `D` kill(削除)/ `↵`/`o` アタッチして再プロンプト / `ctrl-q` デタッチ / **`s` commit して branch を GitHub に push** / **`c` checkout(変更を commit してセッションを一時停止)** / `r` 一時停止セッションを再開 / `tab` preview タブと **diff タブ**を切り替え / `shift-↓/↑` diff ビューをスクロール
  - **`-y/--autoyes`**(experimental): 全インスタンスが claude code / aider のプロンプトを自動承認
  - **配置規約 / セットアップ自動化 / ignored ファイル共有 / prune / ディスク使用量 / ポート・DB 衝突**: **対処なし**(README に記載なし)
  - 既知の罠: `failed to start new session: timed out waiting for tmux session` → 下層プログラム(`claude`)を最新に上げよ
- **Kagi への示唆**: **`c` (checkout) = 「変更を commit してセッションを一時停止」というモデル**が面白い。「作業を止める」を「commit + 一時停止」に写像すると、未 commit 変更が消える経路が構造的に無くなる。Kagi が worktree を「アーカイブ」する操作を作るなら、Zed(git 状態を保存して disk から消す)と claude-squad(commit して一時停止)の 2 案があり、**Kagi の oplog + ODB backup があるなら Zed 型(復元可能な削除)が製品原則に合う**。`profiles` は Kagi の smart-commit CLI provider 設定と同型。
- **難易度**: アーカイブ/復元 L

### 2.19 Crystal → Nimbalyst(stravu → Nimbalyst)

- **何か**: 「複数の Codex / Claude Code セッションを並列 git worktree で走らせる」Electron デスクトップアプリ。star 3113 / fork 197。**2026-02 に deprecated、Nimbalyst に置き換わった**。
- **出典**: <https://github.com/stravu/crystal>(README が移行告知に置き換わっている)、<https://nimbalyst.com/>、
  <https://docs.nimbalyst.com/>、<https://github.com/Nimbalyst/nimbalyst>
- **仕組み**(Crystal のリポジトリ構造から読める範囲):
  - `docs/CRYSTAL_ARCHITECTURE.md` / `DATABASE_DOCUMENTATION.md` / `SESSION_OUTPUT_SYSTEM.md` / `TOOL_PANEL_SYSTEM.md` / `IMPLEMENTING_NEW_CLI_AGENTS.md` / `ADDING_NEW_CLI_TOOLS.md` を持つ。フロントに `CommitDialog` / `CommitModeIndicator` / `CommitModeSettings` / `CommitModeToggle` / `GitStatusIndicator` / **`MainBranchWarningDialog`** / `PermissionDialog` / `ProjectDashboard` / `ArchiveProgress` / `DraggableProjectTreeView` などのコンポーネント
  - Nimbalyst の機能リスト(公式)に「**Git worktree isolation for safer parallel AI coding sessions**」「Project-level workspace management and AI session tracking」「Code-aware agent orchestration」
  - **配置規約 / フック / ignored ファイル共有 / prune / ポート・DB 衝突の具体仕様: 未確認**(Crystal は deprecated、Nimbalyst の docs は本調査では詳細まで未読)
- **Kagi への示唆**: **`MainBranchWarningDialog` と `CommitMode*` 系のコンポーネント名だけでも示唆がある** — 「エージェントが main branch で作業しようとしている」を警告する専用ダイアログを持っていた。Kagi は worktree 作成で branch を強制するので同型の事故は起きにくいが、「main worktree で agent を走らせようとしている」警告は Kagi にも意味がある。**star 3113 の worktree×agent アプリが 1 年足らずで deprecated になった**という事実は、この領域の製品寿命が短く、Kagi のような「安全性という別軸の価値」の方が持続しやすいことの示唆。
- **難易度**: —(参照)

### 2.20 vibe-kanban(Rust + TS、BloopAI)

- **何か**: kanban ボードで計画し、worktree ベースの workspace でエージェントを実行する。`npx vibe-kanban`。**2026 時点で sunsetting 告知済み**。
- **出典**: <https://raw.githubusercontent.com/BloopAI/vibe-kanban/main/README.md>、<https://vibekanban.com/docs>、
  <https://www.vibekanban.com/blog/shutdown>
- **仕組み**:
  - 「各 workspace はエージェントに **branch、ターミナル、dev server** を与える」。10+ のエージェント(Claude Code / Codex / Gemini CLI / GitHub Copilot / Amp / Cursor / OpenCode / Droid / CCR / Qwen Code)を切り替え
  - **ポート管理**(環境変数、README のテーブル): `PORT`(production はサーバーポート、**dev はフロントエンドポートで backend は PORT+1**)/ `BACKEND_PORT`(デフォルト `0` = **自動割り当て**)/ `FRONTEND_PORT`(デフォルト 3000)/ `HOST`(デフォルト `127.0.0.1`)/ `MCP_HOST` / `MCP_PORT`
  - **⭐ `DISABLE_WORKTREE_CLEANUP`**(runtime、デフォルト未設定): 「**孤児(orphan)および期限切れ(expired)workspace の掃除を含む、全ての git worktree cleanup を無効化する**(デバッグ用)」→ 逆に言えば **vibe-kanban は通常時、孤児 worktree と期限切れ workspace を自動で掃除している**
  - `VK_ALLOWED_ORIGINS` / `VK_SHARED_API_BASE` / `VK_SHARED_RELAY_API_BASE` / `VK_TUNNEL` / `POSTHOG_API_KEY`
  - **リモートデプロイ + エディタ紐付け**: Settings → Editor Integration で **Remote SSH Host / Remote SSH User** を設定すると、「Open in VSCode」ボタンが `vscode://vscode-remote/ssh-remote+user@host/path` を生成する
  - 機能: diff レビュー + インラインコメント(エージェントに直送)/ **devtools・inspect モード・デバイスエミュレーション付きの組み込みブラウザで自分のアプリをプレビュー** / AI 生成説明付きの PR 作成とマージ
  - **配置規約 / セットアップ自動化 / ignored ファイル共有の具体仕様: 未確認**(README に記載なし、`https://vibekanban.com/docs` 側は本調査では未読)
- **Kagi への示唆**: 3 点。(a) **`DISABLE_WORKTREE_CLEANUP` の存在は「孤児 worktree の自動掃除は実装したら必ず off スイッチが要る」ことの実証** — Kagi が自動掃除を入れるなら設定で切れる必要がある(そして Kagi の製品原則からは**自動削除自体を提供しない**方が正しく、ADR-0128 が既に「自動掃除は非目標」と決めている。**worktree 側も同じ線で「列挙と手動削除のみ」にすべき**)。(b) `BACKEND_PORT = 0` で **OS に自動割り当てさせる**のは、portRange を管理しない最も単純な解。(c) `vscode://vscode-remote/ssh-remote+...` URL スキームは、Kagi の「エディタで開く」がリモート SSH リポジトリ(ADR-0089)に対応するときの形。
- **難易度**: (b) S / (c) M

### 2.21 Sculptor(Imbue)— コンテナ隔離という別解

- **何か**: 「エージェントを隔離コンテナで走らせる」デスクトップアプリ。オープンソース、ローカル実行(Imbue のサーバーを使わない)。Mac(Apple Silicon)/ Linux / Linux ARM64。
- **出典**: <https://imbue.com/product/sculptor>、<https://imbue.com/blog/sculptor-announce>、
  <https://imbue.com/blog/containers>、<https://github.com/imbue-ai/sculptor>、<https://docs.imbue.com/>
- **仕組み**:
  - **「各 workspace は自分の branch・ターミナル・diff ビューを持つ隔離 worktree」**と説明しつつ、**実体は「新しい Sculptor エージェントを立てるとリポジトリが dev container spec からビルドされた新しい Docker コンテナに clone される」**。つまり worktree ではなくコンテナが隔離境界
  - **worktree ベースツールとの差**(公式ブログの主張): 「コンテナ隔離は本物の問題を解く。worktree ベースのツールと違い、各エージェントが**依存をインストールし、テストを走らせ、コードを実行しても、自分のマシンや他のエージェントに影響しない**」
  - **⭐ Pairing Mode**: 「ワンクリックでエージェントの作業をコンテナからローカルリポジトリに持ってきて、**ファイルと git 状態を同期し続ける**ので IDE から直接協働できる」
  - 「5+ のエージェントを 5+ のチケットで同時に走らせる」。モデルロックインなし(Pi を有効にして任意プロバイダを選択、セッション中のモデル切り替えも可)
  - docs の構成: getting started / **workspaces** / chat / terminal / agents / integrated harnesses / changes / pull requests / skills / command palette / settings / **container backend configuration**
  - コンテナ起動の高速化について専用のブログ記事(<https://imbue.com/blog/containers>)を持つ(「sandboxed coding agents は依存のダウンロードで数分を無駄にする」を 10x 改善)
  - **配置規約 / `.worktreeinclude` 相当 / prune / ディスク使用量の具体仕様: 未確認**
- **Kagi への示唆**: **これは Kagi が「取り込まない」と判断すべきものの代表例**(§4 に記載)。ただし **Pairing Mode の考え方 = 「隔離環境の作業をワンクリックで手元に引き寄せる」は、VS Code の `Migrate Worktree Changes` と同じ問題への別解**で、Kagi がやるなら後者(git 内で完結する)を選ぶべき。「コンテナ隔離 vs worktree 隔離」の対立軸は、Kagi のポジション説明(「Kagi は git の中の安全性を極める。プロセス隔離はやらない」)を書くときの材料。
- **難易度**: —(非採用)

### 2.22 container-use / `cu`(Dagger)— worktree ではなく branch + コンテナ

- **何か**: MCP サーバ兼 CLI。「各エージェントが自分の git branch の中の新しいコンテナを持つ」。star 4027 / fork 202、Apache-2.0、Go 製。`brew install dagger/tap/container-use`。**stability: experimental**。
- **出典**: <https://raw.githubusercontent.com/dagger/container-use/main/README.md>、
  <https://raw.githubusercontent.com/dagger/container-use/main/docs/environment-configuration.mdx>、
  <https://container-use.com>
- **仕組み**:
  - **隔離単位は「environment」= コンテナ + git branch**(worktree ではない)。「標準の git ワークフロー — `git checkout <branch_name>` するだけで任意のエージェントの作業をレビューできる」
  - **設定は `.container-use/environment.json`**(「このディレクトリを commit してチームでセットアップを共有せよ」)。デフォルトは **Ubuntu 24.04 + 標準ツール(git, curl, bash, apt)**
  - **設定は CLI サブコマンドで組み立てる**(ファイルを直接書かせない):
    - `container-use config base-image set python:3.11` / `get` / `reset`(→ `ubuntu:24.04`)
    - **`config setup-command add "..."`**(ベースイメージ pull 後、**コードコピー前**に実行)/ `list` / `remove` / `clear`
    - **`config install-command add "npm install"`**(**コードコピー後**に実行)/ `list` / `remove` / `clear`
    - `config env set NODE_ENV development` / `list` / `unset` / `clear`
    - `config secret set API_KEY` / `list` / `unset` / `clear`
    - `config show [<env>] [--json]`
  - **⭐ 2 層設定モデル**: (1) デフォルト設定 = 全エージェントの出発点、(2) **エージェントの適応 = 作業中にエージェント自身が環境設定を変える(ツール追加・ベースイメージ変更・変数設定)。これは ephemeral で、`container-use config import <env>` で明示的に取り込むまで反映されない**
  - コマンド: `list` / `log` / `diff` / `checkout` / `apply` / `merge` / `delete` / `prune` / `inspect` / `terminal` / `watch` / `stdio`
  - **「Real-time Visibility」**: 「エージェントが**実際に何をしたか**の完全なコマンド履歴とログを見せる。エージェントが主張したことではなく」
  - **「Direct Intervention」**: 「任意のエージェントのターミナルに降りて状態を見て、詰まったら操作を取る」
  - エージェント連携: `claude mcp add container-use -- container-use stdio`。`cmd/container-use/agent/` に `configure_claude.go` / `configure_codex.go` / `configure_copilot.go` / `configure_cursor.go` / `configure_goose.go` / `configure_q.go` がある(=各エージェントの MCP 設定を自動生成する)
  - トラブルシュート: 環境作成が失敗したら `container-use log <environment-id>` → `config setup-command remove "broken-command"` → `add "fixed-command"`
- **Kagi への示唆**: 2 点。(a) **`setup-command`(コードコピー前)と `install-command`(コードコピー後)を分けている**のは Docker レイヤキャッシュのためだが、worktree でも「**worktree 作成前にやること / 作成後にやること**」の分離として意味がある(例: base branch の fetch は作成前、`npm install` は作成後)。Conductor の `setup` 単一フックより表現力が高い。(b) **「エージェントが主張したことではなく実際に何をしたかのログ」**は Kagi の oplog の思想と完全に同じ。Kagi の oplog を「worktree ごとにフィルタして見せる」と、この価値がそのまま出る。
- **難易度**: (a) S / (b) M

### 2.23 pnpm global virtual store(node_modules 共有の正解)

- **何か**: pnpm の `virtualStoreType: global`。worktree を跨いで node_modules をほぼゼロコストで共有する公式手段。**pnpm が「multi-agent development」向けの専用ドキュメントページを持っている**。
- **出典**: <https://pnpm.io/git-worktrees>(pnpm docs v11 & 12)、<https://pnpm.io/global-virtual-store>
- **仕組み**:
  - **bare リポジトリ + worktree レイアウト**を推奨:
    ```sh
    git clone --bare https://github.com/your-org/your-monorepo.git your-monorepo
    cd your-monorepo
    git worktree add ./main main
    git worktree add ./feature-auth feat/auth
    ```
  - `pnpm-workspace.yaml` に **`virtualStoreType: global`**(**v11.23.0 より前は `enableGlobalVirtualStore: true` という綴り**)
  - 各 worktree で `pnpm install`。**最初の install がグローバルストアにダウンロードし、以降の worktree の install は symlink を張るだけなのでほぼ即時**
  - 結果の構造(各 worktree の `node_modules` は**グローバルストアへの symlink だけ**):
    ```
    your-monorepo/                      (bare git repo)
    ├── main/node_modules/lodash → <global-store>/links/@/lodash/...
    ├── feature-auth/node_modules/lodash → <global-store>/links/@/lodash/...  ← same target
    └── fix-api/node_modules/lodash → <global-store>/links/@/lodash/...       ← same target
    ```
  - **デフォルト挙動との差**: 通常の pnpm は content-addressable store から `node_modules/.pnpm` へ**ハードリンク**する。global virtual store では **worktree 内に 1 バイトもコピー/ハードリンクされない**
  - **利点(公式の主張)**: worktree ごとのオーバーヘッドがほぼゼロ / 新 worktree の install が即時 / **各 worktree が自分の node_modules ツリーを持つので、branch ごとに違う依存バージョンを入れても衝突しない**
  - **⚠ セキュリティ注記**: 「このセットアップは worktree とエージェントが**同じ信頼境界**を共有することを前提とする。**相互に信頼できないエージェントやユーザーで 1 つの書き込み可能な pnpm ストアを使ってはいけない**」
  - pnpm 自身のリポジトリがこの構成を使っており、ヘルパースクリプトを同梱: `pnpm worktree:new <branch-name|pr-number>`。plain `git worktree add` を超えてやっていること 3 つ:
    1. **PR 番号は `git fetch origin pull/<number>/head` で取る(fork でも動く)**
    2. **`feat/my-feature` のような `/` 入り branch 名はディレクトリ名では `-` に変換**(`feat-my-feature`)
    3. **`.claude` ディレクトリを bare リポジトリの git common dir から新 worktree へ symlink する**(全 worktree が同じ Claude Code 設定と承認済みコマンドを共有する)
  - シェルヘルパー `shell/wt.sh` が上記をラップして新 worktree に `cd` する
  - **⚠ 素朴な symlink の危険性**(二次情報だが一致した記述): 「node_modules を worktree 間で symlink するのは**両 branch の依存が完全に一致するときだけ**動く。乖離すると一方の worktree が誤ったバージョンを持ち、デバッグの難しい問題になる」(<https://www.gitworktree.org/guides/node-modules>)
- **Kagi への示唆**: 3 点。(a) **「node_modules を symlink で共有する」を Kagi が推奨機能として出してはいけない** — 依存が乖離した瞬間に壊れる。代わりに **`.worktreelinks` 相当の symlink ステップは提供するが、`node_modules` の共有については「パッケージマネージャの機能(pnpm global virtual store)を使え」と案内する**のが正しい。(b) **`.claude` を git common dir から symlink** という pnpm 自身の運用は、「AI エージェント設定を worktree 間で共有する」という Kagi の AI native 化に直結する具体パターン。(c) **「相互に信頼できないエージェントで書き込み可能ストアを共有するな」**は、Kagi が worktree 隔離を「セキュリティ境界ではない」と明言すべき根拠(Conductor も同じことを書いている)。
- **難易度**: (a) ドキュメント/文言のみ S / (b) S

---

### 2.24 機能比較表(24 行 × 7 観点)

凡例: ✅ = あり / ⚠ = 部分的・条件付き / ❌ = なし / ? = 未確認

| # | ツール / プロダクト | 命名・配置規約(実リテラル) | セットアップ自動化(設定キー) | ignored ファイル共有 | 後片付け | エディタ/端末/セッション紐付け | 横断状態一覧 | ポート・DB 衝突 |
|---|---|---|---|---|---|---|---|---|
| 1 | **git 本体 2.54** | `add <path>`(規約なし)。admin dir は `$GIT_DIR/worktrees/<basename>[N]` | ❌ | ❌ | ✅ `remove`(clean のみ)/ `prune --expire` / `repair` / `move` / `lock --reason` | ❌ | ⚠ `list -v --porcelain`(branch / locked / prunable) | ❌ |
| 2 | **Claude Code `--worktree`** | `.claude/worktrees/<name>/`、branch `worktree-<name>`。PR は `.claude/worktrees/pr-<n>` | ⚠ 「Claude に頼む」or `WorktreeCreate` フックで丸ごと差し替え | ✅ **`.worktreeinclude`**(gitignore 構文、ignored のみ) | ✅ 終了時に検査して自動/確認。`cleanupPeriodDays` スイープ。**自分が作ったものだけ**(git メタデータのマーカー) | ✅ セッションが worktree に紐づく(resume で復帰) | ❌ | ❌ |
| 3 | **Conductor** | `~/conductor/workspaces/<repo>/<workspace>`(都市名風自動生成) | ✅ **`[scripts]` の `setup` / `run`(名前付き複数)/ `archive` / `run_mode`** | ✅ `.worktreeinclude` > `file_include_globs` > `.env*` | ✅ archive(`scripts.archive` が前に走る)+ History から**チャット込みで復元** | ✅ workspace ごとにターミナル・Run ボタン・IDE で開く | ✅ サイドバー + Diff Viewer + **Checks タブ**(git status / CI / deploy / comments / todos) | ✅ **`CONDUCTOR_PORT` から 10 ポート予約** + `run_mode: nonconcurrent` + `CONDUCTOR_WORKSPACE_NAME` を資源名に混ぜる指針 |
| 4 | **Zed** | 設定 **`git.worktree_directory`**、デフォルト `../worktrees`。**detached HEAD で作る** | ✅ **`create_worktree` タスクフック**(`ZED_WORKTREE_ROOT` / `ZED_MAIN_GIT_WORKTREE`) | ⚠ フックで `cp` する(組み込みなし) | ✅ ピッカーから削除(開いていないもののみ)。**スレッドを History に移すと git 状態を保存して disk から消し、復元で戻る** | ✅ worktree ピッカー(現ウィンドウ/新ウィンドウ)+ Threads Sidebar | ⚠ Threads Sidebar でプロジェクト単位にグループ化。**どの worktree にいるか不明**(#53807) | ❌ |
| 5 | **VS Code 本体** | SCM Repositories → Worktrees > Create Worktree(場所はプロンプト) | ❌ | ✅ **`git.worktreeIncludeFiles`**(glob、ignored のみ)。**`node_modules/**` を公式に推奨** | ? 公式ドキュメントに削除節なし | ✅ `Open Worktree in New/Current Window` / Open Recent | ⚠ 各 worktree が SCM に別リポジトリとして並ぶ。`git.detectWorktrees` / `detectWorktreesLimit`(50) | ❌ |
| 6 | **JetBrains IDEA 2026.2** | New Worktree ダイアログ(Project name + Location)。**プロジェクト内に作るなと警告** | ❌ | ❌ | ✅ Worktrees タブの Delete / **Prune** | ✅ 別プロジェクトとして開く / ダブルクリックで切り替え / Recent Projects | ⚠ **`Locked` / `Prunable` のみ**。dirty・ahead/behind なし | ❌ |
| 7 | **GitLens (Pro)** | ? | ? | ? | ⚠ 「作成・表示・管理」(詳細記載なし) | ⚠ Worktrees ビュー | ⚠ Worktrees ビュー(詳細記載なし) | ❌ |
| 8 | **git-worktree-manager**(VSCode 拡張) | `Ctrl+Shift+R`。パスはダイアログ | ✅ **`postCreateCmd`** | ✅ **`worktreeCopyPatterns` + `worktreeCopyIgnores`(二段構え)** / Copy Untracked Files | ✅ **`preRemoveCmd`(失敗/キャンセルで削除中止)** | ✅ VSCode workspace に追加 / **Favorites** / `terminal.external.{osx,windows}Exec` | ✅ **`worktreeDescriptionTemplate` / `worktreeLabelTemplate` に `$LAST_COMMIT`(stale 発見用)** | ⚠ `preRemoveCmd` で per-worktree DB スキーマ撤去を例示 |
| 9 | **phantom** | **`.git/phantom/worktrees/<name>`**。`worktreesDirectory` / `directoryNameSeparator`(`feature/test`→`feature-test`) | ✅ **`postCreate.commands`**(順次、初回失敗で停止) | ✅ **`postCreate.copyFiles`**(glob、`dot:true`、dir 除外、`.git/**` 無視、dedupe) | ✅ **`preDelete.commands`(失敗したら削除しない)** / `delete --force` / **`--keep-branch`** / `--current` / `--fzf` | ✅ **tmux 一級**(`--tmux` / `-v` / `-h`)/ `phantom edit`(`phantom.editor`)/ **`phantom ai`**(`phantom.ai`)/ env `PHANTOM`,`PHANTOM_NAME`,`PHANTOM_PATH` | ⚠ `list` / `--fzf` / `--names`。watch・ahead-behind なし | ⚠ `preDelete` に `docker compose down` を例示。**ポート割当なし** |
| 10 | **gwq** | `worktree.basedir`(`~/worktrees`)+ **`naming.template`**(`{{.Host}}/{{.Owner}}/{{.Repository}}/{{.Branch}}`)+ `sanitize_chars`。**レジストリ不要(FS 走査)** | ✅ **`setup_commands`**(Go template → `sh -c`。`{{.Branch}}`/`{{.Path}}` 等)。**🔒 v0.1.0 で trust prompt 必須化** | ✅ **`copy_files`**(glob)。symlink なし | ✅ `remove -f/-b/--force-delete-branch/--dry-run` / `prune`。マージ済み検出・ディスク使用量なし。削除前フックなし | ✅ **`gwq tmux run/attach/kill/list`** / shell 統合(`cd.launch_shell=false` で現シェルの cd)/ ghq×fzf 連携 | ✅ **業界最充実: `status --watch --filter changed --sort activity --json --csv`** | ❌ |
| 11 | **wtp** | `defaults.base_dir`(`../worktrees`)。`/` を保ってネスト | ✅ **`hooks.post_create` が型付きステップ配列: `copy` / `symlink` / `command`(`env` / `work_dir`)** | ✅ **copy(from は常に main worktree 基準)+ symlink 両方** | ✅ `remove --force` / **`--with-branch`(マージ済みのみ)/ `--force-branch`**。prune・ディスクなし。削除前フックなし | ✅ `wtp cd`(tab 補完、`@` で main)/ `wtp exec` / `add --exec` | ⚠ `list` は PATH / BRANCH / HEAD の 3 列のみ | ❌ |
| 12 | **wt**(raisedadead) | **bare レイアウト**(`.bare/` + 兄弟 dir)。`worktree_root` / `branch_template = "{{type}}/{{number}}-{{slug}}"` | ✅ **名前付きフック(`pre_create`/`post_add`/`post_clone`)+ 同梱 `zoxide`/`gh-default`/**`direnv`**/`github-issue`/`github-pr` + カスタム(`@events:` 宣言)。`hook_timeout` / `--no-hooks`** | ⚠ カスタムフックで `cp`(組み込みなし) | ✅ `delete` / **`prune --merged`(削除済みリモート・マージ済み branch)** / **`repair`** / `--dry-run` | ✅ **TUI**(info/diff/log タブ、単キー操作)/ shell wrapper で `switch` が cd / zoxide 連携 | ✅ `list`(status 付き)+ **全コマンド `--json`(統一エンベロープ)** | ❌ |
| 13 | **worktree-link / `wtl`** | —(既存 worktree に張るだけ) | —(symlink 専業) | ✅ **`.worktreelinks`**(gitignore 互換 glob)。**dir は丸ごと 1 本の symlink / 絶対パス / `.git` 常に除外 / force なしで上書きしない** | ✅ **`--unlink`(source を指す symlink だけ消す)** / `--dry-run` | ❌ | ❌ | ❌ |
| 14 | **git-worktree.nvim** | ❌(パスを渡す) | ⚠ `Hooks.register(Hooks.type.SWITCH/DELETE, fn)`。**builtins はデフォルト無登録** | ❌ | ⚠ `delete_worktree(path)` のみ | ✅ telescope 拡張 / `SWITCH` フックが `(path, prev_path)` | ❌ | ❌ |
| 15 | **twm** | —(`.git` / `.twm.yaml` を含む dir をワークスペース検出) | ⚠ layout + `TWM_TYPE` で分岐する共通スクリプト。**「worktree を window で開く」は意図的に非実装** | ❌ | ❌ | ✅ tmux セッション(`-e`/`-g`/`-G`/`-d`/`-N`)。env `TWM`,`TWM_ROOT`,`TWM_TYPE`,`TWM_NAME` | ❌ | ❌ |
| 16 | **sesh** | —(git repo / remote / dir 名からセッション名生成、`[[name_substitution]]` で変換) | ✅ `startup_command`(`[default_session]` / `[[session]]` / `[[wildcard]]`) | ❌ | ❌ | ✅ tmux セッション + **worktree ルートへジャンプ** / `alias` + `alias_auto_connect` / picker preview | ⚠ セッション一覧(git 状態は出さない)。`cache`(TTL 5s, SWR) | ❌ |
| 17 | **Uzi** | データは `~/.local/share/uzi`(worktree パスは未確認) | ⚠ **`devCommand` に install も全部含める**(copy/symlink をしない設計) | ❌(意図的) | ⚠ `uzi kill <name>` / `all` / `uzi reset`。prune・ディスクなし | ✅ **エージェントごとに tmux セッション**。`uzi run` / `broadcast` | ✅ **`uzi ls -w`(1秒更新): `AGENT`/`MODEL`/`STATUS`/`DIFF (+0/-0)`/**`ADDR (http://localhost:3003)`**/`PROMPT`** | ✅ **`portRange: 3000-3010` + `devCommand` の `$PORT`** |
| 18 | **claude-squad / `cs`** | ? | ❌ | ❌ | ⚠ `D` で kill。**`c` = commit してセッション一時停止 / `r` で再開** | ✅ **tmux セッション/エージェント**。`~/.claude-squad/config.json` の **`profiles`** | ✅ TUI(preview タブ + **diff タブ**) | ❌ |
| 19 | **Crystal → Nimbalyst** | ?(deprecated 2026-02) | ? | ? | ⚠ `ArchiveProgress` コンポーネントあり | ✅ セッションごとにターミナル | ✅ `ProjectDashboard` / `GitStatusIndicator` / **`MainBranchWarningDialog`** | ? |
| 20 | **vibe-kanban** | ?(sunsetting) | ? | ? | ✅ **孤児 + 期限切れ workspace の自動掃除**(`DISABLE_WORKTREE_CLEANUP` で無効化) | ✅ workspace = branch + ターミナル + dev server。**Remote SSH Host/User で `vscode://vscode-remote/ssh-remote+...`** | ✅ kanban ボード + diff レビュー(インラインコメント)+ **組み込みブラウザ(devtools/inspect/デバイス)** | ✅ `PORT` / **`BACKEND_PORT=0`(OS 自動割当)** / `FRONTEND_PORT` / `MCP_PORT` |
| 21 | **Sculptor**(Imbue) | コンテナ内に clone(**worktree ではない**) | ✅ **dev container spec** | ✅(コンテナイメージに含む) | ⚠ workspace 単位 | ✅ workspace ごとにターミナル + **Pairing Mode でローカルに同期** | ✅ workspace ごとに diff ビュー | ✅ **コンテナ隔離なので構造的に衝突しない** |
| 22 | **container-use / `cu`** | environment = **branch + コンテナ**(worktree ではない) | ✅ **`setup-command`(コードコピー前)/ `install-command`(コードコピー後)/ `env` / `secret`** を `.container-use/environment.json` に | ✅(コンテナイメージに含む) | ✅ `delete` / `prune` | ✅ `cu terminal`(**エージェントの端末に降りる**)/ `cu watch` | ✅ `list` / `log`(**実際に走ったコマンド全履歴**)/ `diff` / `inspect` | ✅ コンテナ隔離 |
| 23 | **pnpm global virtual store** | **bare repo + 兄弟 worktree**。`/`→`-` に変換 | ⚠ `pnpm worktree:new <branch\|PR番号>` ヘルパー | ✅ **`virtualStoreType: global` で node_modules がストアへの symlink のみ**(worktree 内に 1 バイトもコピーしない)+ **`.claude` を git common dir から symlink** | ⚠ `git worktree remove`(「leftover は安いが溜まる」) | ⚠ `shell/wt.sh` で cd | ❌ | ⚠ **信頼境界の警告あり**(相互不信のエージェントで書込可ストアを共有するな) |
| 24 | **Kagi(現状)** | `../<repo名>-worktrees/<safe_branch>`(モーダル入力、設定なし)。**必ず新規 or 既存 branch とセット** | ❌ | ❌ | ⚠ **standalone 削除なし**(branch 削除の副作用のみ)。`unlock` のみ(**lock なし**)。prune / repair / move / prunable 検出なし | ⚠ repo タブ(🌲 + レーン色)。**埋め込み端末/エディタの紐付けなし** | ✅ **⭐ 全 worktree の WIP をグラフに同時表示(業界唯一)**。staged/unstaged/untracked 件数。**ahead/behind・最終更新・ディスクなし** | ❌ |

---

## 3. Kagi 取り込み候補(優先順)

難易度: **S** = 数百行 / **M** = 1 機能分 / **L** = アーキ変更を伴う

| # | 提案 | 効果 | 難易度 | 依存 | 出典 |
|---|---|---|---|---|---|
| 1 | **`.worktreeinclude` を読んで gitignored ファイルをコピー** — リポジトリルートの `.worktreeinclude`(gitignore 構文)に一致 **かつ** gitignore されているファイルだけを新 worktree にコピー。plan に「コピーするファイル一覧」を列挙して confirm に出す | 実運用の最大の摩擦(`.env` が無い)が消える。**Claude Code / Conductor / VS Code のユーザーの既存ファイルがそのまま効く**ので設定コストゼロ。git 操作ではないので Kagi 自前実装で安全 | **S** | なし | [Claude Code](https://code.claude.com/docs/en/worktrees) / [Conductor](https://www.conductor.build/docs/reference/files-to-copy) / [VS Code `git.worktreeIncludeFiles`](https://github.com/microsoft/vscode-docs/blob/main/docs/sourcecontrol/branches-worktrees.md) |
| 2 | **standalone の worktree 削除**(`WorktreeAction::Remove`)— dirty なら blocker(`--force` は提供しない)、`--keep-branch` / `branch も削除` の二択、ODB blob backup + oplog、main worktree は常に不可 | 全 GUI/CLI が持つ基本操作が Kagi に無い。「worktree だけ消して branch を残す」が今できない。既存の discard 系 ODB backup 機構をそのまま流用できる | **S** | なし | [git remove](https://git-scm.com/docs/git-worktree) / [phantom `--keep-branch`](https://raw.githubusercontent.com/phantompane/phantom/main/docs/commands.md) / [wtp `--with-branch`](https://raw.githubusercontent.com/satococoa/wtp/main/README.md) / [JetBrains](https://www.jetbrains.com/help/idea/use-git-worktrees.html) |
| 3 | **`lock --reason` を追加して unlock との非対称を解消** — サイドバー右クリックに `Lock worktree…`(理由入力)。さらに **Kagi 自身が「埋め込みターミナルでプロセスが走っている worktree」に `kagi: <reason>` の lock を張る** | 現状 `WorktreeRecovery::Unlock` が「戻すなら CLI で」と案内している = 機能の穴を自認している。Claude Code が lock を「実行中の削除ガード」に使っている先例があり、Kagi の埋め込みターミナルと組めば実質的な安全機構になる | **S** | なし | [git lock](https://git-scm.com/docs/git-worktree) / [Claude Code のロック運用](https://code.claude.com/docs/en/worktrees) |
| 4 | **`prunable` / `locked` バッジ + 明示 `Prune` 操作** — `git worktree list --porcelain` の注記をサイドバー WORKTREES セクションのバッジに。孤児(ディレクトリが手で消された)worktree を検出して `Prune`(`--dry-run` で件数プレビュー → confirm) | 孤児 worktree は「気づけない」種類のゴミ。`--porcelain` から**タダで取れる**。JetBrains が `Locked`/`Prunable` だけをバッジにしているのは、これが実際に必要な最小集合だという証拠。Kagi は dirty 件数も出しているので上位互換になる | **S** | なし | [git `list --porcelain`](https://git-scm.com/docs/git-worktree) / [JetBrains Prune](https://www.jetbrains.com/help/idea/use-git-worktrees.html) |
| 5 | **`Repair` 操作** — main worktree / linked worktree の移動でリンクが切れた worktree を復旧。「main を動かしたなら main で repair、linked を動かしたならその中で repair、両方なら main で全パスを列挙」の 3 ケースを plan の説明文に落とす | git worktree で最も復旧が難しく、かつ CLI を知らないと詰む障害。**「安全性優先の Git GUI」が壊れた worktree を直せないのは説明がつかない**。Kagi の plan の説明力(recovery ブロック)が最も活きる操作 | **S** | #4(prunable 検出と同じデータ源) | [git repair](https://git-scm.com/docs/git-worktree) / [wt `repair`](https://raw.githubusercontent.com/raisedadead/wt/main/README.md) |
| 6 | **worktree 作成のデフォルトパスを設定化** — `settings.json` に `worktree.base_dir`(デフォルト現行の `../<repo>-worktrees`)+ `worktree.dir_separator`(`/` をディレクトリ名で `-` に平坦化するか)。`.git/kagi/worktrees/` も選択肢として提示 | 毎回モーダルにパスを打つのは全ツールが解決済みの摩擦。Zed は `git.worktree_directory` の 1 キー、phantom は `.git/phantom/worktrees/`(`.gitignore` を汚さない)。**`/` の平坦化は phantom が「branch 名は変えずディレクトリ名だけ」と明示している**通り、branch 名を壊さずに実現できる | **S** | なし | [Zed `git.worktree_directory`](https://zed.dev/docs/git) / [phantom `worktreesDirectory` / `directoryNameSeparator`](https://raw.githubusercontent.com/phantompane/phantom/main/docs/configuration.md) / [gwq `naming.template`](https://raw.githubusercontent.com/d-kuro/gwq/main/README.md) |
| 7 | **型付き post-create ステップ + trust prompt** — `.kagi/worktree.toml` の `[[post_create]]` に **`copy` / `symlink` / `command` の 3 型**を順序付きで。`copy`/`symlink` は Kagi 自前実装で無確認、**`command` だけを gwq 方式の trust 対象にする**((絶対パス, SHA-256) を `trusted` に記録、内容変更で再確認、表示時に制御バイトを `\xHH` エスケープ、headless では絶対に実行しない) | **機能表で Kagi が唯一目立って空白の欄**(全ツールが持つ)。型付きにすると **plan に「何が起きるか」を正確に列挙できる**(文字列コマンドの羅列では「シェルコマンドを 3 つ実行」しか書けない)。gwq v0.1.0 のセキュリティ修正が示す通り trust は省略不可で、**GUI の Kagi は confirm モーダルとして出せるので CLI より有利** | **M** | #1(copy の実体を共有) | [wtp 型付きフック](https://raw.githubusercontent.com/satococoa/wtp/main/README.md) / [🔒 gwq v0.1.0 trust](https://raw.githubusercontent.com/d-kuro/gwq/main/docs/release-notes/v0.1.0.md) / [Conductor `scripts.setup`](https://www.conductor.build/docs/reference/scripts) |
| 8 | **pre-remove ステップ(失敗したら削除しない)** — 同じ `.kagi/worktree.toml` の `[[pre_remove]]`。`docker compose down` / per-worktree DB スキーマの teardown を worktree 内で実行し、**失敗またはキャンセルで削除を中止** | 「worktree を消したらコンテナと DB が孤児になる」は現実の障害。**「失敗したら削除しない」は Kagi の preflight 思想と完全に一致**し、2 つの独立した実装(phantom `preDelete`、VSCode 拡張 `preRemoveCmd`)が同じ契約に到達している | **M** | #2, #7 | [phantom `preDelete.commands`](https://raw.githubusercontent.com/phantompane/phantom/main/docs/configuration.md) / [git-worktree-manager `preRemoveCmd`](https://raw.githubusercontent.com/jackiotyu/git-worktree-manager/main/README.md) / [Conductor `scripts.archive`](https://www.conductor.build/docs/reference/scripts) |
| 9 | **既存 Branch Cleanup ペインに worktree 列を足す** — ADR-0128 のテーブルに「この branch を pin している worktree」列 + ディスク使用量を追加し、`FullyMerged` の一括削除に **worktree ごと消すオプション**を付ける。**新しいペインを作らない** | ADR-0128 の分類(`FullyMerged`/`SquashMergedLikely`/`MergedThenGrown`/`stale`)と `WorktreeCheckout`(branch を pin している worktree の検出)が**両方すでにある**。並列の新実装を作るのは重複。`wt prune --merged` が同じことをやっている先例 | **M** | #2 | [wt `prune --merged`](https://raw.githubusercontent.com/raisedadead/wt/main/README.md) / [gwq `remove -b`](https://raw.githubusercontent.com/d-kuro/gwq/main/README.md) / 自 ADR-0128 |
| 10 | **WIP 行 / サイドバーに ahead-behind・最終コミット相対時刻・ディスク使用量を追加** — 既存の `WorktreeWip { staged, unstaged, untracked }` を `WorktreeState` に拡張。`$LAST_COMMIT` 相当の相対時刻(「3 weeks ago」)で stale を一目で分かるように | **ADR-0103 の「全 worktree の WIP を 1 グラフに同時表示」は業界唯一の柱**で、ここを厚くするのが最も差別化に効く。JetBrains は状態を出さず、gwq/Uzi は別画面のリスト。git-worktree-manager が `$LAST_COMMIT` を「stale を一目で見つけるため」と明示している。**ADR-0128 の教訓通り、収集はバックグラウンドタスクで**(snapshot をメインスレッドで重くしない) | **M** | なし。ADR-0128 追記の非同期スキャン方式を踏襲 | [git-worktree-manager `$LAST_COMMIT`](https://raw.githubusercontent.com/jackiotyu/git-worktree-manager/main/README.md) / [gwq `status --sort activity`](https://raw.githubusercontent.com/d-kuro/gwq/main/README.md) / 自 ADR-0103 / ADR-0128 |
| 11 | **埋め込みターミナルを worktree に紐づけ、環境変数を注入** — worktree タブごとにターミナルの cwd を固定し、`KAGI_WORKTREE_PATH` / `KAGI_WORKTREE_NAME` / `KAGI_MAIN_WORKTREE` / `KAGI_DEFAULT_BRANCH` を注入。post-create の `command` ステップも同じ変数を受ける | Kagi は**埋め込みターミナル(ADR-0008/0035)を既に持っている**ので、phantom/gwq/Uzi が tmux でやっていることを外部依存なしに実現できる。**変数名は Zed の 2 変数(`ZED_WORKTREE_ROOT`/`ZED_MAIN_GIT_WORKTREE`)で 8 割が書けるという実証**に合わせる | **M** | #7 | [Zed `create_worktree` 変数](https://zed.dev/docs/tasks) / [phantom `PHANTOM_*`](https://raw.githubusercontent.com/phantompane/phantom/main/docs/commands.md) / [Conductor `CONDUCTOR_*`](https://www.conductor.build/docs/reference/environment-variables) / [twm `TWM_*`](https://raw.githubusercontent.com/vinnymeller/twm/master/README.md) |
| 12 | **worktree ごとのポート払い出し** — `settings.json` に `worktree.port_range = "3000-3099"`、worktree ごとに連続 10 ポートを予約して `KAGI_PORT`..`KAGI_PORT+9` として注入。サイドバーに `http://localhost:<port>` を表示。共有リソースが 1 つしかないプロジェクト用に `worktree.run_mode = "nonconcurrent"` も | **「worktree で並列開発」の最後の実務的な壁**。Conductor(10 ポート予約)と Uzi(`portRange` + `$PORT`)が独立に同じ解に到達している。Kagi は埋め込みターミナルを持つので注入するだけ。**`nonconcurrent` は「並列できないプロジェクトを正しく諦める逃げ道」**として重要 | **M** | #11 | [Conductor `CONDUCTOR_PORT` / `run_mode`](https://www.conductor.build/docs/reference/scripts) / [Uzi `portRange`](https://raw.githubusercontent.com/devflowinc/uzi/main/README.md) / [vibe-kanban `BACKEND_PORT=0`](https://raw.githubusercontent.com/BloopAI/vibe-kanban/main/README.md) |
| 13 | **symlink ステップ(`.worktreelinks` も読む)** — `[[post_create]]` の `symlink` 型を実装。**ディレクトリは丸ごと 1 本の symlink / 絶対パスで張る / `.git` は常に除外 / 既存ファイルは絶対に上書きしない**。削除時は「source を指す symlink だけ」を外す | `.bin` / `.cache` / IDE 設定の共有に実需がある。worktree-link(Rust 製)の 4 つの安全仕様がそのまま使え、特に「force なしで上書きしない」は Kagi の破壊操作禁止原則と一致。**ただし `node_modules` の symlink 共有は推奨せず、pnpm `virtualStoreType: global` に案内する**(依存が乖離した瞬間に壊れる) | **M** | #7 | [worktree-link `.worktreelinks`](https://raw.githubusercontent.com/km-tr/worktree-link/main/README.md) / [wtp `type: symlink`](https://raw.githubusercontent.com/satococoa/wtp/main/README.md) / [pnpm global virtual store](https://pnpm.io/git-worktrees) |
| 14 | **PR から worktree を起こす** — 既存 PR 一覧の行アクションに「Create worktree from PR」。`refs/pull/<n>/head`(GitHub)を fetch して `<base>/pr-<n>` に作る。ホストで fetch パスを分岐(github → `pull/<n>/head`、gitlab → `merge-requests/<n>/head`) | Kagi は **PR 一覧・PR merge・PR conflict preview を既に持っている**のに、PR をローカルで動かす導線が checkout しかない。Claude Code / phantom / wt / pnpm の 4 つが独立に実装済み。fork の PR も `pull/<n>/head` なら取れる | **M** | なし | [Claude Code `--worktree "#1234"`](https://code.claude.com/docs/en/worktrees) / [phantom `github checkout`](https://raw.githubusercontent.com/phantompane/phantom/main/docs/commands.md) / [wt `--pr-review`](https://raw.githubusercontent.com/raisedadead/wt/main/README.md) / [pnpm `worktree:new <PR番号>`](https://pnpm.io/git-worktrees) |
| 15 | **`--detach` オプション(branch を後で決める)** — 作成モーダルに「branch を作らず detached HEAD で作る」を追加。作成後に既存の branch 作成/切り替え導線で branch を決める | ADR-0025 は「worktree は必ず新規 branch とセット」で git の制約を先回りしたが、**Zed は逆に detached HEAD をデフォルトにして同じ制約を回避している**。「とりあえず試す worktree」は git 公式が `add -d` を「throwaway worktree」として推奨する用途で、branch 名を先に決めさせるのは摩擦 | **S** | なし | [Zed の detached HEAD 方針](https://zed.dev/docs/git) / [git `add -d`(throwaway worktree)](https://git-scm.com/docs/git-worktree) |
| 16 | **cross-worktree diff** — WIP 行 / サイドバーから「この worktree の変更を diff で見る」。既存の split diff view を再利用 | Kagi は全 worktree の変更**件数**を見せているのに、**中身を見る導線が「切り替える」しかない**。VS Code の `Compare with Workspace` が同じ問題を解いている。ADR-0103 が回避した GitLens #5311 の「見えるが操作できない」罠を、もう一段深く解消することになる | **M** | なし(データは `collect_worktrees` にある) | [VS Code `Compare with Workspace`](https://github.com/microsoft/vscode-docs/blob/main/docs/sourcecontrol/branches-worktrees.md) |
| 17 | **`worktree.useRelativePaths` / `--relative-paths` 対応** — 作成時に相対パスリンクを選べるようにし、`repair` は絶対/相対の不一致も直す | リポジトリごとディレクトリを移動する運用(dotfiles 管理、外部ドライブ)で worktree が壊れなくなる。git 側の新しい機能で、GUI で露出しているツールは調査範囲に**存在しなかった** = 先行できる | **S** | #5 | [git `--relative-paths` / `worktree.useRelativePaths`](https://git-scm.com/docs/git-worktree) |
| 18 | **oplog を worktree でフィルタして見せる** — 各操作がどの worktree で起きたかを oplog に記録し、worktree ごとにフィルタ表示 | container-use が「エージェントが**主張した**ことではなく**実際に何をした**かのログ」を売りにしているのと、Kagi の oplog は同じ思想。**worktree = 並列作業の単位なら、履歴も worktree で切れる必要がある**。既存 oplog にフィールドを 1 つ足すだけ | **M** | なし | [container-use の「Real-time Visibility」](https://raw.githubusercontent.com/dagger/container-use/main/README.md) |
| 19 | **worktree の「アーカイブ」(復元可能な削除)** — 削除する代わりに、branch と未 commit 変更を ODB に退避してディレクトリだけ消し、oplog から復元できるようにする | **Zed が正確にこれをやっている**(「スレッドを History に移すと worktree の git 状態を保存して disk から消し、復元すると worktree も戻る」)。Kagi は既に discard 用の ODB blob backup と oplog undo を持っているので、**「削除しても戻せる」は Kagi が最も自然に提供できる価値** | **L** | #2, #18 | [Zed の worktree 保存/復元](https://zed.dev/docs/ai/parallel-agents) / [claude-squad の `c`(commit して一時停止)](https://raw.githubusercontent.com/smtg-ai/claude-squad/main/README.md) |
| 20 | **Favorites / ピン留め + `worktree.detect_limit`** — サイドバーで worktree をピン留め、一覧のスキャン上限を設定可能に | worktree が 20〜50 個になると一覧が使えなくなる。**VS Code の `git.detectWorktreesLimit` がデフォルト 50** = 業界の想定規模。git-worktree-manager が Favorites を主要機能に挙げている。sesh の「auto-connect はオプトイン、頻繁に飛ぶ数個だけ価値がある」という粒度の指針も効く | **S** | #10 | [git-worktree-manager Favorites](https://raw.githubusercontent.com/jackiotyu/git-worktree-manager/main/README.md) / [VS Code `git.detectWorktreesLimit`](https://github.com/microsoft/vscode-docs/blob/main/docs/sourcecontrol/branches-worktrees.md) / [sesh `alias_auto_connect`](https://raw.githubusercontent.com/joshmedeski/sesh/main/README.md) |
| 21 | **「Kagi が作った worktree」をメタデータで識別** — `$GIT_DIR/worktrees/<name>/` に Kagi のマーカーを書き、一括操作(#9 の cleanup、#4 の prune)の対象を**自分が作ったものだけ**に限定 | **Claude Code が v2.1.246 でまさにこの検査を追加した**(それ以前はユーザーが `git worktree add` した worktree を誤削除しうる)。Kagi が一括削除を提供するなら**同じ事故を先回りで防ぐ必要がある**。安全性優先の製品として、他人が作ったものに一括操作を掛けない | **S** | #4, #9 | [Claude Code のマーカー方式(v2.1.246)](https://code.claude.com/docs/en/worktrees) |
| 22 | **`.claude` / `.cursor` などエージェント設定の worktree 間共有** — post-create の `symlink` ステップの既定候補として提示(または `.worktreeinclude` の推奨例に含める) | **pnpm 自身のリポジトリが `.claude` を git common dir から symlink して「全 worktree が同じ Claude Code 設定と承認済みコマンドを共有する」運用にしている**。Claude Code 側も「権限承認は main checkout の `.claude/settings.local.json` に保存され worktree 削除後も残る」(v2.1.211+)と設計を揃えた。Kagi の AI native 化に直結する具体パターン | **S** | #13 | [pnpm の `.claude` symlink](https://pnpm.io/git-worktrees) / [Claude Code の共有モデル](https://code.claude.com/docs/en/worktrees) / [wtp の `copy from: ".claude"`](https://raw.githubusercontent.com/satococoa/wtp/main/README.md) |

---

## 4. 取り込まないと判断したもの(理由付き)

| 対象 | 判断 | 理由 |
|---|---|---|
| **コンテナ / dev container による隔離**(Sculptor、container-use、devcontainer per worktree) | 取り込まない | Kagi は Git クライアント。Docker / Dagger への依存はインストール要件を根本的に変え、macOS/Linux 両対応のリリース(ADR-0047)を壊す。**Sculptor 自身が「worktree ベースのツールと違い」と言っている通りこれは別カテゴリの製品**。Kagi は「git の中の安全性」に集中し、プロセス隔離は明示的に非目標とすべき(Conductor も pnpm も「worktree 隔離はセキュリティ境界ではない」と明記しており、Kagi も同じ立場を文書化する)。[Sculptor](https://imbue.com/product/sculptor) / [container-use](https://raw.githubusercontent.com/dagger/container-use/main/README.md) / [Conductor](https://www.conductor.build/docs/concepts/git-worktrees) |
| **tmux / zellij セッション管理**(phantom `--tmux`、gwq `tmux run`、twm、sesh、claude-squad、Uzi) | 取り込まない | **Kagi は埋め込みターミナル(ADR-0008/0035)を既に持っている**ので、外部マルチプレクサを駆動するのは二重実装。**twm の作者が「worktree branch を window で開く機能は入れない。layout + スクリプトの方が常に柔軟だ」と明示的に拒否している**のがそのまま Kagi にも当てはまる。代わりに候補 #11(自分のターミナルタブを worktree に紐づける)をやる。[twm の Feature Philosophy](https://raw.githubusercontent.com/vinnymeller/twm/master/README.md) |
| **エージェント並列実行のオーケストレーション本体**(Uzi `prompt --agents claude:3,codex:2`、claude-squad、Conductor のエージェント実行、vibe-kanban のボード) | 取り込まない | Kagi は Git クライアントで、エージェントのプロセス管理・プロンプト配送・モデル選択は範囲外。**star 3113 の Crystal が 1 年足らずで deprecated、vibe-kanban も sunsetting** という事実が、この層の製品寿命の短さを示している。Kagi は「エージェントが作った worktree を安全に見て・レビューして・掃除する」側に立つ方が持続する。[Crystal deprecation](https://github.com/stravu/crystal) / [vibe-kanban shutdown](https://www.vibekanban.com/blog/shutdown) |
| **確認プロンプトの自動承認**(Uzi `uzi auto`、claude-squad `-y/--autoyes`、Claude Code の `bypassPermissions`) | **絶対に取り込まない** | Kagi の二段確認と `plan → confirm → preflight → execute → verify → oplog` は製品の存在理由。自動承認はそれを構造的に無効化する。**この機能を持たないことが Kagi の価値**。[Uzi `auto`](https://raw.githubusercontent.com/devflowinc/uzi/main/README.md) |
| **孤児 worktree の自動バックグラウンド削除**(Claude Code の `cleanupPeriodDays` スイープ、vibe-kanban の orphan/expired 自動掃除) | 取り込まない | ADR-0128 が branch について既に「自動掃除(起動時のバックグラウンド削除など)は非目標。列挙と手動削除のみ」と決めている。**worktree も同じ線を引くべき**で、これは一貫性の問題。**vibe-kanban が `DISABLE_WORKTREE_CLEANUP` という off スイッチを持っている事実は、自動削除が実際に困る場面があることの実証**。代わりに候補 #4(prunable を**見せる**)+ #9(**手動**一括削除)。[vibe-kanban](https://raw.githubusercontent.com/BloopAI/vibe-kanban/main/README.md) / 自 ADR-0128 |
| **`node_modules` の symlink 共有を推奨機能として出すこと** | 取り込まない(symlink 機構自体は #13 で入れるが、`node_modules` は推奨しない) | 「両 branch の依存が完全に一致するときだけ動く。乖離すると一方の worktree が誤ったバージョンを持ち、デバッグの難しい問題になる」。**Kagi が「安全」と銘打って壊れる設定を推奨してはいけない**。代わりに pnpm `virtualStoreType: global` を案内する(worktree 内に 1 バイトもコピーせず、かつ各 worktree が独立した依存ツリーを持てる正解)。**VS Code は公式に `node_modules/**` のコピーを推奨しているが、これは symlink ではなくコピーなので壊れない**(ただし遅い・stale が入る。Conductor はまさにその理由で非推奨としている)。[gitworktree.org](https://www.gitworktree.org/guides/node-modules) / [pnpm](https://pnpm.io/git-worktrees) / [Conductor の非推奨理由](https://www.conductor.build/docs/reference/files-to-copy) |
| **bare リポジトリレイアウトの強制**(wt `.bare/` + 兄弟 dir、pnpm の推奨構成) | 取り込まない(**対応はする、強制はしない**) | bare レイアウトは「main も worktree の 1 つ」という綺麗なモデルだが、既存リポジトリの再 clone を要求する。**Zed は bare レイアウトで worktree ピッカーが列挙できないバグを抱えており(#54553)、GUI で bare を正しく扱うのは実際に難しい**。Kagi は `repo.is_worktree()` / `commondir()` ベースで既に bare 配下の worktree を開ける想定なので、**「bare でも壊れない」ことをテストで担保するだけ**にとどめる。[wt](https://raw.githubusercontent.com/raisedadead/wt/main/README.md) / [Zed #54553](https://github.com/zed-industries/zed/discussions/54553) |
| **`--force` 付きの worktree 削除 / `--force --force`(locked を無視)** | 取り込まない | `git worktree remove -f` は未 commit 変更を消し、`-f -f` は lock という明示的な保護を無視する。**`push --force` / `reset --hard` / `git clean` を持たない製品が、worktree 削除で同じことをするのは矛盾**。dirty / locked は blocker のままにし、「まず commit するか discard せよ」「まず unlock せよ」と案内する(branch 削除の既存 `DeleteBranchInDirtyWorktree` / `DeleteBranchInLockedWorktree` と同じ扱い)。[git remove](https://git-scm.com/docs/git-worktree) |
| **`Migrate Worktree Changes`(他 worktree の変更を自分側にマージ)** | 今回は取り込まない(候補 #16 の cross-worktree **diff** までにとどめる) | VS Code の唯一無二の機能で価値は高いが、**「他の worktree の未 commit 変更を自分の index に適用する」は Kagi のどの既存操作にも当てはまらない新しい危険クラス**。plan で何が起きるか正確に示すには worktree 間の 3-way が必要で、ADR-0147 の worktree digest を**両側**で取る設計になる(片側だけ digest を持つ現在の前提を壊す)。まず diff(読み取り専用)で導線を作り、需要が確認できてから設計する。難易度 L。[VS Code](https://github.com/microsoft/vscode-docs/blob/main/docs/sourcecontrol/branches-worktrees.md) |
| **`--orphan` worktree** | 取り込まない | git 2.48 で入った「空の index + unborn branch」。`gh-pages` の初期化のようなニッチ用途で、Kagi のコミットグラフ中心 UI では表現が難しい(コミットが 0 個の worktree)。要望が出るまで保留。[git `--orphan`](https://git-scm.com/docs/git-worktree) |
| **`extensions.worktreeConfig` / per-worktree git 設定 UI** | 取り込まない | 「古い git はこの拡張を持つリポジトリへのアクセスを拒否する」という互換性の断崖があり、かつ `core.worktree` を誤って共有すると壊れる。GUI で露出させると事故を招く。**Kagi 内部で必要になったときだけ使う**。[git CONFIGURATION FILE 節](https://git-scm.com/docs/git-worktree) |

---

## 5. 未解決の疑問

1. **配置規約: `.git/kagi/worktrees/` vs `../<repo>-worktrees/` のどちらを既定にするか。** phantom は `.git/phantom/worktrees/`(`.gitignore` を汚さない、リポジトリ外に散らからない)、Zed / wtp は `../worktrees`、gwq は `~/worktrees/<host>/<owner>/<repo>/<branch>`、Conductor は `~/conductor/workspaces/<repo>/<workspace>`、Claude Code は `.claude/worktrees/`。**`.git` 内に置くと `du` での可視化(候補 #10 のディスク使用量)と外部ツールからの発見性が落ちる**が、Kagi のサイドバーが唯一の入口なら問題にならない可能性がある。実測が必要。
2. **GitLens の worktree 設定キー(`gitlens.worktrees.*`)の実体が未確認。** 公式ヘルプが `/gitlens/gitlens-settings/#worktrees-view-settings` にリンクしているが当該アンカーの内容を読めていない。「Create Worktree for Pull Request」機能の有無も未確認。`vscode-gitlens` の `package.json` を読めば確定する。
3. **VS Code の worktree 削除操作が公式ドキュメントに無い。** Worktrees サブメニューにあると推測されるが記載がない。`git.worktreeIncludeFiles` で `node_modules/**` をコピーした worktree を削除するとき、そのコピーがどう扱われるか(`git worktree remove` は untracked を理由に拒否するはず)も未確認。
4. **Kagi の `collect_worktrees` のコスト。** `docs/performance-review.md:182-186` が「worktree ごとに 2 回 `Repository::open` + 1 回 full status」を MEDIUM 課題として挙げている。候補 #10(ahead-behind + ディスク使用量)を足すとここが悪化する。**ADR-0128 の追記(同期スキャンが「stash が異様に遅い」の実体だった)と同じ罠に入る**ので、収集の非同期化が #10 の前提条件になるかを実測で確定する必要がある。
5. **worktree ごとの oplog スコープ(候補 #18)は「操作が起きた worktree」で切るのか「操作が影響した worktree」で切るのか。** branch 削除は複数 worktree に影響しうる(pin していた worktree を消す)。oplog v2 フォーマット(ADR-0074)にフィールドを足す際の設計判断が未決。
6. **Conductor の `scripts.run` を Kagi の埋め込みターミナルで再現する際のプロセス管理契約。** Conductor は「`SIGHUP` → 200ms → `SIGKILL`」「`&` でのバックグラウンド化を避け `concurrently` を使え(でないとバックグラウンドプロセスがポートを掴んだまま残る)」と明記している。Kagi の vendored gpui-terminal(ADR-0035)がプロセスグループをどう扱うか未調査で、候補 #12(ポート払い出し)の実効性がここに依存する。
7. **`.worktreeinclude` の `**/` パターンの癖をどう扱うか。** Claude Code は「`**/` パターンが丸ごと ignore されたディレクトリの中に届かない」既知の挙動を持ち、v2.1.239 で変更している。Kagi が同じファイルを読むなら**どちらの挙動に合わせるべきか**(互換性 vs 直感)が未決。
8. **Nimbalyst / Terragon / omux の worktree 実装詳細が未確認。** Nimbalyst は「Git worktree isolation for safer parallel AI coding sessions」を機能に挙げているが `docs.nimbalyst.com` の詳細を読めていない。**Terragon** は公式サイトを確認できなかった。**omux** は GitHub 上で正体を特定できなかった(worktree + tmux + agent 系のはずだが、同名プロジェクトが複数ある可能性)。この 3 つは本調査では**特定できず**。
9. **`worktree.useRelativePaths` の導入バージョンが未確定。** git 2.54.0 のドキュメントに `--relative-paths` / `worktree.useRelativePaths` が載っていることは確認したが、初出バージョンは未確認(git-scm の変更履歴では 2.48.0 に大きな変更マークがあるので**2.48 が候補**だが `[推測]`)。Kagi が最低要求 git バージョンを上げる判断に必要。
10. **JetBrains の worktree 機能は 2026.2 が初出か。** ヘルプページは 2026.2 のもので、YouTrack の `IDEA-143404`(IJPL-112226)が長年の要望チケット、`IDEA-386301`(Native Git Worktree Management: UI, Visual Indicators, and Shared Indexing)が 5 ヶ月前の起票。**2026.1 以前にどこまであったかは未確認**。特に `IDEA-386301` の「Shared Indexing」(worktree 間でインデックスを共有する)は Kagi には無い観点だが、Kagi にインデックスの概念が無いので影響なし。
