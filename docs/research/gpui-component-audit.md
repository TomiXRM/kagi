# gpui-component 0.5.1 コンポーネント棚卸し + kagi 置換判定

- Status: Research (実装は別チケット。本書は採否判定のみ)
- Date: 2026-06-13
- 担当: worktree 調査 agent
- 一次資料: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.5.1/src/`(ソース直読。README/公式サイトは ADR-0006 の通り git-main 版 gpui 準拠でズレるため不採用)
- 関連 ADR: 0006(段階導入)/ 0031(流用ポリシー・Apache-2.0 ゲート通過)/ 0034(Zed/gpui)/ 0036(カラーテーマ機構)
- 関連チケット: W9-THEME(自前 theme 導入中)/ W10-TOOLBAR(Finder 風ツールバー)

---

## 0. 最重要所見 — Theme 整合性(W9 の `theme()` と gpui-component `ActiveTheme` は二重化しない)

W9-THEME は「自前 `src/ui/theme.rs` + global atomic `theme()` accessor + 意味名フィールド」を導入中
(ADR-0036)。一方 gpui-component の **全コンポーネントは色を `cx.theme()`(= `ActiveTheme` トレイト)から取る**。
一見二重管理に見えるが、**実態は競合しない**。両者は「読み口が2つあるだけ」で衝突しない構造である:

### gpui-component 側の Theme の実体(`theme/mod.rs`, `theme/theme_color.rs`)

- `Theme` は `gpui::Global`。内部に `colors: ThemeColor`(**Hsla 103 フィールド**: `background` /
  `border` / `primary*` / `danger*` / `sidebar*` / `list_active` / `tab_*` / `scrollbar*` /
  `popover` / `overlay` / `selection` / `input` / `accent` …)を持つ。
- `Theme::global_mut(cx) -> &mut Theme` が **public**。`cx.theme()` は単に `Theme::global(cx)` を返すだけ。
- つまり kagi は **テーマ切替の度に、自前 `theme()` の意味名フィールドを `ThemeColor` の対応フィールドへ
  コピーして `Theme::global_mut(cx).colors` を上書きする**だけで、採用した全 gpui-component が
  kagi のパレットで描画される。新たな抽象は不要。

### 推奨する統合パターン(W9 実装時に組み込む)

```rust
// テーマ切替ハンドラ内(index 更新 + cx.notify の直前/直後):
fn sync_gpui_component_theme(cx: &mut App) {
    let k = theme();                       // kagi 自前 Theme
    let gc = gpui_component::Theme::global_mut(cx);
    gc.colors.background       = k.bg_base.into();
    gc.colors.border           = k.border.into();   // ※意味名は W9 の確定フィールドに合わせる
    gc.colors.primary          = k.color_branch.into();
    gc.colors.sidebar          = k.sidebar.into();
    gc.colors.list_active      = k.selected.into();
    gc.colors.popover          = k.modal.into();
    gc.colors.overlay          = k.overlay.into();
    gc.colors.danger           = k.blocker.into();
    gc.colors.warning          = k.warning.into();
    // … 採用したコンポーネントが参照するフィールドのみで十分(全 103 個埋める必要はない)
    gc.mode = if k.dark { ThemeMode::Dark } else { ThemeMode::Light };
}
```

- **注意1(初期化順)**: `gpui_component::init(cx)` 内で `theme::init` が走り、起動時に
  **システム appearance に同期される**(`sync_system_appearance`)。kagi は起動時 settings 読込後に
  上記 sync を一度呼んで自前パレットで上書きすること。さもないと採用済 Input/Tooltip 等が
  システム配色のまま出る(現状は Input/Tooltip しか使っていないため目立たないが、採用を増やすと顕在化)。
- **注意2(diff highlight)**: `Theme.highlight_theme` は別系統。kagi は既に
  `HighlightTheme::default_dark()/light()` を直接呼んでいる(mod.rs:847 付近)。ADR-0036 の
  dark/light 連動方針と一致。gpui-component 採用を増やしても highlight は独立のままで良い。
- **結論**: **二重化リスクは「無い」**。むしろ自前 `theme()` を single source に保ったまま、
  境界で `ThemeColor` へ push するのが正攻法。採用コンポーネントの色ズレ対策はこの sync 1関数に集約できる。
  これは置換判定の前提(どのコンポーネントを採っても色は kagi 側に従わせられる)。

---

## 1. 全モジュール一覧(60+)

`L` = 採用容易度 / `S`=stateless RenderOnce, `E`=Entity state 必須, `D`=delegate トレイト実装必須,
`R`=Root 必須, `I`=専用 init 必須, `F`=自身が Focusable(呼び出し側要件ではない)。色は全て `cx.theme()`。

| モジュール | 主型 | 種別 | kagi 関連性 |
|---|---|---|---|
| `button/button.rs` | `Button` | S | ★ toolbar/checkbox 代替・W10 |
| `button/toggle.rs` | `Toggle` | S | トグルボタン |
| `button/button_group.rs` | `ButtonGroup` | S | セグメント選択 |
| `button/dropdown_button.rs` | `DropdownButton` | S+menu | branch picker 候補 |
| `checkbox.rs` | `Checkbox` | S | ★「Checkout after create」の真の checkbox |
| `switch.rs` | `Switch` | S | トグル設定 |
| `radio.rs` | `Radio`/`RadioGroup` | S | 排他選択 |
| `input/` | `Input`/`InputState` | E+I | **採用済**(IME) |
| `select.rs` | `Select`/`SelectState` | E+D+I | branch picker 候補 |
| `menu/popup_menu.rs` | `PopupMenu` | I+F | ★ context menu 中核 |
| `menu/context_menu.rs` | `ContextMenuExt` | (trait) | ★ 右クリックメニュー(親に Focusable 不要) |
| `menu/dropdown_menu.rs` | `DropdownMenu` trait | E | dropdown |
| `menu/app_menu_bar.rs` | `AppMenuBar` | — | kagi は `cx.set_menus` 採用済(維持) |
| `dialog.rs` | `Dialog` | R+I | ★ modal 群代替 |
| `sheet.rs` | `Sheet` | R+I | スライドインパネル |
| `notification.rs` | `Notification`/`NotificationList` | (Root 経由) | ★ toast 代替 |
| `popover.rs` | `Popover` | E | ポップオーバー |
| `tab/tab_bar.rs` | `TabBar`/`Tab` | S | ★ tab strip 代替 |
| `tree.rs` | `Tree`/`TreeState` | E+I | ★ file tree 候補 |
| `sidebar/` | `Sidebar`/`SidebarMenu` 他 | S(子=Collapsible) | sidebar sections 候補 |
| `collapsible.rs` | `Collapsible` | S | 折りたたみ原子 |
| `accordion.rs` | `Accordion` | S(内部 state) | アコーディオン |
| `resizable/` | `ResizablePanelGroup`/`ResizableState` | E(任意) | ★ divider リサイズ代替 |
| `list/` | `List`/`ListState`/`ListDelegate` | E+D+I | commit list 候補 |
| `table/` | `Table`/`TableState`/`TableDelegate` | E+D+I | 表形式 list 候補 |
| `virtual_list.rs` | `VirtualList`/`v_virtual_list` | (handle) | ★ uniform_list の上位互換 |
| `scroll/scrollbar.rs` | `Scrollbar` | (handle) | ★ スクロールバー表示 |
| `scroll/scrollable.rs` | `Scrollable` trait | (trait) | スクロール領域 |
| `progress.rs` | `Progress` | S | ★ Busy 進捗 |
| `spinner.rs` | `Spinner` | S | ★ Busy スピナー |
| `skeleton.rs` | `Skeleton` | S | ロード中プレースホルダ |
| `icon.rs` | `Icon`/`IconName` | S | **採用済** |
| `tooltip.rs` | `Tooltip` | (build) | **採用済** |
| `kbd.rs` | `Kbd` | S | ショートカット表示 |
| `badge.rs` | `Badge` | S | ★ Pull/Push count チップ(W10) |
| `tag.rs` | `Tag` | S | ref バッジ候補 |
| `label.rs` | `Label` | S | ハイライト付きテキスト |
| `link.rs` | `Link` | S | リンク |
| `divider.rs` | `Divider` | S | 区切り線(視覚のみ・リサイズ不可) |
| `alert.rs` | `Alert` | S | インライン警告 |
| `clipboard.rs` | `Clipboard` | (keyed state) | コピーボタン |
| `breadcrumb.rs` | `Breadcrumb` | S | パンくず |
| `avatar/` | `Avatar`/`AvatarGroup` | S | kagi 自前 avatar と比較 |
| `group_box.rs` | `GroupBox` | S | 枠付きグループ |
| `description_list.rs` | `DescriptionList` | S | ラベル/値ペア(detail panel 候補) |
| `form/` | `Form`/`Field` | S | フォームレイアウト |
| `highlighter/` | `SyntaxHighlighter`/`HighlightTheme` | — | **採用済**(tree-sitter) |
| `color_picker.rs` | `ColorPicker` | E+I | 不要 |
| `slider.rs` | `Slider` | E | 不要 |
| `time/date_picker.rs` `calendar.rs` | `DatePicker`/`Calendar` | E+I | 不要 |
| `chart/*` `plot/*` | 各種チャート | E | 不要(Git GUI に無関係) |
| `dock/*` | `DockArea`/`Dock`/`Panel` | E+R+登録 | Study only(ADR-0034 既決。重い) |
| `setting/*` | `SettingPage` 他 | S | 設定 UI(later) |
| `text/text_view.rs` | `TextView`(MD/HTML) | E+I | リッチテキスト(不要) |
| `title_bar.rs` | `TitleBar` | S | カスタムタイトルバー(later) |
| `window_border.rs` | `WindowBorder` | S | Linux 装飾窓(不要・macOS) |
| `history.rs` | `History<I>` | (汎用) | undo/redo データ構造(kagi は oplog 採用済) |
| `description_list` `group_box` `form` | 上記 | S | inspector/form 補助 |
| `root.rs` | `Root`/`WindowExt` | — | **採用済**(必須土台) |
| `styled.rs` `geometry.rs` `event.rs` | trait 群(`Sizable` 等) | — | `Sizable` 採用済 |
| `inspector.rs`(gpui-component) | デバッグ inspector | I | kagi の inspector.rs とは別物(無関係) |

---

## 2. 置換候補の詳細判定

各項目: ①実 API ②kagi 要件適合 ③置換コスト ④挙動リスク ⑤判定。

### 2.1 context menu(自前 overlay → `ContextMenuExt` / `PopupMenu`) — **置換推奨(条件付き)**

- **①API**: `menu/context_menu.rs` の `ContextMenuExt::context_menu(|menu, w, cx| menu.menu(label, Box<dyn Action>)…)`
  は **`ParentElement + Styled` にだけ生える拡張トレイト**。親要素(`div()`)に付けるだけ。
  右クリックで `PopupMenu` を `deferred(priority=1)` で描画、`snap_to_window_with_margin(px(8.))` で
  画面内に収める。アイテムは fluent: `.menu` / `.menu_with_icon` / `.menu_with_check` /
  `.menu_with_disabled` / `.separator` / `.label` / `.submenu`。`menu::init`(= `gpui_component::init`
  に含まれる)で key binding 登録。
- **②適合**: kagi の `CommitAction`(ShowDetails/CopySha/CherryPick/Revert/Reset…)と `ItemState`
  (Enabled/Disabled(reason)/Hidden)は `.menu_with_disabled` / 条件付き `.menu` 追加で素直に表現可能。
  色は §0 の sync で kagi パレットに従う。**過去の見送り理由(`PopupMenuExt` が Focusable を要求)は
  再評価の結果ほぼ解消**: (a) `ContextMenuExt`(右クリック用)は **親に Focusable を要求しない**。
  (b) Focusable を実装するのは `PopupMenu` 自身のみで、内部 `focus_handle` を持つ。
  (c) そもそも `KagiApp` は既に `root_focus`/`modal_focus` 等の `FocusHandle` を所有・focus 済み
  (mod.rs:1101/1145, 窓 open 時 `window.focus`)。Focusable 要件は今や障害ではない。
- **③コスト**: 中。`context_menu.rs`(自前 overlay 描画・行レイアウト・位置計算)を撤去し、
  ビルダ呼び出しへ移植。`CommitAction` を `gpui::Action`(`actions!` マクロ)へ写像する必要あり
  (kagi は ADR-0034 で Action/`actions!` を正式採用済なので親和的)。ヘッダ/グループ行・disabled 理由の
  footer 表示など kagi 独自 UX は `.label`/`.separator` と submenu で再現するか、一部は維持判断。
- **④リスク**: 中。(a) action dispatch は menu 閉鎖後に `action_context` の FocusHandle へ飛ぶ設計
  なので、`KagiApp` の focus handle を `action_context` に渡す配線が要る。(b) headless ログ互換:
  現在の context-menu ログ(`[kagi] context-menu:` 等があれば)はメニュー構築を kagi 側で行う限り
  維持可能だが、表示トリガが deferred になる点に注意。(c) disabled-reason の footer は PopupMenu に
  該当機能が無い → tooltip かサブラベルで代替。
- **⑤判定**: **置換推奨(中優先)**。フォーカス障害は解消済み。ただし disabled 理由表示・ヘッダ行の
  独自 UX をどこまで許容するかで「部分置換(PopupMenu を採用しつつ kagi 独自装飾を一部維持)」も可。

### 2.2 modal 群(自前 overlay → `Dialog`) — **置換推奨**

- **①API**: `dialog.rs` `Dialog::new(window, cx)` を `window.open_dialog(cx, |d,w,cx| d.title(..).child(..))`
  で開く(`WindowExt`)。`.confirm()`/`.alert()` プリセット、`.on_ok(Fn->bool)`/`.on_cancel(Fn->bool)`
  (false で閉鎖抑止)、`.button_props(DialogButtonProps{ok_variant: ButtonVariant, …})` で
  **destructive を `.ok_variant(Danger)` 表現可**、`.overlay_closable(bool)`/`.close_button(bool)`/
  `.width(px)`。Esc/Enter は `dialog::init` 登録(= `gpui_component::init` 済)。複数 dialog スタック可。
- **②適合**: kagi の plan card / create-branch / stash 等の確認モーダルに合致。`Input`(採用済)を
  child に置けるので create-branch のテキスト入力も自然。destructive policy(ADR-0023)の赤ボタンも
  `ok_variant(Danger)` で表現。色は §0 sync。
- **③コスト**: 中〜大。kagi は現在 overlay を `KagiApp::render` 内で `modal_focus` を持って自前描画。
  各 modal を `open_dialog` 呼び出しへ移すと **modal 状態管理(どの modal が開いているか)を
  Root.active_dialogs に委譲**でき、kagi 側の overlay 描画コードと `modal_focus` 管理を削減できる。
  ただし modal の中身(plan card の差分プレビュー等)は child として移植。
- **④リスク**: 中。**Root 必須**だが kagi は既に Root を窓の第一層にしている(mod.rs:10473)ので前提充足。
  Esc キーが `dialog::init` の binding と kagi 既存 `CloseMainDiff`(escape, context=None)と**競合し得る**
  → dialog 表示中は dialog の Esc が優先される設計か要検証(headless で Esc 動作確認必須)。
  headless モーダルログ(`[kagi] modal:` 等)は kagi が開閉を制御する限り維持可。
- **⑤判定**: **置換推奨(中優先)**。Root 充足済で導入障壁は低い。Esc 競合の検証を完了条件に。

### 2.3 toast(自前 → `notification` / `push_notification`) — **置換推奨(再評価で見送り根拠が解消)**

- **①API**: `Notification::new().info(msg)/.success/.warning/.error`、`.title`/`.icon`/`.autohide(bool)`
  (既定 5s 自動消滅)/`.action(|s,w,cx| Button)`/`.on_click`。投入は **`window.push_notification(n, cx)`**。
  描画は Root が `render_notification_layer` で行い、独立した `NotificationList`(VecDeque, 最大10)で管理。
- **②適合**: kagi の操作完了/失敗 toast に合致。型別アイコン色は `cx.theme().info/success/warning/danger`
  → §0 sync で kagi 配色。
- **③コスト**: 小。toast 自前描画を撤去し `push_notification` 呼び出しに置換。`autohide` も内蔵。
- **④リスク**: 低。**過去の見送り理由「Root 再入懸念」は誤り(再確認済)**: `NotificationList` は
  `Root::update()` 経由で push されるだけで、Root の render 中に Root を再 borrow する再入は無い
  (`Timer::after(5s)` の独立タスクで自動 dismiss、click → dismiss → on_click の順)。kagi は Root を
  既に持つため追加コストなし。headless で toast 表示をログ化しているなら push 箇所で同等にログ可能。
- **⑤判定**: **置換推奨(高優先)**。最も低コスト・低リスクで自前コードを削減でき、見送り根拠が解消した。

### 2.4 tab strip(自前 → `TabBar`/`Tab`) — **現状維持寄り(採用は任意)**

- **①API**: `TabBar::new(id).selected_index(i).on_click(|ix,w,cx| …).children(tabs)`、
  `Tab::new().label().icon().prefix().suffix().on_click().selected()`。variant(underline/pill/segmented)。
  state は呼び出し側保持(Entity 不要)。
- **②適合**: リポジトリタブ(ADR-0027)に概ね合致。ただし kagi のタブには close ボタン・中クリック閉じ・
  ドラッグ並べ替え・未保存マーカ等の固有挙動があり得る。`Tab.suffix()` で close ボタンは置けるが
  並べ替え/中クリックは自前配線が残る。
- **③コスト**: 中。`tabs.rs` を移植。W6-TABSPEED で速度最適化済の自前実装を捨てる判断が必要。
- **④リスク**: 中。tab の見た目/挙動が gpui-component 既定に寄り、kagi 既存 UX(tooltip 等)との差分調整。
- **⑤判定**: **現状維持(低優先で再検討)**。自前が安定動作・最適化済(W6)で、置換利得が小さい。
  視覚刷新を別途やる時に `TabBar` を候補に。

### 2.5 file tree / sidebar sections(自前 → `Tree` / `Sidebar`) — **現状維持(Tree は不適合寄り)**

- **①API**: `Tree`: `TreeState::new(cx)` を `Entity` 保持、`tree(&state, |ix, entry, selected, w, cx| ListItem)`。
  ノードは `TreeItem::new(id,label).child(..).expanded(..)`。`tree::init` 必須。
  `Sidebar`: `Sidebar::left()` + 子は `Collapsible`(`SidebarGroup`/`SidebarMenu`/`SidebarMenuItem`)。
- **②適合**: kagi の file tree は Git status(変更種別5色・ステージ状態・diff 連動)と密結合。`Tree` は
  ノードを `TreeItem`(label 中心)で持ち、行は `ListItem` 描画に固定されるため、kagi 独自の
  変更種別アイコン/色・ステージトグルを差し込む自由度がやや低い(render コールバックで `ListItem` を
  返す制約)。sidebar sections は `SidebarGroup`/`Collapsible` で素直だが、kagi 既存と機能差は小。
- **③コスト**: 大(file tree)。データモデルを `TreeItem` 階層へ写像 + `Entity<TreeState>` 管理。
- **④リスク**: 中。selection/expand のキー処理が `tree::init` の binding と kagi 既存キーと競合し得る。
- **⑤判定**: **現状維持(file tree)/ 低優先で sidebar の Collapsible のみ部分採用検討**。
  file tree は Git 固有要件が強く `Tree` の抽象に乗せる利得 < 移植コスト。折りたたみ UX が欲しい
  箇所だけ `collapsible.rs`(stateless・軽量)をピンポイント採用する手はある。

### 2.6 divider リサイズ(自前 drag → `resizable`) — **置換推奨**

- **①API**: `h_resizable(id)`/`v_resizable(id)` + `resizable_panel().size(px).size_range(min..max)`、
  `.on_resize(|state,w,cx| …)`。サイズは `Entity<ResizableState>`(任意で外部保持→**永続化に使える**)。
  `ResizablePanelEvent::Resized` 発火。ハンドル色 `cx.theme().drag_border`。
- **②適合**: kagi の T023 ペイン分割 / bottom panel リサイズ(T-BP-002)に合致。`ResizableState.sizes()` を
  読めば size 永続化(settings.json)も自前 drag より容易。
- **③コスト**: 中。`resizable/` への移植。kagi は inspector divider の実測 bounds + fallback 定数
  (`INSPECTOR_TOP_OFFSET`)方式を持つ → ResizablePanel の flex basis 方式へ寄せると W10 の
  「ヘッダ高変更で fallback 更新」作業が不要になる副次利得。
- **④リスク**: 中。リサイズの min/max・初期サイズの挙動が自前と微妙に異なる(flex shrink vs basis)。
  headless レイアウトログ(ペイン幅)が変わらないか検証。
- **⑤判定**: **置換推奨(中優先)**。永続化・複数ペインの一貫管理で利得あり。W10/W9 と領域が重なるため
  着手タイミングは調整。

### 2.7 ボタン・checkbox(自前 div → `Button` / `Checkbox`) — **置換推奨(checkbox 高優先)**

- **①API Button**: `Button::new(id).label().icon().primary()/danger()/ghost()/outline().on_click().disabled()
  .tooltip().loading(bool).with_size(Size).compact()`。**アイコンは label と横並び(h_flex, gap_2)固定で、
  縦積み(アイコン上・ラベル下)は非対応**(button.rs:564-593 で確認)。
- **①API Checkbox**: `Checkbox::new(id).label(text).checked(bool).on_click(|&new_checked,w,cx| …).disabled()`。
- **②適合 checkbox**: 「[ ] Checkout after create」が現在ただのテキスト(本物の checkbox でない)→
  `Checkbox` で即解決。実 on/off 状態と a11y を持つ正しい UI になる。**明確な機能改善**。
- **②適合 Button(W10)**: W10 は **Finder 風「アイコン大 + 下に小ラベル」= 縦積み**を要求。
  `Button` は縦積み非対応のため、**W10 の本体レイアウトには `Button` を直接使えない**。
  ただし hover bg/rounded/disabled muted/tooltip は `Button` の方が楽。
  → W10 は「`div` で縦 flex を組み、中の icon は `gpui_component::Icon`(採用済)、count は §2.8 `Badge`」
  というハイブリッドが現実解。単純な横並びボタン(将来増えるダイアログ内ボタン等)は `Button` を採用。
- **③コスト**: 小(checkbox)/ 中(汎用ボタン置換)。
- **④リスク**: 低。`on_click` シグネチャ・disabled 色が kagi 既存と揃うか確認。
- **⑤判定**: **checkbox = 置換推奨(高優先・小コスト・機能バグ解消)**。
  **汎用 Button = 新規採用推奨(中優先、ダイアログ等の新規ボタンから)**。
  **W10 ツールバー縦積み = Button 不可(縦積み非対応)→ 自前 flex + Icon + Badge で構成(現状維持の延長)**。

### 2.8 Pull/Push count チップ(W10 新規 → `Badge`) — **新規採用推奨**

- **①API**: `Badge::new().count(n).max(99)` / `.dot()` / `.icon()`、`.color(hsla)`。子要素の右上に
  オーバーレイ。既定色 `cx.theme().red`。
- **②適合**: W10 の「Pull ↓N / Push ↑N をアイコン右上の数字チップ、0 で非表示」に直結。`count(0)` 非表示は
  kagi 側で出し分け。
- **③コスト**: 小。W10 のツールバーボタン(自前 flex)に `Badge` を重ねるだけ。
- **④⑤判定**: **新規採用推奨(W10 着手時、中優先)**。自前オーバーレイ計算を省ける。

### 2.9 commit list(uniform_list 直 → `List` / `Table` / `VirtualList`) — **現状維持**

- **①API**: `List` は `ListDelegate`(`items_count`/`render_item`→`Selectable+IntoElement`、同一高さ必須)
  + `Entity<ListState<D>>` + `list::init`。`Table` は `TableDelegate` + 列定義。`VirtualList`/`v_virtual_list`
  は uniform_list 同等の低レベル仮想化 + `VirtualListScrollHandle`。
- **②適合**: kagi の commit list は **per-row graph lane canvas(T009)+ ref badge + 選択 + 詳細連動**を
  uniform_list 直で精密制御している。`List` の `ListDelegate` は「同一高さ・`ListItem` 中心」前提で、
  kagi の canvas レーン描画(行ごとに PathBuilder)を載せる自由度が下がる。`Table` は列前提で不一致。
- **③コスト**: 大。④リスク: 大(graph 描画の作り直し)。
- **⑤判定**: **現状維持(uniform_list 直)**。T008/T009 で作り込んだ graph 連動を `ListDelegate` 抽象に
  乗せる利得が無く、リスク大。**ただしスクロールバーは §2.10 で別途付与可能**。

### 2.10 スクロールバー表示(無し → `Scrollbar`) — **新規採用推奨**

- **①API**: `Scrollbar::vertical(&scroll_handle)`(`UniformListScrollHandle` を**そのまま受け付ける**:
  `ScrollbarHandle` トレイトが既存ハンドルに実装済)。親に `.child(Scrollbar::vertical(&handle))` を
  足すだけで **既存 uniform_list を再構築せずオーバーレイ可能**。表示モード `.scrollbar_show(Scrolling/Hover/Always)`。
  色 `cx.theme().scrollbar*`。
- **②適合**: kagi の commit list は `UniformListScrollHandle`(mod.rs:`UniformListScrollHandle`)を既に保持。
  そのハンドルを `Scrollbar::vertical` に渡せば**現状維持の commit list にスクロールバーだけ追加**できる。
  ADR-0006 が挙げた「スクロールバー非表示」弱点をピンポイント解消。
- **③コスト**: 小。④リスク: 低(オーバーレイ描画のみ、レイアウト非破壊)。
- **⑤判定**: **新規採用推奨(高優先・小コスト)**。commit list を置換せずに弱点だけ潰せる最良コスパ。

### 2.11 branch picker(自前 → `Select` / `DropdownButton`) — **新規採用検討(低優先)**

- **①API**: `Select` は `SelectDelegate` + `Entity<SelectState>` + `select::init`(検索付きドロップダウン)。
  `DropdownButton` は `Button` + `PopupMenu` を組み合わせる軽量版。
- **②適合**: ブランチ数が多い時の検索付き `Select` は UX 改善余地あり。ただし kagi の branch picker が
  既に機能している前提では利得は中。
- **⑤判定**: **新規採用検討(低優先)**。branch picker を作り込む将来チケットで `Select`(検索)/
  `DropdownButton`(軽量)を候補に。今すぐの置換は不要。

### 2.12 progress / spinner(Busy → `Progress` / `Spinner` / `Skeleton`) — **新規採用推奨**

- **①API**: `Progress::new().value(0..100)`、`Spinner::new().with_size().color()`(0.8s 回転内蔵)、
  `Skeleton::new().secondary()`(pulse 内蔵)。いずれ stateless・init 不要。
- **②適合**: kagi の Busy 表示/非同期リポジトリ読込(ADR-0030)に直結。自前アニメ不要。
- **③コスト**: 小。④リスク: 低。
- **⑤判定**: **新規採用推奨(中優先)**。Busy/ローディング表示を簡潔化。`Skeleton` は async repo loading の
  プレースホルダに好適。

### 2.13 detail panel ラベル/値(自前 → `DescriptionList`) — **新規採用検討(低優先)**

- `DescriptionList`(label/value グリッド、`.bordered()`/`.columns()`)は commit メタデータ詳細
  (author/date/sha/parent…)の表示に合致。自前レイアウトの代替候補。**低優先**。

### 2.14 avatar(自前 `avatar.rs` → `Avatar`/`AvatarGroup`) — **現状維持**

- gpui-component `Avatar` は `.src()/.name()` で画像 or イニシャル。kagi は W11-AVATAR / ADR-0037 で
  GitHub アバター + 独自 `avatar_color`(hsla 直計算, ADR-0036 で theme lane 配列へ寄せる方針)を持つ。
  色計算が kagi 固有のため `Avatar` 採用利得は小。**現状維持**(必要なら画像読込部だけ参考)。

---

## 3. 推奨アクション優先順位リスト

### 高(低コスト・高利得・低リスク。即着手候補)
1. **Scrollbar を commit list に付与**(§2.10) — 既存 `UniformListScrollHandle` に `.child(Scrollbar::vertical)`
   1行追加でレイアウト非破壊。ADR-0006 の弱点解消。
2. **toast → `push_notification`**(§2.3) — 見送り根拠(Root 再入)が誤りと判明。最小コストで自前削減。
3. **「Checkout after create」を本物の `Checkbox` に**(§2.7) — 現状テキストだけの機能バグを解消。

### 中(明確な利得だが移植コスト中。W9 sync 実装後に着手)
4. **modal 群 → `Dialog`**(§2.2) — Root 充足済。Esc 競合検証を条件に。
5. **context menu → `ContextMenuExt`/`PopupMenu`**(§2.1) — Focusable 障害は解消。Action 写像が要。
6. **divider リサイズ → `resizable`**(§2.6) — 永続化容易・W10 の fallback 定数管理が不要に。
7. **progress/spinner/skeleton 採用**(§2.12) — Busy / async loading 表示の簡潔化。
8. **Pull/Push count → `Badge`**(§2.8、W10 と同時)。
9. **汎用ボタン(ダイアログ内等)に `Button` 採用**(§2.7、横並びに限る)。

### 低(将来の刷新チケットで再検討)
10. tab strip → `TabBar`(§2.4、W6 最適化を捨てる判断要)
11. sidebar の折りたたみに `collapsible.rs` 部分採用(§2.5)
12. branch picker → `Select`/`DropdownButton`(§2.11)
13. detail panel → `DescriptionList`(§2.13)

### 現状維持(明確な理由あり)
- **commit list(uniform_list 直)** — T008/T009 の graph lane canvas 連動が `ListDelegate`/`Table` 抽象に
  乗らない。置換利得 < リスク。Scrollbar だけ §2.10 で足す。
- **file tree** — Git status 密結合(変更種別/ステージ/diff 連動)。`Tree` の `ListItem` 固定描画に対し
  自由度が下がる。移植コスト大。
- **avatar** — kagi 独自の色計算 + GitHub 連携(W11)。`Avatar` 採用利得小。
- **app menu bar** — kagi は `cx.set_menus`(ADR-0029)採用済で適切。`AppMenuBar` は不要。
- **W10 ツールバー縦積みレイアウト** — `Button` がアイコン縦積み非対応。自前 flex + `Icon` + `Badge` が解。
- **dock/chart/plot/date_picker/slider/color_picker/text_view/window_border** — Git GUI 要件外
  (ADR-0034 で dock は Study only 既決)。

---

## 4. チケット案(置換推奨項目の一行サマリ)

- **[高] T-GC-SCROLLBAR**: commit list(及び diff/file-tree スクロール領域)に `gpui_component::scroll::Scrollbar`
  をオーバーレイ。既存 `UniformListScrollHandle` を流用、レイアウト非破壊。
- **[高] T-GC-TOAST**: 自前 toast を `window.push_notification(Notification::…)` へ置換。`notification.rs` 撤去。
- **[高] T-GC-CHECKBOX**: 「Checkout after create」等の擬似チェックボックスを `gpui_component::Checkbox` に置換
  (実状態 + a11y)。
- **[中] T-GC-DIALOG**: plan card / create-branch / stash 等の自前 modal overlay を `window.open_dialog` へ移行。
  destructive は `ok_variant(Danger)`。Esc 競合(既存 `CloseMainDiff`)を headless 検証。
- **[中] T-GC-CTXMENU**: 自前 commit context menu を `ContextMenuExt`/`PopupMenu` へ移植。`CommitAction`→`gpui::Action`
  写像、`action_context` に `KagiApp` の focus handle。disabled 理由は tooltip 代替。
- **[中] T-GC-RESIZABLE**: T023/bottom-panel の自前 divider drag を `h_/v_resizable` + `ResizableState`(永続化付き)へ。
- **[中] T-GC-BUSY**: Busy/async-loading 表示を `Progress`/`Spinner`/`Skeleton` で統一。
- **[中] T-GC-BADGE**(W10 内): Pull/Push count を `Badge` チップで(0 非表示)。
- **[前提] W9 sync**: テーマ切替時に自前 `theme()` を `gpui_component::Theme::global_mut(cx).colors`
  (`ThemeColor`)へ push する `sync_gpui_component_theme()` を W9 実装に組み込む(§0)。
  **上記 GC 系チケットの色整合はすべてこの 1 関数に依存**するため、W9 完了が GC 採用拡大の前提。

---

## 5. ライセンス確認(ADR-0031 ゲート)

- gpui-component 0.5.1 = **Apache-2.0**(crate root LICENSE 原文。ADR-0006/0034 で確認済)。
  → Adopt directly 可。NOTICE/著作権表記の保持義務を順守(Cargo 依存として既に充足)。
- 本書の採用提案は全て「依存として利用」であり、GPL/FSL コードの転写は一切含まない。
