# ADR-0110: Toast stack as a child `Entity<T>` (Phase 5 Step 5.1)

- Status: Accepted
- Date: 2026-06-21
- Implements: `/docs/refactor-plan.md` Step 5.1 (decompose `KagiApp` into child
  `Entity<T>` panels — `Toasts` first, the lowest-risk start)
- Follows: the `cx`-threading prep (PR #72) that gave every toast push a `cx`

## Context

The toast notification stack started as inline methods on the 100+-field
`KagiApp` god-struct, then (an earlier step) moved into a self-contained
`ToastStack` struct held as `Rc<RefCell<ToastStack>>`. The data/logic was
isolated, but rendering and re-render scope were not: every push, expire, and
slide-out tick called `cx.notify()` on **`KagiApp`**, repainting the entire app
(graph, sidebar, diff, status bar) just to animate a small overlay card. This is
one of the "329-notify repaints" the refactor plan targets.

`Rc<RefCell>` was a stopgap: `push_toast` had ~38 callers, many without a `cx`
in scope. PR #72 threaded `cx` through `record_op` and the remaining toast
callers and removed `push_toast_deferred`, so every push site now has a `cx` —
unblocking the move to an entity.

## Decision

Hold the toast stack as `toast_stack: Option<Entity<ToastStack>>` on `KagiApp`:

1. `ToastStack` gains `impl Render` (in `render.rs`, with the other `render_*`
   code): it renders only the flex-column of toast cards. The dismiss `×`
   listener is `cx.listener(|stack, _, _, cx| stack.begin_exit(id, cx))` — a
   `Context<ToastStack>` listener, so a click re-renders only the overlay.
2. The auto-dismiss ticker and per-toast slide-out timers move onto the entity
   (`push_notify` / `begin_exit` / `ensure_ticker`), each spawned via
   `Context<ToastStack>`. `toast_ticker_alive` moves from `KagiApp` to the
   entity. The pure data methods (`push` / `start_exit` / `remove` /
   `expiring_ids` / `has_pending`) stay `cx`-free so the unit tests are
   unchanged.
3. `KagiApp::push_toast` becomes a one-liner that does
   `stack.update(cx, |s, cx| s.push_notify(kind, msg, cx))`.
4. `KagiApp::render_toasts` still owns the absolute overlay container and the
   **busy snackbar** (driven by `busy_op`, which is `KagiApp` state); it embeds
   the toast entity as a child (`.child(toast_stack)`). Layout is unchanged — the
   container is one `flex-col gap-2` with the busy snackbar (if any) above the
   toast entity, which is itself a `flex-col gap-2` of cards.

`Option` is required because the pure `KagiApp` constructors (`from_snapshot`,
`with_error`) have no `cx`. The entity is created in `open_main_window`'s
`cx.new` closure (`app_state.toast_stack = Some(cx.new(|_| ToastStack::new()))`)
and is `None` only before the window exists, where `push_toast` is a safe no-op.

## Consequences

- A toast push / expire / dismiss now re-renders **only the overlay subtree**,
  not all of `KagiApp` — the Step 5.1 win.
- `KagiApp` sheds two fields' worth of toast plumbing (`toast_ticker_alive` and
  the `start_toast_exit` / `ensure_toast_ticker` methods) and the render no
  longer nudges the ticker each frame; `push_notify` (re)starts it.
- No `[kagi]`/klog contract lines and no toast message text changed; the toast
  path emits no contract lines, so the headless harness is unaffected.
- `OpLogPanel` followed with the identical shape (`Entity<OpLogPanel>`): the
  ring buffer plus the expanded-row / scroll UI state moved onto the entity, and
  `record_op`'s push + a row-expand click now re-render only the panel. Its one
  extra wrinkle: the panel is seeded from a disk-loaded tail read by the pure
  constructors, so `KagiApp` carries `op_log_seed` until `open_main_window` can
  build the entity (and the `KAGI_BOTTOM` headless dump reads the seed
  pre-window).

## Rollout

One PR touching `src/ui/toast_stack.rs` (entity methods), `src/ui/render.rs`
(`impl Render for ToastStack`, free `big_sync_icon`, `render_toasts`), and
`src/ui/mod.rs` (field type, constructors, `open_main_window`, `push_toast`).
`cargo build` / `cargo test` / `cargo fmt --check` green. UI-affecting:
verify in the running app that toasts slide in/out, auto-dismiss, cap at 4, the
`×` dismiss works, and the busy/sync-spinner snackbars are unchanged.

## Reading and dismissal amendment (#1240, 2026-10-12)

