# W12-GCADOPT: gpui-component 採用第1弾(監査 TOP 推奨の実装)

- Status: queued(W10-TOOLBAR / W11-AVATAR merge 後に着手 — mod.rs 競合回避)
- 担当: worktree agent(Opus)
- 関連: docs/research/gpui-component-audit.md(判定の正)/ ADR-0036(theme)

## スコープ(監査の高優先 3 + 前提 1)

1. **theme 同期関数(前提)**: `sync_gpui_component_theme()` — kagi の `theme()` 値を
   `gpui_component::Theme::global_mut(cx).colors` の対応フィールドへ push。
   起動時(settings 読込後)と View>Theme 切替時に呼ぶ。これで採用済み Input/Tooltip も
   kagi パレットに揃う(現状はシステム配色のまま)
2. **Scrollbar**: commit list の `UniformListScrollHandle` に `Scrollbar::vertical` を付与
   (監査確認済み: `impl ScrollbarHandle for UniformListScrollHandle`)。
   ついでに inspector の message/files スクロール枠・sidebar にも適用可否を確認して付与
3. **Checkbox**: create-branch dialog の「[ ] Checkout after create」(現状テキスト)を
   `Checkbox` に置換
4. **notification 移行は本チケットでは様子見**(自前 toast は W3 で安定稼働中。
   監査で再入懸念が誤りと判明したのは記録済み — 移行は利得が出た時に別チケット)

## 完了条件

- [ ] テーマ切替で Input / Tooltip / Scrollbar / Checkbox が kagi パレットに追従(6テーマ、PM 確認)
- [ ] commit list にスクロールバーが表示され、ドラッグでスクロールできる
- [ ] Checkout after create が実チェックボックスで動作(plan への反映回帰なし)
- [ ] 既存 headless ログ回帰なし / `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/theme.rs`(sync 関数)/ `src/ui/commands.rs`(切替時呼び出し)/
  `src/ui/mod.rs`(scrollbar・checkbox 配線の最小限)/ `src/ui/inspector.rs` / `src/ui/sidebar.rs`
- `docs/tickets/W12-GCADOPT.md`

## 触ってはいけないファイル

- `src/git/` / `vendor/` / `tests/*` / `scripts/*` / `Cargo.toml`

## リスク

- ThemeColor は 103 フィールド — 対応表は監査 doc を正とし、未対応フィールドは
  gpui-component デフォルトのままで可(全埋め不要)
- `gpui_component::init` の system appearance 同期に**上書きされない**順序にする(監査の注意点)
- 文字列切り詰めは chars() ベース / force 系コード追加禁止(全体規約)
