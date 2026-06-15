# ADR-0085: メニュー定義の単一ソース化（macOS ネイティブ / Linux 自前描画の統合）

- Status: Accepted(2026-06-15、ユーザー依頼「Ubuntu のメニューバーが機能しない／Mac と内容が違う。OS ごとに別実装になっている。定義を一本化したい」)
- Date: 2026-06-15
- Builds on: ADR-0029(Command Registry とメニューバー)、ADR-0036(テーマ)、ADR-0048(i18n)

## Context

ADR-0029 で **Command Registry**(`COMMANDS` スライス + `command_state` + 1:1 の gpui Action)を
「処理・状態・ラベルの単一の正準」とした。しかし **メニューの“構造”**(どの id を、どのセクションに、
どの順で、区切り/サブメニューをどう置くか)は依然として **2 か所に手書き重複** している:

- `commands.rs: build_menus() -> Vec<Menu>` … macOS ネイティブメニュー(`cx.set_menus`)用。
- `mod.rs: PLATFORM_MENUS: &[PlatformMenuSection]` … Linux/FreeBSD の自前描画ドロップダウン用。

この二重定義がドリフトし、ユーザー報告の症状になった:

1. **内容差**: Linux 側は `Edit` メニュー(Undo/Redo/Cut/Copy/Paste/Select All)が欠落するなど、
   macOS と項目がズレる。構造を 2 回書いているので必然的にズレる。
2. **機能不全(別バグ)**: Linux ドロップダウンのパネルに `.occlude()` が無く、項目クリックの
   mouse-down が背後の dismiss レイヤーへ貫通してメニューが閉じ、`on_click` が完成しない。
   → 本 ADR とは独立に `.occlude()` を追加して修正済み(`context_menu.rs` と同じ既知対策)。

### なぜ OS ごとに分かれるのか

GPUI の `cx.set_menus(Vec<Menu>)` は **macOS のネイティブ NSMenu 専用**。Linux/Windows には
OS 共通のアプリメニューバーが無く、`set_menus` は表示上 no-op。よって Linux ではタイトルバー内に
**自前でメニューを描画**する必要がある。「描画方法が OS で異なる」こと自体は不可避。

### Zed の前例

Zed はメニュー構造を `app_menus() -> Vec<Menu>` に **一度だけ** 定義し、macOS は `set_menus` へ、
Linux/Windows は `title_bar` クレートの `ApplicationMenu` が **同じ定義から** ポップアップを構築する。
全項目を gpui Action へディスパッチするので、ラベル・項目がズレない。本 ADR はこの方針に倣う。

## Decision

### 1. メニュー構造を宣言的な単一テーブルにする(正準)

`commands.rs` に、レイアウトの唯一の正準となる宣言的ツリーを置く。データのみ(挙動は ADR-0029 の
registry / `handle_menu_command` のまま)。

```rust
/// メニュー1項目。レイアウトの正準は MENU_BAR(下記)。
pub enum MenuNode {
    /// Command Registry の id。ラベル/keystroke/state は registry から引く。
    Command(&'static str),                 // 例: "file.newTab"
    Separator,
    /// 動的サブメニュー(✓ は現在値から生成)。macOS は入れ子 Menu、Linux は
    /// パネル内にインライン展開(現状の挙動を維持)。
    Submenu(DynSubmenu),                    // Theme | Language
    /// OS 標準 Edit 項目。macOS は os_action(レスポンダチェーンが処理)。
    /// Linux ではレスポンダチェーンが無く Ctrl+C/V/X/Z が gpui 既定で効くため、
    /// メニュー項目としては **描画しない**(下記 §3)。
    OsEdit(OsEditItem),                     // Undo/Redo/Cut/Copy/Paste/SelectAll
}

pub struct MenuSection {
    pub label: &'static str,                // "File"
    pub items: &'static [MenuNode],
    /// macOS 専用セクション(レスポンダチェーン前提)。Linux は丸ごとスキップ。
    pub mac_only: bool,
}

/// メニューバー全体の唯一の正準。build_menus()(mac)と Linux ドロップダウンの両方がこれを読む。
pub const MENU_BAR: &[MenuSection] = &[ /* kagi, File, Edit(mac_only), View, Repository,
                                           Branch, Commit, Window, Help */ ];
```

