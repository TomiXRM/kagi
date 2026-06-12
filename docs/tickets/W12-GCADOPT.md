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

---

## 実装メモ(W12-GCADOPT 完了)

### 1. theme 同期関数 `sync_gpui_component_theme(cx: &mut App)`(src/ui/theme.rs)

kagi `theme()`(`0xRRGGBB` u32)→ `gpui_component::Theme::global_mut(cx).colors`(ThemeColor、Hsla)へ
**一方向 push のみ**。u32→Hsla は `Hsla::from(gpui::rgb(v))`(gpui に `Hsla: From<Rgba>` 実装あり)で
ヘルパ `to_hsla()` に集約。逆流なし(ThemeColor からは一切読まない)。`gc.mode` は `theme().dark` で
`ThemeMode::Dark/Light` を設定。

呼び出し:
- 起動時 — `run_app` 内 `gpui_component::init(cx)` の**直後**(mod.rs)。init の `sync_system_appearance` が
  システム配色で seed した後に kagi パレットで上書き。`theme::init_active()` は main.rs で init より前に
  active index を確定済みなので、起動時 sync は確定済みパレットを push する。
- テーマ切替時 — `KagiApp::set_theme`(commands.rs)で `eprintln!` 直後に呼ぶ。

#### ThemeColor 対応表(主要・push したフィールドのみ。他 ~70 は gpui-component デフォルト維持)

| ThemeColor フィールド | kagi theme() ソース |
|---|---|
| `background` | `bg_base` |
| `foreground` / `popover_foreground` | `text_main` |
| `border` / `selection` / `list_active` | `selected` |
| `muted` / `list_hover` | `surface` |
| `muted_foreground` | `text_muted` |
| `popover` | `modal` |
| `overlay` | `modal_overlay` |
| `primary` / `primary_hover` / `primary_active` / `ring` / `link` / `info` | `color_branch` |
| `primary_foreground` | `bg_base` |
| `accent` | `selected` / `accent_foreground` = `text_main` |
| `input` | `text_muted`(Checkbox 未チェック枠・Input 枠) |
| `caret` | `text_main` |
| `success` / `warning` / `danger` | `color_success` / `color_warning` / `color_blocker` |
| `list` | `bg_base` |
| `sidebar` / `sidebar_foreground` | `sidebar` / `text_main` |
| `scrollbar` | `bg_base` |
| `scrollbar_thumb` / `scrollbar_thumb_hover` | `text_muted` / `text_sub` |
| `drag_border` | `color_branch`(将来の resizable 採用用) |

### 2. Scrollbar(`gpui_component::scroll::Scrollbar::vertical`)

ヘルパ `with_vertical_scrollbar(id, &handle, list)`(mod.rs)を新設:
`div().relative().flex_1().min_h(0).flex_col().child(list).child(Scrollbar::vertical(handle))`。
Scrollbar は `position:Absolute` + `size relative(1.)` でコンテナにオーバーレイ(レイアウト非破壊)。

**適用箇所(永続化済 `UniformListScrollHandle` を持つ uniform_list 3 つ)**:
- commit list(`"commit-list-scroll"`)— 必須要件
- main-diff list(`"main-diff-list-scroll"`)
- Operation Log list(`"oplog-list-scroll"`)

**見送り(理由)**: inspector の message/files スクロール枠・sidebar は `overflow_y_scroll()` の
**element 内蔵スクロール**(永続 `ScrollHandle` を state に持たない)。`Scrollbar` は `ScrollbarHandle`
(= 永続 `ScrollHandle`/`UniformListScrollHandle`/`ListState`)を要求するため、付与には
KagiApp への新規 `ScrollHandle` フィールド追加 + 各 render 関数シグネチャへの handle スレッド +
`track_scroll` 再配線が必要。「mod.rs 最小限」の制約と非破壊原則に反するため本チケットでは見送り、
別チケット候補とする(`render_sidebar` は free fn で KagiApp state 非参照、inspector も同様)。

### 3. Checkbox(`gpui_component::checkbox::Checkbox`)

create-branch modal の `[ ]/[x] Checkout after create`(ただのテキスト div)を実 `Checkbox` に置換。
`Checkbox::new("create-branch-checkout-after").label("Checkout after create").checked(modal.checkout_after)
.on_click(cb)`。`on_click` は `Fn(&bool, &mut Window, &mut App)` 型のため `cx.listener` ではなく
`cx.entity()` を capture し `app_entity.update(cx, |this, cx| { modal.checkout_after = new; replan; notify })`
で既存 toggle + `replan_create_branch` ロジックを維持(modal.checkout_after と replan 接続は不変)。

### 検証

- `cargo test` 全パス(268 件)/ own-code clippy warning 0(残る doc_lazy_continuation は
  mod.rs:7469 の既存箇所のみ、本変更由来なし)。
- headless windowed 起動で panic なし:
  - `KAGI_THEME=xcode-light KAGI_SELECT_FIRST=1`(commit list + Scrollbar、light)
  - `KAGI_THEME=xcode-light KAGI_CREATE_BRANCH=...`(create-branch modal + 実 Checkbox、light)
  - `KAGI_BOTTOM_PANEL=1`(oplog Scrollbar、dark catppuccin)
  - `KAGI_MENU_DUMP=1` ログ回帰なし(`theme:` / `menu:` 既存フォーマット維持)。

### PM スクリーンショット確認事項

1. commit list / diff / Operation Log の右端にスクロールバーが出てドラッグでスクロールできるか
   (6 テーマで thumb 色が kagi パレットに追従しているか)。
2. create-branch ダイアログの「Checkout after create」が実チェックボックス表示で、クリックで
   on/off 切替 + plan(Current→Predicted)が追従するか。
3. Input / Tooltip が各テーマで kagi 配色になっているか(従来はシステム配色のままだった点の解消確認)。
