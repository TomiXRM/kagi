# T-TERM-INTERACT-001: 埋め込みターミナルの操作性修正(zellij ハング / マウス無反応 / スクロール不通)

- Status: done (PM accepted 2026-07-07 — unit+gates verified; zellij/mouse/scroll GUI script pending user)
- Group: terminal / vendored gpui-terminal(ADR-0035)
- 仕様の正: 本ファイル。実装は `vendor/gpui-terminal/src/{event.rs,terminal.rs,mouse.rs,view.rs}`。

## 背景(ユーザー報告)

Kagi 埋め込みターミナル(vendored `gpui-terminal`, alacritty_terminal ベース)で:

1. zellij が起動しない(ハング/白画面になる)。
2. マウスクリックがアプリ内で効かない(フォーカス移動・カーソル移動などが起きない)。
3. スクロールが効かない(特にフルスクリーンアプリの中)。

## 根本原因(診断→本チケットで修正)

1. **`Event::PtyWrite` が握りつぶされていた**(`vendor/gpui-terminal/src/event.rs`。旧モジュール doc
   にも「無視している」と明記されていた)。alacritty_terminal はカーソル位置問い合わせ(DSR)、
   端末種別問い合わせ(DA1/DA2)、bracketed-paste の確認、キーボードモード報告など、
   「端末自身がプログラムからの問い合わせに答える」ケースで `PtyWrite(String)` を発行する。
   これが PTY に書き戻されないと、起動時に問い合わせて応答をブロック待ちするプログラム
   (zellij が該当)は永遠にハングする。
2. **マウスレポーティング未実装**: `TermMode::MOUSE_REPORT_CLICK` / `MOUSE_DRAG` / `MOUSE_MOTION`
   +`SGR_MOUSE` をアプリ(zellij/vim/tmux)が有効化しても、`on_mouse_down/up/move`
   (`vendor/gpui-terminal/src/view.rs`)はローカル選択しか実装しておらず、PTY に何も書かれない。
3. **スクロールがローカルスクロールバック専用**: `on_scroll` は常に `scroll_display` を呼ぶ。
   alt screen(zellij/vim/less)にはスクロールバックが無いため、視覚的に何も起きない。

## 修正内容

### 1. PtyWrite の折り返し配線(zellij ハングの修正)

`GpuiEventProxy`(`event.rs`)に `pty_responses: Arc<parking_lot::Mutex<Vec<u8>>>` キューを追加。
`Event::PtyWrite(data)` はこのキューにバイト列を積むだけ(**チャンネル経由の `TerminalEvent`
配送は使わない** — そちらは render 時にしかドレインされず遅すぎる)。

`TerminalState::process_bytes`(`terminal.rs`)は `Term` ロックを**解放したあとに**このキューを
drain し、戻り値 `Vec<u8>` として返す。呼び出し側(`view.rs` の PTY reader task)は
`process_bytes` が返したバイト列を **同じ tick 内・`cx.notify()` より前に** PTY へ書き戻す。

- **配送方式**: 「レンダーフレームを待たない同期書き戻し」を選択(チャンネル越しの
  `TerminalEvent::PtyWrite` ラウンドトリップ方式は不採用)。理由: alacritty の `send_event` は
  VTE ハンドラ実行中(`Term` ロック保持中)に同期的に呼ばれるため、その場で書き込むと
  writer ロックが term ロックの中にネストする。キューに逃がし、`process_bytes` が term ロックを
  離した直後にドレインすることで、term ロックと writer ロックを同時に持つ経路を作らずに済む。
- **レイテンシ保証**: reader task は PTY からバイトが届くたびに即座に起床する
  (`flume::unbounded` の `recv_async`)。応答の書き戻しは、そのバイト列を処理した
  `process_bytes` 呼び出しの直後・同一 tick 内(render をまたがない)。GPUI の描画スケジュールに
  一切依存しない。

### 2. SGR マウスレポーティング(zellij/vim でのクリック修正)

`mouse.rs` に純粋関数を追加/修正:

- `mouse_button_report` / `scroll_report`: **SGR (`TermMode::SGR_MOUSE`) 必須**にガードを追加
  (未設定なら `None` を返す)。X10 (1 バイトエンコード、col/row 223 で破綻)は実装しない
  — zellij/vim/tmux はマウスモードと同時に SGR も有効化するため、この簡略化は実用上問題ない。
- `mouse_motion_report`(新規): ドラッグ/ホバー移動用の SGR モーション報告(+32 フラグ)。
- `mouse_reporting_active(mode, shift_held)`(新規、純粋・Window 不要): マウスレポート系
  モードが立っているか、かつ Shift が押されていないかを判定。**Shift 押下時は常にローカル選択に
  フォールバック**(標準的な端末の慣習。zellij/vim の中でもテキスト選択・コピーができる)。
