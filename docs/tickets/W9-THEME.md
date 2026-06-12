# W9-THEME: カラーテーマ 6 種 + メニュー切替(ユーザー要望)

- Status: done(headless 検証済み・GUI スクリーンショットは PM 確認待ち)
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

- [~] 6 テーマがメニューから切替でき、graph/sidebar/inspector/terminal/diff まで即時反映
      (メニュー登録・dispatch・set_theme 実装済み + headless ログ確認。即時反映の目視は PM)
- [~] default(Catppuccin)の見た目が現行と一致(回帰ゼロ — 値はバイト厳密移植 + テスト担保。PM 比較)
- [~] light テーマで文字が読める(意味名 alpha 調整済み — コントラストの目視は PM)
- [x] 再起動でテーマが維持される(settings.json read/write round-trip 確認済み)
- [x] KAGI_THEME headless 検証 + 既存ログ回帰なし
- [x] `cargo test` 全パス(228)+ own-code warning 0
- [x] 実装メモ(パレット出典含む)を本ファイル末尾に追記

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

## 実装メモ(W9-THEME 完了)

### 色 const インベントリ(置換前)
ハードコードされていた `const u32` / 直書き hex の分散先:
- `mod.rs`(正・パレット定義元)+ `inspector.rs` / `sidebar.rs` / `context_menu.rs` に
  「mirrors mod.rs」のローカル const コピー。`tabs.rs` / `commands.rs` は `use super::{BG_BASE,…}`
  で mod.rs の const を借用。
- 直書き hex: `mod.rs`(mauve `0xcba6f7` cherry-pick、`0xf38ba8` 削除✕、`0x6c7086u32` dir、
  `0x2a2a3a` wip-bg、`0x3a1c1c` conflict 行)、`sidebar.rs`(`0x1e1e2e` filter bg、`0xf38ba8` hover✕)、
  `inspector.rs`(mauve/sky action button)、`context_menu.rs`(`0x8f5360` disabled-dangerous)、
  `commit_panel.rs`(change-kind 5色 + conflict)。
- HSLA 直計算: `graph_view.rs::lane_color`(6 hue)、`avatar.rs::avatar_color`(FNV-1a→12 hue、s=0.70/l=0.60)。
- terminal: `terminal.rs` の `ColorPalette::builder()`(bg/fg/cursor + 16 ansi + selection rgba)。
- diff highlight: `mod.rs::highlight_diff_rows` の `HighlightTheme::default_dark()` 固定。

### Theme struct(`src/ui/theme.rs`)フィールド
slug / name / dark + 背景(bg_base, surface, selected, panel, sidebar, modal, modal_overlay)+
text(main, sub, muted, label)+ ref(head, branch, remote, tag)+ status(success, warning,
blocker, blocker_muted)+ diff(added_bg, removed_bg, hunk)+ change-kind 6(added, modified,
deleted, renamed, typechange, dir)+ accent / accent_alt + lane_hsl[6](hue,sat,light)+
avatar_sat / avatar_light + terminal 19(bg/fg/cursor + 16 ansi)+ term_selection rgba。
アクセスは `static ACTIVE: AtomicUsize` + `pub fn theme() -> &'static Theme` +
`pub static THEMES: &[Theme]`(先頭 = Catppuccin = default)。

### 6 テーマのパレット出典
1. **Catppuccin Mocha**(default, dark): 置換前 const の**バイト厳密移植**。lane HSL も
   旧 `graph_view::HUES` を、avatar s/l(0.70/0.60)・terminal 値も旧 builder を厳密に再現
   (`default_is_catppuccin_exact` テストで担保 → 見た目回帰ゼロ)。
2. **Xcode Dark**(dark): Apple Xcode "Default (Dark)" 公知パレット(editor bg #292a30、
   keyword pink #ff7ab2、string #ff8170、type teal #6bdfff、number #d9c97c)。
3. **Xcode Light**(light): Apple Xcode "Default (Light)"(bg #ffffff、keyword #9b2393、
   string #c41a16、type #0b4f79、number #1c00cf、comment #5d6c79)。
4. **One Dark**(dark): Atom/VSCode "One Dark"(bg #282c34、fg #abb2bf、red #e06c75、
   green #98c379、yellow #e5c07b、blue #61afef、purple #c678dd、cyan #56b6c2)。
5. **One Light**(light): Atom/VSCode "One Light"(bg #fafafa、fg #383a42、red #e45649、
   green #50a14f、amber #c18401、blue #4078f2、purple #a626a4、cyan #0184bc)。
6. **Monokai (Warm Hybrid)**(dark): `docs/research/reference/tomixrm-warm-hybrid.json`(MIT)から
   抽出。editor.background #2f2b31 / foreground #c8c8c8 / cursor=accent #ff9940、terminal.ansi*
   16色をそのまま、tokenColors(keyword #ff668c、string #f4cd62、function #9ed06c/#a4d671、
   type #7bdae7、magenta #b39af5、cyan #7dd7e6)を ref/accent にマップ。元 JSON は light/dark
   混在の hybrid なので **dark 系キー(editor/terminal)のみ採用**し統一した dark テーマに整形。

### 主要変更点(PM merge 用)
- `src/ui/theme.rs`(新規): struct + 6 themes + `theme()`/`set_active()`/`init_active()` +
  settings.json 永続化(手書き JSON・serde 無し・KAGI_LOG_DIR 対応)+ 単体テスト 6 件。
- `src/ui/mod.rs`: const ブロック撤去 → `use theme::theme;`。全 `rgb(CONST)`→`rgb(theme().field)`
  機械置換(BSD sed の word-boundary `[[:<:]]`)。diff highlight を `theme().dark` で dark/light 切替。
  `register_menu_actions` に theme 6 action を追加。
- `src/ui/commands.rs`: theme 6 action + COMMANDS 6 entry(always Enabled)+ `theme_submenu()`
  (active に "✓ " prefix)を View メニュー末尾に submenu 追加 + `handle_menu_command` の theme 分岐 +
  `set_theme()`(set_active → 全 terminal session に `update_config` live 適用 → `cx.set_menus` 再構築 → notify)。
- `src/main.rs`: `fn main` 冒頭で `crate::ui::theme::init_active()`(KAGI_THEME→settings→default)。
- terminal.rs: `build_color_palette()` / `build_terminal_config()` を theme 経由に。
  graph_view.rs/avatar.rs/commit_panel.rs/inspector.rs/sidebar.rs/context_menu.rs/tabs.rs も theme 経由。

### 検証結果
- `cargo test`: 228 passed / 0 failed(EXIT 0)。own-code warning 0(`block v0.1.6` の依存警告のみ)。
- grep 残骸: `0x1e1e2e|0x313244|0x45475a|0xcdd6f4`(theme.rs 除く)= 0、`rgb(0x……)`(同)= 0。
- headless(fixture): KAGI_THEME 全 6 slug + bogus 起動 → `[kagi] theme: <slug> dark=<bool>` ログ・
  panic 0。settings.json read(env 無し時)/ KAGI_THEME 優先 / bogus→catppuccin fallback を確認。
  既存ログ(repo/commits=9/branches 等)回帰なし。KAGI_MENU_DUMP に theme.* 6 件 enabled 表示。

### 未解決(PM スクリーンショット確認事項)
- 実 GUI でのメニュー切替即時反映(graph/sidebar/inspector/terminal/diff)と "✓" 移動。
- light 2テーマ(xcode-light / one-light)のコントラスト・半透明 overlay/selection/toast の見え方
  (alpha は意味名フィールドで調整済みだが目視確認推奨)。
- terminal の live `update_config` 適用(走行中 session への色反映)。
