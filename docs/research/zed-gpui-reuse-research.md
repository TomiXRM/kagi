# Zed / GPUI 流用調査

- 調査日: 2026-06-12 / 調査者: research subagent
- 対象: `zed-industries/zed` shallow clone (`/tmp/kagi-research/zed`)
- ライセンス(原文 crate 単位確認済):
  - `crates/gpui` → **Apache-2.0**(`crates/gpui/Cargo.toml` `license = "Apache-2.0"`、`LICENSE-APACHE` は root へのシンボリックリンク `-> ../../LICENSE-APACHE`)
  - `crates/terminal` / `terminal_view` / `ui` / `project` / `git` / `git_ui` / `editor` → **GPL-3.0-or-later**(各 `Cargo.toml` で確認、`LICENSE-GPL` 同梱)
  - root に `LICENSE-APACHE` と `LICENSE-GPL` が併存(crate ごとに混在)
- 関連: ADR-0034(zed/gpui component reuse)、ADR-0006(既存)、ADR-0031

## バージョン非互換リスクの結論

- Zed in-repo の `crates/gpui` は **version = 0.2.2**。kagi が使う crates.io `gpui = "0.2.2"` と**一致**。
- ADR-0006 の注記(gpui-component 公式サイトは git main 版 gpui 準拠で API がズレる)は依然有効だが、**gpui 本体の 0.2.2 という点では Zed リポジトリと crates.io が揃っている**。ただし shallow clone の main は 0.2.2 表記でも内部に未公開差分があり得るため、**一次資料は引き続き docs.rs/gpui/0.2.2**(コードを読むのは設計理解のみ)。

## Apache / GPL 境界(最重要ゲート)

| 領域 | crate | ライセンス | 流用可否 |
|---|---|---|---|
| UI フレームワーク core(Entity/Render/App/Context) | gpui | Apache-2.0 | コード参照可(ただし kagi は既に crates.io gpui に依存済) |
| Action トレイト + `actions!` マクロ | gpui | Apache-2.0 | 概念・API として利用可(gpui 経由) |
| Keymap / KeyBinding / context predicate | gpui | Apache-2.0 | 利用可(gpui 経由) |
| elements / styling | gpui | Apache-2.0 | 利用可(gpui 経由) |
| Panel / Dock / PaneGroup / StatusBar | workspace | GPL-3.0+ | **コード不可、パターンのみ** |
| Terminal | terminal/terminal_view | GPL-3.0+ | **コード不可** |
| Command palette UI | command_palette | GPL-3.0+ | **コード不可、パターンのみ** |
| UI コンポーネント(Button 等 43 種) | ui | GPL-3.0+ | **コード不可** |
| git / git_ui / editor / project | 各 | GPL-3.0+ | **コード不可** |

**ゲート**: GPL crate のコード転写は kagi(非 GPL 配布想定)に**ライセンス汚染**を持ち込む。設計パターン参照のみ可(ADR-0006 既定を踏襲)。Apache の gpui は既に依存済なので「流用」というより「正しく使う」話。

## 観点ごとの findings

### 1. gpui crate(Apache-2.0)

- 場所: `/tmp/kagi-research/zed/crates/gpui/`(`publish = true`、内部 zed crate へ非依存。依存は collections / gpui_macros / gpui_util 等の周辺 crate と外部のみ → **standalone publishable**)
- core: `Entity<T>` / `WeakEntity<T>` / `AnyView`、`Render` トレイト、`App` + `Context<T>`、`IntoElement` / `Styled`、`Action` + `actions!`、`Keymap` / `KeyBinding`、`Window`、`EventEmitter<E>`。
- 評価: kagi は既にこれらを利用中。**追加の流用作業は不要**。設計理解の一次資料としては docs.rs/gpui/0.2.2 を優先。

### 2. Layout パターン(dock / pane / pane_group)

- ファイル: `crates/workspace/src/dock.rs`(約 56KB)、`pane_group.rs`(約 53KB)、`pane.rs`(約 354KB)
- `Panel` トレイト(dock.rs L36-96): `Focusable + EventEmitter<PanelEvent> + Render` を要求し、`position/default_size/icon/toggle_action/activation_priority` を提供。`PanelEvent`(ZoomIn/Out/Activate/Close)。
- `PaneGroup`: `Member { Pane | Axis }` の再帰ツリーで H/V split。永続化は persistence.rs。
- 結合: pane.rs は約 20 の内部 crate(workspace/project/settings 等)に依存。**GPL 結合が強い**。
- 評価: kagi は Bottom Panel(ADR-0007/0017)・repo tabs(ADR-0027)を自前実装済。**Panel トレイトの「登録式パネル + position/icon/toggle メタ」という設計パターンのみ参考**。コードは不可。

### 3. Panel / Dock / Status bar

- `StatusItemView` トレイト(`crates/workspace/src/status_bar.rs` L42-59): `set_active_pane_item()` コールバック + `hide_setting()` + Render。左右に runtime 登録。
- 評価: kagi の Status Bar(ADR-0018)は実装済。「active item 変化を購読して status item を更新する」パターンのみ概念参考。

### 4. Terminal

