# ADR-0034: Zed / GPUI コンポーネント流用

- Status: Proposed
- Date: 2026-06-12
- 関連調査: docs/research/zed-gpui-reuse-research.md
- 関連 ADR: 0031(流用ポリシー), 0001(gpui), 0006(gpui-component), 0008(terminal), 0018(status bar), 0029(command registry/menubar)
- ライセンス(原文 crate 単位確認済): `crates/gpui` = Apache-2.0(LICENSE-APACHE は root へのシンボリックリンク)。`terminal`/`terminal_view`/`ui`/`project`/`git`/`git_ui`/`editor` = GPL-3.0-or-later。

## Context

Zed は gpui(Apache-2.0)とその上の機能 crate(大半 GPL-3.0-or-later)で構成される。kagi は crates.io `gpui = "0.2.2"` に依存済。Zed in-repo の gpui も version 0.2.2 で一致するが、一次資料は docs.rs/gpui/0.2.2 に固定する(ADR-0006 既定)。GPL crate のコード転写は kagi へのライセンス汚染となるため不可(ADR-0031 ゲート)。

## Decision

- **Adopt directly(既済)**: gpui core(Entity/Render/App/Context、elements、styling)。Apache-2.0 で kagi は既に依存中。新規作業なし。
- **Adopt directly(gpui 経由)**: `Action` トレイト + `actions!` マクロ、`Keymap`/`KeyBinding`/context predicate。いずれも **gpui(Apache)内**に存在し GPL 非結合。kagi の Command Registry(ADR-0029)とキーバインドの土台として正式採用。設定の永続化・マージは kagi 独自実装(Zed settings は GPL のため不可)。
- **Study only**: Panel/Dock 登録パターン(position/icon/toggle/activation_priority を持つ登録式パネル)、PaneGroup の split layout、StatusItemView の active-item 購読パターン、command palette UI、context menu component(item enum 構造)。いずれも GPL crate(workspace/ui/command_palette)のため**設計概念のみ**参照。kagi は Bottom Panel(ADR-0007/0017)・Status Bar(ADR-0018)・Command Registry(ADR-0029)・commit context menu(ADR-0020)を自前実装済。
- **Study only / コード Reject**: terminal(alacritty_terminal を wrap する GPL ラッパ)。kagi は gpui-terminal + portable-pty を採用済(ADR-0008)。基盤が alacritty で共通である確認のみ。
- **Reject**: `crates/ui`(43 GPL コンポーネント、theme/icons/menu に密結合・standalone 不可)、および `git`/`git_ui`/`editor`/`project`(GPL・内部結合特大)。UI コンポーネントは gpui-component 0.5.1(Apache-2.0、ADR-0006)で代替済。

## Consequences

- gpui(Apache)の Action/Keymap を kagi の command/keybind 基盤として明示。GPL の command_palette/settings には触れず、永続化は自前。
- GPL crate からは一切コードを転写しない汚染ゲートを ADR で固定し、subagent 指示にも明記(ADR-0006 既定の踏襲)。
- terminal / UI コンポーネントは既に Zed より適した選択肢(gpui-terminal、gpui-component)を採用済で、本 ADR は既存方針の追認。
- gpui は crates.io 0.2.2 に pin 継続し、Zed コードは設計理解専用とする(API 一次資料は docs.rs/gpui/0.2.2)。
- Panel/Dock の設計言語は将来のパネル/レイアウト・リファクタの参考として Study に留置。
