# T-DIFF-WRAP-001: diff ペインの折り返し継続行がクリップされる問題を修正(ユーザー報告)

- Status: review
- 依存: T-UI-003(main diff の全幅表示・`render_main_diff_view`)、T-WS-EDITOR-001(Editor
  Workspace の hunk ペイン)、ADR-0117(`render_diff_list` を File History と共有する形へ抽出)

## 背景

ユーザー報告: diff 表示(特に Editor Workspace の狭い hunk ペイン)で、長い行が
視覚的には折り返される(ソフトラップ)のに、行の高さが 1 行分で固定されたまま
なので折り返し継続行がクリップされて見えない。

根本原因: `render_diff_list`(`src/ui/render_helpers.rs`)が `uniform_list`(全行
固定高)で仮想化しているのに対し、行の中身(`render_main_diff_row`、旧
`render_main_diff_rows`、`src/ui/diff_view.rs`)は折り返し可能なテキストだった。
`uniform_list` は「行はすべて同じ高さ」が前提の仮想化 API なので、内容が2行に
折り返わっても行の外枠(と `overflow_hidden()`)は1行分の高さのまま — 2行目は
描画されるが親の高さでクリップされ、見た目には「消えている」。

ユーザーは以下の3択のうち **フル折り返し(可変行高)** を明示的に選択:
1. 折り返さない(nowrap + 横スクロール)
2. 折り返すが表示上は clip のまま(何もしない)
3. **折り返しを機能させ、行の高さを内容に合わせて可変にする ← 採用**

## スコープ

1. `render_diff_list` を `gpui::list`(`ListState` ベースの可変行高リスト)で
   描画するよう置き換え(`uniform_list` から移行)。行の描画は
   `render_main_diff_row(rows, i) -> AnyElement` という単一行ビルダーに整理
   (旧 `render_main_diff_rows(rows, range) -> Vec<..>` から改名)。
2. 3 つのオーナー(`KagiApp.main_diff_scroll_handle` / `FileHistoryState.diff_scroll`
   / `EditorWorkspaceView.diff_scroll`)の型を `UniformListScrollHandle` から
   `gpui::ListState` に変更。
3. 行のスタイル調整(折り返しを実際に効かせる):
   - 行コンテナ: `overflow_hidden()` を撤去、`items_center()` → `items_start()`
     (折り返して2行以上になったとき、行番号列が最初の視覚行の高さに揃うよう)。
   - コンテンツ div: `overflow_hidden()` を撤去。`flex_1()` は維持し
     `min_w(px(0.))` を追加(折り返し前の内在幅で `flex_1` の親を押し広げず、
     ペイン端で折り返すように)。ハイライトの範囲検証(char boundary チェック)
     はそのまま維持。
   - Hunk ヘッダー行は `.truncate()`(1行固定・省略記号)のまま — 折り返し対象外。
4. スクロールバー: `with_vertical_scrollbar` を
   `H: gpui_component::scroll::ScrollbarHandle + Clone` にジェネリック化。
   gpui-component 0.5.1 は `ScrollbarHandle` を `UniformListScrollHandle` と
   `gpui::ListState` の両方に実装済み(`src/scroll/scrollbar.rs`)なので、
   kagi 側のローカル newtype は不要だった。

## `ListState` のライフサイクル

`render_diff_list` の冒頭で毎レンダー、`scroll_handle.item_count()` と
`view.rows.len()` を比較し、不一致なら `scroll_handle.reset(row_count)` を呼ぶ
一箇所に集約。3 オーナーで diff/`MainDiffView` が代入される箇所は
`diff_view.rs` / `file_history_render.rs` / `editor_workspace.rs` に約10箇所
散らばっているが、そのすべてが直後に `cx.notify()` を呼ぶため、次のレンダーで
必ずこのチェックを通る。個々の代入箇所に reset を書いて回るより堅牢(書き漏れが
構造的に起きない)。`reset` はスクロール位置も先頭へ戻すが、これは行数が実際に
変わったときだけ発火する — 元々ファイル切替時にスクロール位置を明示的に
リセットしていなかった挙動と実質同じ(むしろ改善)。

## 完了条件

- [x] `cargo build` / `cargo test --workspace` 通過
- [x] `cargo fmt --check` クリーン
- [x] `cargo clippy --workspace --all-targets` の warning 数がベースライン(45)から増えない
- [x] `grep -rE 'git2::|Repository::open' src/ui` = 0
- [x] headless: `KAGI_EDITOR_WS=1 KAGI_NO_RESTORE=1 KAGI_OPEN_REPO=<fixture>` が
      `editor-ws: open/files/file` を出してクラッシュしない
