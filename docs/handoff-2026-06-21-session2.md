# Kagi 再アーキテクチャ — セッション引継ぎ (2026-06-21)

> 前セッション (#67/#68 をマージ) からの引継ぎ。次の担当 (Claude or 人間) 向け。
> 作業ブランチ: `claude/gifted-wozniak-ljx2rj`（designated branch、PR はここから main へ）。

## このセッションで完了したこと

| 項目 | PR | 状態 |
|---|---|---|
| **Task 1: kagi-git crate 分離** | #67 | ✅ merged (`adeb00f`) |
| **Task 3: headless 退役** | #68 | ✅ merged (`4f5c450`) |
| stale 旧 PR クローズ (#65 handoff-WIP, #59 docs重複) | — | ✅ |
| stale ブランチ整理 | — | ⚠️ 環境が `git push --delete` を 403 で拒否。GitHub UI で手動削除要 |

### Task 1 詳細 (ADR-0115)
- `src/git/**` → `crates/kagi-git/**` を `git mv`（履歴保持、`mod.rs`→`lib.rs`）。
- `kagi-git` の依存は `kagi-domain` / `git2` / `ureq` / `tempfile` のみ（UI/gpui 無し）。
- 呼出し側 `kagi::git::` / `crate::git::` → `kagi_git::`（src 312 + tests 31 + `src/remote` + `src/lib.rs` から `pub mod git` 削除）。
- ルート `Cargo.toml` に workspace member + path 依存追加。`tests/push_test.rs` の safety-grep パス更新。`ci.yml` の no-git2 gate ヒント更新。

### Task 3 詳細 (ADR-0077)
- `src/headless.rs`: **1713 → 208 行**、hooks **33 → 11**。
- 削除した mutating plan/execute hooks: `KAGI_PULL`/`PUSH`/`UNDO`/`POP`/`DISCARD`(+ALL)/`AMEND`(+MSG)/`DELETE_BRANCH`/`PLAN_CHECKOUT`/`CHECKOUT_COMMIT`/`CREATE_BRANCH`/`PLAN_WORKTREE`/`STASH_PUSH`/`STASH_APPLY`/`CHERRY_PICK`/`REVERT`/`COMMIT_PANEL`(+`STAGE_FILE`/`UNSTAGE_FILE`/`COMMIT_MSG`)/`AUTO_CONFIRM`、および helper `record_headless_op` / `run_headless_discard`。
- 維持した read-only UI-state hooks: `OPEN_REPO`/`SELECT_FIRST`/`JUMP`/`CONTEXT_MENU`/`COMPARE_HEAD`/`COMPARE_WT`/`OPEN_FIRST_FILE`/`COMPACT`/`BOTTOM_PANEL`/`TERMINAL`/`MENU_DUMP`。
- **検証根拠**: どのテスト/CI/スクリプトもこれらの var をセットしていない（binary-spawn テストは `KAGI_LOG_DIR`/`LANG`/`OFFLINE` のみ）。`[kagi]` 契約行は不変。

### ⚠️ Task 3 で露見した既存の穴（要フォローアップ）
`open_undo_modal`（ADR-0107「直前コミット取消」）は削除した `KAGI_UNDO` hook **だけ**から呼ばれていた。Cmd+Z の操作履歴 undo (`open_history_undo_modal`, ADR-0081) とは別物。render+confirm+cancel は配線済みだが **GUI トリガーが存在しない**。現状 `#[allow(dead_code)]` で保持（`src/ui/operations/history.rs`）。→ メニュー/キー配線するか、機能ごと削除するか判断要。

## 検証状態（main = 4f5c450 時点）
- `cargo test --workspace`: **784 passed / 0 failed**。
- `cargo fmt --all --check`: clean。`grep -rnE 'git2::|Repository::open' src/ui/` = **0**。
- clippy: 既存 debt のみ（kagi-git lib 6件は `src/git` から持ち越し、kagi lib 2件 doc-list）。新規警告なし。
- **環境メモ**: GUI リンクに `libxkbcommon-dev` / `libxkbcommon-x11-dev` が必要（`apt-get install` 済み。コンテナ再起動で要再インストール）。これが無いと bin/統合テストのリンクが 403... ではなく `-lxkbcommon` で失敗する。

## 残タスクと次の一手

### Task 2: Entity<T> 化（Phase C）— 大規模・複数PR・過去に失敗
目的: `cx.notify()` 329 箇所による全体再描画を、子 Entity 化で局所化する。

- **ToastStack** (`src/ui/toast_stack.rs`): 今は `Rc<RefCell<ToastStack>>`（`KagiApp.toast_stack`）。Entity 化には (a) `ToastStack` に `Render` 実装、(b) `KagiApp.render` で子 Entity を埋め込み、(c) `push_toast`/`start_exit` を `entity.update(cx, …)` 経由に。**核心の難所**: `push_toast`(`src/ui/mod.rs:2211`) を呼ぶ ~38 箇所に `cx` を通す必要。`blocking_ops` 系は `cx` を持たない呼出しがある。前回失敗の教訓: 複数行 `push_toast(...)` の **閉じ括弧直前** に `, cx` を入れること（行頭に入れて壊した）。
- **OpLogPanel** (`src/ui/oplog_panel.rs`): 同様。`record_op` ~122 箇所の cx スレッディングが必要でより大きい。
- 推奨順: ToastStack（最小）→ OpLogPanel → CommitPanel/ConflictEditor/Sidebar/Inspector/FileHistory。各 1 PR。
- **注意**: render/UI 変更はビルド+テスト緑でも不十分、人間が GUI を起動して目視確認が必要（CLAUDE.md）。

### Phase B: render path の per-frame clone 排除（低リスク・1PR だが要目視）
roadmap は「最初にやる」推奨。ただし調査の結果、各項目は一言ほど自明ではない:
- **`row.edges.clone()`** (`src/ui/render_helpers.rs:275`): `graph_canvas`(`src/ui/graph_view.rs:227`) の paint closure が `'static` で `edges: Vec<GraphEdge>` を move-capture するため、借用化は不可。**真の最適化は `CommitRow.edges` を `Arc<[GraphEdge]>` に**してフレーム毎の deep clone を O(1) に。ただし `commit_list.rs:297/307` の `.push()` で増分構築しているので「Vec で組んでから `.into()`」が必要。`stash_lanes.to_vec()` も同様に `Arc<[usize]>` 化候補。
- **`compare_view.clone()`/`conflict.clone()`** (`render.rs:583/642`): フィールドを `Arc<…>` 化。`conflict` は参照箇所が多く中規模 churn。
- **avatar_color/avatar_initial** (`render_helpers.rs:116-117`): 毎フレーム計算 → `CommitRow` ビルド時に precompute。
- **`theme()` 106回 / `scaled_px` 73回**（render.rs）: render 冒頭で 1 ローカルに hoist。広範囲 diff。

### Phase A: worker thread 呼出し側マイグレ（roadmap 参照）
インフラ (#58) 済み、`*_blocking` 呼出し側 ~32 箇所が未移行。

## 安全ルール（不変）
1. `git2::` を `src/ui/` に書かない（CI gate）。Git は `kagi_git::` 経由。
2. `reset --hard` / `push --force` / `git clean` / `unsafe` をコードに書かない。
3. `[kagi]` ログ契約行の文言を変えない（headless/統合テストが grep）。
4. push 前に `cargo fmt --all`。`cargo test --workspace` 緑。
5. main を消さない・巻き戻さない。
6. PR は designated branch から。CI green 後 squash-merge（このセッションは #67/#68 をそうした）。
