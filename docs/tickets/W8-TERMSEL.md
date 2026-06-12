# W8-TERMSEL: ターミナルのテキスト選択 + Cmd+C コピー(vendored gpui-terminal)

- Status: done
- 担当: worktree agent(Opus)
- 関連 ADR: 0035(vendor)/ 0008

## 背景

ユーザー報告: ターミナルで Cmd+C / テキスト選択ができない。上流 0.1.0 は選択が TODO スタブ。
ADR-0035 で `vendor/gpui-terminal/` に vendor 済み(path 依存切替済み、ビルド・テスト確認済み)。
cmd-v paste は app 側で実装済み(src/ui/mod.rs render_terminal_body + terminal.rs SharedWriter)。

## スコープ(vendor 内の実装が主戦場)

1. **マウス選択の配線**(`vendor/gpui-terminal/src/view.rs` の TODO 3箇所):
   - on_mouse_down: クリック位置 → セル座標(render.rs のセル寸法計算を流用)、
     click_count で Simple/Word/Line(`mouse.rs` の `selection_type_from_clicks` が既存)、
     選択開始 + 既存選択クリア
   - on_mouse_move(ドラッグ中): 選択範囲更新 + cx.notify
   - on_mouse_up: 選択確定。スクロールバック越え(viewport 外へのドラッグ)は v0 では clamp でよい
   - `mouse.rs` の `Selection` 構造体(start/end/contains)が既存 — これを使う
2. **選択ハイライト描画**(`render.rs`): 選択範囲内セルの背景を選択色で描画
   (alacritty_terminal の selection API を使うか、view 側の Selection を render に渡すかは
   既存構造に合わせて判断。`config` に selection 色があるか確認、なければ追加)
3. **Cmd+C コピー**: 選択があれば選択テキストを取得(grid からセル文字を収集、行末は \n)し
   gpui の clipboard へ(`cx.write_to_clipboard`)。選択がなければ何もしない
   (ctrl-c の SIGINT は従来どおり keystroke_to_bytes 経由 — 干渉させない)。
   実装位置は view.rs の on_key_down 先頭(platform modifier 判定)
4. **選択解除**: クリック(非ドラッグ)/ 新規入力(キー送信時)で選択クリア(一般的な端末挙動)
5. **kagi 側**: 必要なら src/ui/terminal.rs の config に選択色を渡す程度。app ロジックは増やさない
6. 改変箇所には `// kagi:` コメント(ADR-0035。upstream PR 可能な汎用実装を保つ)

## 完了条件

- [x] ドラッグで選択ハイライトが出る(単語=ダブルクリック、行=トリプルクリックも)— ユーザー実機確認済み(2026-06-13)
- [x] Cmd+C で選択テキストがクリップボードに入る(`pbpaste` で確認)— ユーザー実機確認済み(2026-06-13)
- [x] 選択なし Cmd+C は no-op、ctrl-c の SIGINT は従来どおり(control 除外で非干渉)
- [x] クリック/入力で選択解除
- [x] cmd-v paste(既存)に回帰なし(app 側 listener 無改変・cmd-c は vendor 側で処理)
- [x] `cargo test` 全パス(vendor 内 unit test 含む)+ own-code warning 0
- [x] 実装メモを本ファイル末尾に追記(upstream PR に使える変更サマリ含む)

## 触ってよいファイル

- `vendor/gpui-terminal/src/`(主戦場)
- `src/ui/terminal.rs` / `src/ui/mod.rs`(最小限)
- `docs/tickets/W8-TERMSEL.md`

## 触ってはいけないファイル

- `Cargo.toml`(vendor path 切替済み、依存追加禁止)/ `src/git/` / `tests/*` / `scripts/*` / 他 docs

## テスト方法

1. `cargo test`(exit code 直接確認)
2. fixture repo で KAGI_BOTTOM_PANEL=1 KAGI_TERMINAL=1 起動 → `ls` 等を打って出力を選択 →
   cmd-c → `pbpaste` 確認(PM も実機確認する)
3. 検証は fixture / tempdir のみ

## リスク

- alacritty_terminal の grid 座標系(viewport vs スクロールバック absolute Line)の取り違え —
  mouse.rs の既存テストと render.rs の描画座標計算を正とする
- 選択状態での再描画コスト — 選択変更時のみ notify(毎フレーム再計算しない)
- vendor の改変は選択・コピーに限定(ADR-0035)。force 系コード追加禁止(全体規約)

## 実装メモ(upstream PR 用変更サマリ)

すべての vendor 改変には `// kagi:` コメントを付与。kagi 固有要素は app 側 (`src/ui/terminal.rs`)
の 1 行(`.selection(...)` 呼び出し)のみで、vendor は汎用のまま(upstream PR 可能)。

