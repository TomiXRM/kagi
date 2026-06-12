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
