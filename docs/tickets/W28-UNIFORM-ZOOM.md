# W28-UNIFORM-ZOOM: 全 UI 一律ズーム(px 定数の zoom 連動)

- Status: in-progress
- 発端: ユーザー指摘 — W27 のズームがテキスト(rem)だけ効き、行間・パネル・commit graph が
  px 固定で追従せず graph のアラインメントが崩れる。gpui 0.2.2 に一律スケール機構は無いと確認済み。
- 方針: `theme::scaled_px(n) = px(n * zoom())`(main 済み 268d2e5)に**レイアウト px を通す**。
  テキストは既存の rem_size スケールのまま → 両者が同じ zoom() で一律拡縮。
- 分割: w28-graph(graph_view.rs + mod.rs の行高/グラフ結合点)/ w28-rest(その他ファイル)
