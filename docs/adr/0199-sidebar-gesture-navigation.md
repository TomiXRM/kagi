# ADR-0199 — Sidebar gesture navigation

- Status: accepted
- Date: 2026-09-18
- Supersedes the release-only mode switch added with the Arc-like navigation row.

## Context

A horizontal trackpad gesture over the sidebar used to switch workspace mode at
the instant the fingers lifted: nothing moved during the gesture, and the whole
window changed at once. That reads as a jump, and it gave the user no feedback
about which page a gesture was heading for, or that a short gesture would be
rejected.

The obvious fix — a paged scroller — is the wrong shape here. The main pane is
not a page in a horizontal strip: it is the workspace, and it must stay
anchored. Only the sidebar has a previous/next.

## Decision

The gesture moves the **sidebar only**. `prev`/`current`/`next` exist for the
sidebar and nowhere else; the main pane is never a paged scroll renderer and
never translates.

1. **One state machine, in the domain.**
   `kagi_domain::sidebar_swipe::SidebarSwipe` is `Idle → Tracking → Settling →
   Idle`. It owns the gesture's raw distance, the resisted visual offset, and
   `pending` — the page a release committed to. It does **not** own the active
   page: that stays `KagiApp::workspace_mode()`, so there is no second copy of
   which workspace is on screen.
2. **The origin is fixed for the whole gesture.** `start(origin, page_count,
   width)` pins it, and the only candidates are `origin ± 1`. Any amount of
   scroll delta therefore moves at most one page.
3. **Resistance.** `offset = width * tanh(raw_dx / width * 0.35)`. The sidebar
   always trails the fingers, and `tanh` asymptotes at exactly one width, so no
   gesture can drag a second page into view.
4. **The commit decision uses raw distance,** not the resisted offset:
   `|raw_dx / width| >= 0.20`. A release under that boundary snaps back.
5. **Settling is a spring, not a jump.** The settle starts from the offset the
   gesture ended at and springs to `±width` (commit) or `0` (cancel). It is
   critically damped, so it never overshoots into the page beyond. Driven by a
   timer task in `release_sidebar_gesture`, ticked with a fixed step, which
   makes it deterministic under the test clock (`advance_clock`).
6. **Logical navigation happens only when the settle rests.** In order: the
   state machine clears `pending` and normalises the offset to `0`, then
   `finish_sidebar_settle` switches the workspace mode — which re-bases the
   sidebar on the new active page and moves the main pane content with it. The
   main pane is untouched for the whole gesture and the whole settle.
7. **Renderer separation is structural.** `workspace_mode::render_sidebar_pages`
   is the *only* renderer that sees the offset: it draws the fixed shell, the
   pinned page navigator, the clipped viewport, and the two pages inside it.
   Neither `render_body`, `render_pr_mode` nor `render_issues_mode` receives the
   offset or the gesture state — they name their page and read
   `workspace_mode()` for the main pane, so criterion "the main pane must not
   move 1px" cannot regress by accident.
8. **One page-content builder.** `page_content(app, mode, cx)` builds any page's
   body — Graph's navigator, the PR list, the Issue list — so the page a gesture
   *heads for* is the same element tree as the page it *lands on*. That is why
   `render_sidebar` derives its inputs (rows, scroll handle, filter, merged
   count) from `app` instead of having them threaded from `render`: the second
   call site (previewing a neighbour) has no such thread.

Pages are positioned **absolutely** in the viewport. A margin would shrink each
page's content box and re-lay-out the list as it slides; the gesture must
translate a page, not reflow it.

The adjacent page shows **already-loaded content when there is any**, and a
shell otherwise (`page_is_cached`). Rendering a page is a pure read — it is
`show_*_mode` that fetches — so previewing the neighbour cannot start an
inactive workspace's GitHub reads. Graph counts as always loaded: `sidebar.rows`
is flattened every frame regardless of which page is on screen. A GitHub page
with an empty list is treated as *not* cached, because drawing its empty state
mid-gesture would claim the neighbour has nothing in it.

**The gesture owns the wheel while it is live** (`SidebarSwipe::owns_wheel`), so
a horizontal swipe has no vertical component. A scrollable list is the innermost
hitbox under the pointer and consumes the `y` delta itself, which no bubble-phase
listener can undo — gpui exposes only bubble-phase `on_scroll_wheel`. So while
the gesture owns the wheel, one `occlude()`-ing layer covers the viewport and
carries the same listener: `Hitbox::should_handle_scroll` is false for everything
a `BlockMouse` hitbox covers, so the pages beneath stop scrolling entirely. The
claim starts with the gesture and is dropped the moment the axis resolves
*vertical*, so a vertical gesture keeps scrolling the list; it is held for the
whole settle so momentum cannot scroll the page either.

Known limit: the layer appears on the frame *after* the gesture's first event,
so a swipe that begins over a scrollable list leaks that one event's `y` delta.
Closing it would need a capture-phase wheel listener, which gpui does not offer.

Reduced motion (ADR-0173) skips the animation and runs the same terminal
immediately, so the navigation still happens through one code path.

## Consequences

- `SidebarSwipe` grew from a release-time verdict to the gesture's whole
  lifecycle; the three sidebar renderers each lost their own wheel listener,
  navigator row and shell, which now exist once.
- The commit boundary is a fraction of the *rendered* sidebar, so it scales with
  UI zoom — deliberate: 20% of what the user sees, per the spec.
- The spring runs at ω = 60 rad/s (~150 ms to rest). Semi-implicit Euler needs
  `damping * step < 2`, which a whole 60 Hz frame sits exactly on at that ω, so
  `MAX_STEP` sub-steps each frame four times. Raising ω again means lowering
  `MAX_STEP` with it.
- Momentum scroll on macOS arrives as `Moved` with no `Started`
  (`gpui_macos::events`), so an idle state machine ignores it; a gesture cannot
  begin while a settle is still running, which is what keeps "one gesture, one
  page" true across back-to-back flicks.
- The `Pull Requests (N)` row was removed from the Graph sidebar in the same
  change: the pinned page navigator already names PRs, and the row was a second
  entry point to the same takeover.
