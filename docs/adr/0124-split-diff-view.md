# ADR-0124: Diff display mode — unified / side-by-side (split)

- Status: Accepted
- Date: 2026-07-19

## Context

kagi の diff 表示は unified(1 カラム、+/- 行)のみだった。一般的な Git GUI は
unified と side-by-side(横並べ 2 カラム)の両方を提供する(ユーザー要望
2026-07-19)。diff バックエンドは外部ツール(delta 等)ではなく自前:
`kagi-git` が git2 で計算 → `kagi-domain` の `FileDiff`/`Hunk`/`DiffLine`
(各行が `old_lineno`/`new_lineno` を保持)→ UI の `DiffRow`(ハイライト
付与済み)→ 共通の `render_helpers::render_diff_list` が描画する。
3 つの diff 埋め込み先(メイン diff / File History / Editor Workspace)は
すべて `render_diff_list` を通る。

## Decision

1. **ペアリングは純粋関数として `kagi-domain` に置く** —
   `diff::split_pairs(&[DiffLineKind]) -> Vec<SplitPair>`。
   Context は両カラム、`Removed` 連続 + 直後の `Added` 連続は index-wise に
   ペア(長い側の余りは filler)、単独 run は filler と組む。ユニットテストは
   kagi-domain 側。
2. **UI 側は index の張り替えのみ** — `src/ui/diff_split.rs`(sibling、
   ADR-0121)が unified の `DiffRow` 列を `SplitDiffRow`(`Full(idx)` /
   `Pair{left,right}`)へ変換し 2 カラム描画する。行番号・シンタックス
   ハイライトは unified 行のものをそのまま参照(二重計算なし)。
   hunk ヘッダ / Binary 行は unified レンダラへ委譲。
3. **モードはグローバル + 永続** — `kagi-ui-core::theme` の atomic
   (`diff_split()` / `set_diff_split()` / `init_diff_split()`、
   `graph_compact` と同型)+ settings.json キー `"diff_split"`
   (`"true"`/`"false"`、既定 = unified)。切替トグルは
   `render_diff_list` のヘッダ(全埋め込み先に共通で出る)。
   トグル時に `[kagi] diff-mode: split|unified`、起動時に
   `[kagi] diff_split: <bool>` を klog。
4. ペアリングは表示中 diff の再 render ごとに O(rows) の index Vec を
   1 本作るだけ(内容は共有)。`ListState` の行数はモードで変わり、既存の
   count-sync が reset(先頭へ)する。

## 非目標

- hunk 内の word-level(intra-line)ハイライト。
- 左右独立スクロール(行ペアは常に同期)。
- per-pane のモード(グローバル設定のみ。要望が出たら pane 側で上書き)。

## Consequences

- 3 つの diff 埋め込み先すべてが 1 実装で両モード対応になる。
- unified 行列が唯一のソースなので、今後の diff 機能(word-diff 等)は
  unified 側に足せば split にも波及する。
- モード切替で ListState が reset され、スクロール位置は先頭に戻る(許容)。
