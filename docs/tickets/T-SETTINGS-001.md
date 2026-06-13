# T-SETTINGS-001: Settings button (top-right) + Settings window (OpenLogi-style)

- Status: todo
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

(担当 agent が完了時に追記)
