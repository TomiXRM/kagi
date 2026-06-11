# アーキテクチャ設計

## 1. 全体構成(レイヤー)

```
┌─────────────────────────────────────────────────┐
│  UI Layer (GPUI)                                 │
│  Workspace / GraphView / DiffView / Panels       │
└───────────────▲─────────────────────────────────┘
                │ AppState (gpui Entity) の購読 / Action 発行
┌───────────────┴─────────────────────────────────┐
│  App Layer                                       │
│  AppState, OperationController(plan→confirm→    │
│  execute→verify), OperationLog                   │
└───────────────▲─────────────────────────────────┘
                │ RepoSnapshot / OperationPlan / OperationResult
┌───────────────┴─────────────────────────────────┐
│  Domain Layer (pure Rust, UI/Git 非依存)          │
│  git_model: Commit, Branch, Head, Status, ...    │
│  graph_layout: lane 割り当て・edge 計算            │
│  plan: OperationPlan の生成・検証ロジック           │
└───────────────▲─────────────────────────────────┘
                │ trait GitBackend
┌───────────────┴─────────────────────────────────┐
│  Git Backend Layer                               │
│  git2 (libgit2) 実装。読み取りと安全な書き込みのみ。  │
│  バックグラウンドスレッドで実行し、結果を UI に送る。  │
└─────────────────────────────────────────────────┘
```

設計原則:

- **Domain Layer は pure Rust**。gpui にも git2 にも依存しない。unit test はここに集中させる(特に `graph_layout`)。
- **GitBackend は trait** にして UI/App 層から実装を隠す。テストでは fixture repo + 実 backend、または mock を使う。
- **Git I/O は必ずバックグラウンド**(gpui の `background_executor`)。UI スレッドで libgit2 を呼ばない。
- **読み取りはスナップショット方式**。repo を開く / 操作完了 / 外部変更検知のたびに `RepoSnapshot`(commits + refs + status + stashes)を丸ごと取り直し、UI は常にスナップショットを描画する。差分更新は性能問題が出てから。

## 2. 画面構成(GPUI)

```
┌──────────────────────────────────────────────────────────┐
│ Toolbar: repo名 / 現在branch / ahead↑behind↓ / Open Repo   │
├────────────┬─────────────────────────────┬───────────────┤
│ Sidebar    │ Graph Panel                 │ Detail Panel  │
│            │                             │               │
│ LOCAL      │  ●─┐  message   author date │ [Commit選択時] │
│  main ✓    │  │ ●  message   ...         │  metadata     │
│  feat/x    │  ●─┘  message   ...         │  changed files│
│ REMOTE     │  │                          │  → file diff  │
│  origin/.. │  (lane描画 + 仮想化リスト)     │               │
│ TAGS       │                             │ [WT行選択時]   │
│ STASHES    │  先頭行: Working Tree 状態    │  status一覧    │
├────────────┴─────────────────────────────┴───────────────┤
│ Status Bar: 直近操作ログ / バックグラウンド処理 / エラー      │
└──────────────────────────────────────────────────────────┘
       + Operation Plan Modal(全 Git 操作の実行前に表示)
```

- **Sidebar**: local branches(HEAD マーク付き)/ remote branches / tags / stashes。クリックで graph 上の該当 commit へスクロール。コンテキストメニューから checkout / create branch 等。
- **Graph Panel**: 中心となるビュー。左に lane 描画(canvas 的描画)、右に message / author / date。行は仮想化。最上部に working tree の状態行(uncommitted changes)を出す。
- **Detail Panel**: commit 選択時は metadata + changed files、file 選択で diff 表示に切り替え。
- **Operation Plan Modal**: 操作内容・影響(現在状態 → 実行後の予測)・リスク・キャンセル/実行ボタン。**すべての書き込み系操作はここを必ず通る。**

## 3. データモデル(Domain Layer)

