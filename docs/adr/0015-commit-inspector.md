# ADR-0015: Commit Inspector Right Panel

- Status: Accepted / Date: 2026-06-12

## Decision
- 並び: **1. Summary(タイトル・short SHA・copy ボタン・ref ラベル)→ 2. Metadata(author/authored・committer/committed・parents・full SHA・本文)→ 3. Contextual Actions(Create branch / Cherry-pick / Copy SHA。Revert・外部リンクは later)→ 4. Changed Files(count・status・Path⇄Tree トグル・選択ハイライト)**。Diff は main pane(T-UI-003 決定の維持 — 原文要件4の「右ペイン内 Diff Viewer」は main pane 方式で満たす)
- 原則「**情報が先、危険操作は後**」。history-changing は plan 経由(既存)
- copy SHA はクリップボード API(gpui)で full SHA をコピー(ZWSP 等の混入禁止 — raw 値を使う)
- 実装は `src/ui/inspector.rs` へ抽出

## Consequences
- 既存の Create branch/Cherry-pick ボタンが上部から Actions セクションへ移動(導線変更をユーザーに周知)

## Amendment — 2026-10-12: complete messages and bounded presentation (#1307)

The complete sanitized commit message is retained in `CommitDetail`, with only
trailing whitespace trimmed; no 2,000-character model cap remains. Copy message
uses this full source, not reflowed HTML or a preview. The SHA-keyed Inspector
model prepares HTML, co-authors and trailers once per selection identity, not
on each render.

The existing independently scrolling message region exposes the entire body.
Co-author and trailer blocks belong below the body inside that same scroller,
not in the fixed header: large trailer sets cannot consume the message/files
split or push changed files and the footer outside the window. Title, author,
hash, refs and actions retain their compact header. No explicit expansion state
is needed because the message region already has definite bounds and the
system scrollbar.

### Stress render evidence (#1306 / #1307)

The native `text_caps_warm_render` scenario mounted the read-only
`stress-pr-repo` fixture (121 local branches, 126 commits, 120 graph lanes),
selected its first commit and settled 20 draws. Each measurement is the median
of 31 ten-draw batches, in the same debug profile and default 1440×900
presentation. After the review-follow-up gates completed, frozen original
`50b28c69` production sources and the final production sources ran alternately
three times each with the same harness:

- Before: 23.536750, 13.202917, 13.132425 ms/frame; median **13.202917 ms**.
- After: 13.603454, 13.594504, 13.192571 ms/frame; median **13.594504 ms**.
- Median change: **+2.97%**, within the requested 5% bound.

Concurrent machine load remains a limitation: earlier non-alternating After
runs ranged from 13.425571 to 19.909288 ms/frame. The earlier alternating
series measured +3.67% and had 34.361996/44.905217 ms outliers; the final
series also has a 23.536750 ms Before outlier. These are forced warm-render
timings, not real-app FPS, an idle-machine guarantee, or a cold-message
preparation benchmark.


## Amendment — 2026-10-13: changed-file keyboard entry (#1375)

The session retains stable full-path row focus for Inspector changed files,
independent of whether the main diff is visible. One roving stop uses shared
RowFocus/RowList arrows/Home/End and destination reveal; Enter/Space reuse the
existing exact-file diff opener, preserving graph selection and Git state.
Directory headings remain structural. Generated disclosure shares this owner,
exposes expanded/collapsed state, toggles with Enter/Space and expands with Right.
Named selected file rows include localized change status and sanitized full path,
never only a potentially ambiguous basename. Focus-visible uses the shared ring.
The retained native list virtualizes viewport rows. A scaled height estimate
gives unmeasured rows a scroll extent; actual measurements replace it, and
zoom changes remeasure while preserving the scroll anchor. The existing
100-file display cap and generated grouping remain unchanged.
Disclosure changes splice only generated children, preserving the existing
list anchor and the disclosure's measured bounds. A native list with a
trailing overflow note reveals that note when its last focusable row is
reached, without turning the note into another Tab stop or replacing the
file's real entry index. Regression coverage uses a viewport-exceeding
Generated fixture and freshly cleared overflow probes after measured-row
arrow navigation, so old offscreen probe bounds cannot satisfy the check.

## Amendment — 2026-10-13: sidebar-density Inspector (#1464 / #1467)

The Inspector is a dense contextual list. Row density follows Kagi's sidebar;
the owner's Finder list reference (#1467) supersedes the initial `text_xs`
request with a 13-reference-pixel font and 16-reference-pixel badge column.
UI-core's `theme::FILE_TREE_FONT_PX` is shared through the existing
`text_size(scaled_px(token))` convention. Other file/ref surfaces adopt it
independently; this amendment changes only Inspector.
Files, directories, Generated disclosure and the unfocusable More note share
sidebar's `SIDEBAR_ROW_H` for their explicit 20-reference-pixel height and the
native list's unmeasured extent. No per-row margins inflate the scroll estimate.
File badges/names use 1.2em leading: 1em clips Inter descenders in the truncated
name column's content mask. The existing fixed focus ring still fits inside
the dense frame. Tree indentation remains 12 reference pixels. At 167%, rows
exceed 20 logical pixels. Finder's grouping, font family and selection policy
are not copied by this size-only change.

