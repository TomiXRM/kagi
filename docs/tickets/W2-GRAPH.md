# W2-GRAPH: コミットグラフ視覚改善(GK-parity 第2波)

- Status: in-progress
- 担当: worktree agent
- 関連 ADR: 0016

## 背景

ユーザー要件(GK-parity): グラフ上で「どれが HEAD か / どれが merge commit か / どの行を選択しているか」を
一目で判別できるようにする。GitKraken は HEAD node を大きく・merge node を二重円で描く。

## スコープ

1. **HEAD node の視覚区別**(`src/ui/graph_view.rs` + 呼び出し側):
   - HEAD コミット行の node を一回り大きく(例: 半径 1.5 倍)+ 外周リング(branch 色)
   - `graph_canvas` に `is_head: bool` を渡す(`CommitRow` に持たせるか引数追加かは実装判断。
     HEAD の CommitId は `KagiApp` が既に知っている)
2. **merge commit node の視覚区別**: `parent_ids.len() >= 2` の行は node を二重円(外円+中抜き)で描く。
   `CommitRow` に `is_merge: bool` を追加し構築時に設定(detail の parent_ids を参照)
3. **選択行の強調強化**(`src/ui/mod.rs` の row render):
   - 現状の `BG_SELECTED` 背景に加え、行左端に 2px のアクセントバー(COLOR_BRANCH)を表示
4. **compact mode トグル**:
   - `KagiApp.graph_compact: bool`(default false)。row 高さを 24px → 18px に切替
   - グラフ列ヘッダ(または列ヘッダ右端)に小さいトグルボタン(`▤`/`▥` 等のテキストで可)
   - `uniform_list` の行高は固定値依存なので、ROW_H を関数化して compact を反映
   - headless: `KAGI_COMPACT=1` で起動時に compact、`[kagi] graph: compact=on row_h=18` ログ
5. **label→node の視覚接続**: badge 列の primary chip から node まで、行内で badge 色の細い水平線
   (1px)を引く(badge 列と graph 列の間)。lane x 座標は `graph_canvas` が知っているので、
   接続線は graph canvas 側で「lane 0 の左端まで」を描く簡易実装でよい(完全な chip 位置追跡は不要)

## 完了条件

- [ ] `cargo test` 全パス + own-code warning 0
- [ ] fixture headless: 既存ログ回帰なし + `KAGI_COMPACT=1` ログ
- [ ] HEAD node / merge node / 選択行 / compact / 接続線が PM スクリーンショットで判別できる
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/graph_view.rs` / `src/ui/commit_list.rs` / `src/ui/mod.rs`(最小限)/ `src/main.rs`(KAGI_COMPACT のみ)
- `docs/tickets/W2-GRAPH.md`

## 触ってはいけないファイル

- `src/git/` / `src/graph/` / `tests/*` / `scripts/*` / `Cargo.toml` / 他の docs

## テスト方法

1. `cargo test`(パイプで握りつぶさず exit code 確認)
2. `bash scripts/make_fixture.sh` で fixture 作成 → headless 起動(macOS に timeout コマンドなし:
   バックグラウンド起動 + sleep + kill)
3. 検証は fixture / tempdir のみ。ユーザー repo 禁止

## リスク

- canvas は bounds 外に描けるが負方向 clip がない → 接続線は graph 列内に収める
- 文字列切り詰めは必ず chars() ベース(byte slice 禁止)
- mod.rs の変更は最小限にし、変更点を完了報告で全列挙(PM が merge する)

## 実装メモ (W2-GRAPH 完了時)

### 変更ファイル一覧

**src/ui/commit_list.rs**
- `CommitRow` に `is_head: bool`, `is_merge: bool` フィールドを追加
- `head_target(head: &Head) -> Option<&str>` ヘルパー追加
- `build_commit_rows`: snapshot の `head` から HEAD SHA を取得し、各コミットの `c.id.0 == sha` で `is_head` を判定
- `commit_to_row`: `is_head`, `is_merge` を受け取り `CommitRow` に格納
- `is_merge` は `c.parents.len() >= 2` で判定

**src/ui/graph_view.rs**
- `graph_canvas` のシグネチャに `is_head: bool`, `is_merge: bool`, `has_badges: bool` を追加
- HEAD node: 1.5× 半径の filled circle + 外周 ring (stroke)
- merge node: 標準半径 filled circle + 外周 ring (stroke)
- label→node 接続線: `has_badges && node_lane < clip && x_node > ox + 0.5` のとき、lane 色 1px 水平線を `(ox, mid_y)` → `(x_node, mid_y)` で描画

**src/ui/mod.rs**
- 定数追加: `ROW_H_FULL = graph_view::ROW_H (24.0)`, `ROW_H_COMPACT = 18.0`
- 関数追加: `fn row_height(compact: bool) -> f32`
- `KagiApp` に `graph_compact: bool` フィールド追加 (default `false`)
- `from_snapshot` と `with_error` の初期化ブロックに `graph_compact: false` を追加
- `render_rows` のシグネチャに `graph_compact: bool` を追加; `.h(px(rh))` で可変行高を使用
- `render_rows` 内: selected 行の左端に 2px accent bar (`border_l_2()` + `border_color(rgb(COLOR_BRANCH))`, padding は `pl(px(10.))` に調整)
- `render_rows` 内: `graph_canvas` 呼び出しに `is_head`, `is_merge`, `has_badges` を渡すよう更新
- `uniform_list` の `cx.processor` クロージャ内で `this.graph_compact` を渡すよう更新
- WIP 行の `.h(px(graph_view::ROW_H))` を `.h(px(row_height(self.graph_compact)))` に変更
- graph 列ヘッダに compact トグルボタン(`▤`/`▥`)を追加; クリックで `graph_compact` をトグル

**src/main.rs**
- `KAGI_COMPACT=1` 環境変数の処理追加: `app_state.graph_compact = true` + `[kagi] graph: compact=on row_h=18` ログ出力

### 検証結果
- `cargo test`: 150 tests all passed (lib 19 + bin 31 + integration 100)
- own-code warning: 0
- fixture headless 正常起動 + 既存ログ全件確認
- `KAGI_COMPACT=1` ログ `[kagi] graph: compact=on row_h=18` 確認

### 注意事項
- `border_l_2()` は gpui のスタイリング API で「border-left: 2px solid」に相当。`px_3()` の代わりに `pl(px(10.))` を使い視覚幅を揃える
- HEAD SHA 比較は full SHA 文字列同士の比較 (`c.id.0 == sha`); short SHA 比較は衝突リスクがあるため不使用
- compact mode の row 高さ 18px は uniform_list の item height 自動測定機構に依存するため、ROW_H 定数変更は不要