### vendor/gpui-terminal の変更ファイル

1. **src/mouse.rs**
   - `pub fn clamp_point_to_grid(point, cols, rows) -> Point`: 生のセル座標を viewport
     `[0,cols) x [0,rows)` にクランプする純関数(scrollback 越えドラッグは v0 で edge に clamp)。
   - 既存の `Selection`/`SelectionType`/`pixel_to_cell`/`selection_type_from_clicks` を再利用(無改変)。
   - 追加 unit test 4 件(下記)。

2. **src/colors.rs**
   - `ColorPalette` に `selection: Hsla` フィールド追加(デフォルト = 半透明スレートブルー a=0.4)。
   - `ColorPalette::selection()` accessor、`ColorPaletteBuilder::selection(r,g,b,a)` 追加。

3. **src/render.rs**
   - `TerminalRenderer::paint` シグネチャに `selection: Option<&Selection>` 引数を追加。
   - 選択ハイライト pass を背景描画後・テキスト描画前に追加(各行の連続選択列を 1 quad で half-transparent
     描画 → グリフはその上に乗り可読性維持)。`paint_selection_span` ヘルパ追加。
   - 選択座標は renderer の `Line(line_idx)` (viewport 相対・display_offset なし) と一致。

4. **src/view.rs**
   - `TerminalView` に `selection: Option<Selection>` / `selecting: bool` / `geometry: Rc<Cell<Option<PaintGeometry>>>`
     を追加。`PaintGeometry`(origin/cell_width/cell_height/cols/rows)は paint closure が毎フレーム publish。
   - `on_mouse_down`: クリック→セル変換(geometry 経由)、`click_count` で Simple/Word/Line 判定、
     Word=`semantic_search_left/right`・Line=`line_search_left/right`(alacritty Term API)で anchor 展開、
     選択開始 + notify。geometry 未確定時は選択クリア。
   - `on_mouse_move`(ドラッグ中のみ): anchor から再展開して end 更新、変化時のみ notify(毎フレーム再計算回避)。
   - `on_mouse_up`: 選択確定。非ドラッグの単純クリック(start==end & Simple)は選択解除。
   - `on_key_down` 先頭: **platform modifier(Cmd)+ "c"** で `selection_text()`(`Term::bounds_to_string`)を
     `cx.write_to_clipboard(ClipboardItem::new_string(..))` へ。選択なしは no-op、いずれも `stop_propagation`。
     **control は判定から除外**したので ctrl-c SIGINT(0x03 via keystroke_to_bytes)に非干渉。
     通常キー入力時は選択を take()(解除)+ 変化時 notify。
   - `selection_text()`: start<=end 正規化 → `Term::bounds_to_string`(wrap/wide-char/行末空白を正しく処理)。

### app 側 (src/ui/terminal.rs)

- Catppuccin Mocha の selection 色を builder に渡すのみ(`.selection(0x58,0x5b,0x70,0x99)`)。
- cmd-c は vendor 側で処理。app 側 `render_terminal_body` の key listener は cmd-v のみで非干渉(回帰なし)。

### 追加 unit test 一覧(全 6 件・全パス)

- `mouse::tests::test_clamp_point_to_grid_in_range`
- `mouse::tests::test_clamp_point_to_grid_past_edges`
- `mouse::tests::test_clamp_point_to_grid_negative_line`
- `mouse::tests::test_clamp_point_to_grid_zero_dimensions`
- `terminal::tests::test_bounds_to_string_extracts_selection`(コピーテキスト抽出の end-to-end)
- `terminal::tests::test_semantic_and_line_search_expansion`(Word/Line 展開→抽出)

### テスト結果

- `cargo test -p gpui-terminal --lib`: 64 passed / 0 failed(doctest 29 passed)。
- `cargo test`(全体): 全スイート pass、回帰なし。
- `cargo clippy -p gpui-terminal`: warning 0(own-code)。kagi 側の clippy warning は本変更と無関係の既存物。
- 起動確認: fixture repo で `KAGI_BOTTOM_PANEL=1 KAGI_TERMINAL=1` 起動、shell 起動・panic なし、headless ログ回帰なし。

### PM 実機確認事項(残リスク)

- 実際のドラッグ選択ハイライト表示、ダブル/トリプルクリックの語/行選択、Cmd+C → `pbpaste` 一致を実機で確認。
- 選択座標は display_offset=0(スクロールバック未スクロール)前提。scrollback スクロール中の選択は
  別チケット(scroll 実装は本スコープ外・TODO のまま)。viewport 外ドラッグは edge clamp(仕様どおり v0)。