Top padding is halved (`pt_2` → `pt_1`); metadata, co-author/trailer and action
bottom margins use `mb_1` rather than `mb_2`. Comparing uses `py_px`/`mb_1`.
Title leading, avatar/hash sizing and message/files split semantics are unchanged.
Rest, hover, selected and focus-visible states retain their existing colours
and shared RowFocus ring; disclosure/Enter/Space, overflow reveal, localized
full-path AX names, unavailable reads and the 100-file cap are unchanged.
No Git operation or confirmation policy changes.

`inspector_scale` records the old 24-reference-pixel estimate as a failing
oracle and pins native row geometry, badge/name typography, first-file offset
against frozen before measurements, directory/disclosure/More heights,
keyboard activation and preferred widths. It covers 1392×883 / 940×660,
100% / 167%, EN / JA and Apple Light / Dark. Existing #1375 keyboard and
#1355 pane-fit coverage remain authoritative. Current narrow presentation
keeps the fitted right slot: below-center placement is unmerged #1225 and
is intentionally not introduced by this scale-only amendment.

Native regression evidence: the original estimate failed at 21.599998 logical
pixels (24 reference × 0.9 base) versus the required 18 (20 reference × 0.9).
Actual old file frames were 25.5 / 39 logical pixels at 100% / 167%;
explicit new frames measure 18 / 30, including the fixed focus border.
First-file offsets from the header top changed 416.5 → 410 (wide, 100%),
338.5 → 332 (940px, 100%), 487.5 → 476 (wide, 167%), and
436.5 → 425 (940px, 167%); both languages/themes gave identical geometry.
The scoped Inspector/File History/Changes/pane-budget/graph-fit run passed
all 17 scenarios without changing an existing neighboring oracle. These
are renderer/simulated-keyboard measurements, not VoiceOver or real-app FPS.

Tier B: default-feature builds captured the same fixture commit `1955d837`
at 1392×883, EN, Apple Light/Dark and 100%/167%, using isolated PID-targeted
input and window-ID captures. Native-scale before/after crops and a Finder
list-view/Kagi side-by-side comparison are retained outside Git under
`/tmp/kagi-hig-audit-20261008/inspector-scale-tierb/`. The Finder comparison
does not resample either image; Kagi's existing 0.9 density base remains,
so 20/13 reference pixels are 18/11.7 logical pixels at 100%.
The narrow 940px/167% attempt delivered selection but retained an occluded
stale frame; PM explicitly accepted the wide matrix plus Finder comparison
instead of requiring activation or a narrow screenshot. Native Tier A still
covers all narrow geometry/keyboard legs. All launched processes exited, and
only the newly created Finder reference window was closed after capture.
VoiceOver delivery, IME and real-app FPS were not exercised.

Review regression: filename fixtures contain `package-000.json`, including
`p`, `g` and `j` descenders. The native text system shapes the name and computes
baseline-relative glyph extents against the actual paint-stage content mask.
Before the 1.2em correction, `p` ended at 472.3941px beyond the 471.5px mask:
the new assertion failed verbatim rather than accepting a contained line box.
Glyph containment is checked in every Inspector scale presentation cell.
The review-scoped native run again passed all 17 scenarios, including the
16 glyph-containment matrix cells. The four wide Tier B after captures were
refreshed with default features and unchanged fixture commit `1955d837`;
`review-comparison-*.png` and `review-finder-kagi-native-scale.png` are the
final owner-facing comparisons. Each raw capture is 2784×1766, matching the
baseline's 1392×883 logical window without resampling or window shadow.

## Amendment — 2026-10-15: full-width changed-file options (#1559)

Inspector file options explicitly fill the virtual list's available width.
Their existing click handler, hover/selection fill and shared keyboard-focus
border belong to that same outer row. Tree indentation is internal left
padding; the filename remains flexible and truncating, while the diffstat
retains its existing trailing nonshrinking column. Path and Tree therefore
share a full-row target rather than a target sized to their text.
No focus/selection owner, keyboard activation policy, repository read,
setting, receipt or klog contract changes.

`inspector_file_row_hit` exercises filename, diffstat and right-edge clicks,
native selection/focus pixels and arrow/Space activation at 1000×720 and
1400×900, EN/JA, 100%/167%, Apple Light/Dark, Path/Tree. Each case uses
the actual list's right edge rather than deriving its click from the
potentially undersized row, and compares repository fingerprints.
The file-row geometry probe forwards the leaf's own layout, prepaint and paint
without adding a container that stretches intrinsically sized rows only in tests.
The pre-fix right-edge click fails with `left: None` versus
`right: Some("src/nested/a.txt")`.

