---
name: kagi-verify
description: Verify Kagi changes against fixture repositories, the native GUI, and the browser story harness. Use for runtime, fixture, or E2E validation work in this repository. Driving the real GUI does NOT require taking the pointer, and does not take the foreground either — except a scenario that clicks the tab strip, which needs a key window, so ask the user first or use a machine nobody is working on. Tier B uses scripts/pidclick.swift (CGEventPostToPid). cliclick is banned for new validation.
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
  user can keep working while a scenario runs. The one exception is a scenario that
  clicks the **tab strip**, which needs a key window and therefore the foreground —
  see Tier B's launch pitfalls. Ask the user before running one, or run it on a
  machine nobody is working on.
- **Never run the full `gui_e2e_runner`.** Always scope it with `KAGI_GUI_E2E_ONLY`.
  An unfiltered run once opened roughly 1,400 windows and crashed macOS.

| Need | Lane |
| --- | --- |
| Deterministic assertions on real UI state, no windows on the user's screen | Tier A runner (`KAGI_GUI_E2E_ONLY` always) |
| A real running app, real clicks, without taking the pointer or foreground | Tier B `pidclick` (tab-strip scenarios take the foreground — see its pitfalls) |
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

# Issue #462 compact cards plus the disclosure baseline they must not break.
# 600/700/750/900 x EN/JA, one hidden window at a time, all unmounted.
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='modal_compact,modal_sections' \
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

