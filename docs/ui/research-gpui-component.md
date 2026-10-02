# #931 調査: gpui-component、Kagi の操作部品、Zeron

## 範囲と読み方

- 調査対象: `origin/main` の `e200a6f781f6c36ee034fad8a1d20436f76c8e1e`、[`Cargo.lock`](../../Cargo.lock) が固定する gpui-component **0.5.2 / [`b004e595cf5de98a73b6b561394a559a94ae1e2a`](https://github.com/longbridge/gpui-component/tree/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/ui/src)**。公式サイトの現行 API、古い ADR の 0.5.1、Zeron の別 fork の API を、この固定版の仕様として扱わない。
- Kagi の調査範囲は原則 `src/ui/**/*.rs`。`div()` は GPUI の**レイアウトの構成要素**でもあり、出現回数は自作の操作部品数でも画面に出る個数でもない。置換候補は操作の意味・keyboard/focus・accessibility・Git の安全確認を保てるものだけ。
- 外部比較: Zeron の[公式掲載画面](https://zeron.sh/)と公開ソース [`zeronsh/zeron@01832f2`](https://github.com/zeronsh/zeron/tree/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0)。公式画像は製品の掲載例であり、Kagi や pinned story をローカルで描画した証拠ではない。
- 既定寸法は source の `px` / `rem` / `Theme` 由来の宣言値。ウィンドウ上の**実測サイズ**、フォントメトリクス、JA 文字列や UI zoom 後の当たり判定とは区別する。

## pinned story の実画面（native macOS）

`gpui-component-story` を同じ `b004e595…` のチェックアウトから `cargo +stable build --locked -p gpui-component-story` でビルドし、`Button` で起動。ウィンドウを PID 指定の `scripts/pidclick.swift` で操作し、`screencapture -l<window-id>` の実ピクセルを保存した。最初の nightly ビルドは `pathfinder_simd 0.5.5` の `simd_fmin` / `simd_fmax` が解決できず失敗し、stable `rustc 1.97.1` では成功。アプリは確認後に終了した。スクリーンショットは [独立した `pr-assets/issue-931` の commit](https://github.com/TomiXRM/kagi/tree/234a5f054e32be35dbb9bb2057e60096da4138e6) に置き、調査ブランチには binary を入れていない。

| 実際に操作した状態 | 記録された画面 | 視認できる範囲 / 限界 |
|---|---|---|
| Button: variant, icon, loading, outline, size | [PNG](https://raw.githubusercontent.com/TomiXRM/kagi/234a5f054e32be35dbb9bb2057e60096da4138e6/button-default-light.png) | default/primary/secondary/danger/warning/success/info/ghost/link/text の違いと `sm` / `xs` の密度。状態を切替える story のチェックボックスも表示。 |
| Input: ordinary/disabled/password/content types | [PNG](https://raw.githubusercontent.com/TomiXRM/kagi/234a5f054e32be35dbb9bb2057e60096da4138e6/input-default-light.png) | 白背景の枠・プレースホルダ・disabled のグレー、入力内容の右側 clear アイコン。IME・focus ring・狭幅での省略は静止画から判定しない。 |
| Dialog: `Open Dialog` をクリックして表示 | [PNG](https://raw.githubusercontent.com/TomiXRM/kagi/234a5f054e32be35dbb9bb2057e60096da4138e6/dialog-open-default-light.png) | overlay、card、入力、Select、カレンダー trigger と Cancel/Confirm を確認。story 上の確認であり、Kagi の安全 plan を置き換え可能という証拠ではない。 |
| Settings: sidebar search / sections / switch / checkbox / select / stepper | [PNG](https://raw.githubusercontent.com/TomiXRM/kagi/234a5f054e32be35dbb9bb2057e60096da4138e6/settings-default-light.png) | story はページ左の検索、右の group 見出し・説明・コントロールを備える。Kagi は Settings の同部品を以前採用して色の不整合で撤退しており（[`settings_view.rs:8-14`](../../src/ui/settings_view.rs)）、無条件で再採用できない。 |

全画像は upstream **Default Light** の 3060×1988 PNG（画面は 2× Retina）、Kagi のテーマ/フォント/zoom を当てた比較ではない。story のフッターに `v0.5.1` と出るのは story crate 自身の [`Cargo.toml`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/story/Cargo.toml#L1-L5) の version。Kagi に解決された `gpui-component` **0.5.2** のバージョン表示ではない。gallery が起動することと、Kagi 上の統合が正しく動くことは別。

## 固定版 gpui-component: 全公開部品と story

一次資料は固定 commit の [`crates/ui/src/lib.rs:4-101`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/ui/src/lib.rs#L4-L101) と [`crates/story/src/gallery.rs:36-107`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/story/src/gallery.rs#L36-L107)。`pub mod` **57**、登録された story **62**（Getting Started 1、Components 61）。表は全 57 module を名前で列挙し、窓の private module から公開再 export される UI も含める。「story なし」は独立したページがない意味で、部品自体が使えない意味ではない。以下のファイル名は断りがなければ固定版 `crates/ui/src/` 起点。

| 公開 module / 再 export（複数名はすべて棚卸し対象） | 何ができるか | story で確認可能なページ / 無い理由 |
|---|---|---|
| `button` | Button variants、ButtonIcon / ButtonGroup、DropdownButton、Toggle / ToggleGroup | Button、DropdownButton、ToggleButton |
| `input` | IME 入力、複数行、code editor、NumberInput、OtpInput、mask/検索、Rope | Input、Textarea、Editor、NumberInput、OtpInput |
| `checkbox` / `radio` / `switch` / `slider` / `rating` | 選択・切替・値編集・星評価 | Checkbox、Radio、Switch、Slider、Rating |
| `select` / `combobox` / `searchable_list` | 単一選択、絞り込み+補完/複数選択、その共通 delegate/state | Select、Combobox。`searchable_list` の独立ページはなし |
| `color_picker` | 色指定 | ColorPicker |
| private `time` → 公開 `calendar`, `date_picker` | 暦表示・日付/範囲選択 | Calendar、DatePicker |
| `form` / `setting` | label/説明/required 付き fields、Settings の page/group/item/field/sidebar | Form、Settings |
| `dialog` / `sheet` | Dialog / AlertDialog（内容・footer）、片側 sheet | Dialog、AlertDialog、Sheet |
| `popover` / `hover_card` / `tooltip` | anchor 浮動層、hover card、遅延 tooltip | Popover、HoverCard、Tooltip |
| `notification` | window 内 notification の追加と stack | Notification |
| `menu` / `native_menu` | Popup / context / dropdown / app menu、OS native menu | Menu、NativeMenu。`AppMenuBar` は gallery title bar にも現れる |
| `list` / `tree` | delegate/state の仮想行、ListItem、TreeState と row renderer | List、Tree |
| `table` | 静的 Table、仮想 DataTable（sort / resize） | Table、DataTable |
| private `virtual_list` の公開再 export | **可変行サイズ**の仮想 list（要 item-size） | VirtualList |
| `tab` / `sidebar` | 5 種の Tab/TabBar、sidebar menu/group/header/footer/collapse | Tabs、Sidebar |
| `breadcrumb` / `pagination` / `stepper` | ナビ経路、ページ送り、手順表示 | Breadcrumb、Pagination、Stepper |
| `resizable` / `dock` | divider と panel、DockArea/Tiles/TabPanel の保存可能な構成 | Resizable。Dock は独立ページなし、`story/examples/dock.rs`・`tiles.rs` |
| `scroll` | Scrollable/Scrollbar/AutoScroll | Scrollbar |
| `group_box` / `collapsible` / `accordion` / `description_list` / `separator` / `status_bar` | 枠、開閉、説明項目、仕切り、status | GroupBox、Collapsible、Accordion、DescriptionList、Separator、StatusBar |
| private `title_bar` / `root` / `window_border` / `window_ext` の公開再 export | GPUI タイトルバー、窓の Root と overlay、窓装飾、open_dialog 等 | TitleBar/Root は各 gallery 窓で使用。window_border / WindowExt は独立ページなし |
| `badge` / `avatar` / `tag` / `kbd` / `label` / `link` | 表示 badge、人物、タグ、キー表記、テキスト、リンク | Badge、Avatar、Tag、Kbd、Label。Link の独立ページなし |
| `skeleton` / `spinner` / `progress` / `alert` / `clipboard` | 待機骨組、spinner、進捗、警告、コピー | Skeleton、Spinner、Progress、Alert、Clipboard |
| private `icon` の公開再 export | asset に依存する SVG icon | Icon。Image story は gpui 自体の `img`（gpui-component の module ではない） |
| `chart` / `plot` | 6 種の chart、軸/shape/scale/tooltip 基盤 | Chart。plot 単独 story なし |
| `text` / `highlighter` | Markdown/HTML TextView/selection、code 用構文着色 | 個別ページなし。Welcome 等で TextView、Editor 等で highlighter。`story/examples/{html,markdown,stream_markdown,editor}.rs` |
| `theme` / `global_state` / `history` / `animation` | global palette/tokens、floating 状態、undo history、補間/motion | Theme Colors のみ専用ページ。他は内部の story から使用、または story なし |
| private `styled` / `geometry` / `element_ext` / `event` / `focus_trap` / `index_path` / cfg `inspector` の公開再 export | Size/StyledExt、配置・interaction 拡張、focus trap、IndexPath、debug inspector | 非描画の基盤 API。各 story で間接使用。inspector は debug build でも入る |

上表に名前がない `Image` と `Welcome` も登録済みの 2 story。公開 API のない `async_util` / `macos_accessibility` / `actions` は実装内部。Chart の個別 area/bar/line/pie/sankey/candlestick や `input` の code editor は module 内の subfeature で、独立 `pub mod` 件数に加算しない。[`story/src/stories/mod.rs`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/story/src/stories/mod.rs) と gallery の登録数は一致する。

### 既定の寸法・角丸・余白・状態（source の宣言値）

`Size::XS/S/M/L` の共通 helper（[`styled.rs:341-541`](https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/ui/src/styled.rs#L341-L541)）は通常 `rem` に依存し、Root の `rem_size` は `theme.font_size` 既定 16px（`root.rs:555`, `theme/mod.rs:214`）。以下の換算は **16px/rem** 前提。`px(N)` と記したものは zoom に追従しない固定値。標準 `radius=6px`、`radius_lg=8px`（`theme/mod.rs:224-225`）。

| 部品（固定版 source） | サイズ・角丸・余白の既定 | hover / focus / disabled と注意点 |
|---|---|---|
| Button `button/button.rs:455-653` | XS/S/M/L 高さ **20/24/32/32**、水平 4/12/16/16、文字 12/14/16/16、通常 radius 6 | 変種ごとの hover/active token。keyboard focus ring、disabled は文字を減光するが Secondary 背景の `.opacity(1.5)` は減光しない。Loading ×0.8 |
| Input `input/input.rs:81-101,344,483-527` | XS/S/M/L 高さ 20/24/32/44、左右 4/8/12/16、文字 12/14/14/16、radius 6、1px 枠 | focus は ring 色の枠、disabled は bg / 枠 / prefix を減光。placeholder は muted。multi_line や suffix の高さ・padding は別途検証が必要 |
| PopupMenu `menu/popup_menu.rs:1092-1130,1300-1345` | item **固定 26px** / 左右 8 / text 14 / radius 6、popup min 幅 128、max 幅 500、外周 padding 4 | hover と keyboard 選択は accent、disabled は muted、focus ring なし。Small 用 20px 分岐に API 上到達不可 |
| Popover `popover.rs:337-360`; HoverCard `hover_card.rs`; Tooltip `tooltip.rs:117,146-153` | popover padding 12 / offset 4 / snap margin 8 / radius 6。Tooltip radius **固定 6px**、上下 2/左右 8 / 文字 14 | popover の影は `theme.shadow` を参照しない。tooltip 500ms delay / 300ms grace / 150ms enter。HoverCard は hover 起点 |
| Dialog `dialog/dialog.rs:190,468-670`; AlertDialog | **固定幅 448px**、最小高 96、padding 16、radius 8、title 16 semibold / description 14、footer gap 8 | overlay + close、250ms entrance。外枠影は `theme.shadow` を無視。Kagi の安全 plan の不変条件は含まれない |
| Sheet `sheet.rs:37-43,66-67,229-283` | 右側 350px、title 左右 16/12・上下 8、body 横 16、footer 16/12、角丸なし | 150ms slide。`resizable` フィールドは宣言されるが利用されていない |
| Notification `notification.rs:317-340,434-503` | 幅 448、横 16 / 縦 14、gap 12、radius 8、文字 14 | default 右上、最大 10、**5 秒**自動 hide。Kagi の最大 4 件 / footer 表示と契約が違う |
| `searchable_list` row `searchable_list/item.rs:96-138` | 中サイズ 左右 12 / 上下 4、文字 14、radius 6、**固定行高なし** | hover accent×0.7、selected accent、disabled muted。check icon 12px |
| Select / Combobox `select.rs:473-586`, `combobox.rs:527-541,907-1016` | Input と同じ 32px / radius 6 の trigger。popup max 高 320、padding 4、幅 trigger+2 | focus/open は枠色 ring、disabled opacity .5。複数選択は chip でなく `, ` 連結表示 |
| Form `form/form.rs:95-106`, `form/field.rs:27,297-345` | row gap 6/8/12、列 gap 18/24/36、label **幅 140**、14px、説明 12px | required marker は danger。Field.visible は未使用で、Form 自体の Styled 指定も描画で適用されない |
| Settings `setting/settings.rs:18,53-54,304-310`, `item.rs:278-294` | sidebar 幅 250（160–360 に resize）、480px 以下で縦積み、row gap 12、Input 幅 256 | title+description の row、disabled opacity .5。未対応の Field 型は `unimplemented!` |
| Tab `tab/tab.rs:24-43,346-356`, `tab_bar.rs:246-253` | Tab/Outline/Pill/Segmented 高さ 20/24/32/36、Underline は 26/30/36/44。Pill radius 99 | 選択 style と 200ms indicator。**focus/keyboard 操作は内蔵されない**。幅超過時の active tab 自動 scroll なし |
| Sidebar `sidebar/mod.rs:27-29`, `sidebar/menu.rs:266-289` | 幅 255 / collapse 48、menu row 高 28、padding 8、radius 6、文字 14 | hover/active/disabled 色あり、keyboard focus 未実装。collapse 200ms |
| Tag `tag.rs:209-269`; Kbd `kbd.rs:217-244`; Skeleton `skeleton.rs:37-58` | Tag 横 6 または 10 / 縦 2 または 4・radius 3/6。Kbd 横 4/縦 2・min 幅 20・radius 3。Skeleton 高 16、radius なし | Tag は hover opacity .9。Kbd は表示専用。Skeleton は 2 秒 pulse、ユーザーの opacity を上書き |

他の既定（16px/rem で換算）: ListItem 横12/縦4・hover `list_hover` / selected `list_active`（`list/list_item.rs:176-247`）、Tree は同じ ListItem で indent は呼び手、DataTable 行高 26/30/32/40・列 resize handle 2、static Table の cell 最小幅 100（`table/table.rs:9,94`）。Scrollbar track 16 / thumb 6→8・最小 thumb 48・2秒待って fade（`scroll/scrollbar.rs:17-29`）、TitleBar **固定 34**（`title_bar.rs:14-18`）、StatusBar 横8/縦4（`status_bar.rs:83-93`）。Checkbox/Radio の印 12/14/16/18・focus ring 2、Switch track 28×16 または 36×20・150ms、Slider track 6 / thumb 16、Progress 高 4/6/8/10、Spinner 800ms、Avatar 16/24/48/80、Badge dot 6 / count text 10、GroupBox padding 16、Separator 1px、Resizable handle 見える線 1 / grip 4、Dock TabPanel tab 30、Label line-height 1.25rem、Link hover/active の文字明度 ×0.8/×0.6（`checkbox.rs:220-269`, `radio.rs:189`, `switch.rs:128-171`, `slider.rs:500,726`, `progress/progress.rs:86-89`, `spinner.rs:23`, `avatar/mod.rs:13-16`, `badge.rs:109-150`, `group_box.rs:156-162`, `separator.rs:81-95`, `resizable/resize_handle.rs:11-12`, `dock/tab_panel.rs:647`, `label.rs:206`, `link.rs:80-84`）。
Navigation/表示系でも Accordion はサイズ別 header padding/14・16px text（`accordion.rs:224-306`）、Alert はサイズ別 padding と radius（`alert.rs:180-183`）、Pagination は dropdown 幅 55以上/高さ 240以下（`pagination.rs:245`）。Breadcrumb / Stepper / Rating / chart/plot / DatePicker / Calendar / Clipboard / text highlighter / Icon は表示内容・入力値や asset に寸法が依存し、単一の既定行高を全用途へ割り当てられない。**全部品が 32px の均一仕様ではない**。component を混ぜるほど、合わせる対象・状態・zoom を先に決める必要がある。

### Kagi のテーマへの適合と、置換前に直面する制約

Kagi は既に [`sync_gpui_component_theme`](../../crates/kagi-ui-core/src/theme.rs) で開始時/切替時に `ThemeColor` の mode-matching preset を seed し、背景/文字/アクセント/ボタンの variant・list/sidebar・scrollbar・font/syntax を上書きし、**最後に `ThemeTokens::from(&gc.colors)`** を再生成している（`:1005-1166`）。色だけは今の Bridge に乗せられる。`ThemeColor` は約 140 role（`theme/theme_color.rs:59-345`）だが、dialog / sheet / form / settings の専用色 token はなく、一般の background/popover/border を共有。`radius`、`radius_lg`、font_size、shadow、spacing/density は Bridge で全画面共通値に設定していない。Kagi の `scaled_px` と上流の rem-based / `px`-固定値が混在するので **ズーム後の同寸・focus・hitbox は実画面で確認**。

優先して検証する実装契約:

1. `gpui_component::init(cx)` と `Root` は Kagi で導入済み（`src/ui/e2e.rs:297-301`, `mod.rs:3328`）。ただし Root が dialog/sheet/notification を自動で描くわけではなく、story は親で各 layer を描いている（`root.rs:157,206,232`; `story/src/lib.rs:696-718`）。Kagi の一モーダル規約（`ActiveModal`）と安全 pipeline に結び付ける必要がある。
2. Input は `Entity<InputState>`、List は `ListState<Delegate>` の**固定行高**（先頭行で測定）、可変行高には別の `VirtualList`。Table は `TableState<Delegate>`、Tree は行 renderer が必要。Kagi の既存 `uniform_list`、commit graph の同期した行高と a11y `ListBoxOption` を置換できるかは未検証。
3. PopupMenu は `Focusable` 親、Action dispatch の前提があり、Kagi の右クリック対象選択 / disabled 理由 tooltip / danger 行の UX と一致しない（[`ADR-0020`](../adr/0020-commit-context-menu.md)、`src/ui/menu_overlay.rs:221-268`）。Tab/Sidebar/Toggle に focus と keyboard がないため、「ライブラリ部品 = アクセシブル」とは言えない。
4. 実装上の具体例: `ButtonCustomVariant.foreground`/`.hover` を受けるが描画経路では読まない（`button/button.rs:113-123,811,942-946`）。Secondary disabled の alpha 1.5、Button Large と Medium の同高、Sheet の未使用 `resizable`、PopupMenu の submenu に TODO（`menu/popup_menu.rs:1357`）、Dialog の focus なし Escape の FIXME（`dialog/dialog.rs:553-557`）。**source からの制約であり、Kagi 全環境での GUI 再現は未実施**。既に Kagi が [`button_style.rs:13-30`](../../src/ui/button_style.rs) で古い variant 色の不一致を回避している。
5. upstream docs の値に食い違いがある: menu 最小幅 doc 120 vs code 128、AlertDialog doc 420 vs code 448、Form label doc 100 vs code 140、Tag default doc Primary vs code Secondary。画面と固定ソースを優先する。Kagi のアイコンは AssetSource 登録が必要（[`ADR-0119`](../adr/0119-code-ecosystem-hotspot-viewer.md)）。上流の `Theme::change` は Kagi の直接書き換えを次の切替で消せるので、Bridge 再同期を必須とする。

## Kagi の自作操作部品: 画面別、件数の多い順

**数え方と限界。** `src/ui/**/*.rs` の `\bdiv\(\)` は文字列検索で **1,200 箇所 / 77 ファイル**。コメント・文字列・テストを除いたコードは **1,196 箇所**。別の 4 UI crate (`kagi-ui-{core,editor,ecosystem,file-history}`) に **183 箇所**ある。再現の入口は `rg -o --no-filename '\bdiv\(\)' src/ui -g '*.rs' | wc -l`（調査コマンドであり CI gate ではない）。操作部品は単なる `div()` ではなく、click/mouse/hover/key/focus/drag 等の handler が付く *builder の宣言箇所* を数え、helper の call site を含む/含まない区別をした概算。`src/ui` では 296 handler chain = **190 自作 div 系 + 95 component 系 + 11 分類不能**。190 のうち 18 は key/focus 容器、13 は tooltip だけ、10 は backdrop、5 は描画入力などの非典型 control として除き、**自作操作 builder 144**。UI crate 側は自作 39 のうち **操作 builder 32**、全画面の下表は合計 **176**。分類不能は全体で 14（本体 11 + UI crate 3、共有 `menu_overlay.rs:256` 等）で、表の件数には含めていない。分類は handler と周辺コードの読解による**推定**で、helper の多重呼出しや仮想行の実行時個数は表していない。

**反証の規模感。** Kagi には既に `Input::new` 19 / `InputState::new` 25、`Button::new` 66 + `KagiButton::accent*` 18、`Switch::new` 6、`Checkbox::new` 3、`Select::new` 1、`Tooltip::new` 27 が `src/ui` 内にある。別 UI crate を含めると Input 21 / InputState 27。[`settings_view.rs:30-35`](../../src/ui/settings_view.rs) だけでも Select / RadioGroup / Switch / Input / Button が存在。自作テキスト入力はこの調査で見つからなかった。Tooltip は handler 記録 37 のうち 33 が component、12 箇所で Button.tooltip も使用。最多 `div()` の modal 群は **21 ファイル/241 箇所**だが自作操作 builder は 3 件、Button 32 + KagiButton 10 件を既に使っている。従って `1,200 div` を `1,200 未採用 control` と読まない。

表中の `→` は **代替を評価する候補**であり移植の勧告ではない。`なし` は既製部品では意味/行 layout/安全契約を表せない箇所。行番号は上の Kagi commit で確認した宣言または call site。数は表題の「自作操作 builder」だけ、括弧中 `div` は layout を含む参考数。順序は数の降順（同数は順不同）。

| 画面・数 | `div()` で構成する操作要素と根拠 | gpui-component 候補 / 壊しやすい契約 |
|---|---|---|
| PR mode **17** (`div` 143) | merge / open-gh / checks / thread 等の buttons (`pr_mode.rs:1323,1363`, `pr_page.rs:382`)、commit/file/card rows (`pr_mode.rs:1442,1795`, `pr_nav.rs:347`)、underline tab (`pr_mode.rs:1513`)、commits disclosure (`:1391`)、splitter (`:1102`) | Button、TabBar、Collapsible、Resizable。rows は専用の選択と PR 文脈があるため ListItem 直置換は保留。 |
| 左 sidebar **16** (`div` 56) | branch / remote / tag / stash rows (`sidebar.rs:538,696,759,811`)、worktree row (`sidebar_worktree_row.rs:596`)、section 開閉 (`sidebar.rs:345,384`)、delete・port・filter 操作 (`sidebar_panes.rs:313,347`)、divider | Tree/Collapsible、Button、Resizable は候補。branch の drag/drop + right-click と tree a11y を維持しないなら **rows はなし**。Input は既に component。 |
| ヘッダー toolbar **15** (`div` 23) | [`make_btn`](../../src/ui/render_header.rs#L330) が 13 箇所に展開し、refresh/update 2 ボタンが追加（同ファイル `:538,607-826`） | Button。ただし「操作不可でも click で理由を footer に出す」(`:323-325,397-404`) は `Button.disabled(true)` にすると消える。disabled の表現と理由通知を別に設計するまで機械移行不可。 |
| Analyze / ecosystem **14** (`div` 73; 別 UI crate) | 共通 `text_button`、6 個の filter chip、health fix (`crates/kagi-ui-ecosystem/src/render.rs:344-383`, `graph.rs:156`, `health.rs:51`) | Button / ToggleGroup。graph canvas の pan/zoom は **なし**。 |
| Editor workspace **14** (`div` 55; 別 UI crate) | tab chip/close (`crates/kagi-ui-editor/src/lib.rs:2368,2795,2834`)、blame/markdown toggle (`:3053,3078`)、tree row (`:2603,2697`)、divider (`:2291,2305`) | TabBar / Toggle / Button / Tree / Resizable。ただし editor file-tree の depth と a11y、保存済み split 幅を維持。Input・Scrollbar は導入済み。 |
| Commit panel **12** (`div` 41) | file row (`commit_panel_render.rs:198`)、fold `:416`、coauthor menu `:740,784`、segmented toggle `:1034`、stage/discard/unstage `:1093-1171`、icon button `:1223` | Button / ToggleGroup / PopupMenu。status ごとの virtualized file row は **なし**。既存 Input/KagiButton/Tooltip と整合させる。 |
| Inspector **9** (`div` 54) | file rows (`inspector.rs:168,1091`)、generated fold `:231`、Path/Tree `:332,344`、hash chip `:551`、trailer link `:716`、divider `:929` | Collapsible、ToggleGroup、Link/Tag、Resizable。hash copy/tooltip の semantics を保持。 |
| Conflict editor **7** (`div` 47) | side/hunk/order/line の選択 (`conflict_editor.rs:571,917,965,1065`)、Preview/Edit `:1525-1555`、dividers `:1563,1581` | ToggleGroup / Resizable。**resolution row の判定入力はなし**。Input・Scrollbar・Button は既存。 |
| Graph + history row **6** (`div` 75) | commit/WIP/stash row (`render_helpers.rs:187`, `render_wip.rs:102,293`)、Load More (`render_helpers.rs:471`)、badge click (`badges.rs:197`)、wheel | Load More は Button。graph canvas、badge/ref の data、virtual row と click/copy の契約は **なし**。 |
| Branch cleanup **6** (`div` 32) | 9 箇所から使う `action_button` (`branch_cleanup.rs:481`)、Close `:574`、名/PR/delete row cells `:772,801,930` | Button と DataTable は候補、削除の安全状態と row の意味を守る。Checkbox `:473` は以前の ☐/☑ から component に既に移行済み。 |
| 本体 split/solo **6** (`div` 23) | sidebar/badge/graph/detail dividers (`render_body.rs:74,252,318,603`)、compact 切替 `:302`、Solo 退出 `:220` | Resizable / Button。既存比率と graph 列位置の同期を先に比較。 |
| Bottom panel **6** (`div` は Oplog と合算で 59) | Operation Log/Terminal/Activity tab と granularity (`render_bottom.rs:92,104,116,186`)、divider `:27`、activity bucket `:340` | TabBar / ToggleGroup / Resizable。Terminal の key container `:454` は操作部品の置換対象でない。 |
| Issues **5** (`div` 50) | issue card/row (`issues_mode.rs:319,486`)、Retry/Load More `:615,632`、thread 戻る (`issues_thread.rs:35`) | Button。paging 付き `gpui::list` row は **なし**。Composer Input は導入済み。 |
| File History **4**（別 UI crate） | context item、retry、2 divider (`crates/kagi-ui-file-history/src/detail.rs:191`, `render.rs:248,310,378`) | PopupMenu / Button / Resizable（hist selection を維持）。 |
| Context menus **4** | file menu rows (`file_menu.rs:141,153,165,287`)、共通 menu row (`menu_overlay.rs:188-277`) | PopupMenu は候補。ただし `menu_overlay.rs:256` は分類不能として上の 4 に未算入。disabled 理由 tooltip + danger 色を失うなら **なし**。 |
| PR/Issue filter strip **4** | filter chip `list_filter_strip.rs:138,170,317,350` | ToggleGroup / ButtonGroup。検索 Input `:301` は既製。 |
| Oplog **4**（Bottom と合算 `div` 59） | row と Copy (`oplog_render.rs:161,211,262,477`)、scrollable list | ListItem / Button を比較。oplog 履歴・状態・復旧操作を変えない。 |
| Repo tabs + Welcome **4** | repo tab、Open / Remote / Recent (`tabs.rs:885,955,973,1022`) | TabBar / Button。閉じる・reopen・session 所有規約の移行負担あり。 |
| Linux menu bar **3** | `platform_menu.rs:77,206` と `mod.rs:3407` | AppMenuBar は候補。OS menu と shortcut の同値性に注意。 |
| Modal 群 **3** (`div` 241/21 files) | smart-model row (`modal_renderers_misc.rs:117`)、section disclosure (`modal_shell.rs:345`)、copy (`modal_copy.rs:39`) | Select / Collapsible / Button。カード外形・scrim・key/focus 容器は **安全 UX の一部**、Dialog 丸ごと置換不可。 |
| Conflict view **2** | choice row と icon button (`conflict_view.rs:1410,1660`) | Button、resolution choice はなし。 |
| PR home **2** | PR dashboard などの action/row (`pr_dashboard.rs:56`) | Button / ListItem（状態と selection を保持）。 |
| Remote Browse **2** | directory rows (`remote_browse.rs:603,624`) | Tree 候補。階層と remote の再読込条件を保持。 |
| Status bar **2** | `render_status.rs:129,143` の操作 | Button。status text は表示専用。 |
| Settings **2** (`div` 30) | provider/model chips (`settings_view.rs:613,710`) | ToggleGroup。Switch ×6 / RadioGroup ×2 / Select / Input / Button は既製。Settings の外枠は独自 theme を維持。 |
| Workspace mode **2** | `workspace_mode.rs:40,75` の切替 | TabBar を比較。ただし Role::Tab を維持。 |
| Command palette **1** | command row (`command_palette.rs:466`) | SearchableList は選択候補。ただし閉じる焦点、custom command の分類を維持。検索 Input は既製。 |
| Branch pick **1** | `commands.rs:2190` の menu 項目 | Select / PopupMenu。 |
| Diff action **1** | `diff_view/hunk_action.rs:54` の hunk 操作 | Button。drag-selection (`diff_view.rs:489`) は候補なし。 |
| Operation strip **1** | `operation_strip.rs:79` の abort | Button。busy state と操作取消 semantics に注意。 |
| Toast **1** | `render_overlay.rs:129` の dismiss | Notification は候補。既存 [`toast_stack.rs`](../../src/ui/toast_stack.rs) の最大 4 / footer / animation と比較してから。 |
| App root **0** | layout/focus/scrim が主 | 裸 `div` の件数を replacement backlog に加えない。 |

**この表をそのまま置換 issue にしない理由:** Kagi の graph は `graph_view.rs:347-626` の canvas と共有 `ROW_H` に結合し、List の仮想化と `ListBoxOption` の位置/件数 a11y（[`list_a11y.rs:18-47`](../../src/ui/list_a11y.rs)）を持つ。gpui-component List は `List/ListItem` の role と先頭行固定高で契約が違う。modal は [`modal_shell.rs:9-12,103-107`](../../src/ui/modal_shell.rs) の対象常時可視と action 行固定が優先。Toast は最大 4/exit/footer、splitter は比率保存、menu は disabled 理由を表示する。**前提と回帰を個別に示してから** component の試作/画面比較を行う。現時点では置換 code も新 issue も作らない。

## Zeron: 「GPUI なのにモダン」をどう作っているか

公式の [3 ペイン画面](https://zeron.sh/assets/shots/showcase.webp)は本文・コード・セッションの視覚階層を各ペインの境界と見出しで分け、操作は細いトップバーと文脈に近い位置へ集めている。[History 画面](https://zeron.sh/assets/shots/history.png)はグラフの色をノードと線に限定して、一覧の文章・行境界を主役にしている。[Diff 画面](https://zeron.sh/assets/shots/diff.png)は変更数と状態を小さな色付き情報として表示する。**これは掲載画像の観察**であり、hover/focus の品質や速度は画像から証明できない。Kagi の [`docs/images/hero.png`](../images/hero.png) は古い作例で、現在の UI 全体との優劣比較には使えない。

| 観察できる設計上の手段 | 実装上の根拠 | Kagi での取り込み判定 |
|---|---|---|
| 見出し 44px、タイトルバー 38px、status 24px、パネル角丸 10px、操作角丸 6px、余白の階段 4/8/12/16px。検索/ブラウザー toolbar の操作は 24px、icon 14px、横 inset 8px。 | [Zeron `theme.rs:815-844`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/theme.rs#L815-L844)、[`surface_chrome.rs:7-43`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/surface_chrome.rs#L7-L43) | **寸法をコピーせず基準を共通化**。Kagi の狭幅、JA、zoom、実際のクリック範囲を測って決める。24px は特にアクセシビリティと操作密度の両面で試す必要がある。 |
| 色は役割別、レイアウトと切り離し。ダークの色を単純反転せず light の surface 高低・文字 contrast を別設計し、contrast テストを持つ。 | [`theme.rs:1-32`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/theme.rs#L1-L32)、[`theme.rs:2414-2435`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/theme.rs#L2414-L2435) | **考え方を採用**。Kagi にも [`Theme`](../../crates/kagi-ui-core/src/theme.rs) の役割色、4.5:1 のボタン文字テスト（同ファイル `:1644-1672`）が既にある。グラフの色は ref/branch の意味を担うので、単色化を目的にしない。 |
| Popover は画面端を踏まえた anchor と外側クリック、key navigation/search、open→closing→closed を共通化。退出アニメーション中に hit-test を無効化する。 | [`popover.rs:1-108`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/popover.rs#L1-L108) | **仕様の参考**。Kagi の modal と menu は別々の安全・focus 規約を持つ。Kagi の既存の click-away / Esc / disabled 理由を破棄する理由にはならない。 |
| 入場 motion は menu 140ms / dialog 180ms、exit は menu 100ms。reduce-motion とスピナー再描画の CPU 上限を明示。GPUI の pinned rev は div の scale transform が無いため、scale の見た目を fade+translate で近似。 | [`motion.rs:1-56`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/motion.rs#L1-L56)、[`motion.rs:379-385`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/motion.rs#L379-L385) | **質感の参考**だが安全確認の modal を演出で遅らせない。`prefers-reduced-motion` / low-power、再描画のコストを受け入れ条件に含める場合だけ導入を検討。 |
| Zeron は「全部 `div` 手作り」でも Kagi pinned gpui-component でもない。自前 presentation に [`gpui-base`](https://github.com/zeronsh/gpui-component/tree/2f73e5c2bc03d6768cb5fcc92442c4cf4b963b70/crates/base) の非装飾基盤を依存・初期化し、scrollbar のスタイルを指定している。ただし調べた主要画面で `gpui-base::Button` 等の利用までは確認していない。 | [Zeron `Cargo.toml:55-88`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/Cargo.toml#L55-L88)、[`lib.rs:155`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/lib.rs#L155)、[`theme.rs:1639-1642`](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/crates/ui/src/theme.rs#L1639-L1642)、[gpui-base README](https://github.com/zeronsh/gpui-component/blob/2f73e5c2bc03d6768cb5fcc92442c4cf4b963b70/crates/base/README.md) | **構造の参考**。Zeron の GPUI は別 fork `zeronsh/zui@667d0aaf`、Kagi の固定版に `gpui-base` はない。直接依存・ソース移植を「簡単な見た目改善」と見なさない。Zeron のアプリは [MIT](https://github.com/zeronsh/zeron/blob/01832f2cac72af4818bf0dcd9c6525c5dde5c4a0/LICENSE) でも、移植は API / ライセンス表示 / 保守コストを別途確認する。 |

**反証ポイント:** Zeron の見た目は特定の部品を import した結果ではなく、役割色・寸法・余白・状態・motion を画面横断で揃えた結果。従って `div()` の機械的置換も、Kagi の全 graph 行を Zeron 風に無彩色化することも、ユーザーの問題を自動で解決しない。先に具体的な UI 例とその状態差分を決め、既存の GPUI コンポーネントで満たせるなら流用する。

## #931 に書かれた PM 仮説への独立した判定

| 仮説 | 支持する事実 | 反証・限定 | 判定 / 次の基準案で試すこと |
|---|---|---|---|
| 1. Issue / 受け入れ条件に見た目の基準がない | [#923](https://github.com/TomiXRM/kagi/issues/923) は picker の内容・失敗状態・操作を明記するが、行の高さ/角丸/余白、既存画面と比較する見本画像は規定しない。[#922](https://github.com/TomiXRM/kagi/issues/922) も JSON/theme 動作と文章契約が中心。 | 全件に当てはまるわけではない。[`T-SETTINGS-001`](../tickets/T-SETTINGS-001.md) と [`ADR-0080`](../adr/0080-settings-window.md) は OpenLogi の外観と Settings の構造を参照し、[#923](https://github.com/TomiXRM/kagi/issues/923) は Tier B のスクリーンショット提出を指定している。ただし「撮る」ことは「参照と照合する」ことではない。 | **一部支持**。新 UI issue ごとに *既存・参照・目標・disabled/focus/empty/JA/zoom* の例を添える。個々の issue に巨大な全社共通スタイル表を複製しない。 |
| 2. Tier A/B は振る舞いと描画を検証するだけで見本と比べない | [`gui_e2e_runner.rs:17-36`](../../tests/gui_e2e_runner.rs) は状態 assertion を oracle、PNG を triage 用と明記。native offscreen の screenshot は [`kagi-verify` 手順](../../.claude/skills/verify/SKILL.md)で best-effort。Tier B は実アプリの手動スクリーンショット検査であり自動的なデザイン比較ではない。 | **幾何の見た目**は検査済みの箇所もある。[`header_fit.rs:87-138`](../../tests/recovery/header_fit.rs) は狭幅・JA・zoom の control bounds が画面内に収まることを assert。[`theme.rs:1644-1672`](../../crates/kagi-ui-core/src/theme.rs) は主ボタンと warning の文字 contrast 4.5:1 を確認する。Tier B の人手レビュー自体も「ただ描画されたか」より強い。 | **参照画像との比較不足を支持**、「描画以外を一切見ない」は反証。状態ごとに同条件の見本と実画面を並べて、見出し/入力/disabled/空状態の差を人が理由付きで評価する。自動 pixel 比較だけを正解にしない（OS/文字レンダラ/テーマ差）。 |
| 3. `div()` 手作り、gpui-component は input/button/tooltip 中心 | `src/ui` の `div()` は **1,200**（非テスト code は 1,196）。PR/左 sidebar/toolbar にはそれぞれ 17/16/15 の自作操作 builder がある。Settings は [`settings_view.rs:8-14`](../../src/ui/settings_view.rs) に `setting` を色の不整合で撤回した記録がある。 | 1,196 div のうち handler を持つ div 系は 190、control に絞ると 144。既製 handler 系も 95。Input の自作入力は発見されず、別 crate 込みで Input / InputState は 21/27。modal 241 div に自作操作は 3。Settings は Select/RadioGroup/Switch を、全体は 62 story 対応の多様な部品を既に採用。既存 [`ADR-0006`](../adr/0006-gpui-component.md) は graph/list の一括置換を避ける決定。 | **局所的に支持、総称として反証**。`div` 数を負債量にせず、上の 176 の候補宣言（本体 144 + UI crate 32）のうち focus・安全・virtualization・theme が同等なものだけ試す。 |
| 4. 共通の角丸/余白/高さ/状態/文字サイズ基準がない | 33 ファイルに local な `f32/Pixels` const が 131。row 高さは modal 18 / sidebar 20 / menu 24 / PR lane 27 / graph 29、menu 幅は stash 220 / tag 260 / context 280 / branch 300 と用途別。hover bg の指定も surface 40 vs selected 34 vs branch 色 15 箇所（数は検索の出現箇所、同じ画面内でも状態の意味は異なる）。[`settings_view.rs:143-204`](../../src/ui/settings_view.rs) は raw `px` で card の寸法/角丸を指定。 | 色と一部の寸法は**既に共有**。[`button_style.rs:33-78`](../../src/ui/button_style.rs) の KagiButton、[`theme.rs:1005-1166`](../../crates/kagi-ui-core/src/theme.rs) の mode/色/font bridge + `scaled_px`（本体に 577 使用箇所）、[`modal_shell.rs:21-25,97-127`](../../src/ui/modal_shell.rs) の対象行高・カード幅 504/576/648px がある。違う行高には list 仮想化や画面目的の理由もある。 | **画面横断の spacing/radius/hover/focus 規範不足を支持、「無基準」は反証**。既存の共通 helper と安全固有の数値を消さず、同じ役割の要素に寸法/状態の目標を追加し、JA/zoom で観察する。 |

以上は PM の独立案を読まずに評価した時点の資料。欠落している基準は画面全体の aesthetic だけでなく **component の複数状態と、Kagi の全テーマ / EN・JA / zoom に対する見本**。Zeron や story の既定 light 画像だけを gold master にするのではなく、Kagi 上の比較条件を揃えて検証する。