```rust
// ===== 識別子 =====
struct CommitId(/* 40-hex SHA。内部は [u8;20] or String */);

// ===== Git オブジェクト =====
struct Commit {
    id: CommitId,
    parents: Vec<CommitId>,        // [0] が first parent
    author: Signature,             // name, email, time
    committer: Signature,
    summary: String,               // message 1行目
    message: String,               // full message
}

struct Branch {                    // local branch
    name: String,                  // "main"
    target: CommitId,
    upstream: Option<UpstreamInfo>,
}

struct UpstreamInfo {
    remote_branch: String,         // "origin/main"
    ahead: usize,
    behind: usize,
}

struct RemoteBranch {
    remote: String,                // "origin"
    name: String,                  // "main"
    target: CommitId,
}

struct Tag {
    name: String,
    target: CommitId,              // annotated tag は peel して commit を指す
}

enum Head {
    Attached { branch: String, target: CommitId },
    Detached { target: CommitId },
    Unborn { branch: String },     // 初期 commit 前
}

// ===== Working Tree =====
struct WorkingTreeStatus {
    staged: Vec<FileStatus>,
    unstaged: Vec<FileStatus>,
    untracked: Vec<PathBuf>,
    conflicted: Vec<PathBuf>,
}

struct FileStatus {
    path: PathBuf,
    change: ChangeKind,            // Added | Modified | Deleted | Renamed{from} | TypeChange
}

struct Stash {
    index: usize,                  // stash@{N}
    message: String,
    target: CommitId,
}

// ===== Diff =====
struct Diff { files: Vec<FileDiff> }
struct FileDiff {
    old_path: Option<PathBuf>,
    new_path: Option<PathBuf>,
    change: ChangeKind,
    hunks: Vec<Hunk>,
    is_binary: bool,
}
struct Hunk {
    old_range: (u32, u32),         // start, count
    new_range: (u32, u32),
    lines: Vec<DiffLine>,          // Context | Added | Removed
}

// ===== Conflict(v0.2 の merge で使用。モデルだけ先に定義)=====
struct Conflict {
    path: PathBuf,
    ancestor: Option<CommitId>,    // 各 stage の blob 由来
    ours: Option<CommitId>,
    theirs: Option<CommitId>,
}

// ===== スナップショット(Backend → App の受け渡し単位)=====
struct RepoSnapshot {
    head: Head,
    commits: Vec<Commit>,          // topo order (最新が先頭)
    branches: Vec<Branch>,
    remote_branches: Vec<RemoteBranch>,
    tags: Vec<Tag>,
    status: WorkingTreeStatus,
    stashes: Vec<Stash>,
}

// ===== Graph Layout(pure Rust モジュールの出力)=====
struct GraphLayout {
    rows: Vec<GraphRow>,
    lane_count: usize,
}
struct GraphRow {
    commit: CommitId,
    lane: usize,                   // この commit の ● が乗る lane
    edges: Vec<GraphEdge>,         // この行を通過・分岐・合流する辺
    refs: Vec<RefBadge>,           // HEAD / branch / remote / tag
}
struct GraphEdge {
    from_lane: usize,              // 行の上端での lane
    to_lane: usize,                // 行の下端での lane
    kind: EdgeKind,                // Pass | ToCommit | FromCommit(merge)
    color_hint: usize,             // lane 由来の色番号
}
```

## 4. コミットグラフ描画アルゴリズム

入力: `Vec<Commit>`(topo order, 最新→古い)。出力: `GraphLayout`。

1. **DAG 取得**: backend が revwalk(`TOPOLOGICAL | TIME` ソート)で全 ref から到達可能な commit を列挙。MVP は上限 N 件(例: 10,000)で打ち切り、"load more" は later。
2. **topo 順に上から1行ずつ処理**。`active_lanes: Vec<Option<CommitId>>`(各 lane が「次に待っている commit」)を保持する。
3. **lane 割り当て**(commit C を処理するとき):
   - C を待っている lane が複数あれば**最左**を C の lane にし、残りは C へ合流する edge(branch の合流=分岐点)として閉じる or 詰める。
   - C を待つ lane がなければ(branch tip)、空き lane の最左に新規割り当て。
4. **parent の配置**:
   - first parent は C の lane を引き継ぐ(`active_lanes[lane] = parent0`)。
   - 2nd 以降の parent(merge 元)は、既に待ち lane があればそこへ edge を引き、なければ新規 lane を右側に確保(merge edge)。
