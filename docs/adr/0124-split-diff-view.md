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
   派生レイアウトは選択範囲とは別 cache のまま保つ。2026-10-13 の下記 amendment
   で選択も title/count から既存の owner/content-source token へ移行する。
   モード切替では高さを再測定し、同一 source の読書位置は下記 #1660 の規則で移送する。

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
- source の置換では先頭へ戻す。同一 source のモード切替では読書位置を維持する。

## Amendment — 2026-10-18: mode-switch reading context (#1660)

The shared renderer maps a same-source mode transition through unified source
indices, not the old list index or pixel offset. `DiffListLayout` owns the
source/mode distinction; `VariableListState` resets stale measurements while
retaining the mapped anchor's intra-item fraction and meaningful pending
End/Reveal intent. A scrollable list already settled at its end keeps that
boundary, even after its pending End was consumed; a list fitting entirely
inside the old viewport does not imply end navigation. Other sources and
owners still start at the beginning.
Unchanged rendering performs no mapping scan or new full-row allocation.

A Split code pair uses its left source row, or its right row when left is
absent. An expansion uses the first open source row in left-to-right order;
it remains an expansion in the destination, not its parent code row.
Combined cards have only one list-item anchor: transferring that fraction
does not promise exact paragraph/pixel retention inside a particular sub-card.
Pairing and expansion indices reuse the existing derived projections; there
is no additional persistent cache or per-pane mode setting. Selection, Copy
and focus keep their existing owner/content identities.

## Amendment — 2026-10-13: visual keyboard order (#1373)

Split-body arrows, Home/End and viewport-sized PageUp/PageDown navigate the
cached aligned visual rows captured by the rendered pane. The caret retains
its left/right affinity; missing affinity-side cells in an unequal replacement
are skipped symmetrically in both directions. Wholly one-sided add/delete
runs remain navigable through their non-filler cells. Headers remain
full-width caret targets. Selection still stores unified source indices:
Shift retains the original anchor, and code-only clipboard extraction keeps ADR-0016's
inclusive source-range semantics. Cmd+A still selects all source rows.
Unified presentation and its existing bindings remain unchanged. The shared
body and existing ListState own focus and reveal across all diff embeddings.
Tier A `split_diff_keyboard_order` covers an unequal five-to-three replacement,
right-affinity Shift extension, reverse Up traversal, a wrapped-row page and
one-sided additions. Page distance walks only rendered list items beginning
at the logical scroll top, counting fully visible Row bounds and stopping at
the viewport bottom; expansion items consume space without becoming targets.

## Amendment — 2026-10-13: content identity and invisible changes (#1444/#1445/#1446)

Selection is bound to the existing diff-list owner and content-source revision,
not a title/row-count hash. Replacement, close and repository-owner departure
retire the selection. Syntax/theme-only row allocations preserve the source
revision and valid selection. The focused body handles Copy using only its
matching selection. Root Graph Copy additionally requires the visible, uncovered
commit list to own focus (ADR-0170); a saved diff range cannot hijack that action.
The existing inclusive, code-only extraction policy remains unchanged.

Both Unified rows and Split cells use one unsafe-Unicode badge/Tooltip helper.
Context warnings appear in both Split columns; Added and Removed warnings
appear on their actual side, ordinary/filler cells remain unmarked. The badge
retains the existing warning-fill grammar, localized caption and full help;
neither source text nor raw approval data is rewritten.

Pure `kagi-domain` line-ending metadata distinguishes LF, CRLF and no terminator.
The existing index-wise Removed/Added pairing identifies equal visible content
with different endings before UI normalization. `DiffRow` retains that ending
and whether it changed; both renderers show a muted localized annotation naming
the affected side's ending. Ordinary body changes do not receive an ending-only
annotation. Display removes the line terminator, never paints its raw CR, and
clipboard extraction remains code-only. Backend approval digests continue to
hash the original raw patch bytes, not the presentation model.

The surfaces remain dense source-review rows, with existing line numbers,
syntax/word emphasis, synchronized Split layout and full-width hunk controls.
Annotations are noninteractive labels, not extra Tab stops; warning hover opens
the existing Tooltip. Tier A scenarios `diff_selection_revision`,
`diff_split_unicode_badge` and `diff_line_ending_annotation` exercise real Git
fixtures and renderer-fed markers/help; the visible-marker scenarios cover
940×660 / 1392×883, 100% / 167%, EN / JA and Apple Light / Dark.
Native VoiceOver delivery and real-window visual comparison remain Tier B.
