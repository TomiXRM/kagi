---
name: kagi-verify
description: Verify Kagi changes against fixture repositories, the native GUI, and the browser story harness. Use for runtime, fixture, or E2E validation work in this repository. Driving the real GUI does NOT require taking the pointer, and does not take the foreground either — except a scenario that clicks the tab strip, which needs a key window, so ask the user first or use a machine nobody is working on. Tier B uses scripts/pidclick.swift (CGEventPostToPid), which shows where it acts with the scripts/pidcursor.swift agent cursor. cliclick is banned for new validation.
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

The runner stops at the first failing scenario (exit 101). To collect every failure of a selection in one run, add `KAGI_GUI_E2E_KEEP_GOING=1` (it refuses to start without `KAGI_GUI_E2E_ONLY`): each scenario runs alone in a re-executed runner process with its own process group, one at a time; the whole group is killed after `KAGI_GUI_E2E_TIMEOUT_SECS` (default 600) and whatever it left running is killed when it ends.
The run ends with `[gui-e2e] KEEP_GOING summary: …` and one `PASS <name>` or `FAIL <name>: <exit status | timeout> — evidence <dir>` line per scenario (the dir also holds `exit.txt`), then exits 1 if any failed.
`KAGI_GUI_E2E_EXACT=<name>` is the internal seam the parent uses to start each child; do not set it by hand.

A change to how outcomes are presented (footer, toast, notice, modal, Operation
Log) must run the scenarios that assert that presentation with
`KAGI_GUI_E2E_ONLY` before merging. Compiling the GUI tests is not enough: #718
and #747 changed presentation, and three scenarios kept the old expectations
unnoticed until #898.

