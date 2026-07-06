# T-WS-EDITOR-004: エディタワークスペース フィードバック第2弾(tree切替・resize・ヘッダーボタン)

- Status: review
- Group: workspace framework / エディタモード
- 仕様の正: ADR-0120 §Decision 4 + 本チケット(PR #102/#103 マージ後のユーザーフィードバック)。

## 背景

T-WS-EDITOR-001(PR #102/#103)マージ後の 2 巡目フィードバック 3 件:

1. 「Changes だけでなく普通のツリービュー/エディタとしても使いたい — 切り替えられるように」
   — tree は変更ファイルのみで、clean な worktree では空になり使い物にならない。
2. 「トグルは edit みたいなアイコンで、ヘッダーの Analyze の左に」— ヘッダーツールバーボタン。
3. 「pane のリサイズができない」— tree(240px)/ hunks(380px)が固定幅。

## スコープ

1. **tree source 切替(Changes ⇄ All)**: `TreeSource { Changes, All }` を
   `EditorWorkspaceView` に追加(default `Changes`)。tree 上部のチップ UI で切替。
   初回 Changes ロードが 0 件なら自動で All に切替(`generation == 1` でゲート —
   手動で Changes に戻した後は跳ね返さない)。i18n EN/JA、klog
   `editor-ws: source {changes|all}`(自動切替も同じ行)。
2. **All-files 列挙(kagi-git)**: `Backend::worktree_files()` を新設
   (`status::worktree_files` — `include_unmodified` で `working_tree_status` と
   同じ statuses machinery を再利用、独自 gitignore パースなし)。integration test
   (tracked + untracked + ignored の 3 種を用意し、ignored のみ非表示を確認)。
   ponytail: 全件 eager 列挙(遅延展開は T-WS-EDITOR-003 に残す)。
3. **`TreeRow::File.change` を `Option<ChangeKind>` 化**: All モードの未変更ファイルは
   バッジなし。`file_tree::build_file_tree`(既存 `&[FileStatus]` 入力)は変更なしで
   動き続け、新設 `build_file_tree_opt(&[(PathBuf, Option<ChangeKind>)])` が同じ
   `DirNode` 圧縮アルゴリズムを共有(二重実装なし)。呼び出し側
   (inspector.rs / commit_panel_render.rs / modal_renderers_commit.rs / mod.rs /
   editor_workspace.rs)は実ファイルの `ChangeKind` を持つので `Some(..)` /
   `.as_ref()` でラップ。`commit_panel::status_badge` / `inspector::change_badge`
   は `Option<&ChangeKind>` を受け、`None` は空バッジを描画。
4. **ヘッダーボタン**: `render_header.rs` に `tb-editor-ws`(`IconName::File` —
   0.5.1 に鉛筆アイコンなし)を Analyze の左に追加。クリックは
   `handle_menu_command("view.toggleEditorWorkspace", ...)` を直接呼び、
   View メニュー / `secondary-shift-e` と完全に同じ経路・ログ行を通す。
5. **pane リサイズ**: `DividerKind::EditorTree` / `EditorHunks` を追加。
   `tree_w` / `hunks_w` を `EditorWorkspaceView` の実体フィールド化(初期値は旧定数
   240.0 / 380.0)。`render_editor_workspace` 内の 2 本の 4px divider を
   `render_body.rs` の `divider1` と同型の drag source 化。`handle_divider_drag`
   (render_divider.rs)に 2 arm 追加 — Sidebar 相当(EditorTree、絶対カーソル位置)、
   Panel 相当(EditorHunks、右端からの距離 — hunks は左に divider があるので右ドラッグで
   縮む)。クランプ: EditorTree 160–480、EditorHunks 240–700(`src/ui/mod.rs` の
   `EDITOR_TREE_MIN/MAX` / `EDITOR_HUNKS_MIN/MAX`)。幅は非永続(transient)。

## 触ってよいファイル

`src/ui/editor_workspace.rs`, `src/ui/file_tree.rs`, `src/ui/commit_panel.rs`
(`status_badge`), `src/ui/inspector.rs`(`change_badge`), `src/ui/commit_panel_render.rs`,
`src/ui/modal_renderers_commit.rs`, `src/ui/render_header.rs`, `src/ui/render_divider.rs`,
`src/ui/types.rs`(`DividerKind`), `src/ui/mod.rs`(定数 + `use`), `src/ui/i18n.rs`,
`crates/kagi-git/src/status.rs` / `backend.rs`(read-only 列挙のみ), `tests/status_test.rs`。

## 触ってはいけないファイル

`crates/kagi-git/src/ops/*` の write 系、既存 `[kagi]` コントラクト行のワーディング。

## 完了条件

- [x] tree 上部の Changes/All チップで切替でき、選択中ファイルは切替後も存在すれば
      選択が保持される(headless klog で `source changes`/`source all` を確認)
- [x] clean な worktree を開くと自動で All に切替わり `editor-ws: files N`(N>0)になる
      (headless: クリーンな clone で確認)
- [x] `worktree_files` に kagi-git integration test、既存 `working_tree_status` 系
      テストとあわせて `cargo test --workspace` 全パス
- [x] `TreeRow::File.change` の `Option` 化を全呼び出し側で更新、既存
      `editor_workspace`/`file_tree` の単体テストを更新して green
- [ ] ヘッダーの Editor ボタン配置・アイコン・チップ UI・divider のドラッグ感触は
      GUI 目視(subagent は GUI 不可 — PM 確認待ち)
- [x] `grep -rE 'git2::|Repository::open' src/ui` = 0
- [x] `cargo fmt --check` clean、`cargo clippy --workspace` の警告数が baseline
      (HEAD時点 39)から増えていない

## テスト方法

`file_tree` / `editor_workspace` の単体テスト(merge/compression の共有ロジック)+
`kagi-git` の `worktree_files` integration test + headless
(`KAGI_EDITOR_WS=1 KAGI_NO_RESTORE=1 KAGI_OPEN_REPO=<repo>` で dirty fixture と
clean clone の両方を確認)+ PM の GUI 目視(ボタン配置・チップ・divider ドラッグ)。

## リスク

- ヘッダーボタン / チップ / divider の見た目・当たり判定は GUI 目視でのみ確認可能。
- `TreeSource::All` は毎回 `working_tree_status` + `worktree_files` の 2 回
  statuses 呼び出し(全体で index 2 walk)— 巨大 repo での体感は未計測
  (T-WS-EDITOR-003 の遅延展開が本命の解)。
