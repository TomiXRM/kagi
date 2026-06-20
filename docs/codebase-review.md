# Kagi Codebase Review

> Deep, grounded review of the Kagi GPUI Git client (`v0.5.0`, ~84k LOC Rust).
> Method: six focused review passes (architecture, git-safety, performance, UX,
> code-cleanup, GPUI) by parallel subagents reading the actual code, followed by
> direct verification of every headline claim against the tree. Every finding
> cites `file:line` evidence. No generic advice.

## How to read this document

Findings use this compact format:

```
### Finding: <short title>
Severity: Critical / High / Medium / Low
Effort: Small / Medium / Large
Confidence: High / Medium / Low
Evidence: file:line + function/module + excerpt
Problem: ...
Why this matters: ...
Recommended fix: ...
Suggested verification: ...
```

Severity = impact if unfixed. Effort = work to fix. Confidence = how solid the
evidence is (all headline numbers below were directly verified against the tree).

---

## 1. Executive summary

### Verified baseline facts

| Metric | Value | Source |
|---|---|---|
| `KagiApp` fields (god-struct) | **104** | `src/ui/mod.rs:747-1136` |
| `render()` function length | **848 LOC** | `src/ui/render.rs:396-1243` |
| `run_repo_flow` (headless) length | **1456 LOC** | `src/headless.rs:258` |
| `Backend` delegating methods | **112** | `src/git/backend.rs` |
| `Backend::open` call sites in `src/` | **132** (94 in `src/ui`) | grep |
| `git2::Repository::open` in `headless.rs` | **26** | grep |
| `cx.notify()` call sites in `src/` | **329** | grep |
| Files > 800 LOC (AGENTS.md target) | **29** | `wc -l` |
| View-model layer size | **213 LOC** (1 component) | `src/ui/view_models/` |
| `KAGI_*` env-var hooks in headless | **48** | grep |
| Test files copy-pasting `fn git()` | **26** | grep |
| Forbidden ops (`reset --hard`, `push --force`, `git clean`, `unsafe`) | **0 actual** (all hits are doc/comment/recovery text) | grep + manual |
| `git2::` in `src/ui/` | **0** (CI gate holds) | grep |

### Top 5 most serious problems

1. **The safety pipeline is a UI-layer convention, not a backend guarantee.**
   `Backend::execute(op)` (`src/git/backend.rs:304-427`) dispatches Checkout,
   CherryPick, Merge, Revert, Pull, Push, Undo, Amend, StashApply, StashPop, and
   StashPush **straight to `execute_*` with no `preflight_check`, no `verify`,
   and no `append_oplog`**. Only `DeleteBranch` and `Discard` re-plan inline.
   The advertised "`plan → confirm → preflight → execute → verify → oplog`"
   contract holds *only when the UI happens to call it*. Any non-UI caller
   (headless `KAGI_*` hooks, tests, future automation) silently bypasses
   preflight, verify, and the oplog. **This directly contradicts the product's
   safety thesis.** (Git-safety Finding 2; verified at `backend.rs:304-427`.)

2. **`KagiApp` is a 104-field god-Entity on which the entire UI re-renders for
   any change.** Every keystroke, toast expiry, watcher tick, or hover triggers
   `cx.notify()` (329 call sites, almost never batched) → the full 848-line
   `render()` re-runs → the 6100-line `render.rs` and 6406-line `mod.rs` are
   walked. This is the architectural root cause behind most performance
   findings (per-frame Vec clones, per-frame graph-path rebuilds, per-frame
   `theme()` lookups). (Architecture Finding 1, GPUI Findings 1–2.)

3. **Merge is allowed into a dirty working tree (warnings only); cherry-pick is
   blocked.** `plan_merge_branch` calls `merge_dirty_warnings` (advisory yellow)
   and then `execute_merge_into_conflict` writes conflict markers into the
   user's uncommitted files. The in-tree rollback (`ORIG_HEAD`) restores the
   *ref*, not the working tree. This is the single most dangerous Git path.
   (Git-safety Finding 11, at `src/git/ops/merge.rs:26, 359-420`.)

4. **Per-file diff content is not cached and tree-sitter highlighting is
   synchronous on the UI thread.** Clicking between two commits to compare the
   same file recomputes the full git2 diff every time; opening a large-file
   diff blocks the frame on a full tree-sitter parse. Combined with the
   unbounded `git2::statuses` scan (no pathspec, recurses untracked dirs) and
   the on-UI-thread `reload_external`, large/messy repos freeze the app for
   hundreds of ms per external git event. (Performance Findings 1, 12, 13, 15.)

5. **The safety surface advertised in the UI is partly fake, and the safety
   net that exists is unreachable.** Rebase, Create PR, Force-with-lease push,
   Reset-to-commit, Delete-remote-branch, and Pull-ff-only are all wired as
   menu items permanently greyed out with "not implemented yet". Worse, the
   backup-before-discard oplog blobs *exist on disk* but there is no in-app
   restore action — recovery requires `git cat-file -p <sha>` in a terminal.
   Discard itself is single-click (no two-stage confirm like amend has).
   (UX Findings 1, 6, 7, 9.)

### What will break first if development continues without cleanup

- **The safety thesis will be falsified by a bug report.** Someone will use a
  `KAGI_*` hook or a future non-UI caller, hit a dirty-tree state, and lose
  work because preflight was skipped. The oplog won't record it. This is the
  highest-probability, highest-impact failure.
- **Render performance will regress non-linearly.** Every new feature adds
  fields to `KagiApp` and branches to `render()`, and each `cx.notify()`
  repaints more. The codebase is already at 329 notify sites on one entity.
- **Any merge conflict involving uncommitted local edits** will interleave
  conflict markers with the user's unsaved work, and `git merge --abort` (the
  only recovery) is not surfaced.
- **The migration will stall.** The view-model layer (the documented escape
  hatch from the god-struct and the headless harness) is 213 LOC covering one
  component. ADR-0076/0077 are presented as progress but are ~5% delivered.
  Continuing to build features on `KagiApp` deepens the debt the migration is
  supposed to pay off.

### What should be fixed before adding more features

1. Make `Backend::execute` the single enforced pipeline (preflight → execute →
   verify → oplog) and require a confirmed `OperationPlan` for every mutating
   op. (Closes git-safety Findings 1, 2, 3, 6, 12–16, 27 in one change.)
2. Block merge (especially the into-conflict path) on a dirty working tree,
   matching the existing cherry-pick rule.
3. Make `stage_conflict_resolution` atomic (write-to-temp-then-rename) using
   the deferred-`index.write()` pattern already proven in
   `execute_conflict_continue`.