When a scenario panics, the runner writes failure evidence (#516) and prints
`[gui-e2e] FAIL <scenario>: evidence <dir>`. The directory is
`target/gui-e2e/<scenario>/` (worktree-local `target/`) and contains:

- `panic.txt`: the panic message.
- `klog-tail.txt`: the last 200 `[kagi]` lines.
- `fixture.txt`: `git status --short` and `git log --oneline -5` of every
  repository mounted through `mount` / `mount_state`.
- `window-<n>.png` for each window, or `window.txt` saying why there is none.
  Windows stay off-screen, and without Screen Recording permission
  `screencapture` cannot capture them.

Read these before re-running a decayed scenario. The next run of that scenario
deletes the directory first, so whatever is there belongs to the latest run.

After every scenario that passes, the runner compares shared state with what it
was before (#516 slice 3, `tests/support/gui_isolation.rs`). A difference fails
the scenario like an assertion, with the evidence above and a message naming each
resource and what it changed from and to.

What is compared:

- `settings.json`: every key and value, except `session_repos`,
  `session_active` and `recent_repos`.
- `worktree_ports.json`: byte for byte.
- `operations.jsonl`: the bytes already there must stay the same, and every
  appended entry's repo must be under the run's temporary directory or remote.
- Direct entries of the run's own `TMPDIR`: none may be added.

The run also owns `HOME` (and `XDG_CONFIG_HOME`): an empty directory under the
run root whose `.gitconfig` holds only the fixtures' identity (`poc`). Every
inherited `GIT_*` and `GH_*` (and `GITHUB_TOKEN` / `GITHUB_ENTERPRISE_TOKEN`)
is dropped at startup; then `GIT_CONFIG_GLOBAL` names that `.gitconfig`, there
is no system config, and `GIT_TERMINAL_PROMPT=0`. `git`, `gh` (config and
credentials), the editor's trash and the terminal never read the developer's
dotfiles or config. A terminal never starts the user's `$SHELL`: with no seam
shell set it panics before the spawn, failing the scenario.

Restoring state:

- `set_lang`, `set_theme`, `set_zoom`, `set_diff_split` and
  `set_terminal_auto_lock` save their key as well. A scenario that uses them
  holds `gui_isolation::SavedKeys::keep(&[...])`, which puts the saved keys back
  when it is dropped.
- A scenario that starts a terminal holds `gui_isolation::PortStore::keep()`,
  and `gui_isolation::StandInShell::install()` unless it sets its own seam shell
  (`KagiApp::set_terminal_shell_for_e2e`). The stand-in is a `/bin/sh` script
  that reads lines until `exit` or EOF; its PID and cwd are real, so the
  auto-lock cwd probe and exit delivery behave as with a login shell.

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
- custom theme loading (`KAGI_GUI_E2E_ONLY=theme_custom,theme_folder_controls`,
  `tests/recovery/theme_custom.rs`): #922 startup JSON load and slug restore,
  palette/menu/Settings selection and fallback, rejected-file toast, folder
  creation/open error, and a background reload held after parsing while a newer
  reload publishes; releasing the old read must not change colours, menus,
  selection, or toast IDs. A themes folder that cannot be listed (mode 000)
  keeps the loaded themes and the selection and names the folder in a toast.
  Startup loads before the window exists.
- file tree accessibility (`KAGI_GUI_E2E_ONLY=file_tree_roles`,
  `tests/recovery/file_tree_a11y.rs`): #354 Editor Workspace and Commit Panel
  render named EN/JA Trees with TreeItems carrying file status, selection,
  level and sibling position. Editor collapse hides descendants and updates
  `expanded`; Commit Panel generated-file disclosure exposes its children and
  updates positions. Reopening the same panel after changing both staged and
  unstaged file sets reuses its entity but recomputes TreeItem roles and sibling
  counts (the replacement state restarts its revision). A filename containing
  `{}` retains its literal braces in both languages. The recorder proves
  renderer attributes, not native VoiceOver delivery; verify the latter on a
  machine with VoiceOver enabled.
- remove refusal reasons (`KAGI_GUI_E2E_ONLY=remove_public_boundary`,
  `tests/recovery/app_remove.rs`): Enter on a locked removal plan delivers the
  specific EN/JA blocker and unlock guidance in AppNotice and bounded toast.
  One durable English refusal retains the reason; both repositories and the lock
  remain unchanged, and admission releases. Existing successful Enter/button
  removal legs still run. Tier B: right-click a locked linked worktree, open
  Remove, press Enter on the blocked plan, and inspect the notice in EN/JA.
- refusal reasons (`KAGI_GUI_E2E_ONLY=refusal_reasons`,
  `tests/recovery/refusal_reasons.rs`): #353. For delete-branch (current
  branch, two blockers), push (no remote), pull (no upstream, the core's
  no-execute receipt) and stash push (clean tree, the stash app flow) in EN
  and JA, a real root Enter on the blocked plan closes the modal and leaves
  the first blocker's localized text — plus "+N more" — in the Failed footer
  and the Error toast, while the single durable entry keeps every blocker in
  English and the repository is unchanged. The `[kagi] footer:` contract line
  still reads `refused (N blockers)`. Tier B: open the same blocked plans in
  the real app, press Enter, and read the footer and toast in EN/JA.
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
- conflict Continue cache (`KAGI_GUI_E2E_ONLY=conflict_continue_cache`,
  `tests/recovery/conflict_continue_cache.rs`): real Result InputState changes
  cover omitted final LF, empty text versus a blank line, marker addition/removal,
  undo/redo and side selection, with an unchanged repository fingerprint.
  An unchanged parent repaint must not add an undo entry. The #497 temporary
  counter experiment instrumented both derived and fresh marker scans: each of
  five input changes scanned one file, then twenty unchanged native renders
  scanned zero. The counter was removed after acceptance; the behavioral
  scenario remains. Pair with `KAGI_GUI_E2E_ONLY=conflict_` for existing safety
  and ownership coverage; Save/Continue still perform fresh validation.
- repository-health proposal (`KAGI_GUI_E2E_ONLY=repo_health_proposal`,
  `tests/recovery/repo_health.rs`): #358 / ADR-0205. Analyze's Health axis
  reads a fixture without a commit-graph and suggests writing one; asking
  opens the shared plan card (equivalent command, no blockers) and nothing
  else — no commit-graph file, no receipt, unchanged repository fingerprint,
  also after Cancel. A real root Enter on the card writes it once (one
  durable `write-commit-graph` success) and the Health axis re-reads without
  the finding. Backend coverage (fsmonitor local-config-only, existing value
  left alone, unborn HEAD blocked, platform gate) is `tests/repo_health_test.rs`.
  Tier B: open Analyze → Health on a repository without a commit-graph and
  with `core.fsmonitor` unset, check the EN/JA texts, open each fix, read
  the plan card (equivalent command, recovery) and Cancel, then confirm one.
- text-first diff highlight (`KAGI_GUI_E2E_ONLY=diff_highlight`,
  `tests/recovery/diff_highlight.rs`): #495. WIP, Compare and File History
  diffs are read off the UI thread and first appear as unhighlighted text
  (an observer records the first shown state); the frame that renders them
  requests spans off-thread once. With split mode on, twenty unchanged frames
  and a reload that re-reads the same text add no highlight and no split
  projection and keep the same row allocation; a theme switch highlights once
  more. The stale leg leaves a highlight out while another file replaces it,
  moves the active theme under an outstanding request (the old theme's spans
  are never shown), and supersedes a WIP read by a newer commit diff or a
  close. `gui-e2e`-only counters (`diff_view::highlight::e2e`: highlight runs,
  dropped results, split projections) back the assertions; default builds do
  not compile them. Pair with `survives_reload,theme_switch,worktree_panel`
  for the reload / linked-worktree paths the same reads serve.
  `commit_diff_off_thread` (#829): a commit file's diff on a content-cache
  miss is read off the UI thread — nothing installs during the call, the
  shown rows keep their allocation until the read lands, a read superseded
  by another commit's cached file never lands or fills the cache, and a read
  held (`KagiApp::hold_next_main_diff_read_for_e2e`, `gui-e2e` only) across a
  reload that renumbers the rows survives the pane sweep, lands on its
  commit's new row and fills no cache under the stale row key.
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
- Settings focus trap (`KAGI_GUI_E2E_ONLY=settings_focus_trap`,
  `tests/recovery/overlay_focus.rs`, #974):
  - Setup: a stand-in shell runs in the bottom-panel terminal, which holds
    the focus. Settings is then opened with `app.settings`, and the focus
    must be on the trap container.
  - Raw Tab: every press stays inside Settings. It walks into the
    Analyze-ignore editor and out again to Save without editing the text,
    and returns to the editor after one full turn. Shift+Tab also leaves
    the editor (#977).
  - Wrapping: from the container, Shift+Tab, Tab and Shift+Tab wrap around
    both ends while staying inside.
  - Close: Escape closes Settings and gives the focus back to the terminal.
  - Pointer open: a click on `tb-settings` lands the focus on the ring-less
    container. Escape then takes it out of Settings, back to the clicked
    button.
  - Toggle: from a focused terminal, Settings → raw Cmd+J closes both
    Settings and the bottom panel; Escape and several frames later focus
    remains on the visible root, not the hidden terminal. Repeat with the
    panel initially closed to verify the opening direction and with the
    command-id route used by the platform menu / palette. Removing the
    Settings-close helper from the shared toggle fails
    `settings-toggle-terminal: Cmd+J must close Settings`.
  - Checked mutations: without `.focus_trap` the wrap leaves Settings;
    without the focus on open, the focus stays on the terminal; without the
    return-focus capture, Escape does not return to the terminal; with the
    editor made an indenting multi-line input, Tab stays in it.
  - Unmounted return targets (`KAGI_GUI_E2E_ONLY=settings_hidden_return_target`):
    focus a sidebar mode-nav cell, open Settings through `app.settings`,
    press raw Cmd+B, let the sidebar finish closing, then Escape; raw Down
    advances Graph. Repeat with the inspector's selectable commit message
    focused and raw Cmd+Option+B hiding commit details. Neither retained
    handle may receive focus after its pane disappears, even in the same
    session. Disabling the rendered-dispatch-tree membership check must fail.
- overlay focus return (`KAGI_GUI_E2E_ONLY=palette_push_modal_keys,settings_close_returns_focus`,
  `tests/recovery/overlay_focus.rs`): #817 / #812. Every key is raw, with no
  test-side refocusing. The palette scenario first starts the bottom-panel
  terminal the way launch does — it takes focus, and Escape is bound
  `!Terminal` — then `cmd-p` → `push` → Enter opens the blocked Push plan:
  Enter refuses it (Failed footer, one durable Refused receipt) and a
  reopened plan closes on Escape. Settings is opened through `app.settings`,
  its theme picker clicked (the picker must hold focus), then closed by
  Escape and by ×; the next raw ↓/↑ must step File History. Tier B measured
  on 2026-10-01 that the terminal-focus case is what left Escape unmatched
  (`key: "escape"` printed, modal open) while Enter still reached the slot.
- Settings' switches (`KAGI_GUI_E2E_ONLY=settings_switches`,
  `tests/recovery/settings_switches.rs`, #970): every switch is a
  `keyboard_nav::switch`. The scenario opens Settings through `app.settings`,
  checks each switch is named by its row (`e2e::recorded_switch`), walks Tab
  from the root over the six in drawing order
  (`settings_switch_focused_for_e2e`), presses Space then Enter on each of
  the five Appearance switches and checks the setting is saved each time and
  the checked state given to assistive technology follows (Smart Commit's
  is only reached: turning it on probes for local LLMs), and clicks one with
  the pointer (one flip). `SavedKeys` puts the five settings back. The ring
  is not observable in Tier A: Tier B Tabs through Settings and looks.
- Tab panels (`KAGI_GUI_E2E_ONLY=tab_panels`, `tests/recovery/tab_panels.rs`,
  #979): the content a tab list switches goes through
  `tab_panel_a11y::tab_panel`, which records the role and label it set
  (`tab_panel_a11y::recorded_tab_panel`; cleared before each frame, so a
  frame that draws no panel reads `None`). Each workspace-mode nav mode
  (PRs, Issues, Graph) names `sidebar-mode-panel` a `TabPanel` after the
  mode; Branch Cleanup (a takeover, sidebar still drawn) gives it no panel;
  each Home pane names `home-pane-panel` after the pane, without the count.
  `repo_tab_panels` (#983, same file): with two repository tabs the
  workspace body (`repo-tab-panel`) is named after the tab in front, after
  opening the second and after switching back; with Home in front
  `home-tab-panel` is named Home and no `repo-tab-panel` is drawn.
  `gh` is a failing stand-in. What VoiceOver speaks is not observable by an
  agent (the #972 probe found the native AX tree exposes no GPUI content).
- modal input transitions (`KAGI_GUI_E2E_ONLY=remote_browse_escape_focus,pr_fields_escape_focus`,
  `tests/recovery/remote_browse_focus.rs`, `tests/recovery/pr_fields_focus.rs`):
  #755 follow-up. Real InputStates own focus before Remote Browse's
  Connect→Browse transition and PR Fields' Cancel/Apply exits. Raw Escape is
  delivered without test-side refocusing: it closes the browser or the notice
  queued behind the field picker. Both scenarios failed before the fix; PR
  Fields measured lost `CloseMainDiff` routing for both exits. The remote case
  also preserves input focus on failed and superseded connection results.
  Only the connect transport task is queued through
  `src/ui/remote_browse_e2e.rs`; validation, generation checks and completion
  remain production code. PR Fields uses a temporary offline `gh` that refuses
  Apply, so the real dispatch runs without a GitHub write. Its Cancel/Apply
  controls use the existing measured-button seam. Pair with
  `remote_browse_modal_routing,conflict_abort_escape_focus` for the earlier
  routing contracts. This follow-up uses Tier A; it does not claim Tier B or
  live SSH/GitHub coverage.
- bottom-panel placement and toggle (`KAGI_GUI_E2E_ONLY=bottom_panel`,
  `tests/gui_e2e_runner.rs`): the painted Terminal/Operation Log/Activity
  container occupies only the center workspace width, aligned with the commit
  list and stopping before the inspector; sidebar and inspector extend beside
  it to the window-wide status bar. Cmd-J hides it, the action restores it,
  and dragging its top edge changes its height without changing that alignment.
  Tier B: photograph the real window with a selected commit (visible right
  inspector), sidebar, and terminal panel to confirm actual pixels.
  `KAGI_GUI_E2E_ONLY=bottom_panel_nested`
  (`tests/recovery/bottom_panel.rs`) checks the PR and Issues navigators stay
  full-height next to the panel under their center panes, and Editor keeps its
  file tree/hunks beside the panel even after a child-only tree-width change.
  Tier B: also inspect the Editor and PR workspaces with the panel open.
  Graph copy, oplog expand/copy, snapshot creation, theme switching,
  agent provenance, and WIP-to-HEAD connectors retain their existing scenarios;
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
  the measuring spinner and late-delivery rejection after supersession,
  hovering another row and tab close. Bring virtual sidebar rows into view before
  hovering. The detail card appears for the hovered linked worktree only,
  leaves all five navigator panes their full layout height, and remains
  actionable when the pointer moves into it. Moving away hides the card;
  hovering another row changes its identity. Right-click a hovered row:
  the card yields to the existing worktree menu and its actions remain
  clickable. Check the name/path heading, independently scrolling body
  and Refresh action from inside the card in both languages.
  A partial-cache regression accepts one worktree report, switches to another
  repository and returns through the real tab lifecycle. Missing and newly added
  worktrees must then be measured while the completed report remains unchanged,
  even after its directory grows. Backend fixtures also reject mirror-refspec
  and symbolic-alias local refs as pushed evidence. Host unit tests cover Windows
  physical-size selection and unavailable-query errors; cross-checking the
  Windows module is not Windows runtime verification.
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
- Input-confirm cards (`KAGI_GUI_E2E_ONLY=create_branch_input_confirm_ime,input_confirm_disabled_cards,stash_push_stacked_preview`,
  `tests/recovery/operations.rs`): #956. The real Create Branch card measures
  `plan-state-current`, `plan-state-arrow`, and `plan-state-predicted` in
  one horizontally aligned row. Stash Push uses a dirty tracked file and an
  untracked file to verify its real plan has both status counts, a warning and
  a clean predicted state; its two preview regions stack and align, its ready
  and blocked actions are 24px high, and planning never writes to the repository.
  An empty name or a blocked plan renders no `input-recovery` row across all
  six cards. Set Upstream's invalid format appears below its input once, not
  again in the plan blocker list; a measured invalid-plan card catches duplicates.
  Create Branch has no recovery row even when ready. A ready Tag plan displays
  its structured Git command.
  A real marked-text Enter in ready Branch, Rename Branch and Set Upstream
  plans must leave the modal open and the repository unchanged; ordinary
  Enter after unmarking still creates the new branch. These are hidden native
  windows; for colors, glyph weight and chip legibility inspect Tier B.
- PR viewed files (`KAGI_GUI_E2E_ONLY=pr_viewed`, `tests/recovery/pr_viewed.rs`):
  #351 / ADR-0207. A real PR ref fetch (bare remote with `refs/pull/7/head`,
  reached through `url.<file>.insteadOf` for `github.com/example/repo`; other gh
  reads use `pr_fields_focus`'s offline gh) loads two files. Clicking each row's
  measured `pr-file-viewed-<n>` checkbox marks it without selecting the row, and
  the marks land in `pr-viewed/example-repo-7.json` as `{path: blob}`. Closing
  and reopening the tab reads them back from disk; a queued list response with a
  moved head whose new commit changes one file unviews that file only. The
  scenario advances the test clock before unmounting so gpui-component's
  Checkbox animation timers do not outlive the window (leak check). Tier B:
  open a PR, tick files, see `N / M viewed` and dimmed rows (EN/JA), close and
  reopen the tab, then push a commit changing one ticked file and refresh.
- PR review threads on the diff (`KAGI_GUI_E2E_ONLY=pr_threads`,
  `tests/recovery/pr_threads.rs`): #351 / ADR-0209. The `pr_viewed` fixture's
  real PR ref fetch loads `c.txt` (line 2 rewritten); the queued conversation
  carries RIGHT line 2, LEFT line 2 and an outdated RIGHT thread on line 3.
  Unified: badges `pr-thread-badge-<row>` on exactly those rows; a real click
  opens `pr-thread-<row>-<k>` directly under its row with its body
  (`pr-thread-body-…`) laid out and the next row pushed below; the outdated
  thread opens as `pr-thread-outdated-<row>-<k>`; closing both returns the list
  to one item per diff row. Split: `-l` / `-r` badges in the left / right
  gutter, and a RIGHT thread on a context row only on the right.
  `pr_threads_via_gh` (same substring) goes through the production read path: a
  fake `gh` on `PATH` answers two threads only if every field the parser reads
  is requested by its own name (else GraphQL `errors`, exit 1, as GitHub), and
  the tab must hold both; a failed read logs `[kagi] pr-threads: #N read failed:`.
  Tier B: open a
  PR with line comments (current and outdated), click badges in unified and
  side-by-side, check the dimmed outdated card, EN/JA chips, no resolve button,
  and that scrolling does not jump when a thread opens or closes.
- PR suggestion apply (`KAGI_GUI_E2E_ONLY=pr_suggestion_apply`,
  `tests/recovery/pr_suggestion_apply.rs`): #351 / ADR-0210. The same real PR
  ref fetch as `pr_viewed`, with the PR branch checked out so `s.txt` is the
  head's blob, and an injected line comment carrying a ```suggestion block.
  Clicking the measured `pr-convo-apply-suggestion-7000` on the Review page
  opens the plan card with no blocker and writes nothing; `plan-cancel` closes
  it with the file and the oplog unchanged. A second click and `plan-confirm`
  rewrite exactly line 2, stage nothing, and record one successful
  `apply-suggestion` entry with one `refs/kagi/backups/` ref. Blob mismatch,
  missing head, TOCTOU and gc recovery are `crates/kagi-git/tests/
  suggestion_apply_test.rs`. Tier B: open a PR with a suggestion on its
  checked-out branch, apply it (EN/JA card), then edit the file and see the
  "not the PR head's version" refusal.
- header buttons stay reachable (`KAGI_GUI_E2E_ONLY=header_fit`,
  `tests/recovery/header_fit.rs`): #809. A real `KagiApp` at 1000x720, zoom
  1.25, EN and JA opens a commit's long-path file diff, then File History for
  it. Each Main Diff header control (`main-diff-back`, `-ext-editor`,
  `-history`, `diff-mode-toggle`, `main-diff-stats`), each File History header
  button (`fh-back`, `fh-refresh`, `fh-copy-path`, `fh-open-file`,
  `fh-follow`) and the embedded diff's toggle/stats must lie inside its header
  row, and each row inside the window — bounds recorded by the production
  `HeaderFit`, which is what the pane crate can reach. The narrow case must be
  icon-only; 1920x1080 at zoom 1 must keep the labels. Tier B: 1000px wide at
  125% in JA, open a file diff and File History; every button is visible and
  clickable, icon-only buttons name themselves in a hover tooltip, and a wide
  window shows the labels again.
- worktree port in the shell and the sidebar (`KAGI_GUI_E2E_ONLY=worktree_port_env`,
  `tests/recovery/worktree_ports.rs`): #855 / ADR-0171. The shell is swapped for a
  script through `KagiApp::set_terminal_shell_for_e2e` (`gui-e2e` only) that
  writes the `KAGI_PORT` / `KAGI_WORKTREE_PATH` it was spawned with, so the
  oracle is the real PTY child's environment: it must equal the block the store
  holds for the worktree. The main worktree still owns its port block but has
  no sidebar row; a linked worktree assigned before mount shows its block
  (read by the snapshot), and one never assigned shows none and stays
  unassigned. In nonconcurrent mode the linked row displays the main block.
  Tier B: open a linked worktree terminal, see `localhost:<port>` on its row,
  click it and check the browser opens that URL.
- nonconcurrent run mode (`KAGI_GUI_E2E_ONLY=worktree_nonconcurrent`,
  `tests/recovery/worktree_ports.rs`): #859 / ADR-0213. With
  `worktree_run_mode` = `nonconcurrent` and the shell seam swapped for `exec cat`,
  the linked worktree's tab starts a shell; the main worktree's tab of the same
  repository then spawns none and the footer names the running worktree. EOF to
  the first shell (its `cat` exits, observed by the #772 wait) lets the main tab
  start. With the setting removed (default `concurrent`) the linked tab starts
  alongside. The decision is unit-tested in `kagi_domain::worktree_run_mode`.
  #869: while the linked shell runs in `nonconcurrent` mode, the `side`
  sidebar row carries the main worktree's stored port block; main has no
  sidebar row, and `side` has no store entry. The env side is covered by `kagi-git`
  `nonconcurrent_hands_every_worktree_the_main_block`.
  Tier B: set the key, open terminals in two worktrees of one repository, read
  the refusal (EN/JA), `exit` the first, then open the second.
- remove a worktree with a live terminal shell
  (`KAGI_GUI_E2E_ONLY=worktree_remove_live_shell`,
  `tests/recovery/worktree_remove_shell.rs`): #867 / ADR-0171 Follow-up 1.
  - Setup: the shell seam runs a script that starts `nohup sleep 3094 &` and
    then `exec cat`. The linked worktree's tab starts it.
  - With the shell running, the remove plan from the main tab carries the
    `RemoveLiveShell` blocker. Its text names the worktree and says `exit`.
    Confirming refuses (the notice repeats the reason) and the directory stays.
  - After EOF to the shell (exit observed): no blocker, and a
    `RemoveLeftoverProcesses` warning with count 1 for the `nohup` job.
  - After `pkill` of the job: no warning, and confirming removes the worktree.
  - Unit tests cover the decision (`kagi_domain::worktree_remove_shells`) and
    the session probe (`kagi_git::proc::session`).
  - Tier B: open a worktree's terminal, run `nohup sleep 999 &`, try to remove
    it (blocked, EN/JA), `exit`, then plan again (warning) and remove.
- Operation Log badges, recorded ref moves and estimated reflog
  (`KAGI_GUI_E2E_ONLY=oplog_actor_reflog`, `tests/recovery/oplog_panel.rs`):
  #334 slice 1 + 2a / ADR-0214. Three real `Backend::run` writes as Human / MCP /
  CLI, one second apart, become three rows with painted actor and worktree
  badges. A real click on the checkout row paints the **recorded** HEAD move
  (main → oplog-a) and no estimate, and starts no reflog read; the first
  creation paints its recorded `refs/heads/oplog-a` creation. Two old-format
  entries (no `ref_moves`) of another worktree that share one second with a
  reflog line show the **estimate**, that line marked ambiguous. The repo
  fingerprint and the oplog length are unchanged (reads only). Recording:
  `crates/kagi-git/tests/oplog_ref_moves_test.rs` (checkout / commit /
  replay-onto / failed op / a move outside the pipeline); codec compatibility:
  `kagi-git` `ref_moves_distinguish_not_recorded_from_nothing_moved`; window:
  `crates/kagi-git/tests/oplog_reflog_test.rs`. Tier B: open the Operation Log
  tab after a few operations (and one through `kagi` CLI / MCP), check the
  badges, select a new row ("Recorded") and one recorded before this change
  ("Estimated").
- Operation Log revert / restore card (`KAGI_GUI_E2E_ONLY=oplog_restore_card`,
  `tests/recovery/oplog_panel.rs`): #334 slice 2b-2 / ADR-0214 §5. Three real
  `create-branch` runs (keep, drop1, drop2) and one unrecorded entry of another
  repository. The unrecorded row's "Revert this operation…" / "Restore to this
  point…" are both painted disabled. A real click on the first creation's
  "Restore to this point…" opens the card: `Moves` deletions of drop1 / drop2,
  `RefsOnly`, `plan-confirm` drawn, branches unchanged. Enter arms (nothing
  moves), Enter again restores (keep + main left, newest entry
  `restore-to-point`); the restore's own row's "Revert this operation…" puts
  drop1 / drop2 back (`op-revert`). Backend: `crates/kagi-git/tests/
  oplog_restore_test.rs`. Tier B: in a scratch repository, create a few
  branches, select an earlier row, read the card (EN/JA), restore, then revert
  the restore from its row. #334 slice 2c / ADR-0214 §6: a recorded commit on
  main is added first, so the card also draws the graph after
  (`restore-preview`): main's moved label on its target commit, 1 commit off
  every branch (`restore-preview-removed-1`, equal to the drop in `git
  rev-list --count --branches` after confirming); the revert card's target is
  no longer loaded, so it paints `restore-preview-unavailable`. Domain rules:
  `kagi-domain` `restore_preview`. Tier B: read the graph after on the card
  (EN/JA) before confirming.
- Operation Log local-tag restore (`KAGI_GUI_E2E_ONLY=oplog_restore_tag_preview`,
  `tests/recovery/oplog_panel.rs`): #887. A recorded branch point followed by
  a recorded local tag opens Restore to this point. The card lists the tag
  deletion, paints a neutral unavailable graph preview rather than predicting
  commit rows, and two confirmations delete only that tag. Backend round trips
  lightweight and annotated tags through their raw OIDs, with CAS drift
  refusal and retained annotated objects in
  `crates/kagi-git/tests/oplog_restore_test.rs`. Tier B: create a local tag
  after an earlier recorded operation, inspect the card and its neutral
  preview, then confirm and check that only the local tag disappeared.
- Operation Log restore across a resolved merge
  (`KAGI_GUI_E2E_ONLY=oplog_restore_across_merge`, `tests/recovery/oplog_panel.rs`):
  #884 / ADR-0214 §4. `create-branch mark` → `merge-into-conflict side` → the
  production `run_recorded_conflict` save → `merge-commit`; the real "Restore
  to this point…" on mark's row opens a card with no blockers, and two
  confirms put main back before the merge. `stash_conflict_close_reopen` also
  asserts the UI continue's persisted entry records `ref_moves = Some([])`.
  Backend: `crates/kagi-git/tests/oplog_conflict_ref_moves_test.rs`. Tier B:
  merge a conflicting branch, resolve and commit in Kagi, then restore to the
  row before the merge.
- terminal auto-lock compare-and-unlock (`KAGI_GUI_E2E_ONLY=terminal_auto_lock_race`,
  `tests/recovery/worktree_lock_reason.rs`): #836 / ADR-0212. (b) Through the
  Backend race seam (`execute_auto_unlock_worktree_racing`,
  `AutoUnlockRace::RelockBeforeMove`), a lock replaced between preflight and the
  release is put back and refused, and the other reason remains. (c) A hand-placed
  `locked.kagi-*` leftover shows as `LockLeftover` on the real manual unlock card
  and blocks the auto release; Enter records a refusal and leaves the lock and the
  leftover. Backend cases (normal release, relock after the move kept, no-clobber
  restore leaving a leftover, idempotent) are `crates/kagi-git/tests/
  worktree_autolock_test.rs`. Tier B: Phase 1's steps, plus a `locked.kagi-*` file
  placed by hand in `.git/worktrees/<name>/` before opening the unlock card.
- external lock on refresh (`KAGI_GUI_E2E_ONLY=external_lock_reload`,
  `tests/recovery/worktree_lock_reason.rs`): #851. Opened at the linked
  worktree and at main, a `git worktree lock` made outside Kagi after launch,
  then the real Cmd+R (`file.refresh` = manual reload + quiet fetch): the read
  model, the painted sidebar row (🔐) and the row's right-click menu all report
  the lock. The fetch's admission supersedes the refresh's read; a no-op fetch
  must re-issue it. The inspection column (Keep/Unknown) is not part of this —
  a new read makes cached observations stale by design until re-measured.
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
- Home tab (`KAGI_GUI_E2E_ONLY=home_tab,welcome_startup_renders,close_last_tab_welcome_renders`,
  `tests/recovery/home_tab.rs`, #923 / ADR-0219): the tab strip's `+` (`tab-add`)
  and New Tab (⌘T) put Home in front (repo commands off, ⌘W closes Home only); a
  recent row (`home-recent-N`) turns Home into that repository's tab, an already
  open one is switched to (no duplicate); clicking a repository tab
  (`repo-tab-N`) only moves Home to the back and its tab (`home-tab`) brings it
  back; "Connect to SSH remote…" opens Remote Browse over Home; with no tab Home
  is the whole window. Home ends the covered tab's visit: a merge plan started
  just before ⌘T is dropped (no modal), and closing Home re-enters that tab
  (`panes_revalidating`). Home draws the workspace's modal layer: an AppNotice
  (`active-modal/app-notice`), a Pull confirmation (`plan-cancel`) and the
  Editor unsaved-changes guard raised by closing the dirty tab behind Home
  (`active-modal/editor-dirty-guard`; Esc keeps the tab) are on screen, as is
  Settings (`settings-theme-select`). Closing the covered tab does not enter
  its neighbour behind Home. `recent_repos` is restored by `SavedKeys`. For Tier B
  press ⌘T (no foreground needed) or click `+` (needs a key window — ask first);
  "Open Folder…" opens the native dialog, which Tier A does not click.
- Home's GitHub list and clone (`KAGI_GUI_E2E_ONLY=home_github`,
  `tests/recovery/home_github.rs`, #923 #924 #930 review / ADR-0219): the
  stand-in `gh` fails on marker files in a state folder and answers
  `config get user` with `acme` (or `state/user`). A saved list
  (`github_repos_cache.json` beside `settings.json`) is read off the UI
  thread and shown only for the account it was saved as (`github.com/acme`;
  one saved as another account is not), and stays when the own-list refresh
  fails (toast names the 502); after switching the user the shown list is
  dropped; a
  second Refresh while one runs joins it (`own-calls` counts one read); a
  Refresh keeps the list drawn beside the `home-github-updating` mark; a failed
  `gh api user/orgs` is the `home-github-orgs-failed` row and is not saved
  over the full list. An offline `gh` lists
  the user's repositories (one whose `origin` matches a recent repository, one
  that is not local) and two organizations — one listed, one refusing with a
  SAML error, which keeps its section with the reason; `repo clone` really
  clones a local bare repository. The organization's row opens a clone card
  sourced from that owner. The local row (`home-gh-<owner>/<repo>`) opens its
  tab; the other opens the clone card with no folder chosen (Kagi never picks
  one), where `clone-confirm` does nothing. The folder is supplied through
  `replan_clone` — what the native dialog's callback calls; a folder whose
  `widgets` is occupied is refused (the file in it untouched), and once free
  Clone clones, writes one `clone` Success receipt keyed by the destination
  and opens the clone in place of Home; while it runs `may_close_host()` is
  false (⌘Q / window close are held) and true again once it is recorded.
  The card sent to the background while the clone runs comes back, still
  running (`started`), when the row ("Cloning…") is picked again (#944).
  Before that a clone into another
  folder fails (`fail-clone`): Home stays in front with a `clone: failed`
  toast drawn there (`toast-stack`). Back on Home, the clone's tab is matched
  into `home_github.local` though it was opened after the list was read, and
  its row's chip is drawn as `home-gh-acme/widgets:Open`. The list's entries
  are built once per change of what they come from (#937): three redraws
  leave `e2e::home_item_builds()` unchanged, a new filter builds them again.
  `home_list_place` (`tests/recovery/home_list_place.rs`, #942 review) lists
  60 repositories, scrolls to entry 30 and checks it stays there — without a
  rebuild — when the hidden pull request / issue lists land
  (`reload_home_work_for_e2e`), and in place but rebuilt when the language
  changes and when a Refresh lands (`home_list_top_for_e2e`).
  `recent_repos` is restored by
  `SavedKeys`; `home_tab` also installs an offline `gh` because Home reads
  `gh repo list` whenever it opens, and checks that Remote Browse opened from
  Home is drawn centred over it (`remote-browse-card`). For Tier B use a real
  `gh` login, choose a temporary folder with "Choose…" (the native dialog)
  and check `[kagi] clone: done … ok=true` and the receipt.
- Home's pull requests and issues (`KAGI_GUI_E2E_ONLY=home_work`,
  `tests/recovery/home_work.rs`, #928 / ADR-0219 decision 8): an offline
  `gh` answers the three `gh search`es (a PR and an issue of a repository
  cloned locally, a draft PR of one that is not, a review request) and
  `pr view -R`; marker files fail every search (`fail-search`) or the review
  search (`fail-review`), and `state/user` switches the `gh` account. The
  test dispatcher runs `gh` inside its pump, so Home is opened and drawn
  before the pump: the switch draws `home-pane-prs-spinner` (never a count of
  0) and the pane `home-work-loading`. Lists saved as another account are not
  shown; after the account switch that account's saved lists are, and stay
  when its searches fail. Rows are `home-work-<owner>/<repo>-<N>`. A failed search draws
  `home-work-failed` above the list read before and saves nothing; the
  non-local row opens on GitHub (the URL is recorded, `e2e::take_opened_urls`),
  stays focused, and Enter / Space press it again (#944; the scenario sends
  each key's release, on which gpui's keyboard click fires),
  and every row's `-open` end opens on GitHub without the row's click; the
  local PR row draws its `-opening` spinner while `pr view` runs, ignores a
  second click (one `view-calls`), then opens PR #7 in the clone's tab. A
  non-local row picked while that local PR is still opening wins: it opens on
  GitHub, and Home stays in front when the dropped PR's refs arrive; so do a
  row's Open button and a pane switch pressed then (the pick and the press go
  in without the pump between them). With `state/default-repo`
  naming `acme/upstream` (`gh repo set-default` elsewhere) the issue row
  stays on Home with a toast naming that repository; the clone's Issues mode
  is then loaded as acme/upstream (a queued list read), and once `gh`
  resolves the clone to acme/local the issue row opens it in Issues mode with
  #4 selected and the list and Reply re-addressed to acme/local
  (`issue_write_repo_for_e2e`); a reply drafted to acme/upstream's #4 before
  that is not in acme/local's #4 Reply and stays stored for acme/upstream
  (drafts are keyed by the repository written to; `drafts_test` covers the
  hand-over of pre-#940 drafts). For Tier B use a real `gh`
  login, click each switch cell and one PR / issue row of a local clone.
- Home's display leftovers of #960 (`KAGI_GUI_E2E_ONLY=home_review_avatar_host,clone_card_ticker`,
  `tests/recovery/home_p2.rs`, #968): a review request whose Enterprise host
  is spelt `GHE.example.com` gets its author's avatar from the fetcher's disk
  cache (`avatar_fetch::cache_path_for_url`, no network) and its row finds it
  (`e2e::home_review_avatar_shown`). The running clone's card closed and
  brought back from its row within a second leaves two redraw tickers
  (`e2e::clone_tickers`) until the older one wakes; after 1.6 s on the test
  dispatcher's clock (`advance_clock`) one is left.
- Keyboard paths of Kagi's tab lists (`KAGI_GUI_E2E_ONLY=keyboard_nav`,
  `tests/recovery/keyboard_nav.rs`, #944): a cell is focused through
  `focus_mode_nav_for_e2e` / `focus_home_pane_for_e2e`; Tab / Shift+Tab are
  pressed as keystrokes (gpui-component's Root moves the focus).
  `e2e::set_github_nav(true)` draws the nav's PRs / Issues cells whether or
  not the machine has a `gh`. The workspace-mode nav: Tab reaches its
  selected cell; an arrow only moves (the mode stays) and Enter / Space
  enter the mode; after →→ one Tab or Shift+Tab leaves the list and the
  other comes back in on the selected cell (#960 review); with a notice up,
  Enter on a cell does not confirm it; a pointer click on a cell gives the
  focus back to the root; after → the focus taken away another way than Tab
  (here the root's own focus) leaves the selected cell as the next Tab's
  stop, not the arrowed-to one (#968). Home's switch: ←/→/Home/End select as they move,
  without wrapping, and read nothing (`home_reads_for_e2e`); with Home's
  search focused the arrows do not reach the switch (Enter is not pressed
  into the single-line field: the harness types its "\n"). The repository
  tab strip (#959): with two repositories and Home, one Tab from the window
  lands on the Home tab and the next leaves the strip (a tab's × is not a
  stop); → only moves (`tab_strip_focused_for_e2e`, Home stays in front),
  Enter switches to the repository, End reaches Home and Space brings it
  back; with a repository in front, the Tab that leaves the strip is not the
  + (Enter there leaves Home behind), and closing Home while its cell holds
  the focus gives the focus to the window. For Tier B: Tab
  to each list, check the ring appears only for keyboard focus, and read the
  roles in Accessibility Inspector.
- Rows of Home's list (`KAGI_GUI_E2E_ONLY=home_rows`,
  `tests/recovery/home_rows.rs`, #959): a stand-in `gh` lists 60
  repositories and two pull requests. Tab from the switch reaches the list
  at its first row and one more Tab leaves it; ↓ forty times focuses `r40`
  and scrolls it into view; ↑/↓ stop at the ends; Tab out and Shift+Tab back
  returns to `r40`; a filter that drops the focused row moves the focus to
  the first row left, and one that drops every row to the window; with the
  search field focused ↓ stays the field's; PR rows step the same way; a
  second stand-in `gh` for another account whose list read fails turns the
  list into Loading then Failed, and the focused row's focus goes to the
  window. Rows
  are focused through `focus_home_row_for_e2e` and read through
  `home_row_focused_for_e2e` (keys `repo:<owner>/<name>`,
  `<kind>:<owner>/<name>#<n>`).
- Toolbar unavailable reasons (`KAGI_GUI_E2E_ONLY=toolbar_keyboard_reasons`,
  `tests/recovery/toolbar_keyboard.rs`, #972): starting at the root, GPUI's
  `focus_next` visits the rendered toolbar in visual order; an F19 key-down
  identifies the actual focus owner. Pull / Push / Stash / Pop / Undo / Redo
  stay AX-disabled but reachable; Enter and Space produce their localized
  pointer-click footer reason, and the exact value passed to
  `aria_description` is recorded by `e2e::toolbar_description`. The fixture
  fingerprint must remain unchanged. Before adding focus stops this scenario
  failed (none reachable); mutating Pull's description input to `None`
  fails its AX assertion. TestDispatcher cannot prove OS Tab/VoiceOver
  speech (the native AX tree did not expose GPUI content during the #972
  foreground probe). Tier B on macOS with a clean one-commit repository and
  explicit foreground permission: pointer-click unavailable Pull, Tab to
  unavailable Push shows the focus-visible ring, and Enter changes the footer
  to the existing no-remote reason without changing HEAD, status or stash.
  The native `AXUIElement` query returned only the window's title-bar
  controls (`AXGroup` for content had zero children), so actual VoiceOver
  announcement of `aria_description` remains unobserved; do not present the
  Tier A attribute oracle as a spoken-word measurement.
- modal-slot arbitration (`KAGI_GUI_E2E_ONLY=push_failure_keeps_modal,merge_plan_latch,delete_branch_plan_latch,remote_browse_modal_routing`): a push failure lands behind Remote Browse without losing its input and reaches the Failed footer, an Error toast and one durable receipt — no dismiss-only AppNotice, queued or shown after Remote Browse closes (the #747 contract; #824 bisected the stale notice expectation to `e5644c6f`). Delayed Merge/Delete Branch plans wait behind Remote Browse without losing its input, stale plan state, latches, footers, or notices; a reopened Remote Browse rejects an older in-place completion by generation;
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
  #791 extends the final page to 300 loaded rows. A real wheel event must move
  `logical_scroll_top().item_ix` while the drawn frame adds zero Issue-view
  recomputations; typing into the real filter input must add exactly one across
  main and sidebar. The gui-e2e-only counter is adjacent to the production
  `apply_issues` call. Tier B uses a repository with 300+ open Issues: after
  two additional pages settle, wheel within the loaded rows and require no new
  `[kagi] issues: view recomputed loaded=N visible=M` lines. A filter change is
  the positive control. Keep paging/refresh/login changes outside that scroll
  interval, since they legitimately invalidate the derived view.
- Issue write failure ownership
  (`KAGI_GUI_E2E_ONLY=issue_failure_notice_survives_tab_switch`,
  `tests/recovery/issue_write_owner.rs`): a recorded Create failure that lands after
  leaving its owner remains in Operation Log without replacing or extending the
  current tab's modal queue, without invoking a GitHub transport;
- New Issue labels / assignees (`KAGI_GUI_E2E_ONLY=issue_create_fields`,
  `tests/recovery/issue_create_fields.rs`, #866): the composer's measured
  `issue-field-open-labels` / `issue-field-open-assignees` (gpui-component
  `Button`s) open the shared field picker (`FieldTarget::NewIssue`), by click
  and by keyboard: `keyboard_open` steps the window tab order from the title
  and sends Enter / Space as key down + key up (GPUI clicks on the up; a bare
  `simulate_keystrokes` never activates a focused element). Apply only stores
  the picks. An offline `gh`
  serves labels from a file the scenario edits, so a label deleted between the
  pick and the measured `issue-composer-submit` is refused before
  `gh issue create` runs (Refused oplog entry, toast naming the label, body and
  picks kept); the retry sends `--label`/`--assignee` and empties the picks;
  the receipt carries `issue_fields` (Operation Log `labels:` / `assignee:`),
  a plan-time refusal (body that is only a fence → no title) included.
  Assignees are picked keyboard-only: Space opens the picker, tab order from
  the filter box reaches the row Buttons (Space toggles, `aria_selected`) and
  Apply (Space). Apply saves the picks with the draft, and a composer dropped
  with `forget_issue_composer_for_e2e` reloads text and picks through the
  production load (#903; file format and the `.corrupt` set-aside are pinned
  by `crates/kagi-git/tests/drafts_test.rs`).
  `issue-composer-posted-as` and the avatar marker
  `issue-composer-viewer-<login>` follow the login on the repository's host
  (`github_host_logins`, read with `gh api user [--hostname]` after an Issues
  read; `?` until then) — never the window-global `github_login`. Pair
  with `pr_fields_escape_focus` and `workspace_mode_toolbar` for the PR picker;
- Field picker ownership (`KAGI_GUI_E2E_ONLY=field_picker_owner`,
  `tests/recovery/field_picker_owner.rs`, #904 review): a New Issue or PR
  picker opened in tab A and applied after `switch_repo` to B (which shows
  the same PR number) is dropped by the switch — `PrFields` is repo-scoped —
  so neither composer changes and no `pr-edit` is sent. The picker also
  carries its `owner`; Apply only touches that session;
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
`crates/kagi-git/tests/ops_test.rs` cover main/linked occupancy, occupancy arising after approval,
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

### What reaches a real `InputState` and what goes around it (#516)

A scenario that passes says nothing about text entry unless the text went
through a real gpui-component `InputState`. Read the scenario's row here before
citing it as evidence for typing, pasting or a field's value. None of these
cover IME composition or AppKit's input method: Tier A delivers keystrokes and
paste actions, not marked text (use Tier B for IME).

| How text reaches the field | Scenarios (`KAGI_GUI_E2E_ONLY` names) | What it proves |
|---|---|---|
| Typed keystrokes into the focused real `InputState` (`simulate_keystrokes` with characters) | `conflict_save_boundary`, `editor_save_admission`, `editor_save_buffer_identity`, `editor_external_change_banner`, `editor_banner_rename_and_save`, `remote_connect_keeps_dirty_editor`, `cross_worktree_merge` (Editor buffer); `create_branch_presents_backend_receipt`, `create_branch_replan_error` (branch name); `issues_pagination` (list filter); `palette_push_modal_keys` (command palette); `workspace_mode_toolbar` (Issue title) | The key path: focus, the input's key handling, its change event and the product's sync from it. |
| Paste into the focused real `InputState` (`write_to_clipboard`, then `cmd-v` or `input::Paste`) | `remote_browse_escape_focus` (host), `worktree_lock_reason` (lock reason, after `cmd-a backspace`), `pr_fields_escape_focus` (picker filter), `workspace_mode_toolbar` (Issue title / body) | The paste path into the field the product focused, and the sync from it. |
| `InputState::set_value` on the real input (no key or paste event) | `conflict_continue_cache` (Result pane), `home_github` (Home search), `theme_custom` (palette query) | The input's change event and the product's handling of the value; not focus or key handling. |
| `InputState::replace` / `replace_all` on the real input (no key or paste event) | `issue_create_fields` (Issue body, via `insert_issue_body_for_e2e` / `replace_issue_body_for_e2e`), `home_work` (Reply body, via `insert_issue_reply_body_for_e2e`), `pr_same_number`, `workspace_mode_toolbar` (PR composer; Issue body via `insert_issue_body_for_e2e` / `replace_issue_body_for_e2e`; Reply body via `insert_issue_reply_body_for_e2e`) | The input's change event and the product's handling of the replaced text (the Composer / Reply draft subscription included); not focus, key or paste handling. |
| `e2e::set_remote_browse_host_input`: `set_value` on the host input **and** a direct write of `host_input` | `merge_plan_latch`, `delete_branch_plan_latch`, `remote_browse_modal_routing`, `push_failure_keeps_modal` | Remote Browse holding its slot and input; not the form's own sync from the field (that is `remote_browse_escape_focus`). |
| No `InputState` at all: `e2e::open_local_panel_no_inputs` / `open_worktree_panel_no_inputs`, message from the `commit_msg` fallback (`e2e::set_commit_message`, as headless `KAGI_COMMIT_MSG`) | `wip_diff_survives_reload`, `commit_panel_survives_reload`, `worktree_wip_inline`, `worktree_panel_commit`, `worktree_panel_amend_discard`, `worktree_panel_discard_recording_failure`, `diff_highlight_once`, `diff_highlight_stale`, `file_menu_freezes_path`, `file_menu_rejects_stale_owner`, `file_tree_roles`, `hunk_staging`, `modal_compact`, `smart_commit_generation_owner`, `smart_commit_modal_and_probe`, `stage_failure_notice`, `dialog_a11y_roles`, `commit_stage_deferred_owner`, `commit_panel_revalidates_on_activation`, `smart_generation_close_drops_panel`, `commit_panel_refuses_during_activation`, `commit_close_drops_panel`, `manual_reload_releases_revalidation`, `commit_row_layout_wip` | The commit panel's ownership, staging and write paths. Nothing about the message or description inputs: these were built without them because each `InputState` registers an App-level observer that keeps it alive past its window (see `open_worktree_panel_no_inputs`). |

Not input, but also a stand-in: the `e2e::queue_*` seams (`queue_remote_refresh`,
`queue_remote_open`, `queue_smart_generation`, `queue_github_pr_fetch`,
`queue_github_pr_conversation`, `queue_cleanup_scan`, `queue_squash_scan`,
`queue_ecosystem_mine`) hand the next background read a task the scenario
controls instead of running `git`, `gh` or the LLM. They prove what the app does
with a result that lands late, for another owner or out of order — not that the
real read produces it. Used by `remote_refresh_departed_owner`,
`remote_refresh_newest_request`, `remote_browse_modal_routing`,
`smart_commit_generation_owner`, `smart_generation_close_drops_panel`,
`pr_list_roles`, `pr_suggestion_apply`, `pr_threads`, `pr_viewed`,
`pr_fields_escape_focus`, `field_picker_owner`, `ghe_viewer_login`,
`ghe_viewer_login_closed_only`, `github_evidence_restores`,
`github_evidence_background_owner`, `github_evidence_detached_owner`,
`cleanup_evidence_background_owner`, `cleanup_evidence_superseded`,
`cleanup_evidence_read_revision`, `cleanup_evidence_publish_generation`,
`squash_evidence_read_revision`, `squash_evidence_publish_generation`,
`ecosystem_evidence_background_owner`, `ecosystem_evidence_superseded`,
`ecosystem_evidence_detached_samepath` and `workspace_mode_toolbar`.

The same kind of stand-in, outside `e2e::queue_*`:

- `KagiApp::queue_issue_list_fetch_for_e2e` (the next Issues list read, in
  place of `gh issue list`): `issues_pagination`, `home_work`.
- `remote_browse::e2e_transport::queue_remote_connect` (the next Remote Browse connection,
  in place of `ssh`): `remote_browse_escape_focus`.
- `e2e::worktree_inspection::queue` (the next worktree inspection, in place of
  the size and removal-condition read): `worktree_inspection`.

gpui's end-of-run leak detector stays on. `gui-e2e` enables `gpui/test-support`,
which enables gpui's `leak-detection`. The detector runs when the runner's App
is dropped, after the last scenario has passed, and a leaked entity fails the
whole run (`Leaked handle for entity …`, exit non-zero). That is why the commit
panel rows above use the no-inputs seam instead of keeping an `InputState`
alive. Do not get a scenario through by switching the detector off, keeping the
App alive, or forgetting a handle (`mem::forget`, `ManuallyDrop`, `Box::leak`).
If a real input cannot be torn down, use a seam and add the scenario to the
table.

## Tier B — real GUI driver

Build `scripts/pidclick.swift` and, into the same directory, `scripts/pidcursor.swift`
(the agent cursor, below). Launch Kagi with a unique `USER` value, and retain
all three isolation flags. `USER` namespaces the per-user socket name, while
`KAGI_NO_RESTORE=1` and `KAGI_LOG_DIR` prevent fixture work from changing the
user's persisted session, settings, trust, or oplog. `KAGI_NO_ACTIVATE=1` avoids
stealing the foreground application.

```bash
swiftc scripts/pidclick.swift -o /tmp/pidclick
swiftc scripts/pidcursor.swift -o /tmp/pidcursor
VERIFY_USER="kagi-verify-$RANDOM"
VERIFY_LOG_DIR="$(mktemp -d)"
USER="$VERIFY_USER" KAGI_NO_ACTIVATE=1 KAGI_NO_RESTORE=1 \
  KAGI_LOG_DIR="$VERIFY_LOG_DIR" ./target/debug/kagi /tmp/kagi-vfx-a/repo \
  2>"$VERIFY_LOG_DIR/kagi.stderr" &
PID=$!
/tmp/pidclick windows --pid "$PID"
# Select Kagi's main window: the LARGEST layer-0 window of this PID, not the
# first one listed — Kagi also owns small layer-0 windows (title-bar strips,
# helpers), which pidclick refuses or the coordinates fall outside of. Set its ID:
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

**The agent cursor (#948).** `move`, `click`, `rclick` and `scroll` also show
where they act, so a person watching the run can follow it. pidclick sends one
datagram to `<per-user temp dir>/pidcursor/<PID>.sock` and, when nothing listens,
starts `pidcursor --pid <PID>` from its own directory (detached, its own
session), then waits about 0.3 s for the cursor to glide there before it posts
the event. When there is no cursor — `pidcursor` not built, or a daemon that
could not start (its stderr is kept as `<PID>.log` beside the socket, e.g. a
full disk) — pidclick prints one `pidclick: no cursor: …` line with the reason
and posts the event anyway. `key` and `type` do not move it.

- **It never takes the foreground.** The daemon is an accessory app that never
  activates, with one click-through, non-activating panel the size of the
  target window, ordered just above that window. It comes to the very front
  only when the target window already is the frontmost window, so it never
  covers another app. The panel joins every Space, so it follows the target
  window to whichever Space shows it; while the target window is off screen
  (another Space, minimized), the cursor is ordered out. It follows only
  windows of the PID it was started for.
- **It ends by itself:** when the target process exits (checked: killing the
  test Kagi ends it within 0.2 s and removes the socket), or after 10 minutes
  without a message (`PIDCURSOR_IDLE_EXIT=<seconds>` overrides, for testing).
  After 15 s without a message it fades out over 180 ms and comes back with
  the next one.
- **Screenshots do not show it.** `screencapture -l<WID>` captures only the
  target window, and the cursor is a separate window, so before / after images
  stay clean.
- **Turn it off** with `--no-cursor` or `PIDCLICK_CURSOR=0` (for timing-sensitive
  scenarios, where the 0.3 s wait matters).

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

Without the build the run stops at once, before any server starts:
`kagi-web harness not built: <files> missing in …/crates/kagi-web/dist`, with
`scripts/build-web.sh` to run (`e2e/harness.ts`, #516). It used to surface as
`Timed out waiting 60000ms from config.webServer`, which read like a runtime
hang. The check runs while `playwright.config.ts` loads: Playwright waits for
the `webServer` before it runs a `globalSetup`, so a check there would sit
behind the same timeout (measured: 61 s).

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
`crates/kagi-git/tests/backend_fixture_storage_test.rs` drives the migrated branch adapter in
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
Skip の分類順 (#569 (1)) は `cargo test -p kagi --lib conflict_skip::tests`
で確認する。`tests/recovery/conflict_skip_g.rs` は GUI を起動せず、実 repository と
`Sessions` で Unclear の lease 保持・writer 拒否・reconcile 登録、typed
TerminationUnknown の Unknown 維持、既知結果の通常解放を検証する。
既存 `app_writer_admission_test` と `conflicts_test` の `skip_` も併用する。
For M, check that notice dismissal/tab switching does not re-enable PR merge or
remote pull after Unknown/Partial; Failed alone permits retry. Holds persist for
the app lifetime; inspect remote state before restarting. GUI runner build only
when execution is reserved for PM.

Continue の post-read (#569 (2)) は
`cargo test -p kagi --test app_writer_admission_test continue_` で確認する。
実 stash conflict の解決後、壊れた ref で summary を失敗させ、stage 済みであること、
Stopped Unknown・lease 保持・reconcile 登録を検証する。未解決 buffer の実行前拒否と
既知の `Staged` 成功は通常解放する。既存 `conflicts_test` / `stash_conflict_test` も併用する。
これは backend と app settlement の G 検証で、GUI の実行・表示確認ではない。

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
details in EN/JA, checks that trust refusal records without requesting a modal,
and that only a failed oplog append asks for the dismiss-only notice.
Use a fresh KAGI_LOG_DIR as with all cargo tests.

Tier A filter: `KAGI_GUI_E2E_ONLY=stage_failure_notice`. The scenario covers editor
paths, panel file indices and batch buttons under index.lock, including a linked
worktree panel while the main tab remains active. Per attempt it asserts the
Failed footer (cause, path and the actual owning repo), the newest toast is an
Error `<op>: failed …`, **no** `AppNotice` (ADR-0196 §3 as amended by #747 — a
recorded failure is never a dismiss-only modal), one more Failed oplog receipt
for the owning repo, and byte-identical indexes. An admission denial records a
Refused receipt, again without a notice. Until #846 the scenario still asserted
the pre-#747 notice and panicked on it. Compile only when PM owns E execution.
M: hold index.lock, click Stage/Unstage from both surfaces, verify the actual
cause is visible and no success toast appears. If recording also fails, the
attempted failure stays visible with the recording error; it is not a success.

### Hunk staging (#842, Refs #357)

G: `cargo test -p kagi-git --test hunk_staging_test` (a 20-line file edited at
lines 2 and 18: one hunk staged / unstaged leaves exactly that edit in or out of
the index, the working tree untouched; a hunk drawn before the file moved and an
unstage of a range not in the staged diff are refused with `HunkChanged`; an
untracked file's single hunk is the file) and `cargo test -p kagi-domain
hunk_range` (header parse).

Tier A filter: `KAGI_GUI_E2E_ONLY=hunk_staging` (`tests/recovery/hunk_staging.rs`).
It clicks the measured `main-diff-hunk-stage-<row>` on the Commit Panel's
unstaged diff (index holds the first hunk only, panel lists the file on both
sides, the re-read diff shows one hunk), then on the staged diff in split view
(index back to HEAD, pane closed), then under a held index.lock (#490 footer,
index unchanged). M: open a two-hunk file from the Commit Panel, Stage hunk /
Unstage hunk in unified and split view (EN/JA labels), and check the panel and
diff update.

### Busy snackbar labels (#607)

G covers EN/JA labels, unknown-tag fallback, and lease-mirror settlement without
clearing legacy plans. `uv run --project ci check-busy-labels` checks literal
busy tags and finite operation-name producers against the label table.
Tier A: `KAGI_GUI_E2E_ONLY=fetch_busy_label` in `tests/recovery/busy_label.rs`
starts a local fetch through `fetch_async`, checks the renderer's snackbar text
before completion in both languages, and checks release after completion.
Build only when PM owns execution. For M, verify Fetch, Commit, Stash, Discard,
branch deletion and editor Save show an operation label, never `app-writer`.

### Slow-read advice (#355, ADR-0206)

G: `cargo test -p kagi-ui-core -- slow_read busy` (the 2 s boundary and EN/JA
text), `cargo test -p kagi --lib -- slow_reads branch_menu` (per-phase timing,
Skip reaching the probe, unknown counts keeping Pull/Push enabled) and
`cargo test -p kagi-git --test snapshot_probe_test` (a skipped snapshot lists
the upstream with `counts: None`; the next one counts).
Tier A: `KAGI_GUI_E2E_ONLY=slow_read_explained` in `tests/recovery/slow_read.rs`
holds the real reload's snapshot in its ahead/behind phase
(`KagiApp::hold_next_snapshot_for_e2e`) and advances the dispatcher clock in the
tracker's 250 ms ticks: nothing at 1.75 s into the phase, the drawn
`busy-snackbar-advice` and `busy-snackbar-skip` after 2 s, a real click on Skip
hides them, the released read lands `counts: None` (status summary unknown, not
"no upstream"), and the next reload counts again — in EN and JA. Tier B:
`bash scripts/make_fixture.sh <dir> 10500`, add branches with upstreams, reload
and read the snackbar and `[kagi] busy: slow <op> after 2s` /
`[kagi] busy: skip <op>`; after Skip the sidebar shows `—`.