- `DynSubmenu` / `OsEditItem` は小さな enum。`theme_submenu()` / `lang_submenu()` は維持し、
  `DynSubmenu` から呼ぶ。
- 既存の `PlatformMenuSection` / `PlatformMenuEntry` / `PLATFORM_MENUS`(mod.rs)は **削除**。

### 2. macOS: build_menus() を MENU_BAR の純関数にする

- `build_menus()` は `MENU_BAR` を走査して `Vec<Menu>` を生成するだけにする。
  - `Command(id)` → `MenuItem::action(label, action_for_id(id))`。
  - `Separator` → `MenuItem::separator()`。
  - `Submenu(Theme|Language)` → `theme_submenu()` / `lang_submenu()`(入れ子・✓ 付き、従来通り)。
  - `OsEdit(kind)` → `MenuItem::os_action(...)`(従来通り)。
- **id → gpui Action の対応は 1 か所に集約**する。`fn action_menu_item(id: &str) -> MenuItem` の
  単一 match(現在 `build_menus()` 内に散在しているペアリングを移すだけ)。registry に id があるのに
  match に無い場合に気づけるよう、未知 id は `debug_assert!` で落とす。
- `set_menus` は全プラットフォームで呼ばれ続ける(Linux では表示 no-op、害なし)。

### 3. Linux/FreeBSD: 同じ MENU_BAR から自前描画

- `render_platform_titlebar` のヘッダ生成は `MENU_BAR.iter().filter(|s| !s.mac_only)` を走査。
- `render_platform_menu_dropdown` は対応セクションの `items` を走査:
  - `Command(id)` → 現状どおり registry からラベル/keystroke/state を引いてクリック行を生成し、
    `handle_menu_command(id)` を呼ぶ。
  - `Separator` → 区切り。
  - `Submenu(Theme|Language)` → パネルにネスト機能が無いため **インライン展開**(各 theme/lang を
    行として並べ、現在値に ✓)。= 現状の View 配下挙動を維持。
  - `OsEdit` → 描画しない(`mac_only` セクションごとスキップされるので通常到達しない)。
- パネルの `.occlude()`(クリック貫通バグ対策)は維持。
- セクション index と `mac_only` フィルタの整合に注意(ヘッダとドロップダウンで同じ filtered 列を使う)。
  ドロップダウンの left オフセットも filtered index ベースにする。

### 4. Edit メニューの扱い(意図的な OS 差として明文化)

- `Edit` セクションは `mac_only = true`。macOS のみ OS 標準 Edit メニューを出す(レスポンダチェーンが
  Cut/Copy/Paste/Undo/Redo/Select All を処理)。
- Linux では Ctrl+C/V/X/Z 等が gpui 既定で入力欄/ターミナルに効くため、**非機能な Edit メニューを
  あえて出さない**。これは「事故的ドリフト」ではなく「文書化された意図的 OS 差」とする。
- 将来 Linux にも機能する Edit メニューを出す場合は、focus 中の要素へ edit action をディスパッチする
  follow-up が必要(本 ADR の範囲外)。

## Consequences

- メニュー構造の正準が **1 か所(`MENU_BAR`)** になり、mac/Linux のドリフトが構造的に発生しなくなる。
  項目追加は 1 か所の編集で両 OS に反映。
- mac/Linux の差は **`mac_only`(= Edit メニュー)と動的サブメニューの展開方式のみ**に限定され、
  いずれもコード上明示。それ以外は完全一致。
- `action_menu_item` の単一 match が id→Action の唯一の対応表になり、registry と二重管理しない。
- リスク: `MENU_BAR` に列挙した id が registry/`handle_menu_command`/`action_menu_item` のどれかで
  欠けると不整合。`debug_assert!` と、可能なら「`MENU_BAR` の全 `Command` id が `command(id).is_some()`」
  を検証する単体テストでガードする。
- 既存の keystroke 表示・disabled 灰色化(dispatch tree 方式・ADR-0029)・`handle_menu_command` の
  挙動は不変。`.occlude()` 修正も不変。