For history bisection, strip repository-location variables exported by
`git bisect run` before launching a Git fixture. Otherwise a fixture's `git init`
can address the bisected repository instead of its temporary directory (#764).
Keep Cargo's worktree-local `target/` and the scenario filter:

```bash
KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY=preflight_presentation \
  git bisect run env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR \
    -u GIT_INDEX_FILE -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
```

The current suite covers:

- durable stash-drop recovery; history persistence; cleanup stale-tab,
  preflight, open-failure, and partial presentation; remove's public boundary;
  editor writer admission; commit-row and editor-history layout;
- preflight refusal presentation (`KAGI_GUI_E2E_ONLY=preflight_presentation`,
  `tests/recovery/operations.rs`): stale stash-drop plans deliver the specific
  localized reason through a Failed footer and Error toast in EN/JA, with no
  dismiss-only AppNotice. The complete English blocker remains in one durable
  Refused receipt and its Operation Log panel row; stash list and repository
  fingerprint stay unchanged. History Undo retains its inline plan error.
  This follows the #747 delivery contract; #764 bisected the obsolete modal
  expectation to `e5644c6f`, rather than changing production back.
- unobservable remote-write release (`KAGI_GUI_E2E_ONLY=reconcile_unobservable_release,app_notice_modal_replacement`,
  `tests/recovery/reconcile_unobservable.rs`): measured native clicks exercise
  Inspect → arm → final confirmation in EN/JA. The armed warning is drawn;
  Escape and modal displacement discard the arm without releasing the requirement.
  A real append-lock failure preserves the requirement and writer exclusion,
  leaves no durable release row, and renders a Partial receipt rather than raw
  Unknown in the panel. Restoring persistence and confirming again records exactly
  one `reconcile-release-unobservable` audit row before admitting another write;
  the original Unknown and repository fingerprint stay unchanged. AppNotice uses
  `measure_inside` on the absolute modal root so instrumentation cannot relocate
  its controls outside the window.
  App-level tests in `app_unobservable_release_test` additionally reject live writers,
  mismatches, failed transports, unknown families, and failed audit persistence.
- conflict refusal reasons (`KAGI_GUI_E2E_ONLY=conflict_save_boundary`,
  `tests/recovery/conflict_refusal.rs`): Save with remaining markers and Abort
  after an external staged resolution show the specific EN/JA reason in the
  AppNotice and bounded toast, retain the English footer contract and oplog
  detail, and leave the index, working file and merge state unchanged.
  Abort covers both the dashboard and the operation strip; both drift after the
  confirmation opens and confirm it *without* an intervening reload, because a
  reload now closes such a confirmation instead (#755): each leg reopens it,
  reloads, and requires the confirmation to be gone with no second
  `merge-abort` record and index / `MERGE_HEAD` unchanged.
- conflict Abort modal slot (`KAGI_GUI_E2E_ONLY=conflict_abort_escape_focus,conflict_abort_superseded_reload`,
  `tests/recovery/conflict_abort_slot.rs`): Escape closes the confirmation from
  the focus the app actually left behind, and only an *accepted* reload sweeps
  it — a superseded read leaves it standing, the next accepted one closes it,
  and neither runs the abort. The Escape half is the interesting one: the
  Result pane mounts its code editor only in Edit mode, so editing and
  returning to Preview leaves window focus on an
  element that is no longer drawn; gpui then dispatches from the tree root and
  every window action, Escape included, silently does nothing. The scenario
  reaches that state through the product's own toggle, opens the dashboard
  Abort, and delivers a real `escape` **without** re-focusing anything — unlike
  `press_key` in `tests/recovery/operations.rs`, which focuses `root_focus`
  first and therefore cannot see this class. Opening the confirmation takes
  focus back to the root, which is what makes Escape land.
  Tier B measured the same key on 2026-09-23 with `KAGI_NO_ACTIVATE=1` still
  set — a **non-key** window — and `pidclick key 53` closed the confirmation,
  so this check does not need the foreground. Do not read that as a guarantee
  that key delivery to a non-key window is reliable in general; it is what this
  window and this key did. `KAGI_DEBUG_KEYS=1` makes the routing wrapper print
  `[kagi] key: <key> char=<..>` (`src/ui/modal_key_routing.rs`), but **absence
  of that line is not absence of delivery**: gpui dispatches matched actions
  before key listeners and an action handler stops propagation, so a working
  Escape prints nothing. That is exactly what the accepted run recorded —
  no key line, modal closed. A printed `escape` with the modal still open is
  the interesting failure: the key arrived and no binding matched.
- bottom-panel toggle, graph copy, oplog expand/copy, snapshot creation, theme
  switching, agent provenance, and WIP-to-HEAD connectors;
- WIP virtual commit anchors (`KAGI_GUI_E2E_ONLY=commit_row_layout_wip`,
  `tests/recovery/wip_layout.rs`, under the layout suite): actual canvas paints
  must show hollow nodes directly above their own HEADs (columns may repeat),
  with HEAD-coloured badge→node and node→HEAD dashes. Intervening history must
  not move an anchor beyond `abs(anchor_lane - head_lane) <= 1`; the selected
  policy uses the same lane. Node/dash `paint_order` proves that WIP paths are
  behind crossed nodes. The same run covers shared HEAD (stacked rings, one
  trace), detached/unborn state, 1.25× zoom, hover and selection.
  Ring diameter matches the visible HEAD ring (classic) or commit avatar disc
  (swimlane); stroke stays 2px, lane-coloured, without a fill. The trace is
  compile-time `gui-e2e` only and opt-in per window through
  `graph_view::take_paint_trace`; clear it before drawing and drain afterward.
  A measured `commit-list-viewport` wheel event must move every WIP offscreen
  while a visible HEAD retains dashes from the viewport top. WIP and stash
  prefixes share the commit list's virtual scrolling, not a pinned header.
  Pair with `wip_head_connector,worktree_wip_inline,worktree_panel_commit` for
  collision routing, linked-panel interactions and target-keyed row removal.
  Tier B uses three dirty worktrees on distinct branch tips, one extra branch
  commit above a WIP HEAD, and enough older history to scroll. Capture both
  the top and WIP-offscreen states; retain screenshots in the PR and verify
  that another pane does not obscure the graph.
- linked-worktree WIP rows plus commit-panel commit, amend, and discard;
- Worktree occupancy and removal advisory (`KAGI_GUI_E2E_ONLY=worktree_inspection`,
  `tests/recovery/worktree_inspection.rs`): real pushed/dirty/locked/detached
  fixtures cover EN/JA reasons, ignored `target/` allocation, explicit refresh,
  the existing measuring spinner and late-delivery rejection after supersession,
  selection departure and tab close. Bring virtual sidebar rows into view before
  clicking; the selected detail panel reduces the navigator's viewport.
  Actual bounds must cap the panel at 40% of the sidebar in both languages.
  Check the fixed name/path heading and independently scrolling detail body;
  Tier B must confirm that selecting another worktree is not obscured.
  Tier B uses those four states with allocated plus sparse build files and an
  ignored `.env`; inspect capacity/target breakdown, measurement time, localized
  reason, local-ref basis, `.gitignore` warning and existing removal-menu guidance.
  This is read-only: no new delete button, backup policy, lock operation or fetch.
  Unix fixtures compare allocation with `du`; a Windows compile check alone does
  not establish Windows runtime behavior.
- modal and branch-menu Enter isolation from the selected commit checkout;
- compact confirmation cards (`KAGI_GUI_E2E_ONLY=modal_compact,modal_sections`,
  `tests/recovery/modal_compact.rs`, `tests/recovery/layout.rs`): issue #462. The
  amend, discard-all and push cards are mounted at 1200x600/700/750/900
  in EN and JA, also covering 600px at 80% zoom — one window per viewport and locale, because
  `MacWindow::resize` never settles under `VisualTestAppContext`. Each card's
  measured `modal-card`, `modal-footer` and footer buttons
  (`amend-*`/`discard-*`/`plan-*`) must lie inside the window; the
  target rows must fit the actual scroller, with at least three visible at 600px
  and more than three at 700px. Real wheel events must expose the final file
  or commit, not merely a row near the end.
  Recovery is folded by default only while compact, warnings never are, an
  explicit disclosure click survives a zoom change that crosses the compact
  threshold, and reopening the card restores the default. Both destructive cards
  are armed (first confirm click) and cancelled. Warning, recovery, blocker and
  armed text bounds must fit their visible body, not merely remain mounted.
  A blocked plan has no confirm button, and the fixture's HEAD + porcelain
  status remains unchanged. Git operations use only a local bare repo.
  `modal_sections` is the pre-migration disclosure baseline and remains unchanged;
- worktree lock reason (`KAGI_GUI_E2E_ONLY=worktree_lock_reason`,
  `tests/recovery/worktree_lock_reason.rs`): #372 item2. EN/JA use the real
  InputState, clipboard paste and focused Enter to review, then a second Enter
  to lock without test-forced refocusing. Input/plan cancellation makes no lock;
  Unicode/quotes and blank reasons round-trip through Git's NUL-delimited
  porcelain, with a durable Success and unchanged HEAD/working tree. The existing
  explicit unlock is exercised between acquisitions. For Tier B, right-click a
  linked worktree → Lock, edit the reason → Review → inspect the plan, then
  Cancel or confirm. No tab-strip click or foreground activation is required.
  Automatic terminal locking is not part of this change; it is tracked in #772.
- modal-slot arbitration (`KAGI_GUI_E2E_ONLY=push_failure_keeps_modal,merge_plan_latch,delete_branch_plan_latch,remote_browse_modal_routing`): Push failures and delayed Merge/Delete Branch plans wait behind Remote Browse without losing its input, stale plan state, latches, footers, or notices; a reopened Remote Browse rejects an older in-place completion by generation;
- unmerged branch deletion with two confirmations, retained tips, and one-stage merged deletion.
- toolbar centre actions (Pull…Terminal) drawn only in Graph, not PRs/Editor/Analyze
  (`KAGI_GUI_E2E_ONLY=workspace_mode_toolbar`, via the `tb-repo-actions` control bound);
  the same scenario covers the Issues Composer's real New/Reply input-change
  subscriptions, the Reply's lack of a New Issue-only title entity, Markdown
  Preview, scoped Focus Editor, multiline Paste and Undo, and successful write
  settlement consuming its draft without reopening Issues after Graph is selected,
  all without a live GitHub write (ADR-0201; `src/ui/issues_composer_e2e.rs` seeds
  read-side state and enters the production settlement path); since #750 it also
  covers the PR page's shared timeline feed — a seeded line comment's diff hunk
  (`pr-convo-hunk-<n>`) and ```suggestion marker (`pr-convo-suggestion-<n>`) drawn
  inside the shared row — and the pinned PR composer's real toggle click
  (`pr-composer-mode-toggle` → `pr-composer-preview`) keeping the typed text
  across a preview round trip; since #751 it also proves the conversation
  Markdown is *laid out*, not merely parsed: the canonical body
  `tests/support/issues_markdown.md` is cut along its own `## ` headings, each
  section is seeded into the production Thread (`issue-thread-body-4-md`) and
  the production Composer Preview (`issue-composer-preview-md-0`), and the
  drawn box is then selected with a real pointer drag (down / move / up) and
  copied with ⌘C — the clipboard is poisoned with `NOTHING_WAS_COPIED` before
  each drag, so a selection that found nothing fails instead of reusing the
  previous copy. The task list, table and fenced code are asserted from what
  the selection returned, because selection resolves against laid-out text
  runs. `gpui-component`'s `BlockNode` is `pub(crate)`, so a Markdown plugin
  cannot wrap the built-in table/list/code renderer to measure it directly,
  and a plugin that claimed those nodes would be a parallel renderer; the drag
  is the layout-derived seam that remains. Images are never fetched on these
  surfaces (ADR-0142 amendment), so no screenshot ever waits on the network;
  The same scenario exercises the shared Issue/PR label and created-sort menus.
  Its PR-state regression holds a Closed response and checks Refreshing, then
  proves Closed/All results cannot replace shared Open evidence, an Open ticker
  update cannot replace the visible Closed collection, and a late response after
  leaving PR mode cannot restore that collection. Pair with
  `KAGI_GUI_E2E_ONLY=github_evidence_` for session restore/background/detach.
- Issues cursor pagination (`KAGI_GUI_E2E_ONLY=issues_pagination`,
  `tests/recovery/issues_pagination.rs`): the production virtual viewport loads
  100 → 200 → final-page rows without resetting the scroll anchor; an offline
  response retains rows/cursor and the measured retry control resumes the same
  page. Refresh returns to the first page. Only the transport future is queued
  through `src/ui/issues_composer_e2e.rs`; owner/generation settlement and actual
  scroll/click handlers remain production code. Pair with `workspace_mode_toolbar`
  for Composer, sidebar filters, thread return and focus regressions. For a live
  read-only Tier B check, use a repository with more than 100 open Issues, scroll
  the main viewport to its tail, and compare the loaded badge with
  `[kagi] github: issues page=N loaded=M has_more=true|false`. A successful manual
  refresh starts again at page 1.
  A nonempty Mentioning subset must not auto-page at its tail; its measured
  `issue-filter-load-more` click continues exactly once. Returning to Recent
  restores automatic continuation. Native clicks scroll the sidebar first if
  an expanded collection puts the next tab header outside its viewport.
- Issue write failure ownership
  (`KAGI_GUI_E2E_ONLY=issue_failure_notice_survives_tab_switch`,
  `tests/recovery/issue_write_owner.rs`): a recorded Create failure that lands after
  leaving its owner remains in Operation Log without replacing or extending the
  current tab's modal queue, without invoking a GitHub transport;
- dirty Pull auto-stash success and Pull-failure restoration, including a durable
  Operation Log result without a dismiss-only error modal.
- reload keeping the open views (`KAGI_GUI_E2E_ONLY=survives_reload`): a commit's diff
  re-anchored to its renumbered row, the Compare pane and its file diff re-read, a
  Commit Panel file diff re-read (closed once nothing is left to show), and the
  Commit Panel kept while the working tree still has something to list.
- owner-stamped callbacks (`tests/recovery/remote_refresh_owner.rs`,
  `tests/recovery/fetch_owner.rs`, `tests/recovery/file_menu_owner.rs`):
  `remote_refresh_departed_owner`, `remote_refresh_newest_request`,
  `fetch_same_owner_piggybacks`, `fetch_different_owner_does_not_piggyback`,
  `fetch_owner_display_isolated`, `fetch_detach_retains_flight_and_isolates_reopen`,
  `file_menu_freezes_path`, and `file_menu_rejects_stale_owner`.
  Use `KAGI_GUI_E2E_ONLY=remote_refresh_,fetch_same_owner,fetch_different_owner,fetch_owner_display,fetch_detach,file_menu_`.
  Remote tests queue a yielding transport task through `ui::e2e`, then exercise
  the real refresh launch and completion; no SSH process blocks the dispatcher.
  Fetch tests launch a real local fetch and switch/close before draining.
  File-menu tests defer the real panel callback, renumber rows, click the measured
  Discard control, and deliver a retained action after changing owners.
  The existing `unmerged_branch_delete_armed` scenario also rejects delayed
  plans after departure and revisit while releasing the planning latch.
- session-owned positioning and Smart Commit state
  (`tests/recovery/tab_ui_state.rs`, `tests/recovery/operations.rs`):
  `KAGI_GUI_E2E_ONLY=tab_ui_state_ownership,pr_open_enters_before_ref_fetch,smart_commit_generation_owner,smart_commit_modal_and_probe`.
  The position scenario preserves each tab's list/graph positions, paging limit,
  branch folds, and cleanup selection while proving a newly opened tab reads at
  its own default limit. The PR-open scenario proves an unfetched head enters
  its page before the background ref fetch settles. Smart Commit freezes the initiating session for spinner
  and completion state, drops detached-owner completions, uses the shared modal
  slot without Enter/Escape fallthrough, resets modal-list scroll on replacement,
  and probes repository-independent capabilities once per global revision.
- session-owned evidence (`tests/recovery/github_evidence_owner.rs`,
  `tests/recovery/cleanup_evidence_owner.rs`, `tests/recovery/conflict_evidence_owner.rs`,
  `tests/recovery/ecosystem_evidence_owner.rs`):
  `github_evidence_restores`, `github_evidence_background_owner`,
  `github_evidence_detached_owner`, `cleanup_evidence_background_owner`,
  `cleanup_evidence_superseded`, `conflict_detector_owner_guard`,
  `ecosystem_evidence_background_owner`, `ecosystem_evidence_superseded`, and
  `ecosystem_evidence_detached_samepath`.
  Filter with `KAGI_GUI_E2E_ONLY=github_evidence_,cleanup_evidence_,conflict_detector_owner_guard,ecosystem_evidence_`.
  `tests/recovery/evidence_support.rs` supplies yielding replies for queued
  transport tasks; production launch, owner capture, and settlement still run.
  PR restoration observes actual sidebar rows before the return-triggered fetch
  can finish; Analyze observes the existing `copy_diagnostic` clipboard output.
  Drain switch-triggered reads before setting an unrelated-tab sentinel, so a
  normal revalidation cannot masquerade as a foreign completion.
- scan read-revision safety (`tests/recovery/cleanup_evidence_owner.rs`,
  `tests/recovery/squash_evidence_owner.rs`):
  `KAGI_GUI_E2E_ONLY=cleanup_evidence_read_revision,squash_evidence_read_revision`.
  Both hold transport completion, create a real commit, and accept a new read
  while its owner is background. Cleanup checks rows, PR evidence, and the
  selection/delete-plan consumers on return. Squash compares commit/lane/edge
  signatures after returning to the owner, then proves a fresh scan still draws.
  Its test observer holds render-driven scan re-arming to model completion
  before the next scan launches; it never changes generation or read revision.
  Remove each `Reads::is_fresh` guard independently: only that scan's revision
  scenario must fail while the sibling and existing ownership scenarios pass.
- accepted-model generation safety (`tests/recovery/cleanup_publish_owner.rs`,
  `tests/recovery/squash_publish_owner.rs`):
  `KAGI_GUI_E2E_ONLY=cleanup_evidence_publish_generation,squash_evidence_publish_generation`.
  These start a scan after `Reads::begin`, then accept a changed model with the
  same `ReadKey`; request revision and scan generation deliberately stay equal.
  Cleanup observes fresh rows, PR evidence, and select/delete consumers. Squash
  compares commit/lane/edge signatures. The same scenarios cover rejected
  accepts, another-owner publication, `publish_tab_view`, and `amend_tab_view`.
  Removing only either publish-generation guard must fail only its matching
  scenario; the earlier read-revision scenarios must remain green.
- session-owned read caches and history (`tests/recovery/cache_history_owner.rs`):
  `KAGI_GUI_E2E_ONLY=read_cache_revalidates_on_activation,operation_history_session_and_stale_ref,background_operation_history_owner,welcome_drops_root_main_diff`.
  The cache scenario retains A's cache while B is active, changes A externally,
  then proves activation clears every stale payload before the owner full read
  publishes the new WIP/status snapshot. History scenarios prove A and B expose
  separate undo stacks, external ref movement is refused, background operation
  completion records only into its frozen attached owner, and detached completion
  falls through nowhere. The Welcome scenario proves the final session release
  removes its retained Main Diff before ownerless rendering.
- session-owned pane resources (`tests/recovery/pane_resources.rs`,
  `tests/recovery/app_conflict.rs`):
  `KAGI_GUI_E2E_ONLY=retained_pane_resources,conflict_background_owner,conflict_revalidates_after_external_abort,tab_ui_state_rejects_detached_writer`.
  Real File History, Analyze, Editor, Conflict, and Terminal entities freeze
  their owner. Background completions update only that owner's retained state
  and cannot touch another tab's pane, modal, or footer. Returning restores
  entity identity and edited buffers; returning after an external conflict
  abort clears the editor before the fresh read and rejects old-visit
  completions. Closing the owner makes every retained `WeakEntity` dead.
  Pair with `smart_commit_generation_owner`, `diff_survives_reload`,
  `compare_survives_reload`, and `fetch_detach_retains_flight` for Commit Panel,
  diff/compare refresh, and operation-lifecycle coverage.

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
PM can scope
`KAGI_GUI_E2E_ONLY=branch_menu_no_checkout_fallthrough,modal_no_fallthrough,remote_browse_modal_routing,window_modal_exclusivity,app_notice_modal_replacement,pull_failure_notice_waits_for_remote_browse,pull_failure_notice_waits_for_app_notice,pull_failure_notice_displacement_vs_dismissal,update_install_lifecycle`.
The menu and modal scenarios in `tests/recovery/operations.rs` select a non-HEAD
commit and prove Enter cannot fall through to checkout. Remote Browse also proves
that the shared workspace/Welcome modal-key wrapper routes Enter through both its
connection and Browse stages, routes Esc to the slot on both surfaces, and keeps
the workspace's vacant-slot Enter checkout fallback. The window-modal scenario
records the rendered Remote Browse, Update, and AppNotice overlays: an arriving
notice waits behind the occupied slot, Update consumes Enter without acting, and
Esc closes it. The AppNotice replacement scenario proves unrelated modal setters
preserve both actionable and plain unread notices. The production dirty-Pull
failure scenarios prove a recorded fetch failure never replaces a foreground
modal or extends an existing notice queue, stays durable in Operation Log, and
does not interfere with a delayed Acknowledge action.
The update-lifecycle scenario proves an installer remains window-owned while its
modal is closed, rejects a second start after reopening, renders a retained
completion, consumes Enter, and closes via Escape after the last repository tab
transitions the window to Welcome. Native foreground/input-focus behavior remains
a separate Tier B check.

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

Three launch pitfalls, all seen in practice. Launch Kagi from an unsandboxed shell:
started from a sandboxed agent shell the process runs and logs normally, but
WindowServer never shows its window, so there is nothing to click. And with
`KAGI_NO_ACTIVATE=1` the window may not be on screen, so `windows --pid` (which
lists on-screen windows only) prints nothing even though the window exists;
clicks, keys and `screencapture -l` address the window by ID and still work, so
take the ID from `CGWindowListCopyWindowInfo([.optionAll], …)` for that PID.

**Drop `KAGI_NO_ACTIVATE=1` when the scenario clicks the tab strip.** The tabs
live in the window's title bar, and macOS hands a title-bar click on a *non-key*
window to its own window-drag handling instead of the application. The click
never reaches the tab, the window moves under you, and the drag session it
starts swallows every later event — clicks stop working in the content area too,
and only relaunching recovers. Measured while verifying #643 Wave 4: two clicks
on the tab strip moved the window from `180,125` to `215,151` and killed input.
Without the flag, the same clicks switch tabs (`[kagi] tab-switch: <name>
cached=yes`) and the window stays put. Keep the flag for scenarios that stay in
the content area — it is what leaves the user's foreground app alone.

Dropping it has a real cost, so treat it as the documented exception to the
no-foreground rule at the top of this skill: `run_app` calls `cx.activate(true)`
right after `open_main_window` (`src/ui/mod.rs:3187`), guarded by exactly this
variable, so the launch pulls Kagi in front of whatever the user is doing.
`open_main_window` itself does not activate, and the Dock-reopen handler's own
`cx.activate(true)` is deliberately unguarded — the user asked for the window
there. Ask the user first, or run on a machine nobody is working on, and say in
the PR that the scenario needed the tab strip. Every other isolation flag
(`USER`, `KAGI_NO_RESTORE=1`, `KAGI_LOG_DIR`) still applies unchanged — they
protect the user's session, settings, trust and oplog, which foreground does not
touch.

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
working tree — 12 commits, which is deliberately small.

A second argument deepens `main` with that many empty commits, inserted just
above the initial commit so the interesting shape stays at the top of the graph.
Anything that only happens on a long list needs it. Two different thresholds:
the commit list starts scrolling at a screenful (a few hundred is plenty), while
**paging** only begins past `DEFAULT_COMMIT_LIMIT` (10,000 — `COMMIT_PAGE_STEP`
is merely the 1,000 it grows by), so a paging check needs a five-figure fixture
(#737). Roughly 13ms per commit: 1,500 takes about 17 seconds, 10,500 about two
and a half minutes.

```bash
bash scripts/make_fixture.sh /tmp/kagi-vfx-deep 1500
```

For the three-stash menu and all four stash operations, consume the
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

### PR merge local branch cleanup (#705)

G: `cargo test -p kagi --test transport_recording_test --test oplog_nonrun_ops_test`
exercises fake gh against real refs: retained deletion, plan-known checked-out /
PR-head mismatch keeps, late drift, a branch created after approved absence,
queued Success, and failed transport followed by a merged re-read without cleanup.
Fork Unknown reconciliation must acknowledge a merged PR with the local branch
still intact, then permit Kagi's ordinary guarded delete with a recovery ref.
No external force-delete is a prerequisite. One receipt owns both halves.

Tier A filter: `KAGI_GUI_E2E_ONLY=pr_merge_write_lease`. In addition to the existing
lease/hold cases, this drives fake-gh fork merges in EN/JA: Deleted, quiet Absent,
late drift Partial, planned Kept (checked out / not at PR head), and queued Kept.
Planning shows the existing loading state and no confirmable modal until settled.
Only Partial holds re-merge; queued and planned Kept release admission. For M, inspect the frozen local name/OID,
the fork-remote warning, and the localized deletion/refusal notice. Tier B remains
PM-owned when execution is reserved; compile the runner with `--no-run` in that case.
The same scenario attempts best-effort captures before confirmation and after
settlement, tagged `pr-merge-local-{En,Ja}-<case>-{plan,notice}`. Report
the explicit `skipped` diagnostic when this GPUI backend cannot render an image;
never present state assertions as screenshot evidence. A live GitHub PR may be
used to inspect the modal without confirming; do not consume a real PR to test
the post-merge notice.

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
worktree panel while the main tab remains active. It asserts footer + toast +
oplog without a dismiss-only modal, the actual owning repo/path, unchanged indexes,
and a modal only when oplog persistence itself fails. Compile only when PM owns E execution.
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
