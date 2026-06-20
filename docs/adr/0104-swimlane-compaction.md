# ADR-0104: コミットグラフに Gitru 由来の swimlane コンパクション（モード切替・安定色）を導入する

- Status: Accepted
- Date: 2026-06-20

## Context

コミットグラフは ADR-0003 で gitk スタイルの行単位 lane 割り当て（`crates/kagi-domain/src/graph.rs`
の `layout`）を採用している。原理は Gitru（`ruru-m07/gitru`, PR #105 `feat(git-graph):
git graph visualisation`, branch `ruru/git/git-graph/core`, `crates/git/service/graph.rs`）の
VS Code 由来 swimlane アルゴリズムとほぼ同じだが、見た目品質で 2 点の差があった。

1. **色がレーンに紐づかない** — Kagi は描画側 `lane_color(lane_index)` が**列インデックスで
   6 色循環**していた。ADR-0003 は「色の安定化は後回し」と明記していた。
2. **コンパクションしない** — 解放レーンを `None` 隙間として残し（左詰め再利用はするが既存
   レーンを横シフトしない）、分岐が多い履歴で Gitru より横に広がる。

Gitru は (a) 色をレーン構造体に載せ採番カウンタで決めてレーンと共に運ぶ、(b) 解放レーンを
詰めてグラフを細く保つ。これを Kagi に取り込み、GitKraken 目標の「細く・色が安定して追える」
グラフに近づける。

ただしユーザー方針として **コンパクションは将来削除する可能性がある**。よって恒久前提にせず、
後から外せる構造にすることが要件。

## Decision

`kagi-domain` の純粋レイアウトに 2 つを追加する。

### 1. レーン携帯の安定色（モード非依存・恒久）

- 内部レーンを `struct Lane { target, color }` とし、`color` は単調カウンタ
  `% NUM_COLORS`（=8）で**レーン誕生時に確定**、生存中（および列移動後も）保持。
- `GraphRow.color`（ノード色）と `GraphEdge.color`（各エッジの枝色）を公開フィールドに追加。
  描画は列インデックスではなくこの安定色で塗る。これで ADR-0003 が先送りした色安定化を解消。
- first parent は親の lane と色を継承する（枝の幹が色ごと安定）。

### 2. swimlane コンパクション（`GraphLayoutMode::Compact`・opt-in・削除容易）

- `pub enum GraphLayoutMode { Stable, Compact }` と `pub fn layout_with(commits, mode)` を追加。
  `layout()` は `Stable` へ委譲（既存 API・既存挙動・lane index は不変）。
- `Stable` = 現行 gitk スタイル（列は固定、隙間は `None`）。`Compact` = Gitru 方式
  （解放レーンを詰め、右のレーンを左へシフト）。Gitru `process_commit` の input/output
  swimlane モデルを、Kagi の**行内完結エッジ**（`Pass`/`IntoNode`/`OutOfNode`）へ翻訳して移植。
- コンパクションでレーンが列移動する場合は **shift エッジ**＝`EdgeKind::Pass` で
  `from_lane != to_lane` を emit。描画は既存の角丸ヘルパと同様の S 字曲線
  （`src/ui/graph_view.rs::draw_shift`）で「上端列→下端列」を繋ぐ。

### 3. UI 配線

- 採用モードは設定キー `graph_lane_compact`（`src/ui/settings.rs::graph_lane_compact`）で切替。
  **既定は `Stable`**（行高さ用の既存 `graph_compact` とは別キー）。
- `CommitRow.node_color` を追加し、`graph_canvas(node_lane, node_color, …)` へ渡す。
  エッジ色は `GraphEdge.color` から直接読む。stash レーンは従来どおり stash 色で上書き。
- パレットは **8 色の固定 L/C パレット**（`src/ui/theme.rs` の `LANE_PALETTE_DARK` /
  `LANE_PALETTE_LIGHT`）。Gitru の `oklch(0.77 0.174 H)`（明度・彩度を固定し色相だけ回す＝
  等明度・等彩度のカテゴリカル配色）思想を踏襲し、GPUI は OKLCH を持たないため
  **oklch → sRGB へオフライン変換した値を HSL で焼き込む**。色相は隣接インデックスが色相環上で
  最大限離れる順（`(i*3 mod 8)*45°`）。明背景テーマは同じ色相・彩度を低明度（`L=0.58`）にした
  別パレットでコントラストを確保。`NUM_COLORS`（=8）と UI パレット長は 1:1 で一致。

## Rationale

- 安定色はコンパクションの有無に依存しない**恒久的改善**で、ADR-0003 の宿題を片付ける。
- コンパクションを別モード＋既定 Stable にしたことで、(a) 現行グラフを無回帰で温存、
  (b) shift 描画は GUI 目視が必要なため既定オフが安全、(c) 「将来削除」は
  `layout_with` の `Compact` 分岐＋関連テスト削除＋既定維持だけで済む。
- 行内完結エッジモデル（ADR-0003）を維持したので、仮想化描画・stash 注入（ADR-0088）と互換。
- pure Rust のまま（git2/gpui/IO なし）でユニットテスト可能。

## Consequences / Risks

- `GraphRow` / `GraphEdge` に `color` 必須フィールドが増えた。stash エッジ生成 3 箇所
  （`render.rs` ×2, `commit_list.rs` ×1）は stash 色で上書きされるため `color` は便宜値。
- `Compact` の shift 曲線・色の見た目は **GUI 目視（人間）未検証**（本コンテナは GUI を
  リンク/起動できない）。ビルド（`cargo check`）とドメインユニットテストは緑。
- root（親なし）コミットが他レーンと同時に存在する稀なケースで、`Compact` のノード列が
  通過レーンと視覚的に近接しうる。データモデル上は不正ではない。将来微調整の余地。
- 既定 Stable のため出荷時の見た目は不変。Gitru 風の細さは設定 `graph_lane_compact=true` で有効化。

## Verification

- `cargo test -p kagi-domain`（99 件）緑。新規: 安定色の枝内一貫性、別枝で別色、`Compact` の
  線形・shift エッジ emit・列再利用・`lane_count` が `Stable` を超えないこと。
- `cargo check --workspace` 緑（UI 配線含む全コードが型検査を通過）。
- `cargo fmt --check` / `cargo clippy`（自 diff に新規警告なし）。
- 残: 人間が GUI を起動し、`graph_lane_compact=true` で (a) 分岐の多い repo の細さ、
  (b) 枝色が履歴を遡っても一定、(c) shift 曲線の破綻なし、(d) HEAD/merge/stash/badge が従来通り、
  (e) Stable へ戻すと現行見た目、を確認。
