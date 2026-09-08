---
name: kagi-verify
description: Verify Kagi changes against fixture repositories, the native GUI, and the browser story harness. Use for runtime, fixture, or E2E validation work in this repository. Driving the real GUI does NOT require taking the pointer or the foreground: Tier B uses scripts/pidclick.swift (CGEventPostToPid). cliclick is banned for new validation.
---

# Kagi verification recipe

Canonical source: `.claude/skills/verify/SKILL.md`. Codex reads it through
`.agents/skills/kagi-verify`.
PRs changing verification seams, flags, scripts, or runner features must update
this skill and state `skill 更新済み` in the PR body (see Maintenance).

Choose the smallest evidence lane that proves the change. Tier A is deterministic
native UI state coverage, Tier B is a real running application, and Tier C creates
the Git states needed to exercise safety-sensitive flows.

**Read this before driving the GUI.** Two rules are load-bearing and easy to miss by
skimming, so they are stated here as well as where they apply:

- **Never use `cliclick` for new validation.** It moves the user's pointer and targets
  the frontmost application, so it takes the machine away from the user and aims at
  whatever happens to be in front. Tier B's `scripts/pidclick.swift` posts events
  straight to the process with `CGEventPostToPid`: the pointer does not move, the
  foreground application does not change, and the target is the window you named. The
  user can keep working while a scenario runs.
- **Never run the full `gui_e2e_runner`.** Always scope it with `KAGI_GUI_E2E_ONLY`.
  An unfiltered run once opened roughly 1,400 windows and crashed macOS.

| Need | Lane |
| --- | --- |
| Deterministic assertions on real UI state, no windows on the user's screen | Tier A runner (`KAGI_GUI_E2E_ONLY` always) |
| A real running app, real clicks, without taking the pointer or foreground | Tier B `pidclick` |
| Git states for safety-sensitive flows | Tier C fixtures |

## Tier A — native GUI E2E runner

`tests/gui_e2e_runner.rs` is an opt-in macOS main-thread runner. It needs both the
`gui-e2e` feature and `KAGI_GUI_E2E=1`; use an exclusive target directory:

`KAGI_GUI_E2E_ONLY` is not optional — every invocation names the scenarios it
wants. There is deliberately no unfiltered example here to copy: without the
filter the runner opens a window per scenario, which is the ~1,400-window path
that crashed macOS.

```bash
# One scenario: the substring the scenario name contains.
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='bottom_panel' \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture

# Several: comma-separated substrings, any of which may match.
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='bottom_panel,graph_copy' \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
```

It uses `VisualTestAppContext` with deterministic assertions. Its test windows
can appear at the primary display's top-left while a scenario runs; each must
disappear when the scenario ends, and none may remain when the runner exits.
Do not use it to wait on a child process or to validate IME/focus behavior:
`TestDispatcher`'s `run_until_parked` waits while a job is outstanding. Each
new scenario must call the runner's shared `unmount(cx, entity, window)` helper,
which calls `remove_window`, drops the root entity, flushes the app, and then
drains the dispatcher. Drop retained child entities before that helper when
needed, otherwise the runner's leak detector can retain them.

