# T-WS-EDITOR-005: エディタワークスペース コードレビュー是正(10件)

- Status: review
- Group: workspace framework / review remediation
- 仕様の正: 本チケットの findings リスト(下記 1–11)+ ADR-0120。設計判断は
  PM 確定済み — 実装のみ(再議論なし)。

## 背景

T-WS-EDITOR-001〜004(ADR-0120 のエディタワークスペース: 左=file tree /
main=コードビューア / 右=hunk)に対し、検証済みのマルチエージェントコードレビューが
このスライスに 10 件の finding を出した(+ 関連クリーンアップ 1 件、計 11)。

## スコープ(findings 1–11)

1. **`vendor/gpui-terminal/src/input.rs`**: platform(Cmd)修飾キーの guard が
   広すぎ、以前動いていた 3 つの慣習的 macOS ターミナルチョード(VS Code parity)を
   殺していた。`cmd-backspace`(`0x15` / ^U)、`cmd-left`(`home` アームと同一バイト)、
   `cmd-right`(`end` アームと同一バイト)を guard の例外にし、それ以外
   (`cmd-v/c/a/enter`等)は引き続き `None`。
2. **`editor_workspace.rs` `load_selected`**: OOM リスク是正。`fs::metadata` を
   まず取り、`MAX_EDITOR_BYTES`(新設・10MiB)超なら読まずに "too large" 扱い。
   バイナリ探索は先頭 `BINARY_PROBE_BYTES`(8KiB)のみに限定、行数カウントは
   バックグラウンドタスク側に移動(main スレッドの marshal-back は代入のみ)。
3. **`render_body.rs` + `editor_workspace.rs`**: resolver の `layout.left` が
   無視され sidebar トグルがエディタモードのツリーを隠さない policy/render 分岐。
   entity は単一 render のまま(re-entrancy 対策維持)、`render_body` が
   `CenterPane::Editor` アームで `layout.left == FileTree` を新設
   `show_tree: bool` フィールドへ push してから embed(CommitPanel の push と
   同型)。`render_editor_workspace` は `show_tree == false` でツリーペイン+
   divider をスキップ。
4. **`editor_workspace.rs` `open_editor_workspace`**: File History / Ecosystem
   takeover を開く前に close(`close_file_history` / `close_ecosystem_view`)。
   両方とも resolver で Editor モードに優先するため、閉じないと
   Cmd-Shift-E が見た目上何もしない。逆方向(Editor 上に Analyze を開く)は
   意図した overlay 挙動のまま変更なし。
5. **`render.rs` の ↑/↓ ハンドラ**: `editor_workspace.is_some()` 条件に
   `this.ecosystem.is_none() &&` を追加。Analyze が resolver 上でエディタに
   優先するため、非表示のエディタが矢印キーと `editor-ws: file` klog を
   奪ってはいけない。
6. **`editor_workspace.rs` の削除/非UTF-8ファイルのプレースホルダ誤り**:
   `content_missing`(fs::read 失敗 or `ChangeKind::Deleted`)/
   `content_undecodable`(UTF-8 デコード失敗)を新設フィールド化し、
   `Msg::EditorWorkspaceDeleted` / `Msg::EditorWorkspaceUndecodable`
   (i18n EN/JA)を表示。hunks ペインは削除 diff があればそのまま表示。
7. **`editor_workspace.rs` `step_selection`**: 選択ファイルが折り畳みで
   非表示のときに先頭/末尾へテレポートしていた。選択ファイルの base tree
   index が可視ファイル行の中でどこに挿入されるかを求め、↓はその位置
   (末尾でクランプ)、↑はその手前(先頭でクランプ)を選択。純関数
   `nearest_visible_file_row` に切り出し、単体テスト追加。
8. **`editor_workspace.rs` `render_tree_row`**: ゼブラストライプが base tree
   index でキーされていた(折り畳みで縞が飛ぶ)。uniform_list の range index
   (可視位置)を渡し、その偶奇を使用。
