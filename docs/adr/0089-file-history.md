# ADR-0089: File History(1ファイルの変更履歴ビュー)

- Status: Accepted(2026-06-17、ユーザー依頼 `docs/file-history-task.md`)
- Date: 2026-06-17
- 関連: 既存の diff viewer(`render_main_diff_view` / `FileDiff`)、commit inspector、CLI ヘルパ `cli::run_git`

## Context

選択した1ファイルについて「どの commit で変更されたか」を一覧し、各 commit 時点のそのファイル単体の diff・commit 情報・rename 履歴を追えるようにする。リポジトリ全体の履歴(commit graph)とは別物で、1ファイルに特化した閲覧機能。モーダルにはせず、中央メイン領域を専用ビューに切り替える。詳細要件は `docs/file-history-task.md`。

設計上の要は2つ。**rename 追跡**(`git log --follow` 相当)と、**既存 diff viewer の再利用**(diff renderer を二重に作らない)。

## Decision

### レイヤ分担

- **履歴リストの収集だけ CLI**(`cli::run_git`)で行う。rename-follow を libgit2 で正確に再現するのは割に合わないため、`git log --follow` に委ねる。新モジュール `src/git/history.rs`。
- **各 entry の diff は既存の構造化 diff を再利用**する。commit entry は `Backend::commit_file_diff(commit_id, path_at_commit)`、WIP entry は `Backend::unstaged_file_diff` / `staged_file_diff`。いずれも `FileDiff` を返すので、UI は `FileDiffView::from_file_diff` → `MainDiffView` → `render_main_diff_view` の既存経路をそのまま使う。**CLI でパッチ文字列を作って独自描画はしない**。これにより binary / added / deleted も既存 viewer の挙動(Binary 行など)に乗る。
  - 帰結: タスク仕様の `FileHistoryDiff{title, patch}` 型は作らない。diff は `FileDiff` で表現する。

### データモデル(`src/git/history.rs`、純データ・git2 非依存)

```rust
pub struct FileHistoryRequest { pub repo_dir: PathBuf, pub file_path: PathBuf, pub follow_renames: bool, pub include_wip: bool, pub limit: usize }
pub struct FileHistory { pub current_path: PathBuf, pub entries: Vec<FileHistoryEntry> }
pub struct FileHistoryEntry { pub kind: FileHistoryEntryKind, pub commit: Option<CommitSummary>, pub change: FileChangeSummary }
pub enum FileHistoryEntryKind { Wip, Commit }
pub struct CommitSummary { pub full_hash, short_hash, subject: String, pub body: Option<String>, pub author_name, author_email, author_date, committer_name, committer_date: String }
pub struct FileChangeSummary { pub change_type: FileChangeType, pub path_before: Option<PathBuf>, pub path_after: PathBuf, pub insertions: Option<u32>, pub deletions: Option<u32>, pub is_binary: bool }
pub enum FileChangeType { Added, Modified, Deleted, Renamed, Copied, Unknown }
```

WIP entry は working tree / index / untracked から合成し、`kind = Wip`、commit = None。

### 収集の実装(history.rs)

- 1コマンドで型と件数を取る: `git log --follow --find-renames --date=iso-strict --format=<RS区切り> --name-status --numstat -- <path>`。
  - レコード区切りは `%x1e`、フィールド区切りは `%x1f`(コードベース未使用を確認済み)。各 commit ブロックの後に name-status 行(`A`/`M`/`D`/`R100 old new`)と numstat 行(`ins<TAB>del<TAB>path`)が続く。対象パス分を取り出して `FileChangeSummary` を作る。numstat の `-` は binary。
- WIP は `git status --porcelain=v1 -- <path>` と `working_tree_status` で判定(staged / unstaged / untracked)。`include_wip` かつ変更ありのとき先頭に1件足す。
- **path は必ず arg vector で渡す**(`run_git(dir, &[..., "--", path_str])`)。shell 文字列結合をしない。空白・日本語・記号で壊れないこと。`--` でパスとオプションを分離。
- `run_git` の非0 status は握り潰さず `GitError` にして UI まで上げる。

`Backend::file_history(&FileHistoryRequest) -> Result<FileHistory, GitError>` を生やし、`src/git/mod.rs` で `pub use`。

### UI(中央メイン領域の新モード)

- `KagiApp.file_history: Option<FileHistoryState>` を追加。`render_body` の center 分岐(loading → main_diff → commit_list)に **file_history を最優先**で差し込む。左サイドバーと下部 terminal はそのまま。
- `FileHistoryState { rel_path, branch, history: Option<FileHistory>(None=loading), error: Option<String>, selected: usize, diff: Option<MainDiffView>, generation: u64, split: f32 }`。
- 構成: 上に Header、中央上に commit list、中央下に diff viewer(`render_main_diff_view` 再利用)、右に専用 detail pane。list / diff の境界は `DividerKind::FileHistoryRows` で縦リサイズ。
- 導線(メニュー名 `Show File History`):
  1. commit panel の file 行(`render_file_menu_overlay` に追記、既存「Discard changes…」の隣)。
  2. inspector の changed files 行(右クリックメニューを追加)。
  3. diff header(`render_main_diff_view` のパスヘッダ付近に History ボタン)。
- 開く: `open_file_history(rel_path, origin_commit: Option<CommitId>)` → state を loading にし、`cx.background_spawn` で `Backend::file_history` を実行。完了で history を入れ、初期選択を決定(WIP があれば WIP、なければ origin_commit、無ければ最新)。選択 entry の diff は `Backend::commit_file_diff` / `*_file_diff` で取得し `MainDiffView` 化(単一ファイル diff は軽いので選択時に同期取得、既存 `open_main_diff*` と同様)。
- 選択挙動: row click → 下の diff 更新 + 右 detail 更新。右クリックで Copy Commit Hash / Copy File Path at This Commit / Open Commit / Show Commit in Graph(`jump_to_commit` 再利用)。double click → `jump_to_commit`。
- 状態: Loading / Empty(`No history found`)/ Untracked(`This file is untracked. No commit history yet.`)/ Error(詳細 + Retry)。
- Back(`close_file_history`)で `file_history = None` に戻し、commit graph 表示へ。
- 危険操作(Restore / Checkout this version)は v1 では入れない。

### 更新と stale

- `reload()` で `file_history` の history を破棄せず、`generation` を見て stale 扱い。`reload_external`(WatchEvent::Git)時に開いていれば再取得する。working-tree 変更(WatchEvent::WorkTree)時は WIP entry のみ軽く更新できると望ましいが、v1 は再取得で可。
- Git 操作(履歴取得)は UI スレッドをブロックしない(`background_spawn`)。read-only なので `busy_op` ゲートは不要。

## Consequences

- diff renderer は1つだけ。File History は履歴リスト収集に CLI を足すだけで、表示は既存資産に乗る。
- CLI は履歴リストのみ。diff は libgit2 経由のままで、binary/added/deleted/rename も既存挙動で扱える。
- v1 非スコープ: line blame、per-line history、履歴グラフ、複数ファイル同時、restore/checkout、commit 間比較 UI、semantic diff、binary preview。
- CI gate(src/ui は git2 直叩き禁止)は維持。UI は `kagi::git::Backend` 経由のみ。
- テスト: history.rs のパーサを fixture(add/modify/rename/delete/binary を1ファイルに対して持つ repo)で検証。
