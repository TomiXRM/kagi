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
