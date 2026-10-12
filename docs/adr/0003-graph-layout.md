# ADR-0003: コミットグラフは行単位 lane 割り当ての事前計算方式とする

- Status: Accepted
- Date: 2026-06-12

## Context

commit DAG を GUI に描く方式の選択。候補:
1. **gitk 系の行単位 lane 割り当て**(topo 順に1行1commit、active lane 集合を引き継ぐ)
2. 汎用グラフレイアウト(Sugiyama 等)
3. `git log --graph` の ASCII をパース

## Decision

方式1。topo order で上から走査し、`active_lanes`(各 lane が次に待つ commit)を更新しながら各行の lane と edge を**事前に全行計算**して `GraphLayout` として保持する。描画は可視行のみ(仮想化)。

アルゴリズム詳細は architecture.md §4。要点:
- first parent が lane を継承(branch の幹が安定する)
- merge の 2nd parent は右側に新 lane / 既存待ち lane へ接続
- 複数 lane が同じ commit を待つ場合は最左へ合流
- edge は「行の上端 lane → 下端 lane」で行内完結(仮想化と相性が良い)

## Rationale

- gitk / Sourcetree / GitKraken 系で実績のある見た目になり、ユーザーの既存メンタルモデルに合う。
- O(rows × lanes) で 10k commits でも数十 ms オーダー。汎用レイアウトは過剰で遅い。
- ASCII パースは構造情報(lane index, edge 種別)が失われ、色・クリック判定が作れない。
- pure Rust モジュールとして UI から完全分離でき、unit test が書ける(T006/T007)。

## Consequences / Risks

- lane の色が履歴更新で変わりうる(色の安定化は later)。
- 100k+ commits では事前計算とメモリが問題になりうる → MVP は読み込み上限(10k)で対処、incremental layout は later。
- octopus merge(parent 3+)も同じ規則で右側に lane を増やして対応する(テストケースに含める)。

## Amendment — 2026-10-12: responsive Graph columns (#1317)

Commit subjects are primary row content: preserve a zoom-scaled 120-reference-px
minimum before assigning spare space to the two leading columns. Author prefers
96px but may shrink to 40px with native middle ellipsis; the sanitized full name
remains in its tooltip and the row's accessibility label. Compact time retains its
48px width. The shared fit includes row padding, dividers, avatar and provenance
space, reducing GRAPH first and then BRANCH/TAG to their existing minima.

The persisted `badge_col_w` / `graph_col_w` values stay reference-space preferred
widths, not viewport-dependent fitted widths. Rendering scales and fits them once
for the current center pane, including sidebar/right-pane motion, then shares that
pair across headers, commits, WIP and stash rows. Widening restores preferences
without writing settings; existing measured-origin divider drags still own writes.
Visible-lane counts and horizontal wheel limits use the scaled fitted width,
not the stored preference, so the compressed GRAPH still paints and scrolls lanes.
Lane allocation, row density, selection, Git reads and operation safety are unchanged.

Native `graph_columns_fit` pins the old 1392×883/100% x positions and requires
positive subject width plus contained author/time bounds at 940×660/167%, EN/JA.
The `graph_` neighbor selection retains divider drag and history interactions.
