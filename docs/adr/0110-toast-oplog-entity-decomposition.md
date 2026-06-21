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
- `OpLogPanel` is the next Step 5.1 candidate and follows the identical shape
  (it already mirrors the old `Rc<RefCell>` pattern).

## Rollout

One PR touching `src/ui/toast_stack.rs` (entity methods), `src/ui/render.rs`
(`impl Render for ToastStack`, free `big_sync_icon`, `render_toasts`), and
`src/ui/mod.rs` (field type, constructors, `open_main_window`, `push_toast`).
`cargo build` / `cargo test` / `cargo fmt --check` green. UI-affecting:
verify in the running app that toasts slide in/out, auto-dismiss, cap at 4, the
`×` dismiss works, and the busy/sync-spinner snackbars are unchanged.
