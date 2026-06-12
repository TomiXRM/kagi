# W9-THEME: カラーテーマ 6 種 + メニュー切替(ユーザー要望)

- Status: in-progress
- 担当: worktree agent(Opus、調査込み)
- 関連 ADR: 0036 / 0029(Command Registry)

## スコープ

1. **調査**: 既存の色 const 全数インベントリ(mod.rs / inspector.rs / sidebar.rs /
   context_menu.rs / commands.rs / tabs.rs / terminal.rs / graph_view.rs / avatar.rs / detail_panel.rs 等)
   と、`docs/research/reference/tomixrm-warm-hybrid.json`(VSCode テーマ、MIT)からの
   パレット抽出方針を実装メモに記録
2. **`src/ui/theme.rs`**(ADR-0036): Theme struct + `theme()` global accessor + THEMES 6種
   - Catppuccin Mocha(現行値の厳密移植 = default、見た目回帰ゼロ)
   - Xcode Dark / Xcode Light / One Dark / One Light(各テーマの公知のパレットから誠実に作成)
   - Monokai(tomixrm Warm Hybrid の colors/tokenColors から抽出。accent=#ff9940 系)
3. **全モジュールの const 置換**: `rgb(CONST)` → `rgb(theme().field)`。重複 const 削除。
   lane_color / avatar_color も theme 経由に
4. **メニュー**: View > Theme サブメニュー(`theme.catppuccin` 等 6 command、Registry 経由)。
   アクティブに "✓ " prefix、切替時に set_menus 再呼び出し + cx.notify
5. **連動**: terminal(update_config で live 適用 + 新 session)/ syntax highlight(dark/light)
6. **永続化**: `~/.kagi/settings.json`(手書き JSON、oplog 方式、KAGI_LOG_DIR 対応)。
   起動読込・切替保存
7. **headless**: `KAGI_THEME=<slug>` + `[kagi] theme: <slug> dark=<bool>` ログ

## 完了条件

- [ ] 6 テーマがメニューから切替でき、graph/sidebar/inspector/terminal/diff まで即時反映
- [ ] default(Catppuccin)の見た目が現行と一致(回帰ゼロ — PM スクリーンショット比較)
- [ ] light テーマで文字が読める(コントラスト破綻なし — PM 確認)
- [ ] 再起動でテーマが維持される
- [ ] KAGI_THEME headless 検証 + 既存ログ回帰なし
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモ(パレット出典含む)を本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/`(theme.rs 新規 + 各モジュールの色置換)/ `src/main.rs`(KAGI_THEME・settings 読込)
- `src/git/oplog.rs` は触らない(settings は別ファイル `src/ui/settings.rs` か theme.rs 内に)
- `docs/tickets/W9-THEME.md`

## 触ってはいけないファイル

- `src/git/` / `vendor/`(terminal 色は既存 builder API で渡す)/ `tests/*` / `scripts/*` / `Cargo.toml`

## テスト方法

1. `cargo test`(exit code 直接確認)
2. fixture で KAGI_THEME 各 slug 起動 → ログ + クラッシュなし。スクリーンショットは PM
3. 検証は fixture / tempdir のみ

## リスク

- 置換漏れ(片方だけ旧色)— `grep -rn "0x1e1e2e\|0x313244" src/ui` 等で残骸ゼロを確認すること
- light テーマでの半透明 overlay / selection / toast の見え方 — dark 前提の alpha を意味名で調整
- Catppuccin の移植ミス = 全画面の見た目回帰。値はコピペで厳密に
- 文字列切り詰めは chars() ベース / force 系コード追加禁止(全体規約)