- `crates/terminal`(GPL): **alacritty_terminal**(Apache-2.0、external)を wrap。pty は platform 別(`portable-pty` は terminal/Cargo.toml に直接記載なく alacritty/vte 経由の可能性)。view 層 `terminal_view` は workspace/editor に依存(GPL)。
- 評価: kagi は既に **gpui-terminal crate + portable-pty**(ADR-0008)を採用済。Zed terminal は GPL ラッパで流用不可。「alacritty_terminal を基盤にする」点が共通している確認のみ。**Study only / 既存方針維持**。

### 5. Command palette / context menu

- Action 系(**Apache/gpui**): `crates/gpui/src/action.rs` の `Action` トレイト + `actions!` マクロ(namespace 付き unit struct)。GPL 非結合。
- Keymap dispatch(**Apache/gpui**): `crates/gpui/src/keymap.rs`(約 857 行)。`Keymap`(TypeId→binding)、context predicate、`window.available_actions(cx)`。
- Command palette **UI**(GPL): `crates/command_palette` は picker + ui crate に依存。`GlobalCommandPaletteInterceptor` で hook。
- context menu(GPL): `crates/ui/src/components/context_menu.rs` の `ContextMenu`(builder, focus 管理)、`ContextMenuItem { Separator/Header/Label/Entry/Submenu }`。
- 評価: kagi の Command Registry + ネイティブメニュー(ADR-0029)、commit context menu(ADR-0020)は実装済。**Action/Keymap は gpui(Apache)由来なので既に使える**。palette UI・context menu component は GPL なので**パターンのみ**。

### 6. Keyboard shortcut

- dispatch は **gpui(Apache)の keymap.rs** に存在(binding format / context predicate / keystroke parse)。ユーザー keymap のマージ・設定 UI は zed settings(GPL)。
- 評価: kagi は gpui の keymap を直接使える。設定永続化・マージは kagi 独自で実装(GPL settings は不可)。

### 7. UI component crate(crates/ui, GPL)

- 43 コンポーネント(Button/Label/Icon/List/ListItem/TabBar/ContextMenu/Tooltip/DataTable/TreeViewItem/Modal 等)。theme/icons/menu/component 等の内部 crate に密結合、**standalone 不可・GPL**。
- 評価: **全面流用不可**。kagi は gpui-component 0.5.1(Apache-2.0、ADR-0006)を採用済でこの穴は既に埋まっている。Zed ui は設計参考のみ。

## 候補テーブル

| 候補 | 分類 | 理由 | コスト | リスク |
|---|---|---|---|---|
| gpui core(Entity/Render/App) | **Adopt directly**(既済) | Apache・crates.io 0.2.2 依存済。新規作業なし | 0 | 低 |
| Action トレイト + keymap dispatch | **Adopt directly**(gpui 経由) | Apache・gpui 内。kagi Command Registry の土台 | 小 | 低 |
| Panel/Dock 登録パターン | **Study only** | GPL コード不可。登録式パネル + メタの設計概念のみ | 小 | 低 |
| PaneGroup split layout | **Study only** | GPL・workspace 密結合。kagi は自前パネルで足りる | 小 | 低 |
| StatusItemView 購読パターン | **Study only** | GPL。概念のみ(ADR-0018 実装済) | 小 | 低 |
| Terminal(alacritty wrap) | **Study only / Reject(コード)** | GPL ラッパ。kagi は gpui-terminal 採用済(ADR-0008) | 0 | 低 |
| Command palette UI | **Study only** | GPL。kagi は ADR-0029 で実装済 | 小 | 低 |
| context menu component | **Study only** | GPL ui crate。item enum 構造の設計参考のみ | 小 | 低 |
| crates/ui(43 components) | **Reject** | GPL・密結合・standalone 不可。gpui-component で代替済 | 0 | 低 |
| Zed git / git_ui / editor / project | **Reject** | GPL・内部結合特大(ADR-0006 既定) | 0 | 高 |

## kagi への具体的提案

1. **gpui の Action/Keymap を土台として明示**: kagi の Command Registry(ADR-0029)とキーバインドは gpui(Apache)の `Action` + `Keymap` を基盤にする方針を再確認。GPL の command_palette/settings には手を出さず、設定永続化・マージは自前。
2. **Panel/Dock は概念のみ参照**: 「position/icon/toggle/activation_priority を持つ登録式パネル」という設計言語を、kagi の Bottom Panel / tabs / Navigator(ADR-0007/0014/0027)の将来リファクタの参考にする。コードコピーは GPL のため不可。
3. **既存方針の追認**: terminal(gpui-terminal+portable-pty, ADR-0008)・UI コンポーネント(gpui-component, ADR-0006)は Zed より良い選択肢が既に採用済。Zed の該当 crate は GPL のため流用しない方針を維持。
4. **GPL 汚染ゲートの徹底**: gpui(Apache)以外の Zed crate からは**一切コードを転写しない**。subagent への指示にも明記(ADR-0006 既定の踏襲、ADR-0031 で手順化)。
5. **gpui バージョンは crates.io 0.2.2 に pin 継続**: Zed main の gpui も 0.2.2 表記だが一次資料は docs.rs/gpui/0.2.2(ADR-0001/0006)。Zed コードは設計理解専用。

## 確認できなかった事項

- Zed main の gpui 0.2.2 表記と crates.io 0.2.2 の内部差分(コミット単位)の厳密一致。shallow clone のため履歴照合は未実施。**一次資料は docs.rs に固定するため実害なし**。
- `portable-pty` が Zed terminal の直接依存か alacritty/vte 経由かの最終確認(kagi は自前で portable-pty を直接採用済のため影響なし)。
