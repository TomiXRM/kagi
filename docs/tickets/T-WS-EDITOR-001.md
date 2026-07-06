# T-WS-EDITOR-001: WorkspaceMode 導入 + エディタワークスペース v1(read-only)

- Status: review
- Group: workspace framework / エディタモード
- 仕様の正: ADR-0120 §Decision 3–4。枠組みは `src/ui/workspace.rs`(実装済み)。

## 背景

ADR-0120 でスロット解決は `workspace::resolve_workspace` に集約済み。本チケットで
初のモード切替(Graph ⇄ Editor)と、Editor モードの 3 ペイン
(左=file tree / main=コードビューア / 右=hunk)を read-only で通す。

## スコープ

1. **mode**: `KagiApp.workspace_mode: WorkspaceMode { Graph, Editor }`(default Graph)。
   `WorkspaceInputs` の最上流入力として resolver に追加(Editor 時:
   left=FileTree / center=Editor / right=Hunks。takeover(FileHistory/Ecosystem)や
   Conflict Mode は Editor より優先 — 既存テーブルの上に挿す位置をテストで固定)。
   `reset_per_repo_ui` で Graph に戻す。
2. **enum**: `LeftPane::FileTree` / `CenterPane::Editor` / `RightPane::Hunks` を追加。
3. **EditorWorkspaceView**(fat entity、ADR-0117 テンプレート):
   - 所有: working-tree 変更ファイルの `Vec<FileStatus>` → `file_tree::build_file_tree`
     の `TreeRow` 列、選択中ファイル、`Entity<InputState>`(`code_editor`)、
     選択ファイルの `FileDiff`(hunk)。
   - 左 tree レンダラは新規共通実装(inspector/commit_panel の個別実装は触らない)。
   - main: 選択ファイルを読み込み `InputState::code_editor(lang)` に **read-only** 表示。
     lang は `diff_view::lang_for_ext` を配線(`set_highlighter`)。
   - 右: 選択ファイルの WIP diff(unstaged 優先)を `render_helpers::render_diff_list`
     で表示。
   - Backend 読み取りは entity 自身の `cx.spawn`(FileHistoryView と同型)。
     子→親は `WeakEntity<KagiApp>` + deferred のみ。
4. **切替導線**: View メニュー `view.toggleEditorWorkspace`(`command_state`: has_repo)
   + ショートカット 1 つ(既存と衝突しないもの)。i18n EN/JA を `Msg` に追加。
5. **headless**: `klog!` で `editor-ws: open`, `editor-ws: file <path>` 等の
   コントラクト行を新設(既存行の変更は禁止)。

## 触ってよいファイル

`src/ui/workspace.rs`, `src/ui/mod.rs`(field+open/close), `src/ui/render_body.rs`(arm 追加),
新規 `src/ui/editor_workspace.rs`(+render 分割可), `src/ui/tabs.rs`(reset_per_repo_ui),
`src/ui/commands.rs`, `src/ui/i18n.rs`, `src/headless.rs`(検証コマンド追加)。

## 触ってはいけないファイル

`crates/kagi-git/src/ops/*`(write 操作なし)、既存 `[kagi]` コントラクト行、
`inspector.rs` / `commit_panel*` の tree 実装(共通化は本チケットでやらない)。

## 完了条件

- [x] Graph ⇄ Editor をメニュー/ショートカットで往復でき、タブ切替で Graph に戻る
      (`view.toggleEditorWorkspace` / `secondary-shift-e`、`reset_per_repo_ui` で Graph リセット)
- [x] Editor モード: 左に変更ファイル tree、クリックで main にハイライト付き read-only 表示、右にそのファイルの hunk
- [x] FileHistory / Analyze / Conflict は Editor モードより優先される(resolver テストで固定)
      — Conflict は ADR-0120 の設計どおり resolver より上位(`render.rs`)でゲートされるため、
      Editor モードが有効でも `render_body`/resolver 自体が呼ばれず構造的に優先される
      (resolver 側は FileHistory/Ecosystem/Loading の3つをテストで固定)
- [x] `resolve_workspace` の新規優先順位の単体テスト追加、`cargo test --workspace` 全パス
- [x] `grep -rE 'git2::|Repository::open' src/ui` = 0 / 既存 `[kagi]` 行の変更なし
- [ ] GUI 目視は PM(subagent は GUI 不可)

## テスト方法

resolver 単体テスト + fixture repo での headless 検証(`KAGI_*` で editor-ws を開き
klog 行を grep)+ PM の GUI 目視。

## リスク

- entity 再入(既知パターン: 必ず deferred)。大ファイル読み込みで UI ブロック
  → 読み込みは background_spawn、`code_editor` の 50K 行制限をガードに使う。
