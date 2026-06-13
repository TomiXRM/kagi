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
