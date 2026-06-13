# W28-UNIFORM-ZOOM: 全 UI 一律ズーム(px 定数の zoom 連動)

- Status: in-progress
- 発端: ユーザー指摘 — W27 のズームがテキスト(rem)だけ効き、行間・パネル・commit graph が
  px 固定で追従せず graph のアラインメントが崩れる。gpui 0.2.2 に一律スケール機構は無いと確認済み。
- 方針: `theme::scaled_px(n) = px(n * zoom())`(main 済み 268d2e5)に**レイアウト px を通す**。
  テキストは既存の rem_size スケールのまま → 両者が同じ zoom() で一律拡縮。
- 分割: w28-graph(graph_view.rs + mod.rs の行高/グラフ結合点)/ w28-rest(その他ファイル)

## w28-graph lane — Status: done (commit graph + row 結合点)

- theme.rs: `pub fn scaled(n: f32) -> f32 { n * zoom() }` を `scaled_px` の隣に追加(bare-f32 版、graph 座標計算用)。このレーンは theme.rs を他に触らない。
- graph_view.rs: 全レイアウト次元を zoom 連動化。
  - 追加: `pub fn lane_w() -> f32 = scaled(LANE_W)` — レーン横ピッチの単一窓口。
  - スケール対象: `LANE_W`(→lane_w), `NODE_R`(node_radius), `EDGE_W`, `CORNER_R`,
    HEAD リング `+1.5`/stroke `1.2`, merge リング `+2.5`/stroke `1.2`, ラベル→ノード connector `1.0`。
  - レーン x 中心・clip 窓・scroll_lo はすべて `lane_w()` 経由。
  - `lanes_for_width` / `graph_width` / `graph_width_for_lanes` も `lane_w()` 経由
    (列幅↔レーン数の換算が実ピッチと一致 → どの zoom でも clip 数 == 描画数)。
  - 縦方向: canvas は測定済み `bounds.size.height`(=scaled row 高)から `mid_y` を出すので
    ● が常に行中央 = ドリフト 0。geometry helper `lane_center_x` / `node_center_y` /
    `node_radius` を抽出し canvas 本体とテストで共有。
- mod.rs(surgical / commit-row 結合点のみ):
  - `row_height(compact)` を `scaled(..)` 化 — full(29)/compact(22) 両方 zoom 連動。
    これで text-row 高 == graph-canvas 行高 がどの zoom でも一致。
  - inner divider(`INNER_DIV_W`)を header / WIP 行 / commit 行・両境界で `scaled_px`。
  - WIP 行の hollow node(ml/9px)を scaled。avatar 18px/4px, author 130px, date 72px を scaled。
  - graph 横スクロール math(`scroll_graph_by` と clamp)を `lane_w()` 経由に。
  - 列幅 state(`graph_col_w`/`badge_col_w`)と divider-drag math は **意図的に未スケール**:
    drag は sidebar(別レーン所有)座標系に依存 → 競合回避。header と行は同一 state を共有するので
    相互整列は維持。zoom 連動は内部 graph geometry + 行高 + divider/列内要素で達成。
- 検証: `cargo build` own-code warning 0、`cargo test --workspace` 全 green(36 suites)。
  新テスト `graph_view::tests::geometry_scales_uniformly_with_zoom` が 0.8/1.0/1.3 で
  lane_w/node_radius/edge/corner/lane中心x/node中心y/lanes_for_width のスケールを assert。
||||||| 10167e9
## w28-rest lane — 実施メモ (branch worktree-agent-a41f9eeadd11e176f)

レイアウト px を `theme::scaled_px(..)` に通した。テキストサイズ(rem)はそのまま。
crisp な 1px hairline / divider は literal `px(1.)` のまま据え置き。`min_w/min_h(px(0.))`
の flex センチネルと OS ウィンドウサイズ `Bounds::centered(size(px(win_w),px(win_h)))`
は raw のまま。

### 変換ファイルと箇所
- diffstat_bar.rs: SEG_W/SEG_H/SEG_GAP(セグメント寸法とギャップ)。`px` import 除去。
- context_menu.rs / branch_menu.rs: メニュー幅 MENU_W・角丸・ヘッダ/グループ/行高
  (HEADER_H/GROUP_H/ROW_H)。`top/left` はマウス座標(raw)据え置き。画面外クランプ計算は
  scaled フットプリント(MENU_W*z, menu_h*z)で行うよう `zoom()` を掛けて整合。max_h の
  viewport 相対キャップは raw。