9. **`editor_workspace.rs` `sync_editor`**: 毎フレーム content を clone して
   ハッシュしていた。ロード時の marshal-back で `content_sig: u64` を一度だけ
   計算・保存し、`sync_editor` は `pushed_sig` との比較のみ(実際に push する
   ときだけ String を clone)。
10. **`MainDiffView` ビルダーの三重実装**: `build_wip_diff_view`
    (editor_workspace.rs)が「added/removed カウント →
    `FileDiffView::from_file_diff` → `"+{} −{}"` 整形 → `highlight_diff_rows` →
    `MainDiffView` 組み立て」の三つ目のコピーだった。`diff_view.rs` に
    `build_main_diff_view` を新設して一本化、3 箇所
    (`set_commit_main_diff` の headless パス、`FileHistoryView` の diff
    ローダー、`EditorWorkspaceView` の WIP-diff ローダー)を置き換え。出力は
    バイト単位で不変(タイトル/統計文字列/ハイライトとも同一)。
11. **`workspace_mode` フィールドの冗長性除去**(レビュー指摘のクリーンアップ):
    `workspace_mode: WorkspaceMode` は `editor_workspace.is_some()` と重複し
    乖離のもとだったため `WorkspaceMode` enum ごと削除。`render_body` /
    `commands.rs` / `tabs.rs` / `editor_workspace.rs` の open/close で
    `editor_workspace.is_some()` から導出。トグルハンドラの klog は
    `menu: editor_workspace={}` に変更(既存の他の `[kagi]` 行は無変更)。
    ADR-0120 の更新は不要(意図の記述のため)。`workspace.rs` のドキュメントの
    `WorkspaceMode` 言及のみ更新。

## 触ってよいファイル

`vendor/gpui-terminal/src/input.rs`, `src/ui/editor_workspace.rs`,
`src/ui/workspace.rs`, `src/ui/render_body.rs`, `src/ui/render.rs`,
`src/ui/commands.rs`, `src/ui/tabs.rs`, `src/ui/mod.rs`(フィールド削除のみ),
`src/ui/diff_view.rs`, `src/ui/file_history_render.rs`, `src/ui/i18n.rs`。

## 触ってはいけないファイル

`crates/kagi-git/src/ops/*` の write 系、既存 `[kagi]` コントラクト行の
ワーディング・順序(`editor-ws: open/files/file/source` は不変。
`menu: workspace_mode=` は新設されたばかりで依存者なし — finding 11 での
置き換えは許可済み)。

## 完了条件

- [x] `cargo build` / `cargo test --workspace` 全パス(resolver + editor_workspace
      + gpui-terminal の単体テスト含む)
- [x] finding 1: `cmd-backspace`/`cmd-left`/`cmd-right` の新規テスト追加、
      `test_platform_chords_produce_no_output` 更新
- [x] finding 7: `nearest_visible_file_row` 純関数 + 単体テスト追加
- [x] `grep -rE 'git2::|Repository::open' src/ui` = 0
- [x] `cargo fmt --check` clean、`cargo clippy --workspace` の警告数が baseline
      (HEAD 時点 39)から増えていない
- [x] headless: fixture repo で `KAGI_EDITOR_WS=1` が `editor-ws: open` /
      `files N` / `file <path>` を継続して出す。clean clone で `source all`
      自動切替を確認
- [ ] GUI 目視(subagent は GUI 不可 — PM 確認待ち): sidebar トグルでの
      ツリー表示/非表示(finding 3)、削除/非UTF-8ファイルのプレースホルダ
      (finding 6)、ゼブラストライプ(finding 8)、Cmd ターミナルチョードの
      実機挙動(finding 1)

## テスト方法

`cargo test --workspace`(`ui::workspace::tests` / `ui::editor_workspace::tests` /
`gpui_terminal::input::tests`)+ headless
(`KAGI_EDITOR_WS=1 KAGI_NO_RESTORE=1 KAGI_OPEN_REPO=<fixture> ./target/debug/kagi`,
バックグラウンド起動 + sleep + kill 方式、macOS に `timeout` なし)。
