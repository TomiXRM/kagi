# W8-TERMSEL: ターミナルのテキスト選択 + Cmd+C コピー(vendored gpui-terminal)

- Status: in-progress
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

- [ ] ドラッグで選択ハイライトが出る(単語=ダブルクリック、行=トリプルクリックも)
- [ ] Cmd+C で選択テキストがクリップボードに入る(`pbpaste` で確認)
- [ ] 選択なし Cmd+C は no-op、ctrl-c の SIGINT は従来どおり
- [ ] クリック/入力で選択解除
- [ ] cmd-v paste(既存)に回帰なし
- [ ] `cargo test` 全パス(vendor 内 unit test 含む)+ own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(upstream PR に使える変更サマリ含む)

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
