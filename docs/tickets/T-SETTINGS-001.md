# T-SETTINGS-001: Settings button (top-right) + Settings window (OpenLogi-style)

- Status: in-review (実装完了、PM GUI 確認待ち / branch `rearch/settings-window`)
- Group: 新機能 / settings
- 仕様の正: ADR-0080. Reference impl: OpenLogi `crates/openlogi-gui/src/windows/settings.rs`.

## 背景 / 既存(調査済み)

- 設定ストレージは既存:`src/ui/settings.rs`(`settings.json`、`read_setting`/`write_setting`、
  `SETTINGS_KEYS` 10鍵:theme / ui_zoom / lang / panel sizes / tabs 復元 / mergetool 等)。
  apply は `theme::set_theme(slug)` / `theme::set_zoom` / i18n locale / compact など既存。
- メニューバー overlay は `src/ui/commands.rs` の `MenuOverlay`(About / KeyboardShortcuts)。
  → `MenuOverlay::Settings` を追加する形が自然。
- header 描画は `render_header_slot`(`src/ui/mod.rs`)。右側メタ操作群に gear を追加。
- **gpui-component 0.5.1 に `setting` モジュールあり**(`gpui_component::setting::{Settings,
  SettingPage, SettingGroup, SettingItem, SettingField}`)+ `select` / `slider` / `switch`。
  OpenLogi と同じ widget が使える。
- 不変条件(ADR-0078):settings は repo に触れない。view から git2 を呼ばない(grep gate 0 維持)。

## スコープ

1. **Settings ボタン(右上)**:`render_header_slot` の右メタ操作群に gear(`IconName::Settings`)を追加。
   click で settings を開く。menu bar の "Settings…" と `cmd-,` でも開けるようにする。
2. **Settings view(OpenLogi 風)**:`gpui_component::setting::Settings::new(..).sidebar_width(px(210.))`
   に複数 `SettingPage`。Kagi overlay(`MenuOverlay::Settings`)としてホスト、中央 ~820×520。
   pages(MVP):
   - **Appearance**:Theme(`Select`、6テーマ)/ UI Zoom(`Slider` か stepper)/ Compact graph(`Switch`)。
   - **Language**:Interface language(`Select`:English / 日本語)。
3. **live apply + 永続化**:各コントロール変更で既存 apply(`theme::set_theme` 等)+ `write_setting` を呼び、
   ウィンドウを refresh。新規永続化層は作らない(settings.rs 再利用)。

## 完了条件(受け入れ条件)

- [ ] window 右上に gear(Settings)ボタンがあり、click で settings が開く
- [ ] `cmd-,` と menu bar "Settings…" でも開く
- [ ] settings は左 sidebar + ページ構成(OpenLogi 風、`gpui_component::setting`)
- [ ] Appearance ページ:Theme 切替が即反映 & 再起動後も保持(`write_setting("theme",..)`)
- [ ] UI Zoom 変更が即反映 & 保持
- [ ] Compact graph トグルが即反映 & 保持
- [ ] Language ページ:EN/JA 切替が即反映 & 保持(domain word は英語のまま)
- [ ] settings を閉じても repo state は不変、view は git2 を呼ばない(`grep -rE 'git2::|Repository::open' src/ui` = 0)
- [ ] `cargo test --workspace` 全パス
- [ ] 最低限のテスト(settings 値の read/write round-trip、または apply ヘルパの純粋テスト)

## 規約

- 文字列は i18n `Msg` 経由(ADR-0048。domain word/branch 名は英語)。色は `theme()` 経由。
- settings は repo に触れない。view から git2 禁止。fixture/tempdir のみで検証。
- 既存 `settings.rs` の鍵・形式・env override 優先順位を壊さない。新鍵は `SETTINGS_KEYS` に追加。

## やってはいけないこと

新しい永続化層の二重実装 / settings view から git2 呼び出し / 既存 settings.json 形式の破壊 /
theme/i18n の per-frame ホットパスにロックを入れる。

## Implementation memo

実装ブランチ: `rearch/settings-window`(base = `re-architecture` @ 095b945)。PM が merge + GUI 確認。

### 追加/変更点

- **Settings gear ボタン**(`src/ui/mod.rs` `render_header_slot` 末尾、右端の `flex_1()` の後):
  `gpui_component::IconName::Settings` の gear。click で `menu_overlay = Some(MenuOverlay::Settings)` + `cx.notify()`。
  tooltip "Settings"。Refresh(左端)/Terminal(中央右)と同じ meta 操作系列の右端に配置。
- **`MenuOverlay::Settings`**(`src/ui/commands.rs`、About/KeyboardShortcuts と同列):
  - kagi app メニューに `mi("app.settings", OpenSettings)` を追加(About と Quit の間、separator 区切り)。
  - `OpenSettings` action + `COMMANDS` に `app.settings`(label "Settings…"、`cmd-,`)+ `command_state` で常時 Enabled。
  - `register_keybindings` に `KeyBinding::new("cmd-,", OpenSettings, None)`。
  - `register_menu_actions`(mod.rs)に `menu_act!(el, OpenSettings, "app.settings")`。
  - `handle_menu_command` の `"app.settings"` → overlay をセット。
  - `render_menu_overlay` の `MenuOverlay::Settings` → `settings_view::render_settings_overlay(cx.entity(), cx)`。
