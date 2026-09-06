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
4. **派生レイアウトは不変の行所有権で再利用する**(2026-09-06 recovery)。
   `split_projection(&Arc<Vec<DiffRow>>)` がペア列と moved-row 集合を一緒に
   保持する。旧来の title + row count キーは同名・同サイズの別内容を
   区別できず、ペア列も毎 frame 再構築していたため置き換えた。
   選択範囲のキーとは分離し、選択・コピーの挙動は変えない。
   `ListState` のモード切替時の count-sync/reset は従来どおり。

### 派生 cache の所有・上限

- 既存の単一 cache を置換する。キーは元の行 Arc への Weak。内容を複製・
  強参照せず、アドレス再利用や `Arc::make_mut` 後の古い派生を防ぐ。
- FIFO 8 entries を維持し、capacity ベースの保守的な派生 allocation 予算
  8 MiB を追加する。死んだ source は除去し、予算を超える1件は返すだけで
  保持しない。描画中の所有分と元の行データはこの保持予算の対象外。
- PR Conflicts は毎 render 行 Arc を作っていた唯一の既知の生成元だった。
  同じ cutover で、読み込んだ1ファイルの preview を PrTab が所有する。
  raw marker text と派生行を二重保持しない。選択・入力変更で置換し、
  言語・テーマ変更は copy-on-write で更新して古い描画 snapshot を保つ。
- 実測: 32768行の warm 101 render でペア再構築101回から0回、中央値
  885042nsから42ns。cold は依然 O(rows)。数値・試行条件と小さい時間の
  解釈上の注意は `docs/agent-loop/2026-09-astra-recovery/METRICS.md` を参照。
  新しい依存 crate は不要。

## 非目標

- hunk 内の word-level(intra-line)ハイライト。
- 左右独立スクロール(行ペアは常に同期)。
- per-pane のモード(グローバル設定のみ。要望が出たら pane 側で上書き)。

## Consequences

- 3 つの diff 埋め込み先すべてが 1 実装で両モード対応になる。
- unified 行列が唯一のソースなので、今後の diff 機能(word-diff 等)は
  unified 側に足せば split にも波及する。
- モード切替で ListState が reset され、スクロール位置は先頭に戻る(許容)。
