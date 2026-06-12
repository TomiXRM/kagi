# W2-SIDEBAR: Repository Navigator 化(worktree レーン)

- Status: in-progress / 依存: ADR-0014
- 原文要件: requirements-gk-parity.md(要件2)

## 手順(競合対策のため厳守)

1. **最初に** `render_sidebar` 一式を `src/ui/sidebar.rs` に抽出(mod.rs には `pub mod sidebar;` と呼び出しのみ残す。挙動不変でビルド確認)
2. その後 sidebar.rs 内で Navigator 化:
   - セクション: LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES(snapshot に全データあり: KagiApp に remote_branches/tags を保持していなければ from_snapshot で追加保持)
   - 各セクション: ヘッダに件数 `LOCAL BRANCHES (24)` + クリックで折りたたみ(▸/▾)。collapsed 状態は `KagiApp.sidebar_collapsed: HashSet<&'static str>` 等(reload で維持)
   - filter 入力(先頭固定): gpui-component Input(commit message と同じ要領で lazy 生成、`KagiApp.sidebar_filter: Option<Entity<InputState>>`)。入力値で全セクションを部分一致絞り込み(大文字小文字無視)
   - local branch 行: current=✓+強調(既存)、upstream があれば右に小さく `↑a ↓b`(StatusBarSummary ではなく Branch ごとの UpstreamInfo — snapshot の branches に既にある。KagiApp に保持してなければ追加)
   - remote branch / tag 行: click = その commit へ jump(既存 jump_to_branch を一般化 or commit_row_index を引く)。stash 行は既存(apply モーダル)
   - **context menu は今回スコープ外**(W2-DELETE の backend 待ち。右クリックは未実装でよい)
3. ログ: `[kagi] sidebar: local=N remote=M tags=K stashes=S filter="..."` を reload 時に出す

## 完了条件
- cargo test 全パス + 警告 0 / 既存 headless 回帰なし(KAGI_JUMP 含む)
- 4セクション+件数+折りたたみ+filter が動く(構造検証 + ログ。見た目は PM)
- worktree ブランチにコミット(複数可)。push はしない

## 触ってよい: src/ui/(sidebar.rs 新規・mod.rs は抽出と状態フィールド追加の最小限)/ docs/tickets/W2-SIDEBAR.md
## 触ってはいけない: src/git/ src/graph/ src/lib.rs src/main.rs tests/ Cargo.toml scripts docs他