- tabs.rs: TAB_STRIP_H / TAB_MIN_W / TAB_MAX_W / close ボタン ml・px。`px` import 除去。
- sidebar.rs: フィルタ placeholder 高/角丸、ブランチ/グループのインデント(12/20/28/32/44)、
  外殻幅 `.w(scaled_px(width))`。`px(0.)` センチネルのみ残置。
- inspector.rs: ツリーインデント、バッジ列幅(14/16/18)、split divider 高
  (INSPECTOR_SPLIT_DIVIDER_H)、外殻幅 `.w(scaled_px(panel_width))`、action button paddings。
  `text_size(px(9.))` はテキストで据え置き。
- commands.rs: ブランチピッカー/Info オーバーレイのモーダル幅(360/420)・max_h・角丸・行 py。
  `px` import 除去。
- mod.rs(非グラフのみ):
  - ヘッダ/ツールバー: toolbar 高 52、アイコンセル 22/アイコン 20、カウントチップ
    (top/right/min_w/h/px/line_height)、gap/min_w/py、refresh アイコン 16、separator 高
    (幅 1px は hairline 据え置き)、ボタン間スペーサ w(2)。
  - サイドバー/パネル divider 幅 4(divider1/divider2)。
  - 列ヘッダ行高 COL_HEADER_H(列幅 badge/graph は触らず)。
  - ボトムパネル: 高 panel_h、divider 高(BOTTOM_PANEL_DIVIDER_H)、タブ高
    (BOTTOM_PANEL_TAB_H)、oplog 行高 22・列幅 60/100・ml。
  - ステータスバー高 STATUS_BAR_H(22) + 各 chip の ml(2/4/6)。dead_code の
    render_status_footer 高 22 も整合のため scaled。
  - トースト: bottom/left/幅 460/角丸。
  - モーダルカード幅(480×多数 / 540 / 520 / 460 / 420)・モーダル内ツリーインデント
    (pl(indent), pl(8+indent), pr(2), w(12), w(14))・discard リスト max_h 180・
    file-menu py(2/3)。
  - main diff のラインナンバ gutter 幅 44(×2)。
  - commit panel 外殻幅 `.w(scaled_px(panel_width))`。

### resize/drag 整合(永続幅は unscaled 保存・render 時に scaled)
- Sidebar/Panel drag: cursor は raw 画面 px。render が `width*zoom` の位置に divider を
  置くので、`new_width = (cursor - 2*z)/z`(Panel は `(vw - cursor - 2*z)/z`)で unscaled に
  戻して clamp/保存。クランプ定数(SIDEBAR_MIN/MAX 等)は unscaled 空間で正しい。
- BottomPanel drag: `(vh - cursor_y - (22+2)*z)/z` で unscaled body 高へ。max フラクションも /z。
- InspectorSplit: 主経路は canvas 実測 bounds(scaled 画面 px)なので不変。初回ペイント前の
  fallback のみ定数を *z し、span の divider 引きも *z。

### グラフレーン(w28-graph)へ委譲=未変更で残した箇所
- mod.rs: コミット行コンテナ高 `row_height()/rh`、badge_col_w / graph_col_w / INNER_DIV_W、
  グラフ内 1px divider(`w(px(1.))`)、ノードドット `w/h(px(9.))`・`LANE_W`、選択行
  アクセント `pl(px(10.))`、アバター円 18/18・mr(4)、メッセージ列の author/date 幅 130/72。
  これらは行高がグラフレーン管理で unscaled のため、整合性維持のため触らない。
- BadgeCol/GraphCol の drag math は w28-graph 所有のため未変更。**要マージ調整**: これらは
  cursor 空間で `this.sidebar_width + INNER_DIV_W` を参照するが、本レーンで sidebar_width は
  render 時 scaled になった(保存値は unscaled)。グラフレーンの列 drag は sidebar の scaled
  位置(`sidebar_width*zoom`)を基準に直す必要がある(zoom≠1 のときのみズレる)。

### 据え置き(literal px のまま)
- 全 hairline / 1px divider(`w(px(1.))`, separator 幅)。
- `text_size(px(9.))`(テキスト)、terminal.rs の TerminalConfig(font_size/padding は
  gpui_terminal 独自グリッド座標系でアプリの rem zoom に乗らないため未変換 → スキップ)。
- flex センチネル `min_w/min_h(px(0.))`、OS ウィンドウ物理サイズ。

### テスト / lint
- `cargo build` OK、`cargo test --workspace` 全 suite green(0 failed)。
- clippy own-code 警告 増減なし(base=63 / 変更後=63、新規ゼロ)。