5. **edge 生成**: 各行で「上端 lane → 下端 lane」の組を `GraphEdge` として記録。描画は行単位で完結する(行をまたぐ状態は active_lanes が持つ)ため、仮想化リストと相性が良い。
6. **ref バッジ**: branches / remote_branches / tags / HEAD を `CommitId → Vec<RefBadge>` の map にして各行に付与。
7. **色**: lane index ベースのパレット循環。branch 同一性の追跡(色の安定化)は later。

性能方針: 全行の layout は O(commits × active_lanes) で事前計算して保持(10k commits なら十分軽い)。描画は可視行のみ。

参考実装として gitk / sourcetree 系の lane アルゴリズム、`git log --graph` の挙動を仕様の参照点にする。

## 5. 安全な Git 操作レイヤー

すべての書き込み操作は `OperationController` の固定パイプラインを通る:

```
request → plan → (UI で確認) → preflight → execute → verify → log
                       │
                       └ キャンセル → 何もしない
```

```rust
trait GitBackend: Send + Sync {
    // 読み取り
    fn snapshot(&self) -> Result<RepoSnapshot>;
    fn diff_commit(&self, id: &CommitId) -> Result<Diff>;
    fn diff_workdir(&self) -> Result<Diff>;
    // 書き込み(MVP)
    fn checkout_branch(&self, name: &str) -> Result<()>;
    fn create_branch(&self, name: &str, at: &CommitId) -> Result<()>;
    fn stash_push(&self, message: Option<&str>) -> Result<()>;
    fn stash_apply(&self, index: usize) -> Result<()>;
    fn cherry_pick_preview(&self, id: &CommitId) -> Result<CherryPickPreview>; // in-memory merge
    fn cherry_pick(&self, id: &CommitId) -> Result<()>;
}

struct OperationPlan {
    title: String,                 // "Checkout branch 'feat/x'"
    current: StateSummary,         // HEAD, branch, dirty file 数
    predicted: StateSummary,       // 実行後の予測
    warnings: Vec<Warning>,        // 例: "uncommitted changes が 3 files あります"
    blockers: Vec<Blocker>,        // 例: "conflict が予測されるため実行できません"
    recovery: String,              // 失敗時の復旧手順(事前に提示)
}
```

ルール:

- **plan なしの書き込み API を UI に公開しない**(型レベルで強制: UI が触れるのは `OperationController::request(op)` のみ)。
- **preflight**: 実行直前に snapshot を取り直し、plan 時点と repo 状態が変わっていたら中断して再 plan。
- **verify**: 実行後に snapshot を取り、予測と照合。予測外の状態なら警告 + 復旧手順表示。
- **operation log**: 各操作の前後 snapshot 要約(HEAD / branch tips / status 件数)を `~/.local-app-dir/logs/` に追記。
- **cherry-pick preview**: libgit2 の in-memory merge(`merge_commits` / `cherrypick_commit` 相当)で working tree に触れず conflict 有無と変更ファイルを予測する。
- **危険操作(reset --hard / clean / force push)**: MVP では実装自体を持たない。将来入れる場合も backup ref 自動作成 + 二重確認を必須とする(ADR-0004)。

## 6. 並行性・スレッドモデル

- UI: gpui の foreground。AppState(`Entity<AppState>`)が単一の信頼できる状態。
- Git I/O: `cx.background_spawn` で実行。`git2::Repository` は `Send` だが `Sync` ではないため、**repo は専用ワーカースレッドに置き、channel で命令を送る**(`GitWorker`)。結果は foreground に戻して AppState を更新。
- 外部変更検知(他プロセスの git 操作): MVP は「操作完了時 + 手動 refresh」のみ。file watcher は v0.2。

## 7. テスト戦略

- `graph_layout`: pure unit test(直線 / 分岐 / merge / octopus / 複数 root のケース)。
- `GitBackend`: tempdir に fixture repo をスクリプト生成して結合テスト。**fixture 以外の repo に書き込むテストは禁止。**
- plan / preflight / verify: mock backend で状態遷移をテスト。
- UI: MVP では手動確認(fixture repo を開いて目視)。
