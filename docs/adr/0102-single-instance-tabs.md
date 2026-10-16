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
- **Application drain** (`ui::arm_open_requests`): armed once from `run_app`,
  after component initialization and the initial repository window. It takes
  the socket receiver and polls every ~200 ms alongside native URL delivery.
  Each request resolves a typed `KagiApp` repository host (never Settings or
  another utility window), recreates one with saved-session restore if none
  remains, calls `open_repository(path, cx)` (Backend-backed, **no git2 in UI**),
  then activates/notifies it. A `None` message only raises/recreates the host.
  Contract lines remain `single-instance: open tab <path>` and
  `single-instance: focus`.

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

### Amendment: Finder and Dock folder delivery (#1605, 2026-10-15)

- Register pinned GPUI's public `Application::on_open_urls` **before**
  `Application::run`. It has no app context, so decode local `file:` URLs and
  enqueue their paths; do not open a window or initialize widgets there.
  Queued launch-time deliveries are consumed only after normal UI setup.
- Decode percent escapes as filesystem bytes on Unix, preserving spaces,
  Unicode, literal `+`, and non-UTF-8 filenames. Accept an empty authority or
  `localhost`; ignore other schemes, remote authorities, malformed escapes,
  NULs, and unescaped query/fragment components. No dependency changes.
- The application owns both native and Unix-socket receivers for its entire
  lifetime. Closing the last repository window must not destroy the receivers
  or strand later requests; Settings-only and zero-window states recreate the
  repository host using the same dispatch path.
- Native URL paths use `KagiApp::open_external_paths`, the same directory-only
  entry as window folder drops. Directories reach existing `open_repository`
  for canonicalization, tab deduplication, repository validation, and existing
  error footer/toast presentation. Files are rejected through that same failure
  presentation, rather than discovering their enclosing repository.
- Native delivery joins the existing primary-instance dispatch locally; it does
  not forward to its own socket. CLI forwarding and headless single-instance
  gating are unchanged. Existing `single-instance:` klog messages are preserved.
- Decoder/startup-queue unit coverage lives in `src/single_instance.rs`;
  `folder_urls_existing_root`, `folder_urls_without_root`, and
  `folder_urls_settings_only` native GUI scenarios exercise URL batches through
  the actual enqueue/drain/opener path, including encoded aliases, duplicate
  delivery, file rejection, and root recreation with/without utility windows.

## Consequences

- **Unix only (macOS/Linux).** Non-unix targets get no-op fallbacks
  (`try_forward` → `false`, `bind_listener` → `None`), so a second invocation
  there launches a second window as before. The crate still compiles everywhere.
- No new crate dependencies (`std::os::unix::net`, `std::sync::mpsc`, `OnceLock`).
- The socket carries only a single repo-path line; no other IPC surface.
- `cargo build`, `cargo test --workspace` (incl. the binary-spawning i18n tests),
  `cargo fmt`, and `cargo clippy` stay clean. The git2-free-UI grep gate is intact
  (the only UI-side addition calls `open_repository` + `cx.activate`).