- `should_report_motion(mode, button_held)`(新規、純粋): `MOUSE_MOTION` は全 move、
  `MOUSE_DRAG` はボタン押下中のみ、`MOUSE_REPORT_CLICK` 単体では move を報告しない。

`view.rs` の `on_mouse_down/up/move` を上記のディスパッチで書き換え。ローカル選択中
(`self.selecting == true`)は常にローカル選択を優先(ドラッグ中にモードが変わっても
挙動が割れないように)。ボタン登録は `on_any_mouse_down` + `MouseButton::{Left,Middle,Right}`
individually な `on_mouse_up`(gpui 0.2.2 では `on_any_mouse_up` はフルーエント API に存在せず、
`Interactivity` の命令的 API にしか無いための回避)。

### 3. スマートスクロール

`on_scroll`(`view.rs`)を書き換え: `scroll_report` の戻り値で分岐 ——
マウスレポート中(SGR)→ ホイールイベント(button 64/65)、alt screen かつ非レポート中 →
矢印キー列(`APP_CURSOR` を尊重)、それ以外 → 既存の `scroll_display`(変更なし)。

平常時(alt screen でない)のスクロールバックは元々 `scroll_display` を直接呼んでおり、
祖先要素(`src/ui/render_bottom.rs`)にも競合する `on_scroll_wheel` ハンドラは存在しないことを
確認済み — 既存の動作は壊れていない。

## 触ったファイル

- `vendor/gpui-terminal/src/event.rs` — PtyWrite キュー、doc 更新、テスト追加。
- `vendor/gpui-terminal/src/terminal.rs` — `pty_responses` フィールド、`process_bytes` の戻り値化、テスト追加。
- `vendor/gpui-terminal/src/mouse.rs` — SGR ガード、`mouse_motion_report` / `mouse_reporting_active` /
  `should_report_motion` 追加、テスト追加。
- `vendor/gpui-terminal/src/view.rs` — reader task の書き戻し配線、`on_mouse_down/up/move`/`on_scroll`
  の書き換え、`write_bytes` 共通ヘルパー追加。

## 触っていないもの(既存契約の非破壊確認)

- 既存 `[kagi]` klog 行(`src/**`)— 無変更。
- `keystroke_to_bytes` の cmd-chord ガード(`input.rs`)— 無変更。
- cmd-c コピー / cmd-v ペースト(`view.rs::on_key_down` / `src/ui/render_bottom.rs`)— 無変更。
- クリックでフォーカスする祖先ハンドラ(`render_bottom.rs`)— 無変更。

## 完了条件

- [x] `Event::PtyWrite` が PTY に書き戻される(DA1 クエリでの単体テストあり)。
- [x] SGR マウスレポート(press/release/wheel/modifiers/座標)の単体テスト。
- [x] alt screen ホイール→矢印キー変換の単体テスト。
- [x] マウスモード/Shift-override の判定ロジックの単体テスト(Window 不要な純粋関数として抽出)。
- [x] `cargo test -p gpui-terminal` 全パス(87 unit + 32 doctest)。
- [x] `cargo build` / `cargo test --workspace` / `cargo fmt --all -- --check`(root)/
      `cargo fmt -- --check`(vendor crate 単体, workspace 非メンバーのため個別実行)グリーン。
- [x] `cargo clippy --workspace` 新規警告なし(baseline 39 のまま)、`cargo clippy -p gpui-terminal` クリーン。
- [x] `grep -rE 'git2::|Repository::open' src/ui` = 0。
- [ ] GUI 目視は PM(subagent は GUI 不可)— 下記マニュアルテスト手順を参照。

## PM 向けマニュアルテスト手順

1. Kagi のターミナルタブを開く → `zellij` を起動 → 起動時にハングせず描画されること
   (根本原因1の確認)。
2. zellij のペインをクリック → フォーカスが追従すること(根本原因2)。
3. `less README.md`(または任意のフルスクリーンアプリ)の中でマウスホイールをスクロール →
   内容がスクロールすること(根本原因3、alt screen 矢印キー変換)。
4. zellij / vim の中で Shift を押しながらドラッグ → テキスト選択ができ、Cmd/Ctrl+C でコピーできること
   (Shift-override)。
5. 通常のシェルプロンプト(alt screen でない)でマウスホイール → 従来通りスクロールバック履歴が
   スクロールすること(回帰なし)。
6. 通常のシェルプロンプトで cmd-v ペーストが引き続き動作すること(回帰なし)。

## リスク

- gpui 0.2.2 の `on_any_mouse_up` フルーエント API 欠如により、ボタン別に3回 `on_mouse_up` を
  登録している(挙動は同一、コードがやや冗長)。
- X10 マウスエンコードは非対応(意図的な簡略化。SGR 非対応アプリでは黙ってレポートしない)。
- GUI 実機(特に zellij 実行時のハング解消)は subagent では検証不可 — 上記手順で PM 確認が必要。