The dense notification overlay is transient feedback, not a second durable log.
Error cards do not auto-expire (unlimited lifetime, exceeding the ten-second
minimum), including while hovered or keyboard-focused. Explicit dismissal and
cap eviction can still remove a card being read. Info/Success/Sync keep a
four-second timer counting only unhovered time. The four-card cap always admits
the new notification and evicts the oldest existing non-error first. If all
four existing cards are errors, the new notification replaces the oldest error,
regardless of hover/focus; the Operation Log retains complete detail.
Stored error previews preserve at most three newline-separated lines and 768
characters plus an ellipsis. They wrap to at most three painted lines; visual
overflow also shows an ellipsis. Neither storage nor paint retains/shapes the
unbounded durable operation record.
The close control reuses the pinned gpui-kit ghost icon Button with its native
keyboard Tab/Enter/Space handling, focus ring, localized AX name and Tooltip.
Escape first cancels the existing modal/menu front layer, then clears diff-line
selection or closes the PR drawer, then dismisses the newest visible toast,
then performs the main-diff/Graph fallback. Terminal's existing Escape exclusion
and the single active-modal slot are unchanged. Removal and eviction notify the
parent with the removed control's retained FocusHandle, returning orphaned focus
to root like `release_closed`; a still-rendered Input/modal focus is not stolen.
The pinned Button does not implement GPUI's stateful accessibility setters;
a thin adapter delegates its interactivity/rendering unchanged and supplies
the icon-only AX name without painting a redundant visible label.
This remains a dense in-app feedback surface, not a Notification Center
replacement: reuse the existing card colours, spacing and enter/exit motion.
The error body uses a 1.4-rem line height and a three-line maximum height as
well as text clamping, so explicit newline paragraphs cannot bypass the cap.
Rest/hover/pressed/focus-visible close states are owned by gpui-kit; empty
stack has no cards or ticker, and no close action is disabled. Tier A
`toast_dismiss` uses an actual failed local fetch for the lifetime oracle and
measures a three-line component preview at 940×660/167% in EN and JA.
Review coverage also uses actual pointer hover on Info for ten seconds, verifies
the remaining timer after leaving, traverses to the real close Button with Tab
and activates it with Enter, and checks focus repair after keyboard dismissal
and focused Info expiry. Six-line/long-single-line inputs exercise truncation;
JA uses Japanese preview text. Button measurement wraps the component without
adding children that would change its production icon geometry.

## Status metadata amendment (#1337, 2026-10-12)

The retained toast entity owns a single stable `toast-stack` Status node.
Push records the newest preview's ID and text; that text names the node.
Timer, hover, enter/exit animation, cap eviction and dismissal never promote
an older notification into a new arrival. The latest arrival metadata drains
when the stack becomes empty. No focus changes accompany arrival.

The pinned GPUI fork (`caf5007`) has no `aria_live`, live-region priority or
announcement setter in its stateful accessibility API. Therefore the node
exposes Status and its updated label, but neither polite/assertive delivery
nor exactly-once native VoiceOver speech is guaranteed. Tier A observes
renderer-applied role/name and stable arrival identity, not announcements.
The bounded previews, persistent errors, hover pause, keyboard dismissal and
durable Operation Log ownership above are unchanged.

## Viewport budget amendment (#1551, 2026-10-14)

This dense in-app feedback surface follows the HIG Notifications principle of
concise, non-invasive foreground presentation, not Notification Center styling.
Show the newest existing preview and an EN/JA named counted disclosure for the
remaining cards. Do not deduplicate by truncated preview: distinct failures can
share a prefix, and every operation still owns its full durable receipt/cause.
The pinned ghost Button owns Tab/Enter/Space and its focus ring; its localized
name changes to Collapse when expanded. Expanded cards scroll inside the same
30%-of-viewport height budget, independent of UI zoom. The empty/single-card,
error lifetime, explicit dismissal and four-card eviction policies remain.
The existing card role colours, typography, hover pause and motion are reused;
there is no new global settings or notification owner.
The shared out-of-flow host originally moved to the trailing side, below toolbar
chrome, to leave Sidebar/search and the leading Graph hit lane available and
avoid covering the bottom Operation Log. Its placement is superseded by #1590
below; the cap of at most 45% of the viewport width remains.
Disclosure stays above the card scroller so Collapse cannot scroll out of reach.
Native Click actions share the disclosure/dismiss listeners, and hiding controls
repairs their retained focus through the existing ToastRemoved contract.
Review: hidden cards resume their hover-paused timers both on newest-card
replacement and on Collapse; the old subtree cannot be relied on to deliver
hover-leave after it is removed. Open Repository also owns an explicit native
Click action, so activation of a scroll-clipped action does not depend on
synthetic pointer hit-testing. The English one-older-card caption is singular.

## Bottom-left anchor amendment (#1590, 2026-10-15)

Restore a fixed bottom-left anchor for the shared notification host in both
repository and Settings windows, above the status bar rather than below the
toolbar on the trailing side. Adding notifications or expanding the stack grows
the overlay upward; its bottom-left origin does not move with its content.
This supersedes #1551's host-placement decision while retaining its bounded
disclosure policy. Settings uses the same busy-snackbar/toast renderer.

Keep the newest preview and localized EN/JA counted disclosure for older cards.
Expanded cards still scroll under the same zoom-independent 30%-of-viewport
height upper bound, with the disclosure outside the scroller so Collapse stays
reachable. In compact repository windows (under 1000 logical px) with the bottom
panel open, further cap the notification container to the bottom-panel body
height minus the toast inset. Reuse the panel's `bottom_panel_body_height`
calculation so the compact tab strip's primary controls remain exposed, without
moving the exact bottom-left anchor. Retain the 45%-of-viewport width cap,
bounded previews, persistent
errors, hover-paused routine timers, explicit dismissal, four-card eviction,
keyboard/accessibility actions and focus repair. Complete operation details
remain in the durable Operation Log; this is a placement correction, not a new
notification owner, setting or lifetime policy.

Acceptance geometry uses repository widths of 1000/1400 logical px plus 940px
compact coverage. The owner accepted Settings' actual native 715px width in
EN/JA at 100%/167% zoom instead of imposing repository-sized 1000/1400px frames:
production `fit_content` restores that width, and the E2E must not bypass it.
Tier B visual review remains pending while the screen is locked; this amendment
does not claim completed screenshots or native visual acceptance.
