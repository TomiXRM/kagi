# ADR-0102: Single-instance — `kagi <repo>` opens a tab in the running window

**Status:** Accepted — 2026-06-19
**Refs:** ADR-0027/0028 (tabs), ADR-0096 (`klog!` contract), ADR-0077 (headless harness)

## Context

Running `kagi <repo-dir>` in a terminal while a Kagi instance was already open
launched a **second process and window**. Power users opening several repos from
the shell ended up with a window per repo instead of tabs in one window, and the
new window did not necessarily come to the front.

The desired behaviour: a second `kagi <repo>` invocation should hand the repo to
the **already-running** instance, which opens it as a new tab (deduped) and
raises its window. Bare `kagi` (no arg) while running should just focus the
existing window. With no instance running, behaviour is unchanged.

## Decision

A per-user **Unix domain socket** at `${TMPDIR}/kagi-instance-<user>.sock`
(`single_instance.rs`, shell level — outside the `src/ui/` git2 invariant):

- **Secondary process** (`try_forward`): connect to the socket and write one
  line — the canonicalized absolute repo path, or an empty line for focus-only —
  then exit (`klog!("forwarded to running instance")`). Any connect/write error
  → return `false` and launch normally (become the primary).
- **Primary process** (`bind_listener` + `spawn_accept_thread`): unlink any stale
  socket file, `bind`, and run a background `std::thread` that `accept()`s
  connections, reads the first line, and sends `Some(path)` / `None` over an
  `mpsc` channel. The `Receiver` is stashed in a module `OnceLock<Mutex<Option<…>>>`.
- **UI drain loop** (`KagiApp::arm_single_instance_listener`, the only UI-side
  code): armed once from `open_main_window`, it `take()`s the receiver and mirrors
  `arm_watcher` — a `cx.spawn` loop that polls `try_recv` every ~200 ms, calls
  `open_repository(path, cx)` (Backend-backed, **no git2 in UI**) for a path, and
  `cx.activate(true)` (the same window-raise call the Dock `on_reopen` handler
  uses) plus `cx.notify()`. A `None` message activates the window only. New
  contract lines: `single-instance: open tab <path>` and `single-instance: focus`.

### Headless gating (critical)

The single-instance logic is **disabled under the `KAGI_*` headless harness**:
`main::headless_mode()` returns `true` when any of `KAGI_LOG_DIR`,
`KAGI_OPEN_REPO`, `KAGI_MENU_DUMP`, `KAGI_SELECT_FIRST`, or the explicit override
`KAGI_NO_SINGLE_INSTANCE` is set. The integration tests that spawn the real
binary (`tests/i18n_test.rs`) all set `KAGI_LOG_DIR` to isolate settings — that is
the gating signal. Without this gate, parallel test binaries would share one
socket: one could forward to / focus another, breaking the `[kagi]` stderr
contract and the read-a-line-then-kill flow. Real GUI launches set none of these.

### Fallback / cleanup

- `bind_listener` returning `None` (permissions, exotic temp dir) is non-fatal:
  the primary runs normally **without** single-instance.
- Stale-socket recovery relies on `bind_listener` unlinking the file at startup,
  so a previous crash never permanently blocks. No exit-time cleanup is needed.

## Consequences

- **Unix only (macOS/Linux).** Non-unix targets get no-op fallbacks
  (`try_forward` → `false`, `bind_listener` → `None`), so a second invocation
  there launches a second window as before. The crate still compiles everywhere.
- No new crate dependencies (`std::os::unix::net`, `std::sync::mpsc`, `OnceLock`).
- The socket carries only a single repo-path line; no other IPC surface.
- `cargo build`, `cargo test --workspace` (incl. the binary-spawning i18n tests),
  `cargo fmt`, and `cargo clippy` stay clean. The git2-free-UI grep gate is intact
  (the only UI-side addition calls `open_repository` + `cx.activate`).
