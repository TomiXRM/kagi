# ADR-0007: Bottom Panel Architecture

- Status: Accepted
- Date: 2026-06-12

## Context

Git 操作のログ・terminal・将来の Problems 等を表示する、出し入れ可能な下部パネルが必要
(requirements-shell.md §1)。VSCode / Zed の Bottom Panel と同じ操作モデルにする。

## Decision

1. **AppShell の明示化**: KagiApp の render を「Header Toolbar / Body(Sidebar+Main+RightPanel)/
   Bottom Panel / Status Bar」の縦 flex スロット構造に再編する(T-BP-001)。
   各スロットは関数分割し、Body は既存実装をそのまま移す(挙動変更なし)
2. **状態**: `bottom_panel: { open: bool, height: f32 (クランプ 80〜60%), active_tab: BottomTab }` を
   KagiApp に持つ。高さはセッション内記憶(reload で保持。永続化は later)
3. **タブは trait 化しない**: MVP は `enum BottomTab { OperationLog, Terminal }` の match 描画。
   タブ追加が3つを超えた時点で抽象化を検討(YAGNI)
4. **Operation Log タブ**: T017 の oplog(JSONL)を**メモリ上にもリングバッファ(最大500件)**で保持し、
   タブはそれを新しい順に表示。アプリ起動時に既存 JSONL の末尾 100 件を読み込む
5. **失敗時の自動オープン**: OpOutcome::Failed を記録した時点で `open = true` + active_tab = OperationLog
6. **リサイズ**: 上端に水平ディバイダ(T023/T030 と同じ絶対座標方式: `height = viewport_h - cursor_y - statusbar_h`)
7. **開閉ショートカット**: gpui の KeyBinding(`cmd-j` — VSCode/Zed と同じ)+ Status Bar のアイコン

## Consequences

- render 関数の大規模な構造変更(T-BP-001)が先行リスク → 挙動変更なしの純リファクタとして単独チケット化し、
  スクリーンショット比較で回帰確認する
- Operation Log の二重管理(ファイル + メモリ)は許容(ファイルは永続監査、メモリは表示用)
- Terminal タブの中身は ADR-0008 に従う

## Placement revision (2026-10-02)

Decision 1's window-wide Bottom Panel slot is superseded. The shell now lays
out `Sidebar | (Center workspace above Bottom Panel) | Right Panel`, with the
Status Bar still spanning the whole window. Terminal, Operation Log, and
Activity remain the same Bottom Panel tabs and retain their window-global
open/tab/height state, 18% default, top-edge resize, Cmd-J, and footer entry
points. Only the center workspace loses height when the panel opens; navigation
and the inspector/commit panel run to the status bar. The inspector's fallback
split geometry subtracts only the status bar, while File History (a center
takeover) still subtracts the Bottom Panel. Conflict Mode continues to replace
the body and hide the Bottom Panel until resolution.

Editor / PR / Issues はそれぞれ内側に左右ペインを持つため、外側の center
takeover 全体の下に Bottom Panel を置かない。PR / Issues は navigator（PR
では swimlane も）を全高のままにして中央列の下だけへ置く。Editor では
file tree / hunks を全高に保ち、`EditorWorkspaceElement` が親 `KagiApp` の
panel element を uncached な Editor entity の layout 中に渡す。Editor から
`KagiApp` への再入はせず、子 entity の更新でも親から新しい panel を描画する。
Editor entity を `.cached(...)` に変更するとこの受け渡しが失われるため行わない。

## Activity accessibility amendment (2026-10-12, #1301 / #1302)

The Activity period control uses the existing shared automatic TabList: five
named EN/JA choices (Day / Week / Month / Year / All), one Tab stop,
Left/Right/Home/End selection and Enter/Space activation. Selected window-aware
fill wins over hover; hidden Activity controls hand focus back to the root.

The chart retains its commit/merge aggregate and immediate hover readout.
Each bucket has a focusable Status with UTC date/time and localized commit,
merge and distinct-author counts. Authors are counted by raw email once per
bucket in the pure domain aggregate, never from render or extra Git reads.
One roving Tab stop and arrow/Home/End inspection expose the same readout.
Solid commit and dashed merge strokes are painted by the same function in the
chart and legend. Merge gaps first knock out the underlying solid/baseline
with panel-coloured strokes wider than the commit line; the Activity body
explicitly paints that same panel fill. Equal/zero segments retain a
luminance-visible pattern, not alternating series colours or gap rectangles.
Single buckets retain a solid commit mark and a separate two-dash merge mark
3 reference px below it, using the same dash/gap cadence as the legend.
Header wrapping and a 40%-capped ranking preserve the chart at narrow/high-zoom
layouts. No chart manager, new aggregation owner, write operation or Audio
Graph API is added.

Native renderer regressions record installed roles/focus handles, compare
actual selected-fill pixels and coincident dash-gap pixels (the latter against
nearby unpainted pixels), and exercise 0/1/24-bucket values. Stroke probes count
commands in successfully painted solid/dash/gap
paths and record the single-point branch, not the requested encoding. Native
VoiceOver reading and real Tab traversal remain Tier B checks; renderer
attributes alone do not establish those platform observations.

## Activity plot geometry amendment (2026-10-14, #1510)

The chart's padded linear x scale also owns bucket inspection geometry.
Neighboring marks split their hit regions at the midpoint; first and last
regions extend through the plot padding. A single bucket owns the full plot.
The existing Status owner, roving keyboard focus and immediate readout remain
unchanged. This is the dense chart role, referenced to HIG Charts/Stocks
scrubbing: an inspection target must contain the mark it describes.
The pinned component library has no matching chart hit-region component, so
the existing custom canvas and focusable overlay remain, with shared geometry.
Native regression inputs target actual painter-recorded series coordinates,
not overlay centers; focused regions must contain those same coordinates.
