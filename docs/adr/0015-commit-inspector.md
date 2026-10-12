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