`KAGI_GUI_E2E_ONLY` is a comma-separated, case-sensitive substring filter.
Whitespace is trimmed and empty entries are ignored; only an unset value runs
the full suite. An all-empty value or a filter with no enabled scenario match
exits nonzero after reporting each scenario as filtered out.
**Do not run the full suite by hand.** A full run once opened 1,408 real
`NSWindow`s at once and the WindowServer watchdog killed the login session
(#549); always scope it with `KAGI_GUI_E2E_ONLY=<substr>`. Windows now come from
the runner's `macos::open_offscreen` helper, which panics past a budget of 8
live windows and opens them hidden — set `KAGI_GUI_E2E_VISIBLE=1` to see them
for triage.

The current suite covers:

- durable stash-drop recovery; history persistence; cleanup stale-tab,
  preflight, open-failure, and partial presentation; remove's public boundary;
  editor writer admission; commit-row and editor-history layout;
- bottom-panel toggle, graph copy, oplog expand/copy, snapshot creation, theme
  switching, agent provenance, and WIP-to-HEAD connectors;
- linked-worktree WIP rows plus commit-panel commit, amend, and discard;
- modal and branch-menu Enter isolation from the selected commit checkout;
- unmerged branch deletion with two confirmations, retained tips, and one-stage merged deletion.
- dirty Pull auto-stash success and Pull-failure restoration, including the persistent error modal.

For worktree-decorated branch checkout, scope
`KAGI_GUI_E2E_ONLY=graph_worktree_open`. The scenario double-clicks the actual
branch-name hitbox, then dispatches selected-commit checkout after reselecting
the row. On a clean fixture both plans must resolve the undecorated branch and
show the typed other-worktree occupancy blocker. Enter must not change either
worktree's HEAD/index/files; no untracked-file warning may mask auto-execution.
The existing tree-glyph navigation checks still run. Backend regressions in
`tests/ops_test.rs` cover main/linked occupancy, occupancy arising after approval,
and successful checkout after the sibling detaches.

App keybindings, command-registry keybindings, and native menus share their
installation path with `run_app`; component initialization must precede that
installation so app bindings keep the same precedence. For menu/Enter routing,
PM can scope `KAGI_GUI_E2E_ONLY=branch_menu_no_checkout_fallthrough,modal_no_fallthrough`.
The menu scenario in `tests/recovery/operations.rs` selects a non-HEAD branch,
opens its menu, and checks that Enter leaves the checkout modal absent and HEAD
unchanged. The modal slot is the oracle for absence of `plan: checkout`, as in
the existing modal scenario. Native foreground/input-focus behavior remains a
separate Tier B check.

The runner's screenshot capture is best-effort only; its assertions, clipboard
checks, refs, and persisted oplog records are the oracle. It cannot run from the
Codex sandbox because the required macOS `hiservices` XPC path is unavailable;
have the PM or a human run this lane.

The locked GPUI revision does not expose the complete, last-rendered hitbox
collection to `VisualTestAppContext`. Do not treat a scenario-specific recorded
bound (for example, a measured button or footer) as a generic hitbox dump. A
complete `id -> window-relative bounds` failure diagnostic needs a public GPUI
API or a deliberately maintained dependency fork; until then, use Tier A's
state assertions and Tier B's live-window inspection together.

## Tier B — real GUI driver

Build `scripts/pidclick.swift`, launch Kagi with a unique `USER` value, and retain
all three isolation flags. `USER` namespaces the per-user socket name, while
`KAGI_NO_RESTORE=1` and `KAGI_LOG_DIR` prevent fixture work from changing the
user's persisted session, settings, trust, or oplog. `KAGI_NO_ACTIVATE=1` avoids
stealing the foreground application.

```bash
swiftc scripts/pidclick.swift -o /tmp/pidclick
VERIFY_USER="kagi-verify-$RANDOM"
VERIFY_LOG_DIR="$(mktemp -d)"
USER="$VERIFY_USER" KAGI_NO_ACTIVATE=1 KAGI_NO_RESTORE=1 \
  KAGI_LOG_DIR="$VERIFY_LOG_DIR" ./target/debug/kagi /tmp/kagi-vfx-a/repo \
  2>"$VERIFY_LOG_DIR/kagi.stderr" &
PID=$!
/tmp/pidclick windows --pid "$PID"
# Select Kagi's layer-0 application window from this listing, then set its ID:
WID=12345 # Replace with the selected window ID from the listing.
```

`scripts/pidclick.swift` sends events with `CGEventPostToPid`; it does not move
the user's pointer or activate another app. Select a window with `windows --pid`,
then use its ID with `--window-id` and `move`, `click`, `rclick`, `key`, or `type`.
Coordinates are logical points relative to the window's upper-left corner. `click`
and `rclick` include a hover primer. `type` derives virtual key codes from the
active keyboard layout, so it is unsuitable for IME, dead keys, unsupported
characters, newline, or Tab input. Screenshots are physical pixels; do not mix
them with pidclick's logical coordinates.

```bash
/tmp/pidclick --pid "$PID" --window-id "$WID" move 120 80
/tmp/pidclick --pid "$PID" --window-id "$WID" click 120 80
/tmp/pidclick --pid "$PID" --window-id "$WID" rclick 120 80
/tmp/pidclick --pid "$PID" --window-id "$WID" key 53
/tmp/pidclick --pid "$PID" --window-id "$WID" type 'filter text'
screencapture -x -o -l"$WID" "$VERIFY_LOG_DIR/window.png"
tail -f "$VERIFY_LOG_DIR/kagi.stderr" # [kagi] contract lines
```

Read `$VERIFY_LOG_DIR/operations.jsonl` as well as the visible footer and
`[kagi]` log. `KAGI_LOG_DIR` intentionally makes `src/main.rs::headless_mode()`
true, so this isolated launch does **not** create or forward through the
single-instance socket. The worktree command policy deliberately excludes
`KAGI_LOG_DIR` from its own headless-command check; the two policies differ.
Do not add `KAGI_NO_SINGLE_INSTANCE`: it was a headless-routing workaround before
#535 and is not the standard launch recipe. Do not remove `KAGI_LOG_DIR` just to
test socket forwarding, because that would risk the user's persisted state.

### Socket behavior, when it is the subject under test

In a normal GUI launch without a headless signal, the socket is named from
`USER`; a second `kagi <repo>` forwards an open-tab request and a bare `kagi`
forwards focus. Its receiver logs tab/open activity and raises the existing
window. This is separate from the isolated Tier B command above, whose log
directory prevents socket creation.

`cliclick` is historical and must not be used for new validation. It targets the
global pointer/frontmost application. `pidclick` needs Accessibility permission
for the responsible process; `screencapture` needs the applicable Screen Recording
permission. Use `screencapture -x -o -l<WID>` for a window-only image; the known
broken `-R` rectangle path is not a reliable substitute.

## Tier C — fixture states

Create the standard disposable repository only below `/tmp` (the script refuses
to overwrite an existing fixture):

```bash
bash scripts/make_fixture.sh /tmp/kagi-vfx-a
REPO=/tmp/kagi-vfx-a/repo
```

It supplies main/feature branches, a merge, a tag, origin, one stash, and a dirty
working tree. For the three-stash menu and all four stash operations, consume the
initial dirty tree as stash two, then create stash three:

```bash
git -C "$REPO" stash push -u -m 'fixture stash two'
printf 'fixture stash three\n' >> "$REPO/a.txt"
git -C "$REPO" stash push -m 'fixture stash three'
git -C "$REPO" stash list
```

For the manual slow-remove check, create a linked `slowpre_remove` worktree and
commit this configuration there before opening the remove plan:

```bash
SLOW_WT=/tmp/kagi-vfx-a/slowpre_remove
git -C "$REPO" worktree add -b slowpre_remove "$SLOW_WT"
mkdir -p "$SLOW_WT/.kagi"
cat > "$SLOW_WT/.kagi/worktree.toml" <<'TOML'
[[pre_remove]]
type = "command"
run = "sleep 10"
TOML
git -C "$SLOW_WT" add .kagi/worktree.toml
git -C "$SLOW_WT" commit -m 'add slow pre-remove fixture'
```

The command step is a trust-gated precondition. Confirm its displayed plan, begin
remove, then attempt an editor save while it is running: the save must remain Busy
with its bytes and buffer preserved. A failed, untrusted, or headless-blocked
pre-remove command keeps the worktree; it never proceeds with removal.

| Family | Manual M check |
| --- | --- |
| Remove | Plan modal → execute → footer and oplog; while slow `pre_remove` runs, saving is Busy. |
| Stash | Exercise push, apply, pop, and drop against the three-entry fixture. |
| Stash conflict | Pop a conflicting entry → resolve → Continue → confirm the suggested drop path. |
| Conflict | Continue, then verify conflicts are re-detected immediately. |
| Backend policy | Toggle auto-snapshot OFF/ON; reset and amend must use the same policy from button and Enter. Guarded rebase keeps its existing no-auto-snapshot rule; explicit restore still creates its mandatory savepoint. CLI/MCP default to snapshots ON independently of GUI settings. Confirm create-branch with checkout ON/OFF: ON switches to the new branch with one backend operation; OFF keeps HEAD. Absorb with an index-write failure must show Partial and its original full OID, never a plain Failed after HEAD advanced. Worktree config added/removed/changed after the plan must refuse before target creation. |

### Parallel fixture isolation (#573)

Git fixtures use `tests/support/isolated.rs`: each parent test starts only its
own exact test in a child process with a fresh `KAGI_LOG_DIR`. The parent owns
the TempDir until the child finishes. This preserves normal parallelism without
changing the parent process's environment or sharing the user's oplog. Tests
that specifically exercise environment overrides may change them inside that
single-test child. Prefer explicit directory arguments for pure path tests.

The oplog refuses its default home path when runtime `CARGO_MANIFEST_DIR` is
present and `KAGI_LOG_DIR` is absent: `tests must set KAGI_LOG_DIR`. Do not remove
the Cargo marker or disable recording to make a fixture pass. Use the child
helper or pass an explicit storage directory through an existing API.

For an environment-race fix, run the following three times consecutively with
default test parallelism and record the results in the PR:

```bash
KAGI_LOG_DIR="$(mktemp -d)" CARGO_TARGET_DIR="$PWD/target" cargo test -j 8 --workspace
```

`-j 8` controls Cargo build jobs; it does not serialize Rust test threads. Do
not use `RUST_TEST_THREADS=1` to mask a race. GUI runner execution is a separate
lane and is not part of this fixture check.

## Other runtime seams

Use a real filesystem change to test the watcher and wait through its debounce:

```bash
printf 'x\n' >> "$REPO/a.txt"
git -C "$REPO" commit --allow-empty -m watcher-check
```

The launch-only hooks in `src/headless.rs` (`KAGI_SELECT_FIRST`, `KAGI_JUMP`,
`KAGI_CONTEXT_MENU`, compare, bottom-panel, terminal, menu-dump, and pull hooks)
are applied only at startup. They cannot drive an existing instance. There is no
headless hook for branch Solo; use Tier B's context-menu click.

The web/Playwright harness is a separate UI-story catalog and does not verify the
native app's backend flow:

```bash
bash scripts/build-web.sh
cd e2e && npm install && npx playwright install chromium && npx playwright test
```

## Maintenance

When a PR changes a verification seam, environment flag, script, or runner feature,
update the matching part of this skill and state `skill 更新済み` in the PR body.
Adding a `tests/gui_e2e_runner.rs` scenario also requires updating Tier A's inventory.
Keep `.agents/skills/kagi-verify` as a symlink to the canonical source. Manually
run `uv run --project ci check-skill-refs` before merging. It verifies concrete
`scripts/*` and `tests/**/*.rs` references in inline code, fenced commands, and
Markdown links in this canonical skill (#544).

Finish by exiting the launched application so it cannot retain a socket or test
state. Fixtures and isolated log directories live below `/tmp`; remove them only
when their evidence is no longer needed.


### Backend executor visibility (#566)

`tests/support/backend_ops.rs` adapts legacy fixture signatures to Backend run or
a dedicated method, with the optional snapshot policy explicitly OFF. It must
never call raw executors or clear plan blockers. `tests/support/remove.rs` freezes
the opaque remove plan before any fixture drift. Compile-fail doctests guard old
root/ops/conflicts/staging/step-runner imports. D/F fixtures check owner trust and
frozen OID/child-list rejection through the dedicated Backend boundary.
`tests/backend_fixture_storage_test.rs` drives the migrated branch adapter in
the shared isolated child, asserts a record in its log directory and preserves
a fake HOME oplog sentinel. Every new migrated fixture must use the same helper.

The low-level trust/headless acceptance cases are crate-internal tests in
`crates/kagi-git/src/ops/worktree_steps_acceptance_tests.rs`, with global env
changes isolated in child test processes. Run `cargo test --workspace`; GUI E2E
may be compiled with `--features gui-e2e --no-run`, but do not execute it for this
visibility refactor. Preserve the private safe-checkout and unapproved-config
oracles when updating fixture adapters.

### Ref-backed discard/remove recovery (#523)

Use `crates/kagi-git/tests/ref_backups_test.rs` for G: disable optional snapshots,
prune a disposable fixture with real Git GC, and recover the original bytes via
receipt `backup_refs`. A sibling naked blob must disappear to prove actual GC.
Check remove Success/Partial/Unknown and append failure. Explicit oplog retirement
must delete only its unshared roots; the final retained entry controls lifetime.
Default retention is indefinite, matching the oplog; snapshot pruning must not
expire backup refs. For M, inspect the receipt's ref and export its bytes before
choosing to retire that entry. Legacy naked-OID logs do not gain retroactive GC
protection. For Tier A, scope `KAGI_GUI_E2E_ONLY=worktree_panel,remove_public_boundary`:
the worktree discard scenario parses the blob from the existing `backup:` summary
and checks that the structured receipt ref resolves to that blob from both
worktrees. Ref names must not change existing executed lines or after/dirty text;
they have a separate `backup refs:` contract line. The same filter includes
`worktree_panel_discard_recording_failure`: hold the real oplog lock through
append timeout, check "changed but not recorded" and recover from the attempted
receipt, then repeat with a stale owner and require the owner-named notice.
G also covers a queued append after actual retirement, colon-before whitespace,
legacy id-less cleanup, and a pre_remove-created unreadable file: removal must
stop before deleting the worktree.


### Unmerged branch deletion (#584)

G in `tests/delete_branch_test.rs` covers the actual modal arm transition, merged
single confirmation, full-tip preflight refusal, unique reachability counts,
receipt-backed recovery after real GC, and commit-root retirement. It also covers
main/linked checked-out refusals (including clean worktrees and checkout after
planning), symbolic alias chains versus direct roots, and reflog removal before
branch name reuse. Tier A's
`unmerged_branch_delete_armed` scenario drives Enter and the measured button:
first confirmation keeps the branch/HEAD and records nothing; second deletes and
records its retained tip. Merged deletion stays one-stage. The same scenario
starts a plan on A, switches to B before completion, and asserts that B receives
no delete modal or mutation, releases the plan busy latch, and permits a fresh
plan on returning to A. It also holds the oplog sidecar lock to force an
append failure after deletion: expect a retained tip, owner notice and Partial
entry with its original `backup_refs` and owner metadata, no retry modal, and
the existing `async: delete-branch finished` log. G exercises this same display
conversion after a real sidecar-lock failure and retires a successfully recorded
branch receipt after passing it through the panel. PM runs only
`KAGI_GUI_E2E_ONLY=unmerged_branch_delete_armed`, with the usual isolated log directory.
When execution is prohibited, build it with `--features gui-e2e --no-run` only.
For M, compare EN/JA warning counts and armed labels, cancel/reopen to reset the
arm, then use the receipt's `git branch <name> <backup-ref>` recovery command.

### Release review: branch metadata and remove identity (#587)

G in `crates/kagi-git/src/ops/branch_delete_release_tests.rs` injects a test-only
transaction failure after reflog cleanup and checks the persisted Partial receipt,
retained branch tip and recovery ref. It also covers disabled reflogs, directly
written refs and non-NotFound cleanup errors. `backend/remove.rs` unit tests model
Unsupported creation time and verify all remaining fingerprint fields still
reject drift. These are fixture/unit checks; no GUI runner execution is needed.

### Release transport / Skip regressions

G: `transport_recording_test` validates the actual `mergedAt` query with fake gh;
`remote_stash_script_test::drop_forces_c_locale_inside_the_remote_shell` runs the
real script under a simulated translated Git; `conflicts_test::skip_advancing_to_the_next_conflict_is_not_a_failure`
checks that the next same-path conflict has no skipped draft. UI `transport_hold`
unit coverage verifies owner/operation isolation and Partial/Unknown admission.
For M, check that notice dismissal/tab switching does not re-enable PR merge or
remote pull after Unknown/Partial; Failed alone permits retry. Holds persist for
the app lifetime; inspect remote state before restarting. GUI runner build only
when execution is reserved for PM.

### Remote source drag merge (#590)

Tier A filter: `KAGI_GUI_E2E_ONLY=remote_source_merge_into`. The scenario uses
`graph-remote-<remote/name>` and `sidebar-local-<branch>` control bounds to deliver
real drag events, confirms through Enter, and asserts one successful receipt with
HEAD/index/worktree and remote ref unchanged. Each scenario unmounts its window.
When PM owns execution, compile only with `--features gui-e2e --test gui_e2e_runner --no-run`.

G: `tests/drag_merge_test.rs` exercises the public Backend boundary with a remote
source and non-HEAD target, including another-worktree occupancy and changed tip.
M: drop a remote chip onto a non-HEAD local row; the plan must show its full ref,
OID and EN/JA last-fetch warning. Confirm that no local source branch is created
and no fetch occurs; fetch explicitly beforehand when current remote data is needed.

For a checked-out destination in another worktree, scope
`KAGI_GUI_E2E_ONLY=cross_worktree_merge`. It drags the fetched remote graph chip
onto the linked-worktree branch name, checks tab opening/reuse, cancellation,
late-plan suppression, dirty-editor discard/cancel and external destination
branch drift. Enter then performs a normal two-parent HEAD merge in the linked
worktree; its index/files and receipt must agree, while the dirty parent stays
unchanged. The scenario unmounts its window. Backend coverage in
`tests/drag_merge_test.rs` also rejects fresh and stale plans against a dirty
linked destination without touching either worktree.

### Staging failure delivery (#490)

G: `cargo test -p kagi --lib staging_failure` uses real index.lock and oplog
sidecar lock fixtures in isolated children; it compares single/batch failure
details in EN/JA and checks that trust refusal records without requesting a modal.
Use a fresh KAGI_LOG_DIR as with all cargo tests.

Tier A filter: `KAGI_GUI_E2E_ONLY=stage_failure_notice`. The scenario covers editor
paths, panel file indices and batch buttons under index.lock, including a linked
worktree panel while the main tab remains active. It asserts footer + notice +
oplog, the actual owning repo/path, unchanged indexes and no modal on admission
refusal. Compile only when PM owns E execution.
M: hold index.lock, click Stage/Unstage from both surfaces, verify the actual
cause is visible and no success toast appears. If recording also fails, the
attempted failure stays visible with the recording error; it is not a success.

### Busy snackbar labels (#607)

G covers EN/JA labels, unknown-tag fallback, and lease-mirror settlement without
clearing legacy plans. `uv run --project ci check-busy-labels` checks literal
busy tags and finite operation-name producers against the label table.
Tier A: `KAGI_GUI_E2E_ONLY=fetch_busy_label` in `tests/recovery/busy_label.rs`
starts a local fetch through `fetch_async`, checks the renderer's snackbar text
before completion in both languages, and checks release after completion.
Build only when PM owns execution. For M, verify Fetch, Commit, Stash, Discard,
branch deletion and editor Save show an operation label, never `app-writer`.
