# ADR-0017: Bottom Panel and Terminal Behavior

- Status: Accepted / Date: 2026-06-12(ADR-0007/0008 の補強)

## Decision
- default 高さ = **viewport の 18%**(≤20% 要件。最小 80px / 最大 60% は既存維持)。セッション内リサイズ記憶は既存維持、再起動間の永続化は設定ファイル導入時(later)
- Terminal: session 保持・cwd=root・出力の完全解析はしない(既存)。state refresh は **file watch(T029)を主**とし、補助として手動 Refresh。focus-out/command-exit フックは追加しない(watcher で十分、複雑化回避)
- 失敗時 Operation Log 自動オープン(既存)。Operation Log の各エントリは plan 概要・結果・失敗理由・復旧ヒント(recovery)を展開表示(既存 + recovery 表示を追補)

## Consequences
- B-1(default 18%)のみ即時変更。他は現状維持を正式仕様化

## Amendment — 2026-10-13: continuous divider resize (#1372)

The persisted height is the **body** height in reference pixels. The rendered
panel adds the scaled divider and tab strip. Capture body height and pointer Y
on divider press, then apply the scaled pointer delta to that origin; neither
the tab strip nor a grab off the divider centre can introduce a start jump.
The body retains its 80-reference-pixel minimum and viewport-height × 60%
maximum (converted back from screen space). Clamping does not replace the
press origin, so returning from either bound does not accumulate drift.
Viewport and zoom caps affect only the rendered height, never the preferred
height. Press captures both the fitted body height (for pointer deltas) and
the preference (for release change detection and an exact return to origin).
Shrinking then restoring a window therefore restores the larger panel.

Live moves change only UI state. Release writes the final changed body height
once through the settings store, whose repeated-key writes coalesce; subsequent
windows restore it. No repository operation or terminal lifecycle changes.
The existing divider has **no focus handle**, so the conditional keyboard
resize / named Separator path is intentionally not introduced by this fix.
Rest, hover and drag styling remain unchanged; focus-visible, selected,
loading, disabled and error states are not applicable to this pointer-only
divider. Tier B remains a real-window check of continuous pointer tracking.