- **Settings view**(新規 `src/ui/settings_view.rs`):`gpui_component::setting::Settings::new("kagi-settings").sidebar_width(px(210.))`、
  中央 820×520、scrim(`theme().bg_base` 0.55)+ click-to-dismiss(panel 内 click は `stop_propagation`)。OpenLogi と同じ
  `Settings → SettingPage → SettingGroup → SettingItem → SettingField` 構成。
  - **Appearance**:
    - Theme = `SettingField::dropdown`(6 slug↔name)。get=`theme::theme().slug`、set=`KagiApp::set_theme(slug, cx)`
      (= `theme::set_active` 永続化 + `sync_gpui_component_theme` + terminal 再設定 + menu ✓ 再構築 + notify)。
    - UI Zoom = `SettingField::number_input`(min `ZOOM_MIN` 0.7 / max `ZOOM_MAX` 1.5 / step `ZOOM_STEP` 0.1)。
      get=`theme::zoom()`、set=`theme::set_zoom`(clamp+永続化)+ entity `cx.notify()`。
    - Compact graph = `SettingField::switch`。get=`theme::compact_graph()`、set=entity `graph_compact` 更新 +
      `theme::set_compact_graph`(永続化)+ notify。
  - **Language**:Interface language = `SettingField::dropdown`(en/ja)。get=`i18n::lang().slug()`、
    set=`KagiApp::set_lang(Lang, cx)`(= `i18n::set_lang` 永続化 + menu ✓ 再構築 + notify)。
- **永続化**:既存 `theme.rs` の `write_setting`/`read_setting` を再利用。新規に `theme::{compact_graph, set_compact_graph,
  init_compact_graph}`(zoom と同じ atomic パターン)+ `SETTINGS_KEYS` に `"graph_compact"` 追加(10→11)。
  `main.rs` の起動時 init 列に `init_compact_graph()`。`KagiApp` 両コンストラクタの `graph_compact` を
  `theme::compact_graph()` で seed。既存 toolbar の compact トグルも `set_compact_graph` で永続化するよう更新。
- **i18n**:`Msg::Settings*`(Title/Appearance/Language/Theme(+Desc)/Zoom(+Desc)/Compact(+Desc)/InterfaceLang(+Desc))
  を EN/JA 両アームに追加。domain word "graph"/"Git" は英語のまま(ADR-0048)。

### 使った gpui_component::setting API(0.5.1、確認済み)

`Settings::new(id).sidebar_width(px).page(SettingPage)`(`Settings` は `RenderOnce`、内部 state は `window.use_keyed_state` 管理)。
`SettingPage::new(title).default_open(bool).group(SettingGroup)`。`SettingGroup::new().item(SettingItem)`。
`SettingItem::new(title, field).description(text)`。`SettingField::switch(get,set)` / `::dropdown(Vec<(value,label)>, get, set)`
/ `::number_input(NumberFieldOptions{min,max,step}, get, set)`。各 get は `Fn(&App)->T`、set は `Fn(T, &mut App)`。
slider は使わず number_input stepper を採用(0.5.1 の setting field に Slider バリアントは無く、Switch/Checkbox/NumberInput/
Input/Dropdown/Element のみ。Zoom は離散 0.1 step なので number stepper が自然)。

### live-apply の repaint

set クロージャは `&mut App` を受け取るので `Entity<KagiApp>`(`cx.entity()`)を capture し `entity.update(cx, |app, cx| { ...; cx.notify() })`
経由で既存 apply パス(theme/lang は KagiApp メソッド、zoom/compact は global + entity notify)を実行 → 全 `theme()`/`Msg::t()` 読み取り
パスが即再描画。

### テスト

`src/ui/theme.rs` に 2 件追加:`graph_compact_in_settings_keys`(新鍵が `SETTINGS_KEYS` にある=write_setting で round-trip 保持)、
`theme_slug_index_roundtrip`(Theme Select が使う slug↔index が全テーマで lossless)。既存 i18n の網羅 match は新 `Msg` 追加で
コンパイル時に両言語を強制。

### 結果

- `cargo test --workspace`:全 suite green(0 failed)。
- `grep -rnE 'git2::|Repository::open' src/ui | wc -l` = 0(settings は repo 非依存)。
- 不確実点:`gpui_component::setting` の `Settings` は `RenderOnce`(child として追加で OK、build 時 `Window` 不要)。
  内部 search box の placeholder は `rust_i18n::t!` だが未ロードでもキー文字列を返すだけで panic しない(GUI で要目視)。
  number stepper の見た目/Select の開閉は headless では確認不可 → PM GUI 確認対象。