4. Add two-stage confirm to Discard (mirror amend's `confirm_armed`) and an
   in-app "Restore" action on discard/delete-branch oplog rows.
5. Move `reload_external`'s snapshot off the UI thread and cache per-file diff
   content by `(row, file_index)`.

These five are small-to-medium, mostly localized, and directly protect the
product's reason to exist.

---

## 2. Architecture problems

### Finding: `KagiApp` is a 104-field god-struct mixing every concern
Severity: Critical · Effort: Large · Confidence: High

Evidence:
- `src/ui/mod.rs:747-1136` — `pub struct KagiApp` spans ~390 lines, **104 fields**
  (directly counted). Fields mix: UI layout (`sidebar_width`, `inspector_split`),
  ephemeral process state (`busy_op`, `fetch_in_flight`), caches
  (`diff_cache`, `diffstat_cache`, `avatar_images`), 8 `UniformListScrollHandle`s,
  6 `Rc<Cell<(f32,f32)>>` geometry backchannels, 5 `Option<Entity<InputState>>`
  text inputs, terminal sessions, oplog ring buffer, undo/redo history, toast
  stack, and every modal's state.

Problem: One struct owns state for every feature ever added. `&mut self` is
borrowed across ~15 files (`mod.rs`, `render.rs`, 12 `operations/*.rs`). Every
new feature bolts on 3–8 fields. There is no isolation: a change to toast state
can interact with conflict-editor state because they share `self`.

Why this matters: Field 105, 106, 107 is how the struct got to 104. The state
machine is impossible to reason about; merge conflicts concentrate here; the
struct is the reason `cx.notify()` repaints everything (GPUI Finding 1).

Recommended fix: Group fields into owned sub-structs — `KagiApp { layout:
LayoutState, modals: ModalManager, conflict: ConflictModeState, update:
UpdateState, tabs: TabManager, toasts: ToastStack, … }`. The migration has
begun (`ToolbarState`, `TabViewState`, `CommitPanelState`) but ~50 fields
remain flat. Longer term, promote the sub-structs to child `Entity<T>`s (see
GPUI Finding 1).

Suggested verification: `grep -cE '^\s*pub ' src/ui/mod.rs` inside the struct
body should drop monotonically; `cargo test --workspace` green at each step.

---

### Finding: Two god-files (`mod.rs` 6406 LOC, `render.rs` 6100 LOC) hold ~18% of the codebase
Severity: Critical · Effort: Large · Confidence: High

Evidence:
- `src/ui/mod.rs` = 6406 LOC: struct, two `impl KagiApp` blocks (`:1354`,
  `:5884`), types, constants, 44 free functions, blocking git helpers,
  `run_app`, `open_main_window`.
- `src/ui/render.rs` = 6100 LOC: three impl/free blocks (`impl KagiApp` `:16`,
  `impl Render for KagiApp` `:395`, second `impl KagiApp` `:1251`) + 30+ free
  render functions. AGENTS.md:42 admits these are "known oversized god-files
  mid-split."

Problem: These two files alone are 12,506 of ~84k LOC. `impl KagiApp` is spread
across ~15 files (28 `impl KagiApp` blocks total in the tree). Finding where a
method lives requires grepping the whole tree.

Why this matters: Any merge conflict lands here. The "split" extracted ~7800
LOC into `operations/` but `mod.rs` did not shrink — it grew.

Recommended fix: Continue the `operations/` split into `state/`, `actions/`,
`reload/` submodules. Move `render.rs` free functions into per-pane modules
(`render/commit_panel.rs`, `render/file_history.rs`, `render/menu.rs`) — the
rendering is already self-segmenting by comment headers.

Suggested verification: `wc -l src/ui/mod.rs src/ui/render.rs` decreasing each
PR; tests green.

---

### Finding: Documented function/file size targets are violated by 29 files and many functions
Severity: Medium · Effort: Medium · Confidence: High

Evidence:
- AGENTS.md:40 sets ≤800 LOC/file. **29 files exceed it.** Top offenders:
  `ui/mod.rs` 6406, `ui/render.rs` 6100, `ui/modals.rs` 3776,
  `git/ops/pull_push.rs` 2077, `ui/commands.rs` 1748, `headless.rs` 1713,
  `git/conflicts.rs` 1677, `ui/theme.rs` 1562, `git/resolution.rs` 1522,
  `ui/sidebar.rs` 1501, `ui/operations/branch.rs` 1426.
- AGENTS.md:41 sets ≤80 LOC/function. `render()` is **848 LOC**
  (`render.rs:396-1243`), `render_commit_panel` **833** (`render.rs:5268`),
  `render_inspector` **819** (`inspector.rs:61`), `render_header_slot` **576**,
  `run_repo_flow` **1456** (`headless.rs:258`).

Problem: The 80-LOC target is off by 10× for render methods. There is no CI
gate enforcing it. Either the target is wrong (likely for GPUI render) or it
has never been enforced.

Why this matters: Documents a target the codebase has never met. New
contributors read AGENTS.md, believe sizes are controlled, and are misled.

Recommended fix: Either acknowledge render methods need a different budget
(≤300 LOC) with a view-model layer, or enforce the 800-LOC/file target with a
CI `wc -l` gate + shrinking allowlist. Mark ADR-0076 (view-models) and ADR-0077
(harness retirement) as "in progress" with completion %.

Suggested verification: Add the gate; watch the allowlist shrink release over
release.

---

### Finding: The "view-model layer" is vaporware — 213 LOC covering one component
Severity: High · Effort: Large · Confidence: High

Evidence:
- `src/ui/view_models/mod.rs` = 15 LOC (one submodule decl).
- `src/ui/view_models/status_bar.rs` = 198 LOC (only `StatusBarVM`).
- ADR-0076 P5 introduced it; the `mod.rs` header says "introduced
  incrementally; StatusBarVM is the first slice." No further slices arrived.

Problem: The VM layer is the documented escape hatch from (a) the render
god-functions and (b) the headless `KAGI_*` harness (ADR-0077 retirement
depends on it). With 213 LOC implemented, both debts remain fully load-bearing.

Why this matters: ADR-0076/0077 are presented as architectural progress but
are ~5% delivered. Calling it a "layer" is aspirational.

Recommended fix: Either delete the doc claim and accept GPUI render methods ARE
the view, or build VMs for `CommitRow`, `InspectorDetail`, `ModalCard`,
`BottomPanel`. Do not ship ADR-0077's harness retirement on the strength of one
status-bar VM.

Suggested verification: Each new VM ships with unit tests (no GPUI, no git2) —
that is the whole point.

---

### Finding: `Backend` is a 112-method 1:1 delegator, not an abstraction
Severity: High · Effort: Large · Confidence: High

Evidence:
- `src/git/backend.rs` (908 LOC), **112 `pub fn`**, almost all one-line:
  `backend.rs:548 plan_checkout → ops::plan_checkout(&self.repo, branch)`,
  `:556 execute_checkout → ops::execute_checkout(&self.repo, branch)`, etc.
- `backend.rs:53 pub fn repo(&self) -> &Repository { &self.repo }` — the
  leaky escape hatch. **Grep for `\.repo()` across `src/` and `tests/` = 0 call
  sites.** It is dead.

Problem: `Backend` adds no logic, no caching, no policy. Callers could invoke
`ops::*` directly with a `Repository` handle. Adding an op requires (a) a fn in
`ops/`, (b) a 1-line delegator in `backend.rs`, (c) a re-export in `mod.rs` —
three touch points for zero behavioral value. The `repo()` getter exists only
to let callers bypass the facade, and nobody does.

Why this matters: The "facade" is documentation theater. The `repo()` escape
hatch means the "no git2 in UI" invariant holds only because callers are
polite, not because the type system enforces it.

Recommended fix: Either give `Backend` real responsibility (transaction
management, snapshot caching, the enforced pipeline — see Git-safety Finding
2), or delete it and have UI call `ops::*` with an owned repo. At minimum
delete the dead `repo()` getter and make `repo` private.

Suggested verification: `grep -rn '\.repo()' src/ tests/` = 0 after removal;
build green.

---

### Finding: `operations/` split is a physical move, not a view-model layer — it's leaky
Severity: High · Effort: Medium · Confidence: High

Evidence:
- `src/ui/operations/mod.rs:1-5` header: "Pure physical split — behaviour and
  signatures are unchanged." Each file opens `impl KagiApp` and reaches into
  private fields (`self.modal_focus`, `self.active_view`,
  `self.set_create_branch_modal(...)`).
- `modal_state.rs` = **806 LOC of 115 generated accessor methods** (5 per
  variant × 23 modal variants), silenced by `#[allow(dead_code)]` at
  `modal_state.rs:22`. Of the 23 `take_*` methods, **all 23 are dead** (grep
  `\.take_` = 0 in `src/`). Of 23 `*_mut`, only 8 are used → **15 dead**.

Problem: The split moves code without changing coupling. The 115 accessors are
bookkeeping to preserve old call-site names after the `ActiveModal`
consolidation — "reverse-enterprise-pattern": ceremony to avoid touching call
sites.

Why this matters: AGENTS.md presents `operations/` as architectural progress;
it is a file-length win only. The 806-line accessor file is the tax.

Recommended fix: Delete the 23 dead `take_*` and 15 dead `*_mut` methods
(~230 LOC, pure win). Longer term, migrate call sites to match on
`self.active_modal` directly so the per-variant shims can go.

Suggested verification: `cargo test --workspace` green; `wc -l
src/ui/operations/modal_state.rs` drops ~30%.

---

### Finding: 15 `kagi-domain` "shims" are partly parallel implementations — drift risk
Severity: High · Effort: Medium · Confidence: High

Evidence (domain LOC → src/git LOC, type):
- `history.rs`: 388 → 546. **Two different concepts under one filename.**
  Domain has `OperationKind`/`HistoryEntry`/`OperationHistory`; `src/git/history.rs`
  defines `FileHistoryRequest`/`FileHistory`/`FileHistoryEntry`/`CommitSummary`.
- `message_gen.rs`: 892 → 976. Domain holds the rule-based logic; `src/git/`
  re-exports *and* adds 6 git2-backed fns (`collect_staged_files`,
  `collect_staged_diff`, `generate_message`, `ollama_available`, …).
- `resolution.rs`: 905 → 1522. Domain has the pure hunk model; `src/git/`
  re-exports 11 symbols *and* defines its own git2-backed `ResolutionBuffer`.
- `diff.rs`/`status.rs`/`diffstat.rs`/`checklist.rs` are clean splits
(`pub use kagi_domain::*` + git2 impl) — these are the exemplars.

Problem: Not "re-export shims" as the migration README claims. The boundary
between "pure domain fn" and "git2 backend fn" is unclear in `message_gen.rs`
and `resolution.rs`; `history.rs` is an outright filename collision. A hunk
parsing bug may need fixing in two places.

Why this matters: Double-maintenance surface. Reading the code, you must
constantly check which file owns which function.

Recommended fix: Rename `src/git/history.rs` → `src/git/file_history.rs`.
Finish the extraction so `src/git/{message_gen,resolution}.rs` contain *only*
the git2 glue (≤300 LOC each) and delegate all parsing/logic to `kagi-domain`.

Suggested verification: Each `src/git/<x>.rs` should be visibly smaller than
its domain twin and contain no pure logic.

---

### Finding: `headless.rs` (1713 LOC, 48 env-var hooks, 26 raw git2 opens) bypasses every layer
Severity: High · Effort: Large · Confidence: High

Evidence:
- `src/headless.rs:258` `fn run_repo_flow` = **1456 LOC** (longest fn in the
  codebase).
- **48 `env::var("KAGI_*")`** reads — a scriptable harness driving the GPUI app
  from outside the process.
- **26 direct `git2::Repository::open`** calls (`:55, 586, 597, 626, 655, 729,
  737, 751, 791, 847, 868, 903, 994, 1016, 1050, 1137, 1169, 1198, 1332, 1366,
  1391, 1519, 1539, 1558, 1576, 1582`), bypassing `Backend`.
- The file header (lines 1–12) admits it is "behaviour-frozen" and ADR-0077
  "plans to retire/relocate once the layered test pyramid makes the VM and
  controller directly testable."

Problem: The harness directly opens git2, constructs `KagiApp`, mutates its
fields, and calls plan/execute methods — coupling tests to private internals.
The `[kagi] …` stderr lines it greps are a test contract that freezes log
wording (AGENTS.md). It is load-bearing *and* dead weight simultaneously.

Why this matters: Every fix to a plan/execute function risks breaking the
brittle greps. The retirement plan depends on the unbuilt VM layer.

Recommended fix: Route through `Backend` (not raw git2) even inside the harness
— removes 26 git2 sites. Decompose `run_repo_flow` per-operation. Audit the 48
hooks: ~30 duplicate `tests/` integration coverage and can be deleted; ~10–15
genuinely exercise UI state and should stay until VMs exist.

Suggested verification: `grep -c 'git2::Repository::open' src/headless.rs`
drops to 0; `cargo test --workspace` green.

---

### Finding: `Rc<Cell<(f32,f32)>>` geometry backchannels in render paths
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/ui/mod.rs:943 inspector_geom`, `:948 file_history_geom`,
`:1082 conflict_geom`, `:1085 conflict_ab_geom` — each a
`Rc<Cell<(f32,f32)>>`. Written from inside `canvas(…)` paint closures
(`src/ui/inspector.rs:847`, `src/ui/render.rs:3933`,
`src/ui/conflict_editor.rs:264,353`), read from drag-move listeners
(`src/ui/render.rs:697-744`).

Problem: Paint-time geometry is smuggled into a sibling event handler via
interior mutability because there is no clean GPUI channel. A drag starting
before the first paint of a re-laid-out element reads stale coordinates — the
exact "jumps on drag start" bug the comments at `inspector.rs:837-839` describe
having already hit once. `&self` renders mutate entity state.

Why this matters: Each new resizable split adds another `Rc<Cell>` field. The
pattern doesn't compose and is invisible in the render function.

Recommended fix: Store last-known `Bounds<Pixels>` in a normal
`Option<Bounds<Pixels>>` field, updated from the canvas *prepaint* closure
(runs during layout, before paint). Or query live bounds from the drag handler.

Suggested verification: Resize/zoom a split; the divider must not jump on
first drag.

---

### Finding: ~15 pure helpers trapped inside the `ui/mod.rs` god-file
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/mod.rs` defines ~44 free functions. Pure ones (no `&self`/cx):
`:686 format_hms`, `:6162 short_hash`, `:6174 conflict_content_sig`,
`:6191 conflict_split_ratio_from_cursor`, `:6207 hunk_choice_slug`,
`:6140 platform_menu_label`, `:266 busy_label`, `:260 row_height`,
`:1215 build_tab_view`, `:93 draggable_branch_name`, `:101 context_branch_name`,
`:109 collect_history_commits`, `:138 validate_merge_from_drag`,
`:727 localize_plan_blockers`.

Problem: These are unit-testable pure functions living interleaved with
stateful code in a 6406-LOC file. The instinct (extract for testability) is
right; the destination is wrong — they belong in `kagi-domain` or a `ui/pure/`
module.

Why this matters: Navigation cost; the pure logic is harder to find and test
in isolation than it should be.

Recommended fix: Move to `src/ui/util.rs` (or `kagi-domain` where no GPUI
types are touched). Leaves `mod.rs` for `KagiApp` impl + bootstrap.

Suggested verification: New locations have unit tests; `mod.rs` shrinks.

---

### Finding: `tab_cache` is an unbounded `HashMap` — memory leak across repos
Severity: Low · Effort: Small · Confidence: High

Evidence: `src/ui/mod.rs:1025 pub tab_cache: HashMap<PathBuf, TabViewState>`.
Comment at `:1024`: eviction only in `close_tab`. `TabViewState` holds
precomputed commit rows, details, diffstat caches, avatars.

Problem: Every repo ever opened accumulates a `TabViewState` for the session.
No LRU, no memory-pressure eviction.

Why this matters: Long sessions opening many repos balloon memory.

Recommended fix: Bound to N most-recent entries (LRU), or evict on pressure.

Suggested verification: Open 20 repos; RSS stays bounded.

---

### Finding: 9 ad-hoc concurrency-state fields instead of a unified task manager
Severity: Low · Effort: Medium · Confidence: High

Evidence: `src/ui/mod.rs:996 busy_op`, `:976 fetch_in_flight`,
`:973 toast_ticker_alive`, `:979 auto_fetch_ticker_alive`,
`:982 refresh_spin_started`, `:989 modal_replan_gen`, `:987 draft_save_gen`,
`:1029 switch_generation`, `:917 watcher_generation`.

Problem: Each is a hand-rolled "spawn task carrying a generation; on
completion compare and discard if stale" guard. Reinvented per feature.

Why this matters: Subtle races (a reload firing while a modal replan is in
flight). The pattern is correct but verbose and duplicative.

Recommended fix: A `BackgroundTasks` helper owning generation-checked spawning
and liveness: `spawn(name, F)`, `is_busy(name)`, `cancel(name)`.

---

## 3. Performance problems

### Finding: `reload_external` runs the full git2 snapshot on the UI thread
Severity: Critical · Effort: Medium · Confidence: High

Evidence:
- `src/ui/mod.rs:1788` `reload_external` → `:1798 self.reload()` →
  `:1620-1627` `Backend::open` + `repo.snapshot(10_000)` **inline on the UI
  thread**.
- Contrast `refresh_working_tree_external` (`:1826-1836`) which correctly uses
  `cx.background_spawn`.

Problem: A `WatchEvent::Git` (HEAD/refs change) drives a full re-snapshot —
topological commit walk, full `git2::statuses` scan, ahead/behind for every
branch — synchronously inside the UI update closure.

Why this matters: Every external `git pull`/`fetch`/commit/checkout from a
terminal or sibling worktree stalls the UI frame. On large repos, hundreds of
ms of freeze per event.

Recommended fix: Mirror `refresh_working_tree_external`: `cx.background_spawn`
the `snapshot()`, apply on the UI thread via `apply_tab_view`.

Suggested verification: Run `git commit` in a terminal while the app is open on
a 10k-commit repo; the UI must not freeze.

---

### Finding: `git2::statuses` scans the entire worktree with no pathspec
Severity: Critical · Effort: Medium · Confidence: High

Evidence: `src/git/status.rs:51-60` — `StatusOptions::new()
.include_ignored(false).include_untracked(true)
.recurse_untracked_dirs(true).renames_head_to_index(true)` then
`repo.statuses(...)`. Called from `snapshot()` (`src/git/snapshot.rs:77`) on
every reload.

Problem: On a repo with a large untracked dir (`node_modules` not yet
gitignored, a `target/`, a sibling worktree under `.claude/worktrees/`), this
walks every file. The `is_nested_git_dir` skip (`status.rs:152,166-170`) only
helps for dirs containing `.git`.

Why this matters: Dominant reload cost. A 100k-file untracked tree turns every
external git event into a multi-second freeze (compounded by the on-UI-thread
reload above).

Recommended fix: Run status off-thread (previous finding). Consider scoping the
pathspec when only a subdirectory changed (the watcher knows which paths).

---

### Finding: No per-file diff content cache — clicking back recomputes
Severity: High · Effort: Small · Confidence: High

Evidence: `src/ui/mod.rs:770 diff_cache: HashMap<usize, Option<Vec<FileStatus>>>`
keyed by row index — holds only the file *list*. `set_commit_main_diff` /
`open_main_diff` (`src/ui/mod.rs:3523-3533`) calls
`repo.commit_file_diff(&id, &path)` synchronously every time.

Problem: Click commit A → file F (diff computed). Click B. Click back to A →
file F: `commit_file_diff` runs again — full tree-diff, hunk extraction.

Why this matters: Navigating between two commits to compare the same file pays
the full git2 diff cost on every toggle. On large files, 100s of ms each.

Recommended fix: Add `file_diff_cache: HashMap<(usize, usize), FileDiffView>`;
populate on first open, return on repeat. Invalidate with `diff_cache`.

Suggested verification: Profile a click-A/click-B/click-A cycle; second A
visit is O(1).

---

### Finding: Tree-sitter highlighting runs synchronously on the UI thread
Severity: High · Effort: Small · Confidence: High

Evidence: `src/ui/mod.rs:3291 let _ = highlight_diff_rows(&mut rows, &path);`
inline. `src/ui/diff_view.rs:194-285` — builds a `SyntaxHighlighter`, feeds a
Rope, calls `highlighter.update(None, &rope)` and
`highlighter.styles(&(0..combined.len()), &hl_theme)` — a full tree-sitter
parse + style scan of the entire diffed file content.

Problem: For a 5k-line file diff this is a synchronous multi-ms parse on the UI
thread, every time the diff is opened (compounds the no-cache finding above).

Why this matters: Opening a large-file diff visibly stalls the app; clicking
back stalls again.

Recommended fix: `cx.background_spawn` `highlight_diff_rows` (it only fills
`highlights` — easily made `Send`); render un-highlighted first, swap in when
done.

Suggested verification: Open a 5k-line diff; first paint shows text
immediately, highlights arrive a moment later.

---

### Finding: Graph layout recomputed in full on every reload; not cached, not incremental
Severity: High · Effort: Medium · Confidence: High

Evidence: `src/ui/commit_list.rs:189 let graph = layout(&snap.commits);`
inside `build_commit_rows`, called from `build_tab_view` on every `reload()`
(`src/ui/mod.rs:1645`). `layout` is O(commits × active_lanes) with three
linear scans per commit (`crates/kagi-domain/src/graph.rs:137,176,286,296`).

Problem: For a 10k-commit repo the layout pass is ~50k comparisons, recomputed
wholesale after every external change and every op. No incremental path; even
one new commit re-layouts the whole window.

Why this matters: Combined with the on-UI-thread reload, any HEAD movement in a
sibling worktree triggers full snapshot + full layout on the UI thread.

Recommended fix: Cache layout keyed by `(head_oid, commit_count)` on
`TabViewState`; recompute only when the commit set changes. Memoize by storing
`GraphLayout` next to the snapshot; invalidate on commit-graph movement, not
status-only refresh.

---

### Finding: Graph canvas re-strokes every edge per visible row per frame
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/ui/graph_view.rs:228-316` — each row gets its own `canvas(...)`
whose `paint` closure rebuilds a `PathBuilder` for every edge (`:280,334,354`)
and calls `window.paint_path` each. `lane_color(edge.from_lane)` is re-resolved
per edge (`:274`).

Problem: ~50 edges in a dense row = 50 path builds + paints per visible row per
frame. uniform_list mitigates by rendering only visible rows, but a full
repaint (modal open/close, toast) re-strokes all visible edges.

Why this matters: On wide-branch repos (the comment at `commit_list.rs:255`
cites a 24-lane repo) this is the dominant per-frame cost.

Recommended fix: Pre-bake per-row path geometry at snapshot time into an
`Rc<[PathData]>` on `CommitRow`; the canvas `paint` closure just strokes
precomputed paths. Lane color resolved at build time.

---

### Finding: Render closure clones `MainDiffView`/`compare_view`/`conflict` every frame
Severity: High · Effort: Small · Confidence: High

Evidence: `src/ui/render.rs:567 main_diff = self.main_diff.clone()`,
`:568 compare_view = self.compare_view.clone()`, `:627 conflict =
self.conflict.clone()`, `:667 toolbar_state =
self.active_view.toolbar_state.clone()`, `:675 status_summary =
self.active_view.status_summary.clone()`. `MainDiffView` carries a full
`Vec<DiffRow>` of highlighted rows (`src/ui/diff_view.rs:29-91`).

Problem: Cloning is O(diff-size) allocation, in `render` which gpui invokes on
every notify/resize/scroll.

Why this matters: With a 2k-line diff open, every scroll/repaint re-clones a
multi-thousand-element Vec with styled-text spans. Visible as CPU spikes.

Recommended fix: Store as `Arc<MainDiffView>` and clone only the `Arc`, or
render directly off `&self` inside the closure.

---

### Finding: `uniform_list` clones the whole `CommitRow` per visible row per frame
Severity: High · Effort: Small · Confidence: High

Evidence: `src/ui/render.rs:3176-3179` `range.filter_map(...).map(|(ix, row)| {
let row = row.clone(); …`. `CommitRow` (`src/ui/commit_list.rs:144-174`) holds
`author_email: String`, `badges: Vec<RefBadge>`, `edges: Vec<GraphEdge>`,
`parents: Vec<CommitId>` — all cloned per visible row per frame. Also
`:3321 row.edges.clone()`, `:3327 stash_lanes.to_vec()`.

Problem: ~40 visible rows × 60fps × (clone Vec<GraphEdge> + 2 string scans +
4 theme lookups) = measurable constant CPU even idle. The comment at
`commit_list.rs:178` ("only clones SharedStrings") is now false.

Why this matters: Scrolling a dense graph on a wide repo re-clones edge vectors
for every visible row, 60×/s.

Recommended fix: Drop `let row = row.clone();` — handlers only use `ix`. If the
builder body needs `row`, borrow `&rows[ix]`. Change `graph_canvas` to borrow
(`edges: &[GraphEdge]`).

---

### Finding: `avatar_color`/`avatar_initial` recomputed per visible row per frame
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/render.rs:3218-3219` calls `avatar::avatar_color` +
`avatar_initial` per visible row — string scan of author email/name per row per
frame. The comment at `commit_list.rs:140-142` claims pre-computation but
avatar color is NOT pre-computed.

Why this matters: Constant string-scan overhead in the hottest render path.

Recommended fix: Cache `avatar_color`/`avatar_initial` on `CommitRow` at
snapshot time.

---

### Finding: `graph_ahead_behind` computed for every local branch, not just visible ones
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/git/snapshot.rs:126-151` — inside `collect_branches`, for each
local branch with an upstream, `repo.graph_ahead_behind(target_oid, up_oid)`
runs. No filter on current branch.

Problem: `graph_ahead_behind` is an O(commits) graph walk each. A repo with 50
local branches pays 50 walks per snapshot. Worktree WIP refresh opens each
linked worktree and runs `working_tree_status` again (`snapshot.rs:373-381`).

Why this matters: Multi-second snapshots on branch-heavy repos. The sidebar
only shows ahead/behind for visible branches, but all are computed.

Recommended fix: Compute lazily (only visible branches), or cache and
invalidate on fetch.

---

### Finding: Auto-fetch ticker runs per-tab from `render()`; not deduped across tabs
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/commands.rs:43 AUTO_FETCH_INTERVAL_SECS = 180`;
`:1474-1490 ensure_auto_fetch_ticker` spawns a `cx.spawn` loop. Called
unconditionally per render at `src/ui/render.rs:438`.

Problem: N tabs = N independent 180s tickers each doing a fetch. Spawned
lazily from `render` (a frame side effect mutates long-lived task state). The
`auto_fetch_ticker_alive` guard could double-arm on a race.

Why this matters: 5 tabs + auto_fetch on = 5 parallel fetches every 3 min,
possibly to the same remote. Wasteful on metered/SSH remotes.

Recommended fix: One global ticker per remote-URL, armed on app init not from
render.

---

### Finding: Watcher loop polls every 100ms; should block on recv
Severity: Low · Effort: Small · Confidence: High

Evidence: `src/ui/tabs.rs:556-557 loop { Timer::after(Duration::from_millis(100)).await; … match rx.try_recv() { Err(_) => continue } }`. Same shape at `:646`
(single-instance, 200ms).

Problem: Polls an mpsc receiver 5–10×/s for the app lifetime instead of
blocking on `recv`.

Why this matters: Constant background wake-ups; multiplied by open tabs.

Recommended fix: Block on `rx.recv()` (async) so the task wakes only on a real
event; debounce after.

---

## 4. Git operation safety problems

> **Headline:** The project's safety thesis ("`plan → confirm → preflight →
> execute → verify → oplog` for every write; no destructive ops") is
> **half-true**. The "no destructive ops" half **holds** (verified: zero
> `reset --hard`, `push --force`, `git clean`, `--force-with-lease`, or `unsafe`
> in `src/` — all hits are doc/comment/recovery text). The pipeline half
> **does not hold at the backend**: `Backend::execute(op)` skips preflight,
> verify, and oplog for the majority of mutating ops. Safety is a UI-layer
> convention, not a guarantee.

### Finding: `Backend::execute(op)` runs mutating ops with no preflight, verify, or oplog
Severity: Critical · Effort: Medium · Confidence: High

Evidence: `src/git/backend.rs:304-427`. For `Checkout`, `CheckoutCommit`,
`CherryPick`, `MergeBranch`, `MergeIntoConflict`, `Revert`, `Pull`, `Push`,
`UndoCommit`, `Amend`, `StashApply`, `StashPop`, `StashPush` it calls
`self.execute_*(...)` directly. No `preflight_check`, no `append_oplog`, no
verify. Only `DeleteBranch` and `Discard` re-plan inline.

Problem: This is the single entry point any non-UI code uses. The oplog — the
advertised recovery mechanism — will not contain entries for checkout,
cherry-pick, merge, revert, pull, push, undo, amend, or stash done via this
path.

Why this matters: After a crash the user cannot reconstruct what happened. The
safety thesis is violated for the majority of writes when invoked through
`execute()`.

Recommended fix: `execute()` should require a pre-confirmed plan, call
`preflight_check`, dispatch, then `append_oplog` in one function. Drop the
per-op re-plan in Discard/DeleteBranch; require the caller to pass the plan.

Suggested verification: New test: `Backend::execute(&Operation::Checkout{…})`
on a repo that changed since plan → returns a preflight error, not a mutation.

---

### Finding: Headless `KAGI_*` env-var hooks execute directly, bypassing preflight
Severity: High · Effort: Medium · Confidence: High

Evidence: `src/ui/operations/cherry_revert.rs:60,199` comments: "The headless
KAGI_* path executes `execute_cherry_pick`/`execute_revert` directly."
`src/headless.rs:103-188` shows discard does plan+execute, but other `KAGI_*`
ops (`KAGI_CHERRY_PICK=<sha>`, `KAGI_REVERT`, `KAGI_CHECKOUT_COMMIT`, etc.,
48 total hooks) call execute functions directly.

Problem: `KAGI_AUTO_CONFIRM=1` + `KAGI_CHERRY_PICK=<sha>` skips plan/preflight.
Plan-time blockers (dirty-tree, conflict checks at `cherry_revert.rs:84-104`)
are never evaluated.

Why this matters: CI/automation using these hooks can cherry-pick/revert with
the repo in any state.

Recommended fix: Every `KAGI_*` headless handler must `plan_*` first, refuse if
`!plan.blockers.is_empty()`, then `preflight_check`, then execute — exactly as
`KAGI_DISCARD` already does.

---

### Finding: Merge proceeds into conflict with a dirty working tree (warnings only)
Severity: High · Effort: Small · Confidence: High

Evidence: `src/git/ops/merge.rs:26` calls `merge_dirty_warnings(&status,
"merging")` which only warns. `execute_merge_into_conflict`
(`merge.rs:359-420`) runs `repo.merge(...)` with `checkout_opts.safe()` on a
possibly-dirty tree. The cherry-pick rule (`cherry_revert.rs:84`) *blocks* on
dirty tree.

Problem: `repo.merge` writing conflict markers into a tree with uncommitted
changes interleaves the user's local edits with conflict markers. The
`ORIG_HEAD` rollback (`merge.rs:393-400`) restores the *ref*, not the working
tree.

Why this matters: "Resolve conflicts before merging" is the cherry-pick rule,
but merge — more dangerous because it writes markers to disk — only warns. A
user merging with uncommitted edits can end up with markers inside unsaved
work, and `git merge --abort` is the only recovery (not surfaced).

**Failure scenario:** User has unstaged edits to `auth.rs`. Clicks Merge on a
feature branch whose changes also touch `auth.rs`. Plan shows a yellow warning.
On confirm, conflict markers are written *into the user's unsaved `auth.rs`
edits*. `git merge --abort` would discard both the merge AND the user's
pre-merge edits.

Recommended fix: Mirror cherry-pick: block merge when `!status.staged.is_empty()
|| !status.unstaged.is_empty()`. At minimum block
`execute_merge_into_conflict` (the real-conflict path) on a dirty tree.

---

### Finding: `stage_conflict_resolution` is not atomic — partial write leaves WT ≠ index
Severity: High · Effort: Small · Confidence: High

Evidence: `src/git/conflicts.rs:931-984`. The loop (`:953-979`) does
`fs::write` then `index.add_path` per file, then `index.write()` once at
`:980-982`. If `fs::write` for file 3 of 5 fails, files 1–2 are already
overwritten on disk but the index is never written — index shows conflicts
while the WT has partial resolutions.

Problem: No transactional boundary. A disk error mid-loop produces a WT that
matches neither the index nor the original conflict state. The user's manual
edits to files 1–2 are replaced by the buffer's resolution; file 3 is
corrupt/missing.

Why this matters: Conflict resolution is the exact place "never lose user
work" matters most, and a partial write loses both the original markers and
the intended resolution.

Recommended fix: Write all files to temp paths first, then rename atomically,
then stage, then `index.write()` once; roll back temp files on any failure.
(The deferred-`index.write()` pattern is already proven in
`execute_conflict_continue`.)

---

### Finding: `execute_checkout` performs no preflight and no verify
Severity: High · Effort: Small · Confidence: High

Evidence: `src/git/ops/checkout.rs:216-243` `execute_checkout(repo, branch)` —
no plan arg. `src/git/backend.rs:556-558` same. `backend.rs:309-311` dispatches
`Operation::Checkout` straight to it.

Problem: The `head_at_plan` snapshot is never compared to reality inside
execute. A checkout can run against a repo that changed (committed, switched,
entered conflict) between confirm and execution.

Why this matters: A background queue, re-trigger, or double-confirm can check
out a different branch over a tree that has since become dirty/conflicted.

Recommended fix: `execute_checkout(repo, &OperationPlan, branch)`; first line
`preflight_check(repo, plan)?` (mirror `execute_discard`/`execute_delete_branch`).

---

### Finding: Checkout does not block untracked-file overwrite at plan time
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/git/ops/checkout.rs:127-132` — untracked files only produce a
*warning* ("will remain after switching branches"). `predict_checkout_conflict`
(`:362-415`) excludes untracked files from overlap analysis. `execute_checkout`
uses `cb.safe()` (`:232`) which aborts on untracked-overwrite at execute time.

Problem: The plan tells the user untracked files "will remain," implying
safety. If the target branch contains a tracked file at the same path as an
untracked file, git safe-checkout aborts at execute with a libgit2 error —
contradicting the green Execute button.

**Failure scenario:** User has untracked `notes.txt`. Target branch tracks
`notes.txt`. Plan says "1 untracked file(s) will remain." Click Execute →
`checkout_tree failed` with no guidance.

Recommended fix: In `predict_checkout_conflict`, intersect `status.untracked`
paths against the HEAD→target diff's *new* files; emit a blocker naming the
conflicting path.

---

### Finding: `predict_checkout_conflict` fails open — swallows analysis errors
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/git/ops/checkout.rs:362-415` — every step uses `.ok()?`:
`find_commit(...).ok()?`, `tree().ok()?`, `diff_tree_to_tree(...).ok()?`. The
comment (`:359-361`) says "On any analysis failure we return `None` (fall back
to the existing warning) — never invent a blocker."

Problem: Fail-*open*, not fail-closed. If diff analysis fails, the blocker is
suppressed and the user gets a soft warning instead of a hard block.

Why this matters: The exact condition that would prevent data loss is skipped
on analysis edge cases.

Recommended fix: Return `Err`, promote to a blocker ("could not verify checkout
safety — stash or commit first").

---

### Finding: `execute_stash_pop` has no plan, preflight, or dirty-tree re-check
Severity: High · Effort: Small · Confidence: High

Evidence: `src/git/ops/stash.rs:534-543` `execute_stash_pop(repo, index)`. No
plan, no preflight. `Backend::execute` (`backend.rs:354-358`) calls it bare.
Conflict prediction exists only in `plan_stash_pop`. `execute_stash_pop` does
not re-verify HEAD or stash-count.

Problem: If a caller invokes `execute_stash_pop` without the plan, predicted
conflict blockers never run. Worse, a concurrent stash push between plan and
execute shifts indices and pops the *wrong* entry.

**Failure scenario:** User plans pop of `stash@{1}`. Another tool pushes a
stash. User confirms. Execute pops what is now `stash@{1}` = a different entry,
applies and drops it.

Recommended fix: `execute_stash_pop(repo, &OperationPlan, index)`; first line
`preflight_check_stash(repo, plan, plan.stash_count_at_plan())?`.

---

### Finding: Stash apply has no conflict prediction in its plan
Severity: Medium · Effort: Small · Confidence: High

Evidence: `Backend::execute_stash_apply` (`backend.rs:639-641`) takes only an
index. `execute_stash_apply` calls `repo.stash_apply(index, None)` with no
force flag. Conflict prediction (`predict_stash_pop_conflict`) exists only for
pop.

Problem: Apply onto a dirty tree that overlaps the stash produces conflicts at
execute that the plan never warned about.

Recommended fix: Run the same `merge_commits`-based prediction as a *warning*
(apply is non-destructive).

---

### Finding: `execute_stash_drop` is destructive with no plan/preflight requirement
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/git/ops/stash.rs:693-704` `execute_stash_drop(repo, index)` —
public, no plan, `repo.stash_drop`. `plan_stash_drop` sets `destructive: true`
(`stash.rs:684`), but execute does not require the plan.

Problem: ADR-0087 gates drop behind a danger-confirmation modal — but that
gating is UI-only. The backend drops any valid index with no check, no oplog.
Any non-UI caller loses a stash irrecoverably (modulo stash reflog).

Recommended fix: `execute_stash_drop(repo, &OperationPlan, index)` with
`preflight_check_stash` enforcing `stash_count_at_plan`, plan marked
`destructive`.

---

### Finding: Discard of untracked files permanently deletes from disk; verify can fire after deletion
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/git/ops/discard.rs:240-267` deletes untracked files via
`std::fs::remove_file` and prunes empty parent dirs. Backup at `:191-219` via
`repo.blob(&content)`. Verify (`:269-302`) re-reads status and returns `Err` if
any target remains.

Problem: (a) Empty-dir pruning (`:259-267`) walks up with `remove_dir` and
could remove a parent that appears empty but is a gitignored cache — no
`.gitignore` consultation. (b) For untracked targets, deletion happens *before*
the verify `Err` is returned — so a "failed" outcome has already mutated the
WT. The oplog/verify contract is inconsistent: user sees "failed," assumes
nothing changed, unaware files are gone.

Recommended fix: Consult `.gitignore` before pruning parents. Return a distinct
`DiscardOutcome::Partial { deleted, remaining, backups }` variant instead of
`Err` when some targets succeeded. Make oplog-append part of `execute_discard`
itself, not a caller responsibility.

---

### Finding: `execute_cherry_pick`/`execute_revert` skip preflight; dangling-commit risk
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/git/ops/cherry_revert.rs:423-505` (`execute_cherry_pick`),
`:817-928` (`execute_revert`). No plan, no preflight, no dirty-tree re-read.
Both do `repo.commit(None, …)` *then* `checkout_tree(...safe())` *then* move
the ref last.

Problem: If a file was modified between plan and execute, the safe checkout
aborts after the commit object is already created → dangling commit in the ODB,
working tree not synced, HEAD not moved. User sees `checkout_head failed` with
no explanation. If they retry after cleaning, they may cherry-pick twice.

Recommended fix: `execute_cherry_pick(repo, &OperationPlan, id)` with
`preflight_check` and a dirty-tree re-read at the top, before creating any
object.

---

### Finding: `execute_undo_commit` / `execute_amend` trust plan-time "not pushed" — no execute-time re-check
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/git/ops/history.rs:269-336` (`execute_undo_commit`), `:594+`
(`execute_amend`). Plan-time pushed checks at `:146-171` and `:448-471`. Execute
only guards root/merge. `Backend::execute_amend` (`backend.rs:881-886`) takes
no plan. Doc at `:591-593` admits oplog is a caller contract.

Problem: ADR-0011 ("never undo a pushed commit") is enforced only at plan time.
A `git push` between plan and execute makes HEAD pushed; execute still moves
the ref, rewriting published history. Same for amend (ADR-0023 two-stage
confirm is plan-time only).

Recommended fix: Re-run the pushed check inside execute (or take a plan +
preflight capturing the upstream tip); write the oplog entry inside execute.

---

### Finding: `run_git` (fetch/push) on timeout leaves the child running
Severity: Low · Effort: Small · Confidence: High

Evidence: `src/git/cli.rs:77-92`. Background thread calls
`child.wait_with_output()`; main thread `recv_timeout(60s)`. On timeout the
receiver errors and the fn returns, but the thread + child are never killed.

Problem: A timed-out push/fetch keeps running, possibly completing remotely
after the UI shows "timed out."

Why this matters: A "timed out" fetch that completes later updates
remote-tracking refs out from under the user; a timed-out push may succeed
while the user retries.

Recommended fix: Store the child PID before moving into the thread; `kill` on
timeout.

---

### Finding: Push forbids force (good) but no local divergence pre-check
Severity: Low · Effort: Small · Confidence: High

Evidence: `src/git/ops/pull_push.rs:1410-1504` `execute_push`,
`:1926-1973` `execute_push_branch`. Both build `["push", remote, branch]` with
no `--force`. Plan warns "Non-fast-forward pushes will fail" (`:1221`).

Problem: No ahead/behind check at execute; relies on the remote to reject,
surfacing raw stderr (`:1492-1498`).

Why this matters: Poor UX — user gets `push failed (exit 1): ! [rejected] …
non-fast-forward` instead of a plan-time blocker.

Recommended fix: In `plan_push`/`plan_push_branch`, when `behind > 0`, add a
blocker ("branch is behind upstream; pull first").

---

### Finding: `execute_merge_into_conflict`'s abort scope is unclear for WT changes
Severity: Medium · Effort: Small · Confidence: Medium

Evidence: `src/git/ops/merge.rs:393-400` writes `ORIG_HEAD` = pre-merge HEAD.
Doc (`:355-358`) says abort "can roll back" via ORIG_HEAD. But ORIG_HEAD is a
*ref*; `git merge` already wrote conflict markers + stage entries into WT/index.

Problem: Restoring a ref does not restore WT files. The actual abort must use
`git merge --abort` (reset to ORIG_HEAD + clean merge state). Whether
`execute_conflict_abort` does this fully was not verified; the comment
overstates what ORIG_HEAD alone guarantees.

Recommended fix: Verify `execute_conflict_abort` does a full WT/index reset to
ORIG_HEAD, not just a ref move; clarify the comment.

---

## 5. Git UX problems

### Finding: No two-stage confirmation on Discard — single click destroys work
Severity: Critical · Effort: Small · Confidence: High

Evidence: `src/ui/modals.rs:1330-1546` `render_discard_modal`;
`src/ui/operations/discard.rs:131` `start_discard`. Modal has red border +
red button, but **single-click executes**. Only amend gets the
`confirm_armed` two-stage gate (`modals.rs:103,732-784`).

Problem: Discard is the most destructive WT op (`git checkout -- <path>` +
deletes untracked files). A misclick on "Discard all" loses every unstaged
change. The oplog blob backup exists but recovery requires `git cat-file -p
<sha>` in a terminal — undiscoverable.

Why this matters: Directly undercuts the "safety-first" thesis. GitKraken
requires typing the count; Fork shows a red final-confirm.

Recommended fix: Apply amend's `confirm_armed` pattern: first click → button
relabels to "Permanently discard N files"; second click executes.

---

### Finding: Raw libgit2/CLI stderr shown as the entire error message
Severity: Critical · Effort: Medium · Confidence: High

Evidence: `src/git/mod.rs:152-160` — `GitError` has 4 typed variants; everything
else is `GitError::Other(String)`. Surfaced verbatim at
`src/ui/commands.rs:1459` (`Fetch failed: {e}`),
`src/ui/operations/conflict.rs:430`, and ~35
`FooterStatus::Failed(format!("...: {}", e))` sites (`branch.rs:323,763,932`;
`pull_push.rs:63,328`; `stash.rs:363,425`; `discard.rs:54,78`).

Problem: Users see `index.add_path failed: 'foo/bar.txt': index entry contains
empty file path` or `fetch failed (exit 1): fatal: could not read Username for
'https://...': terminal prompts disabled` with no remediation.

Why this matters: Safety-first means errors must be actionable. GitHub Desktop
and Fork map known errors to friendly text + a "learn more" link.

Recommended fix: An error-classification layer (auth/network/conflict/dirty-tree/
not-a-repo/unknown) mapping `git2` codes + CLI stderr patterns to typed messages
with a suggested next action. Keep the raw string in an expandable "details".

---

### Finding: Op/push/discard failures surface only in the footer status line, not a toast
Severity: Critical · Effort: Small · Confidence: High

Evidence: Of ~35 failure sites, only `fetch_async` (`commands.rs:1456-1459`)
and the conflict flow (`operations/conflict.rs:430,476,478`) call
`push_toast(ToastKind::Error, …)`. All branch/pull/push/stash/discard/checkout
failures set `FooterStatus::Failed` only — a single muted-red line at the
bottom (`render.rs:2988`), immediately overwritten by the next `reload()`
success footer.

Problem: When checkout/push fails, the modal closes and the only feedback is
small red text that is overwritten. A user who looks away believes it
succeeded.

Why this matters: Silent push failures (force-with-lease rejected,
non-fast-forward) are how teams lose work or ship the wrong thing.

Recommended fix: `ToastKind::Error` as the default failure channel for every
async op, with longer dwell time. Errors persist until dismissed (excluded from
the auto-dismiss ticker at `mod.rs:2565-2589` and the `TOASTS_MAX` cap).

---

### Finding: No per-hunk or per-line staging — whole files only
Severity: High · Effort: Large · Confidence: High

Evidence: `src/git/staging.rs:74,127,756,798` — the only staging API is
`stage_file`/`unstage_file`/`stage_files`/`unstage_files`, all path-based. Grep
for `stage_hunk|stage_line|patch_stage` = 0. The commit panel
(`commit_panel.rs:55-89`) stores file lists only.

Problem: Cannot stage a subset of a file's changes. A user who fixed two
unrelated bugs in one file must commit both or split with an external editor.
GitKraken, Fork, SourceTree, Zed, GitHub Desktop all support hunk/line staging.

Why this matters: Table-stakes for a modern Git GUI; the single biggest
daily-workflow gap. Pushes users out of the app for routine commits.

Recommended fix: Implement `git apply --cached` on a generated patch for
selected hunks/lines (diff hunks are already parsed at `staging.rs:690-735`).
Add per-hunk "+" buttons in the unstaged diff view.

---

### Finding: No per-hunk Discard — whole-file only
Severity: High · Effort: Medium · Confidence: High

Evidence: `src/git/ops/discard.rs:39` `plan_discard(paths: &[String])`;
`src/ui/operations/discard.rs:15` partitions by whole paths.

Problem: Cannot discard a single change inside a file. Combined with
single-click full-file discard, partial rollback is unsafe and coarse.

Recommended fix: Per-hunk discard via generated reverse patch. GitKraken and
Fork both offer it.

---

### Finding: Rebase is in the menu but entirely unimplemented
Severity: High · Effort: Large · Confidence: High

Evidence: `src/ui/branch_menu.rs:197` `RebaseCurrentOnto`; `rebase_state`
(`:477-487`) unconditionally returns `disabled("not implemented yet")`;
`dispatch_branch_action` (`mod.rs:4503-4514`) routes to `BcmNotImplementedYet`.

Problem: The menu item reads "Rebase current onto X" — a commonly-needed
action — permanently greyed out. Rebase is a top-3 reason users pick a GUI.

Why this matters: Flagship-feature gap vs every competitor. For a safety-first
client, interactive rebase (reorder/squash/drop with preview) is where guided
safety adds the most value.

Recommended fix: Remove the item until ready, OR implement `git rebase` with
the existing 3-pane conflict editor (which already handles `ConflictOp::Rebase`,
`conflict_view.rs:191`).

---

### Finding: Seven menu actions are permanent dead stubs
Severity: High · Effort: Medium · Confidence: High

Evidence: All routed to `BcmNotImplementedYet` (`mod.rs:4503-4514`):
`CreatePr` (`branch_menu.rs:180`), `ForceWithLeasePush` (`:276`),
`ResetCurrentToHead` (`:266`), `DeleteRemoteBranch` (`:282`),
`FetchRemoteBranch` (`:174`), `PullFfOnly` (`:155`), `CreateTagHere` (`:220`).
Commit-side: `ResetToCommit` (`mod.rs:4613-4617`, `Msg::ResetUnimplemented`).

Problem: Seven dead menu items across "Sync", "Create", and "Advanced/Dangerous."
The "Advanced/Dangerous" group being greyed reinforces that the client can't do
the dangerous things its thesis is built to make safe. "Create PR" dead is a
glaring omission vs GitKraken/GitHub Desktop.

Why this matters: Feature surface promising more than it delivers erodes trust
and makes the menu noisy.

Recommended fix: Hide not-yet-built items (`ItemState::Hidden` exists,
`context_menu.rs:47`). Track the rest as roadmap entries with version targets,
not permanent grey-outs. **Note:** `RenameBranchUnimplemented`
(`i18n.rs:158-159`, `commands.rs:755-756`) is an outright lie — rename-branch
is fully implemented (`backend.rs:794-819`, tested in
`tests/branch_menu_ops_test.rs`). Wire the menu item; delete the string.

---

### Finding: No pull strategy choice (merge vs rebase vs ff-only)
Severity: High · Effort: Medium · Confidence: High

Evidence: `src/git/ops/pull_push.rs:487` `plan_pull` hard-decides: fast-forward
if possible, else merge commit. No `--rebase`, `--ff-only`, `--no-ff`. The only
ff-only path (`PullFfOnly`) is a stub.

Problem: A team on "rebase, never merge" policy cannot use this client for pull
— every diverged pull silently creates a merge commit. Conversely no "abort if
not fast-forward" safety.

Why this matters: Pull strategy is a per-repo decision (`.git/config
pull.rebase`) GUIs surface explicitly. Silently merging violates safety-first
for rebase-policy teams.

Recommended fix: Strategy selector on the pull modal (follow config / merge /
rebase / ff-only), defaulting to the repo's `pull.rebase`. GitKraken, Fork,
SourceTree all expose this.

---

### Finding: No reflog/oplog-driven recovery UI — backups exist but are unreachable
Severity: High · Effort: Medium · Confidence: High

Evidence: Discard writes a backup blob + oplog entry (`git/ops/discard.rs:110-114,
192-219`). The Operation Log panel (`render.rs:2698`) is **view-only** — no
"restore this op" or "copy blob SHA" action (grep `restore|recover` in `src/ui`
= only doc comments). Recovery text tells users to run `git cat-file -p <sha>`
in a terminal.

Problem: The safety mechanism is real but the recovery half is missing from the
product. A user who accidentally discards cannot get work back without reading
the oplog JSONL, finding the SHA, and running a CLI command.

Why this matters: "We made a backup you can't access" is worse than no backup
from a UX standpoint. Strongest place to differentiate on safety.

Recommended fix: "Restore" action on discard/delete-branch oplog rows → `git
cat-file -p <blob>` → write file (discard) or `git branch <name> <sha>`
(delete-branch). Recovery SHAs are already captured in `DiscardOutcome.backups`
(`git/ops/discard.rs:304`).

---

### Finding: Stash apply cannot restore the staged index; no stash preview
Severity: High · Effort: Medium · Confidence: High

Evidence: `src/git/ops/stash.rs:339-343` — `execute_stash_apply` calls
`repo.stash_apply(index, None)`, so the index is never reinstated. Stash menu
(`stash_menu.rs:43-57`) offers only Pop/Apply/Drop — no "view contents", no
"apply with --index".

Problem: If a user stashed with staged changes, applying via Kagi flattens
everything to unstaged — silently losing the staging distinction. No way to
inspect a stash before applying.

Why this matters: SourceTree, Fork, GitKraken all offer stash diff and "apply
& keep index." Silent loss of staging state is a data-fidelity bug.

Recommended fix: `StashApplyOptions::with_reinstate_index(true)` behind a
checkbox; "preview stash" via `git stash show -p`.

---

### Finding: Cherry-pick/revert previews show file list only — no diff before confirm
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/ui/modals.rs:2839-2905` renders `plan.preview_files` as a static
A/M/D tree; populated from `diff_tree_to_tree` deltas only
(`git/ops/mod.rs:268-313`). No hunk content, no expandable diff.

Problem: Before applying a cherry-pick the user sees "3 files will change" but
not what. For revert (negating a commit) this is especially dangerous —
committing a negation you haven't reviewed.

Recommended fix: Render the in-memory merge diff (already computed by
`cherrypick_commit` at `cherry_revert.rs:13`) in an expandable pane inside the
modal.

---

### Finding: No explicit "Mark resolved" action; discoverability poor
Severity: Medium · Effort: Small · Confidence: High

Evidence: The conflict dashboard (`conflict_view.rs:619-657`) splits files by
`buffer.has_resolution`. Grep `Mark resolved|mark_resolved` in `src/ui` = 0. A
file moves to "Resolved" only after Keep-current/Take-incoming/Keep-both
(`conflict_view.rs:887-903`) or editing the Result pane.

Problem: No per-file "I fixed this in my editor, mark resolved" button (the
`git add` equivalent during conflict). Users who resolve externally have no
clear way to signal "done."

Recommended fix: Add "Mark resolved" per file (calls `stage_conflict_resolution`,
exists at `git/conflicts.rs`) and "Mark all clean files resolved" bulk action.

---

### Finding: Clicks during a busy op are silently swallowed; no cancel
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/ui/operations/branch.rs:312-314,358-360` (and every `start_*`):
`if self.busy_op.is_some() { self.status_footer =
FooterStatus::Idle(Msg::OpInProgress.t()); return; }`. Grep
`cancel.*busy|abort_handle|cancel_token` in `src/ui` = only modal cancel
handlers.

Problem: Clicking "Push" while a fetch is in flight does nothing; only feedback
is grey text overwritten on next status change. No way to cancel a hung
network op.

Why this matters: Silent no-ops train users to click repeatedly (dangerous
once the op finishes). No cancel means a hung op requires quitting the app.

Recommended fix: Disable buttons while `busy_op.is_some()`; error toast (not
idle footer) for refusal; cancel button on the busy snackbar that drops the
background task for network ops.

---

### Finding: Plan-modal prose (blockers/warnings/recovery) is hardcoded English
Severity: Medium · Effort: Large · Confidence: High

Evidence: `src/ui/i18n.rs:1-9` states "src/git/ plan blocker/warning/recovery
strings are pinned by tests and are wave 2 — untouched here." All
`OperationPlan.warnings`/`blockers`/`recovery` strings are English literals
(`discard.rs:110-114,120-125`; `stash.rs:81-95`; `pull_push.rs:611-615`).

Problem: A JA user (JA is a first-class locale, `i18n.rs:40-43`) gets localized
chrome but every safety-critical message stays English. The most important text
is the least translated.

Recommended fix: Move plan strings through `Msg` keys. At minimum localize
discard/stash/conflict blocker + recovery strings.

---

### Finding: Amend's recovery instruction recommends the wrong command
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/git/staging.rs:512-522` recovery text:
`"To undo the commit while keeping changes staged:\n  git revert HEAD\n"`.
`git revert HEAD` creates a new commit negating HEAD; it does NOT "undo the
commit while keeping changes staged." Correct: `git reset --soft HEAD~1`.

Problem: A safety-first client handing users the wrong recovery command can
cause data confusion.

Recommended fix: Replace with `git reset --soft HEAD~1` (keeps staged) or
`git reset HEAD~1` (keeps unstaged), and clarify which.

---

### Finding: Delete-branch/reset use the same generic card as safe ops — no severity escalation
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/modals.rs:1293-1321` `render_delete_branch_modal` delegates
to `render_plan_modal_card` — same renderer as Pull/Push/Checkout
(`modals.rs:1582`). Only discard (`:1330`) and amend (`:574`) get distinct red
treatment.

Problem: Deleting an unmerged branch looks visually identical to a pull. The
`destructive: true` flag exists (`git/ops/mod.rs:157`) but delete-branch/reset
don't escalate the UI with it.

Recommended fix: Route every `plan.destructive == true` through a red-bordered
card with two-stage confirm (generalize amend/discard pattern).

---

### Finding: Merge-commit message is auto-generated with no dedicated editor
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/operations/conflict.rs:435-444` — on conflict Continue for a
merge, the flow transitions to the commit panel pre-filled with the default
merge message (`conflict_merge_commit_pending = true`). No plan modal previewing
the merge commit.

Problem: The merge-commit message is the historical record of *why* a merge
happened; auto-generated messages are unhelpful. Users get one shot mixed in
with normal commit UX.

Recommended fix: Dedicated merge-commit preview modal (parents, files, editable
message) before `execute_merge_commit`.

---

### Finding: Toast cap + 500ms auto-dismiss can drop error toasts before read
Severity: Low · Effort: Small · Confidence: High

Evidence: `src/ui/mod.rs:705,2123-2124` (oldest dropped beyond cap),
`:2565-2589` (500ms ticker).

Problem: Several toasts queued → cap evicts the oldest, which may be the error
the user needed. Errors share the auto-dismiss cadence of success/info.

Recommended fix: Exclude `ToastKind::Error` from auto-dismiss and cap eviction;
require explicit dismissal.

---

## 6. GPUI-specific problems

### Finding: `KagiApp` is a single god-Entity; every change re-renders everything
Severity: Critical · Effort: Large · Confidence: High

Evidence: `src/ui/mod.rs:747-1136` — one `Entity<KagiApp>` holds 104 fields
across every concern. GPUI invalidates the whole entity on `cx.notify()`. The
per-tab decomposition (`TabViewState`, `build_tab_view` at `:1215`) is a plain
struct, not an `Entity` — zero re-render isolation.

Problem: Any keystroke, toast expiry, watcher tick, or hover repaints the whole
6000-line render tree.

Why this matters: Architectural root cause behind most perf findings (per-frame
clones, path rebuilds, theme lookups).

Recommended fix: Decompose obvious sub-trees into child `Entity<T>`:
`Entity<CommitPanel>`, `Entity<ConflictEditor>`, `Entity<FileHistoryView>`,
`Entity<OpLogPanel>`, `Entity<Toasts>`. The existing `Entity<InputState>`
usage proves the team knows the pattern; it just stops at leaf widgets.

---

### Finding: `cx.notify()` called 329× — almost never batched, often in tight loops
Severity: Critical · Effort: Medium · Confidence: High

Evidence: 329 occurrences across 24 files. Examples: `src/ui/modals.rs` alone
has ~57 (almost every `render_X_modal` ends both cancel and confirm handlers
with `cx.notify()` — a confirm produces two notifications back-to-back).
`src/ui/render.rs:907,918,921,924` — four notifies in adjacent `when()`
clauses. `src/ui/operations/branch.rs` has 21 call sites.

Problem: `cx.notify()` on the root entity triggers the full 848-line `render()`
re-run. No batching with `cx.notify_in(duration)`; handlers do `set_X(cx);
window.focus(&fh); cx.notify();` in sequence.

Recommended fix: When a handler does multiple updates, call `cx.notify()` once
at the end. Move per-concern notify scope to child entities. For cosmetic
animation (toast slide, refresh spin) use a dedicated `Entity<ToastStack>`.

---

### Finding: Synchronous git2 work inside `render()`
Severity: Critical · Effort: Small · Confidence: High

Evidence:
- `src/ui/render.rs:446-450`: `if let Ok(backend) = Backend::open(&repo_path) {
  self.seed_history_from_reflog(&backend); }` runs inside `Render::render`.
- `src/ui/mod.rs:2059` (`ensure_avatars`, from `render()` `:423`):
  `avatar_fetch::repo_github_coords(&repo_path)` — read-only git2 on UI thread.
- `src/ui/mod.rs:1886-1906` (`detect_conflict_mode`, from `render()` `:429`):
  opens repo + `detect_conflict_session` synchronously.

Problem: A guarded "once per repo" call still runs the first git2 open on the
render thread. On a slow/network filesystem, `Backend::open` can take 100ms+,
producing a visible first-frame stall.

Recommended fix: Spawn these one-shot detection passes via
`cx.background_spawn` + `cx.spawn` marshalling (the pattern already used for
`file_history` at `src/ui/mod.rs:3452-3493`).

---

### Finding: Filesystem I/O inside `cx.spawn` (UI executor) instead of `cx.background_spawn`
Severity: High · Effort: Small · Confidence: High

Evidence: `src/ui/operations/commit.rs:140-158` `schedule_draft_save`:
`cx.spawn(async move |this, acx| { Timer::after(250ms).await;
this.update(acx, |app, _cx| { … kagi::git::save_draft(&rp, &branch, &msg,
&mode); }); })`. `save_draft` does real file I/O on the UI thread. Same at
`:887` `clear_draft`.

Problem: `cx.spawn` schedules on GPUI's foreground executor; blocking I/O
stalls the next frame. The 250ms debounce mask hides it during typing but a
slow disk surfaces a hitch exactly when the user stopped typing.

Why this matters: Inconsistent with the otherwise-correct marshal-back used
elsewhere (`cherry_revert.rs:103-150`).

Recommended fix: Hoist `save_draft`/`clear_draft` into `cx.background_spawn`
and await, or fire-and-forget via `.detach()`.

---

### Finding: Watcher/single-instance/auto-fetch tasks detached with no cancellation
Severity: High · Effort: Medium · Confidence: High

Evidence: `src/ui/tabs.rs:552-620` `arm_watcher`: `cx.spawn(async move |weak,
acx| { loop { Timer::after(100ms).await; … } }).detach()`. Same shape at
`:643`, `src/ui/mod.rs:2570`, `commands.rs:1483`.

Problem: Loops rely solely on a generation counter polled each iteration; never
cancelled by `Task::detach`'s inverse. Continue polling (100/200ms timers)
forever until `weak` returns `None`. A forgotten generation check anywhere
(e.g. the WIP path `tabs.rs:602-612`) silently fires on the wrong repo.

Recommended fix: Store the `Task` on `KagiApp` (`watcher_task:
Option<Task<()>>`), replace on re-arm (cancels previous). Drop polling for
`rx.recv().await`.

---

### Finding: Modal focus restored to `root_focus`, not the previously-focused element
Severity: High · Effort: Small · Confidence: High

Evidence: 30+ modal close paths restore to the same `root_focus`:
`src/ui/modals.rs:409-411,416-418,473-475,480-482,506-508,513-515,…` (pattern
repeats through ~57 sites). Single shared `root_focus: Option<FocusHandle>`
(`src/ui/mod.rs:751`).

Problem: If the user was typing in the commit-message `InputState` and opened a
plan modal, after confirming they end up at `root_focus`, not back in the
commit message. Comment at `modals.rs:1859` admits this is intentional for
cmd-j — trading input-focus continuity for keyboard-action reachability.

Recommended fix: Capture `window.focused(cx)` on modal open, store on modal
state, restore on close.

---

### Finding: `cx.spawn` → `weak.update` discards the generation check
Severity: Medium · Effort: Small · Confidence: High

Evidence: Every async op follows `let _ = this.update(acx, |app, cx| { …
cx.notify(); });`. Examples: `operations/branch.rs:860,1009,1138,1378`,
`checkout.rs:409`, `commit.rs:878`, `cherry_revert.rs:105,244`,
`pull_push.rs:268,469`, `stash.rs:128,776`, `discard.rs:171`, `worktree.rs:204`,
`history.rs:690`, `mod.rs:1837,2071,2141,2348,2782,2816,3111,3463,3609,3661`.

Problem: The `let _ =` discards "is the entity alive" *and* the generation
check. Only `file_history` and the tab switcher actually check a generation.
So a push that completes after a tab switch will `reload()` the *wrong* repo,
clobbering the current view.

Why this matters: Long-running op + tab switch = state applied to the wrong
repo.

Recommended fix: Capture `let gen = self.switch_generation;` when spawning;
inside marshal closure, `if app.switch_generation != gen { return; }` before
side effects.

---

### Finding: Render-time side effects on `self` — `render()` mutates observable state
Severity: Medium · Effort: Medium · Confidence: High

Evidence: `src/ui/render.rs:402` (`window.set_rem_size`), `:409-419`
(`self.bottom_panel_height = …`), `:446` (`self.history_seed_attempted = true;
self.seed_history_from_reflog(&backend);`), `:468-480`
(`self.pending_smart_msg.take()` → `set_template_inputs(…)`), `:485-497`
(`self.graph_scroll_x = max;`), `:585-594` (`self.sidebar_rows =
build_sidebar_rows(…)` — rebuilds entire sidebar flat list every frame).

Problem: Mutating *observable* state in render is the classic "render depends
on render" bug. `set_template_inputs` calls `set_value` which notifies →
notify-inside-render. `bottom_panel_height` written from render and read by the
divider drag handler — re-entrant drag-during-render races.

Recommended fix: Move lazy-init guards to a `fn reconcile(&mut self, window,
cx)` invoked from `cx.observe_self` or from the event handlers that change
inputs. Render should be pure presentation.

---

### Finding: `InputState` recreated on every modal open — IME/undo history lost
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/mod.rs:2200-2430` `sync_modal_inputs`: pattern `if
m.input_state.is_none() { let st = cx.new(|cx|
InputState::new(window, cx).placeholder("branch-name")); …
m.input_state = Some(st); }`. But `clear_X_modal()` sets the whole struct to
`None`, so reopening re-creates. Same for `remote_browse_modal` (2222-2235),
`create_worktree_modal` (2268-2281).

Problem: User types a branch name, cancels to check something, reopens →
retype from scratch. IME composition and undo history lost. The `commit_input`
(`:838`) is the one input *not* recreated — proving the team knows the right
pattern but only applied it to the commit message.

Recommended fix: Lift per-modal `InputState`s to top-level `Entity` slots (or
`Entity<ModalInputCache>`), populated once in `open_main_window`, `set_value("")`
on open.

---

### Finding: `theme()`/`scaled_px()` read hundreds of times per frame
Severity: Medium · Effort: Small · Confidence: High

Evidence: `src/ui/theme.rs:146-153` (`theme()` atomic load), `:215,241,254`
(`scaled`/`zoom`). Render calls `theme()` and `scaled_px(...)` on essentially
every `div().bg(rgb(theme().X))`. Comment at `theme.rs:187-188` admits
"`text_sm`/`text_xs` 260+ times". `render.rs:402` reapplies `set_rem_size`
every frame.

Problem: Atomic loads are nearly free individually but in aggregate
(thousands/frame) prevent hoisting and add cache-line traffic. `set_rem_size`
per frame is a no-op most times but forces a layout recompute check.

Recommended fix: At the top of `render()`, bind `let theme = theme::theme();`
and `let z = theme::zoom();` once; pass into helpers. Gate `set_rem_size`
behind `if window.rem_size() != px(rem_size_px())`.

---

### Finding: `eprintln!` debug logging + unconditional atomic fetch_add on hot paths
Severity: Low · Effort: Small · Confidence: High

Evidence: `src/ui/render.rs:457-463` — per-50-frame render counter does
`static N: AtomicU64::fetch_add` on every render even when the env var is unset
(the `if` is *after* the fetch). `render.rs:413`, `mod.rs:179,2061-2065,2077-2080`,
`operations/commit.rs:289-294,3558`.

Problem: Unbuffered `eprintln!` acquires a lock per call; unconditional atomic
fetch_add shows in flame graphs.

Recommended fix: Gate the `fetch_add` behind the env-var check. Replace
remaining `eprintln!`s on hot paths with the `klog!` macro.

---

## 7. Code deletion / simplification candidates

| # | File/function | Why removable/simpler | Risk | Suggested replacement |
|---|---|---|---|---|
| 1 | `Backend::repo()` (`backend.rs:53`) | Dead — 0 call sites | Zero | Delete; make `repo` private |
| 2 | 23 dead `take_*` + 15 dead `*_mut` accessors in `modal_state.rs` | Grep confirms dead | Zero | Delete (~230 LOC) |
| 3 | `tempfile` in `[dev-dependencies]` (`Cargo.toml:39`) | Redundant — already in `[dependencies]` | Zero | Delete the line |
| 4 | `Visibility::Hidden` (`commands.rs:395-396`) | Comment admits unused | Zero | Delete the variant |
| 5 | `render_status_footer` (`render.rs:4664`) | Dead — only def | Zero | Delete |
| 6 | `MAX_LANES`, `graph_width`, `graph_width_for_lanes` (`graph_view.rs:50-98`) | Marked obsolete, 0 callers | Zero | Delete |
| 7 | `WorktreeModalField` enum + `active_field` (`modals.rs:202-232`) | Self-identified legacy, never read | Zero | Delete |
| 8 | `RenameBranchUnimplemented` (`i18n.rs:158-159`) | Feature is fully implemented (`backend.rs:794-819`) | Zero | Delete string; wire menu item |
| 9 | `src/git/history.rs` filename | Collides with `kagi-domain/history.rs` (different concept) | Low | Rename → `file_history.rs` |
| 10 | `open_repository()` (`git/mod.rs:177-221`) | Duplicates `Backend::open` + `head_state` | Low | Delegate to `Backend::open`; expose `Backend::info()` |
| 11 | 26 test files copy-pasting `fn git()`/`write_file()` | Identical 7-env-var setup | Low | Create `tests/common/mod.rs` (~600 LOC recovered) |
| 12 | 18 `#[allow(unused_imports)]` in `git/mod.rs:30-109` | Masks "which re-exports are consumed" | Low | Audit each `pub use`; drop unused |
| 13 | `render_amend_modal` (`modals.rs:574-819`) rebuilds card | Duplicates `render_plan_modal_card` (`:1582`) | Medium | Generalize card with optional armed flag |
| 14 | ~20 duplicated cancel/confirm closures in `modals.rs` | Near-identical 6-line blocks | Low | Extract `modal_cancel_handler`/`modal_confirm_handler` |
| 15 | `headless.rs` ~30 of 48 hooks | Duplicate `tests/` integration coverage | Medium (script/CI dep audit) | Delete redundant hooks; keep ~10 UI-state hooks |
| 16 | 112 delegating `Backend` methods | Pure ceremony (see Arch Finding 6) | Medium-High (many call sites) | Collapse to ~10 semantic methods OR invert to expose `repo()`/`path()` |

**Quick wins (one PR, ~250 LOC + 1 Cargo line):** #1, #2, #3, #4, #5, #6, #7, #8.
**High-value structural:** #11 (shared test harness), #15 (headless audit).

---

## 8. Recommended target architecture

The existing `docs/rearch/architecture.md` target is sound; the gap is
*delivery*, not design. Below is a condensed, opinionated version focused on
what actually moves the needle given the findings.

### Principles

1. **Safety is a backend guarantee, not a UI convention.** The pipeline
   (`plan → confirm → preflight → execute → verify → oplog`) is enforced in
   *one* place: a `GitBackend::run(Operation, confirmed_plan)` method. No
   caller — UI, headless, test, automation — can skip it.
2. **One repo session owns one `git2::Repository` for its lifetime.** No
   per-op `Backend::open`. The session is the cache boundary.
3. **UI re-renders are scoped to what changed.** Panels are child `Entity<T>`s;
   a commit-panel keystroke does not repaint the graph.
4. **The domain layer owns behavior, not just types.** Pure validation,
   conflict-FSM transitions, plan pre-computation live in `kagi-domain` and are
   unit-tested without git2.

### Crate topology (unchanged target, reordered by priority)

```
kagi (bin)            shell, menu, bootstrap — no business logic
  ├─ kagi-ui          Entity trees, Render, components (NO git2 — compile error)
  │   └─ kagi-app     AppState, RepoSession, OperationController, services
  │       └─ kagi-git GitBackend trait + git2/CLI adapters + worker thread
  │           └─ kagi-domain  pure: models, graph, diff, conflict FSM, plan, rules
  └─ xtask             packaging
```

### Responsibilities

| Layer | Owns | Forbidden |
|---|---|---|
| **kagi-domain** | Models, graph layout, diff/hunk model, conflict FSM (`SessionState`), plan types, `Operation` enum, pure rules (message_template, checklist, validate_branch_rename), i18n keys | git2, gpui, I/O |
| **kagi-git** | `trait GitBackend: Send`; **enforced pipeline** (`run(op, plan)` = preflight → execute → verify → oplog); git2 adapter (in-memory dry-runs); CLI adapter (fetch/push); **one worker thread per RepoSession** owning the `Repository`; snapshot; oplog v2 | gpui |
| **kagi-app** | `AppState` (one Entity); `RepoSession` (one per tab: backend handle + snapshot + `Selection` + `RepoMode` + DiffService cache + watcher); `OperationController` (the *only* mutating path; owns cancellation, generation/supersede); services (Settings, DiffService, Avatar, SmartCommit) | git2 direct |
| **kagi-ui** | Child `Entity<T>` panels (CommitPanel, ConflictEditor, Inspector, FileHistory, OpLogPanel, Toasts, Sidebar); thin `Render` over view-models; `enum ActiveModal`; commands | git2 |
| **kagi (bin)** | Window, native menu, `Action` dispatch, bootstrap, thin headless entry | business logic |

### Proposed `src/` directory structure (incremental from current)

```
src/
  main.rs                  bootstrap only
  headless.rs              RETIRE after kagi-app testability (ADR-0077)
  git/
    mod.rs                 re-exports (shrinking allowlist)
    backend.rs             GitBackend trait + enforced run() (was: 112 delegators)
    worker.rs              NEW: per-session worker thread + channel
    snapshot.rs            snapshot builder (off-thread caller)
    status.rs  diff.rs  diffstat.rs  staging.rs  refs.rs  log.rs  cli.rs
    oplog.rs               append-only, SHAs + recovery handles
    ops/                   per-op modules (already split) — each execute_* takes &OperationPlan
    resolution.rs          ONLY git2 glue (≤300 LOC) → delegates to kagi-domain::resolution
    message_gen.rs         ONLY git2 glue → delegates to kagi-domain::message_gen
    file_history.rs        renamed from history.rs
  ui/
    mod.rs                 KagiApp struct (shrinking) + bootstrap
    render.rs              ONLY impl Render for KagiApp (dispatch only)
    view_models/           CommitGraphVM, InspectorVM, DiffVM, ConflictVM, … (grow from 213 LOC)
    panels/                NEW: Entity<CommitPanel>, Entity<ConflictEditor>, …
    modals/                split modals.rs per-cluster; shared ModalCard component
    components/            ModalCard, KagiButton (exists), MenuOverlay (exists)
    operations/            intent handlers (shrink as VMs absorb logic)
    theme.rs i18n.rs settings.rs watcher.rs
  domain/ → crates/kagi-domain (finish migration: behavior, not just types)
```

### Migration sequencing (see `/docs/refactor-plan.md` for the step plan)

1. **Enforce the pipeline in `Backend`** (safety, small) — does not require the
   crate split.
2. **One `RepoSession` owns the backend** (perf + arch, medium) — removes 132
   `Backend::open` sites.
3. **Decompose `KagiApp` into child entities** (perf + GPUI, large) — kills the
   329-notify repaints.
4. **Finish `kagi-domain` extraction** (arch, medium) — collapse the 15 shim
   files to real glue.
5. **Build real VMs; retire `headless.rs`** (testability, large) — ADR-0076/0077.
6. **Crate split** (arch, large) — makes the git2-free UI a compile error.

---

## 9. Risk map

| Area | Risk | Likely bug class |
|---|---|---|
| `Backend::execute` dispatch (`backend.rs:304-427`) | **Highest.** Safety pipeline bypassed for most ops via any non-UI caller. | Silent data loss / dangling objects / missing oplog on headless/automation paths. |
| Merge into dirty tree (`ops/merge.rs:26,359-420`) | **Highest.** Conflict markers written into uncommitted work. | Interleaved markers + user edits; only recovery is unsurfaced `git merge --abort`. |
| `stage_conflict_resolution` (`conflicts.rs:931-984`) | High. Non-atomic write loop. | WT/index mismatch on partial disk failure; lost resolutions. |
| `KagiApp` god-struct + 329 `cx.notify()` | High. Re-render-everything. | Perf regressions; render-depends-on-render bugs as fields grow. |
| Async marshal closures discarding generation (`let _ = this.update(...)`) | High. | Op completes after tab switch → state applied to wrong repo. |
| Checkout untracked-overlap (`checkout.rs:127-132,362-415`) | Medium. Fail-open prediction. | Plan says "safe," execute aborts cryptically. |
| Stash index shift (`stash.rs:534-543`) | Medium. No count re-check. | Pop applies/drops wrong entry after concurrent stash push. |
| Cherry-pick/revert dangling commit (`cherry_revert.rs:423-505,817-928`) | Medium. Commit created before safe-checkout. | Unreferenced commit object; user retries → double apply. |
| Undo/amend pushed-check plan-time only (`history.rs:269-336,594+`) | Medium. | Rewrites published history if pushed between plan and execute. |
| `Rc<Cell>` geometry backchannels (`mod.rs:943,948,1082,1085`) | Medium. Stale coords. | Divider jumps on first drag after tab switch/resize. |
| `kagi-domain` shim drift (`message_gen`, `resolution`, `history`) | Medium. | Bug fixed in one file, not the other. |
| `headless.rs` brittle greps (`[kagi] …` lines) | Medium. | Log wording changes break tests; freezes evolution. |
| `tab_cache` unbounded (`mod.rs:1025`) | Low. | Memory growth in long sessions. |
| Auto-fetch per-tab ticker (`commands.rs:1474`, `render.rs:438`) | Low. | N parallel fetches; rate limits on SSH remotes. |

---

## 10. Prioritized action plan

### Phase 1 — Safety fixes (do first; small, localized, protects the thesis)

1. **Enforce pipeline in `Backend::execute`** — require confirmed plan, call
   `preflight_check`, `verify`, `append_oplog` for every mutating op.
   *Closes git-safety Findings 1, 2, 3, 6, 12–16, 27.* Effort: Medium.
2. **Block merge on dirty working tree** (mirror cherry-pick). *Closes
   git-safety Finding 11.* Effort: Small.
3. **Atomic `stage_conflict_resolution`** (temp-write-then-rename). *Closes
   git-safety Finding 20.* Effort: Small.
4. **Two-stage confirm on Discard** (amend's `confirm_armed` pattern). *Closes
   UX Finding 1.* Effort: Small.
5. **Oplog "Restore" action** for discard/delete-branch. *Closes UX Finding 9.*
   Effort: Medium.
6. **`execute_stash_pop`/`drop` take a plan + `preflight_check_stash`**. *Closes
   git-safety Findings 6, 8.* Effort: Small.

### Phase 2 — Architecture cleanup (unblocks perf and testability)

1. **One `RepoSession` owns the `Backend`** — removes 132 `Backend::open` sites;
   the session is the cache boundary. Effort: Medium.
2. **Finish `kagi-domain` extraction** — collapse 15 shim files to real glue;
   rename `src/git/history.rs` → `file_history.rs`. Effort: Medium.
3. **Delete dead code sweep** — modal accessors (#2), `Backend::repo()` (#1),
   `tempfile` dev-dep (#3), dead render helpers (#4–7), lying rename string
   (#8). Effort: Small.
4. **Shared test harness** (`tests/common/mod.rs`). Effort: Small.
5. **Audit `headless.rs`** — route through Backend; delete ~30 redundant hooks.
   Effort: Medium.

### Phase 3 — Performance improvements

1. **`reload_external` off-thread** (mirror `refresh_working_tree_external`).
   Effort: Small.
2. **Per-file diff content cache** by `(row, file_index)`. Effort: Small.
3. **Tree-sitter highlighting off-thread**; render un-highlighted first.
   Effort: Small.
4. **Arc-wrap `MainDiffView`/`compare_view`/`conflict`**; drop `row.clone()`
   in `render_rows`; cache avatar color on `CommitRow`. Effort: Small.
5. **Graph layout cache** keyed by `(head_oid, commit_count)`; pre-bake
   per-row path geometry. Effort: Medium.
6. **Lazy ahead/behind** (visible branches only). Effort: Medium.
7. **Single global auto-fetch ticker per remote-URL.** Effort: Small.

### Phase 4 — UX improvements

1. **Error classification layer** → typed messages + suggested action; errors
   via persistent toast (not footer). *Closes UX Findings 2, 3.* Effort: Medium.
2. **Per-hunk staging and discard.** *Closes UX Findings 4, 5.* Effort: Large.
3. **Pull strategy selector** (follow config / merge / rebase / ff-only).
   *Closes UX Finding 8.* Effort: Medium.
4. **Hide dead menu stubs**; wire rename-branch; roadmap-tag the rest. *Closes
   UX Finding 7.* Effort: Small.
5. **Cherry-pick/revert diff preview** in modal. *Closes UX Finding 11.*
   Effort: Medium.
6. **"Mark resolved" per file**; merge-commit message editor. *Closes UX
   Findings 12, 17.* Effort: Small.
7. **Localize plan-modal safety strings** (at least discard/stash/conflict).
   *Closes UX Finding 14.* Effort: Large.

### Phase 5 — Code deletion and simplification

1. **Dead-accessor + dead-helper sweep** (one PR). Effort: Small.
2. **Collapse `Backend`** to ~10 semantic methods OR invert to expose
   `repo()`/`path()`. Effort: Medium-High.
3. **Decompose `modals.rs`** per-cluster + shared `ModalCard`. Effort: Medium.
4. **Move pure helpers** out of `ui/mod.rs` into `ui/util.rs`/`kagi-domain`.
   Effort: Small.
5. **Retire `headless.rs`** as VM/controller testability lands (ADR-0077).
   Effort: Large.

---

*Review compiled from six parallel deep passes (architecture, git-safety,
performance, UX, code-cleanup, GPUI) plus direct verification of every headline
number against the tree. See `/docs/refactor-plan.md`,
`/docs/git-safety-checklist.md`, `/docs/performance-review.md` for the
executable follow-ups.*