- [x] headless: `KAGI_SELECT_FIRST=1 KAGI_OPEN_FIRST_FILE=1 <fixture-repo-with-2-commits>`
      が従来通り `[kagi] diff: ...` + `[kagi] main-diff: open ... rows=N` を出す
- [ ] PM の目視確認(下記チェックリスト、GUI はサブエージェントから操作不可)

## PM 向け目視チェックリスト

1. Editor Workspace を開き、hunk ペイン(右側)を狭める。400文字超の長い行を
   含むファイルの diff を表示 → その行が複数の視覚行に折り返され、**すべての
   継続行が見える**(クリップされない)こと。
2. 折り返しによって行が2行以上になった行では、行番号(old/new)がその行の
   **先頭の視覚行**に揃っていること(`items_start`)。
3. 同じ diff を File History の diff ペイン、および main 全幅 diff(commit
   detail からファイルクリック)でも確認 — 3箇所とも同じ描画。
4. マウスホイール/トラックパッドで縦スクロールが機能すること(3箇所とも)。
5. Hunk ヘッダー行(`@@ ... @@`)は折り返さず1行 + 省略記号のままであること。
6. スクロールバー(縦)が出て、ドラッグ/クリックでジャンプできること。

## 触ってよいファイル

- `src/ui/render_helpers.rs`(`render_diff_list` / `with_vertical_scrollbar` / `new_diff_list_state`)
- `src/ui/diff_view.rs`(`render_main_diff_row`)
- `src/ui/mod.rs`(`main_diff_scroll_handle` の型 + 構築箇所)
- `src/ui/file_history.rs` / `src/ui/file_history_render.rs`(`diff_scroll` の型)
- `src/ui/editor_workspace.rs`(`diff_scroll` の型)
- `src/ui/render_body.rs`(スレッディング先の型)
- `docs/tickets/T-DIFF-WRAP-001.md` / `docs/tickets/INDEX.md`

## 触ってはいけないファイル

- `crates/kagi-git/` / `crates/kagi-domain/` / `tests/*` / `Cargo.toml`
- `[kagi]` ログ行のフォーマット・文言・順序(既存コントラクトは変更なし)
- commit-list / sidebar / file-tree の `UniformListScrollHandle`(今回のスコープ外
  — grep で `scroll_to_item` 呼び出しが無いことを確認した3つの diff-list ハンドル
  のみが対象)

## テスト方法

1. `cargo build --workspace` / `cargo test --workspace`
2. `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets`
3. headless: fixture リポジトリで `KAGI_EDITOR_WS=1` と
   `KAGI_SELECT_FIRST=1 KAGI_OPEN_FIRST_FILE=1` の両方を実行しログ確認
4. PM による GUI 実操作(上記チェックリスト)

## リスク

- `gpui::list` は `uniform_list` と違い、行の実測(measure)コストがある
  (可変高リストの宿命)。diff の行数が非常に多い場合のスクロール性能は
  `uniform_list` よりわずかに重くなり得るが、overdraw(`px(1000.)`)で吸収する
  設計(gpui-component の `TextView` と同じ値を採用)。
- `render_diff_list` の item-count 同期を一箇所に集約したことで、たまたま
  同じ行数の別 diff に切り替えた場合はスクロール位置がリセットされない
  (旧仕様でもリセットしていなかったので後退ではない)。

## 実装メモ

### 実装日: 2026-07-07

- `render_helpers::render_diff_list` を `gpui::list` + `gpui::ListState` に置換。
  スクロールバーは `with_vertical_scrollbar<H: ScrollbarHandle + Clone>` に
  ジェネリック化し、`UniformListScrollHandle` と `ListState` の両方で動く
  (gpui-component 0.5.1 が両方に `ScrollbarHandle` を実装済みのため、ローカル
  newtype は不要だった — 当初想定していた `DiffListScroll` ラッパーはスコープ外)。
- `diff_view::render_main_diff_rows(rows, range) -> Vec<impl IntoElement>` を
  `diff_view::render_main_diff_row(rows, i) -> gpui::AnyElement` に単一行化。
- `KagiApp.main_diff_scroll_handle` / `FileHistoryState.diff_scroll` /
  `EditorWorkspaceView.diff_scroll` の型を `ListState` に変更。構築は共有ヘルパー
  `render_helpers::new_diff_list_state()`。
- 動作確認: `KAGI_EDITOR_WS=1` fixture、`KAGI_SELECT_FIRST=1 KAGI_OPEN_FIRST_FILE=1`
  fixture(2コミット、400文字超の行を含む)の両方でクラッシュなし・既存ログ
  行(`editor-ws: open/files/file`, `diff: ...`, `main-diff: open ... rows=N`)を
  確認。
