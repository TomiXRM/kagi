# Kagi Refactor Plan

> Concrete, ordered implementation plan derived from `/docs/codebase-review.md`.
> Each step is scoped to one PR-sized commit (or a short series), keeps
> `cargo test --workspace` green, and states exactly what to touch, what *not*
> to touch, how to verify, and how to roll back.
>
> **Golden rules**
> 1. Never break the `plan → confirm → preflight → execute → verify → oplog`
>    invariant that already works in the UI path — only extend it.
> 2. Never introduce `git2::` into `src/ui/` (CI grep gate).
> 3. Never add `reset --hard`, `push --force`, `git clean`, `--force-with-lease`,
>    or `unsafe`.
> 4. Every step ends with `cargo fmt --all && cargo clippy --workspace &&
>    cargo test --workspace` clean. Run `cargo fmt --check` before pushing.
> 5. UI-behavior changes cannot be fully verified by subagents — the primary
>    session or a human must launch the app and eyeball it.

## Sequencing rationale

Phases are ordered by **risk reduction per unit effort**:

- **Phase 0** — zero-risk deletions that shrink the surface before real work.
- **Phase 1** — safety fixes that protect the product thesis. Small, localized,
  high-value. Do these *before* any architectural movement so the safety
  invariants hold during later refactors.
- **Phase 2** — architectural enablers (RepoSession, domain extraction) that
  *unblock* Phases 3–5. Not user-visible alone, but necessary.
- **Phase 3** — performance (depends on Phase 2's RepoSession).
- **Phase 4** — UX (independent; can interleave).
- **Phase 5** — large structural moves (entity decomposition, headless
  retirement, crate split) that are unsafe to attempt before Phases 1–2.

---

## Phase 0 — Dead-code & cleanup sweep (do first, zero-risk)

**Goal:** shrink the surface so subsequent diffs are readable. One PR.

### Step 0.1 — Trivial deletions
**Touch:**
- `src/git/backend.rs:53` — delete `pub fn repo(&self) -> &Repository`. Make
  `repo` field private. (0 callers verified.)
- `Cargo.toml:39` — delete the redundant `tempfile = "3"` under
  `[dev-dependencies]` (already in `[dependencies]`).
- `src/ui/operations/modal_state.rs` — delete all 23 `take_*` methods (0
  callers) and the 15 unused `*_mut` methods (call-site audit in cleanup
  review). Remove the file-wide `#[allow(dead_code)]` at `:22`.
- `src/ui/commands.rs:395-396` — delete `Visibility::Hidden` (self-admitted
  unused).
- `src/ui/render.rs:4664` — delete dead `render_status_footer`.
- `src/ui/graph_view.rs:50-98` — delete `MAX_LANES`, `graph_width`,
  `graph_width_for_lanes` (self-admitted obsolete, 0 callers).
- `src/ui/modals.rs:202-232` — delete `WorktreeModalField` enum +
  `CreateWorktreeModal.active_field` (self-admitted legacy, never read).

**Commit:** `refactor: delete dead code (Backend::repo, modal accessors, unused helpers)`

**Verify:** `cargo test --workspace` green; `grep -rn '\.repo()' src/ tests/` = 0.

**Rollback:** `git revert <sha>` — pure deletion, no behavior change.

### Step 0.2 — Fix the lying i18n string + wire rename-branch
**Touch:**
- `src/ui/i18n.rs:158-159` — delete `RenameBranchUnimplemented` variant + its
  JA translation (`:562-563` region).
- `src/ui/commands.rs:755-756` — change `branch.rename` from
  `Disabled(RenameBranchUnimplemented)` to `Enabled`, routing to the existing
  rename handler (`backend.rs:794-819` `plan_rename_branch`/`execute_rename_branch`).

**Commit:** `fix(i18n): rename-branch is implemented; wire menu item + drop dead string`

**Verify:** `cargo test --workspace` green; manual: right-click a branch →
Rename works end-to-end.

---

## Phase 1 — Safety fixes (protect the thesis)

**Goal:** make the safety pipeline a backend guarantee, not a UI convention.
These are the highest-value changes in the whole plan.

### Step 1.1 — Enforce the pipeline in `Backend::execute`  ⭐ highest priority
**Touch:** `src/git/backend.rs:304-427` and every `execute_*` signature in
`src/git/ops/*.rs`.

**Approach (incremental, not big-bang):**

1. Add a new method `Backend::run(&self, op: &Operation, plan:
   &OperationPlan) -> Result<OperationOutcome, GitError>` that does:
   ```
   preflight_check(repo, plan)?            // HEAD/stash-count unchanged
   let outcome = dispatch_execute(op)?      // existing execute_* fns
   verify_outcome(op, &outcome)?            // new: per-op post-condition
   append_oplog(op, plan, &outcome)?        // currently caller-side
   Ok(outcome)
   ```
2. Change `execute_*` signatures to take `&OperationPlan`:
   `execute_checkout(repo, &OperationPlan, branch)`,
   `execute_cherry_pick(repo, &OperationPlan, id)`,
   `execute_revert(repo, &OperationPlan, id)`,
   `execute_merge_branch(repo, &OperationPlan, target)`,
   `execute_merge_into_conflict(repo, &OperationPlan, target)`,
   `execute_pull(repo, &OperationPlan)`,
   `execute_undo_commit(repo, &OperationPlan)`,
   `execute_amend(repo, &OperationPlan, mode, message)`,
   `execute_stash_apply(repo, &OperationPlan, index)`,
   `execute_stash_pop(repo, &OperationPlan, index)`,
   `execute_stash_drop(repo, &OperationPlan, index)`.
   First line of each: `preflight_check(repo, plan)?` (or
   `preflight_check_stash` for stash ops).
3. Keep the old `Backend::execute(op)` as a thin wrapper that **requires** a
   pre-built plan (change signature to `execute(&self, op: &Operation, plan:
   &OperationPlan)`), OR delete it and have all callers use `run()`.
4. Add `verify_outcome` per op — at minimum: re-read HEAD/ref and assert it
   matches the predicted state; for discard, the existing status re-read.

**Do NOT:** change the UI call path yet — just route it through `run()` in
Step 1.2. Keep `execute_*` behavior identical except for the added preflight
at the top.

**Commits (split for reviewability):**
- `feat(git): Backend::run enforces preflight+verify+oplog for mutating ops`
- `refactor(git): execute_* take &OperationPlan; call preflight first` (per-op
  sub-commits if large: `checkout`, `cherry_revert`, `merge`, `stash`,
  `history`, `pull_push`)

**Verify:**
- New unit test: build a plan, mutate the repo (e.g. create a commit), call
  `Backend::run(op, plan)` → must return a preflight error, not mutate.
- New unit test: `Backend::run` appends an oplog entry for every mutating op.
- `tests/*` green; headless `KAGI_*` paths updated to call `plan_*` →
  `run()` (see Step 1.4).

**Rollback:** Revert the commit series. The old `execute_*` fns still exist
underneath (the change is additive at the `run()` layer until callers migrate).

**Risk:** Medium. The signature change ripples through `src/ui/operations/*`
(~12 files) and `headless.rs`. Mitigate by doing it op-by-op.

### Step 1.2 — Route UI operations through `Backend::run`
**Touch:** `src/ui/operations/{checkout,cherry_revert,merge,stash,history,
pull_push,branch,discard,worktree}.rs` — each `start_*` already builds a plan
and shows a confirm modal; change the execute step from `backend.execute_X(...)`
to `backend.run(&op, &plan)`.

**Commit:** `refactor(ui): operations route through Backend::run (enforced pipeline)`

**Verify:** `cargo test --workspace`; manual smoke test of each op.

### Step 1.3 — Block merge on dirty working tree
**Touch:** `src/git/ops/merge.rs:26` — replace `merge_dirty_warnings(...)` with
a blocker when `!status.staged.is_empty() || !status.unstaged.is_empty()`.
Keep warnings for untracked-only. Block `execute_merge_into_conflict`
unconditionally on dirty tree.

**Commit:** `feat(git): block merge on dirty working tree (mirror cherry-pick)`

**Verify:** New test: dirty WT + merge plan → blocker present. Manual: try
merge with unstaged edits → modal shows red blocker, no Execute button.

### Step 1.4 — Atomic `stage_conflict_resolution`
**Touch:** `src/git/conflicts.rs:931-984`. Rewrite the per-file loop:
1. Write each resolved file to a temp path (`<file>.kagi-tmp-<n>`).
2. If any write fails, delete all temp files, return `Err`.
3. Rename all temp files atomically to their targets.
4. `index.add_path` each, then `index.write()` once.

**Commit:** `fix(conflicts): stage_conflict_resolution is atomic (temp-write-then-rename)`

**Verify:** Inject a write failure (chmod a target dir read-only mid-loop) →
no files modified, no index write, `Err` returned.

### Step 1.5 — Two-stage confirm on Discard
**Touch:** `src/ui/modals.rs:1330-1546` `render_discard_modal` — port the
`confirm_armed` pattern from `render_amend_modal` (`modals.rs:103,732-784`).
First click → button relabels "Permanently discard N files"; second click →
executes. Reset armed state on modal open/cancel.

**Commit:** `feat(ui): two-stage confirm on Discard (mirror amend)`

**Verify:** Manual — Discard modal requires two clicks; Escape/Cancel resets.

### Step 1.6 — Oplog "Restore" action
**Touch:** New `src/git/ops/restore.rs` with `restore_discarded(paths, backups:
&[DiscardBackup]) -> Result<()>` (`git cat-file -p <blob>` → write file) and
`restore_deleted_branch(name, sha) -> Result<()>` (`git branch <name> <sha>`).
Wire a "Restore" button on discard/delete-branch rows in the Operation Log
panel (`src/ui/render.rs:2698` area).

**Commit:** `feat(ui): Restore action on discard/delete-branch oplog rows`

**Verify:** Discard an untracked file → oplog row → Restore → file reappears
byte-identical.

### Step 1.7 — Stash pop/drop take plan + preflight
**Touch:** `src/git/ops/stash.rs:534-543,693-704` — already covered by Step
1.1's signature change; ensure `preflight_check_stash` is the first line.

**Commit:** (folded into Step 1.1's stash sub-commit)

---

## Phase 2 — Architectural enablers

**Goal:** establish the `RepoSession` (one owner for the backend) and finish
the domain extraction. These unblock Phase 3 (perf) and Phase 5 (entity split).

### Step 2.1 — Introduce `RepoSession` owning the `Backend`
**Touch:** New `src/git/session.rs`:
```rust
pub struct RepoSession {
    backend: Backend,           // opened once
    path: PathBuf,
    // later: snapshot cache, diff cache, worker handle
}
impl RepoSession {
    pub fn open(path: &Path) -> Result<Self, GitError>;
    pub fn backend(&self) -> &Backend;   // the only accessor
}
```
Add `session: Rc<RepoSession>` (or `Arc` once worker is `Send`) to
`TabViewState`. Migrate the 94 `Backend::open` sites in `src/ui/` to read
`self.active_view.session.backend()`. Do this file-by-file.

**Commit (series):**
- `feat(git): RepoSession owns Backend for the tab lifetime`
- `refactor(ui): <file> uses RepoSession instead of Backend::open` (per file)

**Verify:** `grep -rn 'Backend::open' src/ui/` monotonically decreasing;
`cargo test --workspace`.

**Rollback:** Revert; the per-file `Backend::open` calls still work.

**Risk:** Medium — large touch count but mechanical. The `Rc` vs `Arc` choice
matters for the later worker thread; for now `Rc` (single-thread) is fine.

### Step 2.2 — Finish `kagi-domain` extraction (collapse shims)
**Touch:**
- Rename `src/git/history.rs` → `src/git/file_history.rs` (resolves the
  filename collision with `kagi-domain/src/history.rs`). Update `mod.rs`.
- `src/git/message_gen.rs`: move all pure parsing/rule logic to
  `crates/kagi-domain/src/message_gen.rs`; keep only git2 glue
  (`collect_staged_files`, `ollama_available`, CLI probe) in `src/git/`.
- `src/git/resolution.rs`: move pure hunk/chunk logic to domain; keep only
  git2-backed `ResolutionBuffer` materialization.
- Target: each `src/git/<x>.rs` visibly smaller than its domain twin.

**Commit:** `refactor(domain): finish message_gen/resolution extraction; rename history→file_history`

**Verify:** `grep -rn 'pub use kagi_domain' src/git/` shows clean bridges;
unit tests moved with the logic; `cargo test --workspace`.

### Step 2.3 — Move pure helpers out of `ui/mod.rs`
**Touch:** `src/ui/mod.rs` free functions (`format_hms`, `short_hash`,
`conflict_content_sig`, `busy_label`, `row_height`, `validate_merge_from_drag`,
`draggable_branch_name`, `context_branch_name`, `collect_history_commits`,
`localize_plan_blockers`, `platform_menu_label`) → new `src/ui/util.rs` (or
`kagi-domain` where no GPUI types are touched).

**Commit:** `refactor(ui): extract pure helpers from mod.rs to util.rs`

**Verify:** New unit tests for the moved pure fns; `wc -l src/ui/mod.rs`
drops.

### Step 2.4 — Shared test harness
**Touch:** New `tests/common/mod.rs` exporting `git(dir, args)`, `write_file`,
`write_bytes`, `init_repo`, `head_sha`, `head_tree_sha` — the 7-env-var setup
currently copy-pasted in 26 files. Each test file: `mod common;` + drop locals.

**Commit:** `test: extract shared test harness (tests/common/mod.rs)`

**Verify:** `cargo test --workspace` green; `grep -rln 'fn git(dir' tests/`
drops to 0.

---

## Phase 3 — Performance improvements

**Depends on:** Step 2.1 (RepoSession) for clean cache ownership.

### Step 3.1 — Move `reload_external` off the UI thread
**Touch:** `src/ui/mod.rs:1788-1798` — mirror
`refresh_working_tree_external` (`:1826-1836`): `cx.background_spawn` the
`snapshot()`, marshal back via `cx.spawn` + `apply_tab_view`.

**Commit:** `perf(ui): snapshot off-thread in reload_external`

**Verify:** Run `git commit` in a terminal on a 10k-commit repo while app is
open → no frame drop.

### Step 3.2 — Per-file diff content cache
**Touch:** `src/ui/mod.rs` — add `file_diff_cache: HashMap<(usize, usize),
Arc<FileDiffView>>` alongside `diff_cache` (`:770`); populate in
`set_commit_main_diff`/`open_main_diff`; return on repeat; invalidate together
with `diff_cache`.

**Commit:** `perf(ui): cache per-file diff content by (row, file_index)`

**Verify:** Profile click-A/click-B/click-A cycle; second A is O(1).

### Step 3.3 — Tree-sitter highlighting off-thread
**Touch:** `src/ui/diff_view.rs:194-285` — make `highlight_diff_rows` `Send`;
callers (`src/ui/mod.rs:3291`, `set_commit_main_diff`) wrap it in
`cx.background_spawn`, render un-highlighted rows first, swap when done.

**Commit:** `perf(ui): tree-sitter highlight off-thread; render text first`

**Verify:** Open a 5k-line diff → text paints immediately, highlights arrive a
moment later.

### Step 3.4 — Render-path clone elimination
**Touch:**
- `src/ui/mod.rs` — change `main_diff`/`compare_view`/`conflict` fields to
  `Arc<MainDiffView>` etc.; `src/ui/render.rs:567,568,627` clone only the `Arc`.
- `src/ui/render.rs:3176-3179` — drop `let row = row.clone();` (handlers use
  `ix`); change `graph_canvas` (`src/ui/graph_view.rs:212-225`) to borrow
  `edges: &[GraphEdge]`, `stash_lanes: &[usize]`.
- `src/ui/commit_list.rs` — pre-compute `avatar_color`/`avatar_initial` onto
  `CommitRow` at build time; `src/ui/render.rs:3218-3219` reads the cached.
- `src/ui/render.rs:402` — hoist `let theme = theme::theme();` and `let z =
  theme::zoom();` to the top of `render()`; pass into helpers. Gate
  `set_rem_size` behind a change check.

**Commits (split):**
- `perf(ui): Arc-wrap MainDiffView/compare_view/conflict`
- `perf(ui): drop CommitRow clone in render_rows; graph_canvas borrows`
- `perf(ui): cache avatar color on CommitRow`
- `perf(ui): hoist theme()/zoom() to one local per render`

**Verify:** Profile a scroll on a dense graph; allocator calls per frame drop.

### Step 3.5 — Graph layout cache + pre-baked paths
**Touch:**
- `src/ui/mod.rs`/`TabViewState` — store `GraphLayout` next to the snapshot;
  recompute only when `(head_oid, commit_count)` changes (not on status-only
  refresh).
- `src/ui/commit_list.rs` — pre-bake per-row path geometry (`Rc<[PathData]>`)
  into `CommitRow`; `src/ui/graph_view.rs` paint closure strokes precomputed
  paths.

**Commit:** `perf(graph): cache layout by head_oid+commit_count; pre-bake paths`

**Verify:** Profile: external `git commit` on a 10k-commit repo no longer
re-layouts the whole graph if the commit count is unchanged.

### Step 3.6 — Lazy ahead/behind + single auto-fetch ticker
**Touch:**
- `src/git/snapshot.rs:126-151` — compute `graph_ahead_behind` only for visible
  sidebar branches; cache the rest, invalidate on fetch.
- `src/ui/commands.rs:1474` — make `ensure_auto_fetch_ticker` global per
  remote-URL (store on `AppState` or a singleton), not per-tab; remove the
  call from `render()` (`src/ui/render.rs:438`), arm on app init.

**Commits:**
- `perf(git): lazy ahead/behind for non-visible branches`
- `perf(ui): single auto-fetch ticker per remote-URL (not per-tab)`

**Verify:** 5 tabs open + auto_fetch on → one fetch per remote per 180s.

---

## Phase 4 — UX improvements (can interleave with Phase 2–3)

### Step 4.1 — Error classification + persistent error toasts
**Touch:** New `src/git/error_classify.rs` mapping `git2::Error` codes + CLI
exit/stderr patterns → `enum UserError { Auth, Network, Conflict, DirtyTree,
NotARepo, NonFastForward, Unknown(String) }`, each with a friendly message +
suggested action. Replace ~35 `FooterStatus::Failed(format!("...: {}", e))`
sites. Exclude `ToastKind::Error` from auto-dismiss (`src/ui/mod.rs:2565-2589`)
and the `TOASTS_MAX` cap.

**Commit:** `feat(ui): typed error classification + persistent error toasts`

### Step 4.2 — Per-hunk staging
**Touch:** `src/git/staging.rs` — add `stage_hunks(path, &[HunkRange])` using
`git apply --cached` on a generated patch; `src/ui/diff_view.rs` — per-hunk
"+" buttons. (Large; consider splitting into backend + UI commits.)

### Step 4.3 — Pull strategy selector
**Touch:** `src/git/ops/pull_push.rs:487` `plan_pull` — add `PullStrategy {
FollowConfig, Merge, Rebase, FastForwardOnly }`; read repo's `pull.rebase` for
default. `src/ui/modals.rs` pull modal — strategy dropdown.

### Step 4.4 — Hide dead menu stubs; roadmap the rest
**Touch:** `src/ui/branch_menu.rs:155,174,180,220,266,276,282` and
`src/ui/context_menu.rs` ResetToCommit — change `disabled("not implemented
yet")` to `Hidden` for items with no near-term plan; keep visible-but-disabled
only for items on the roadmap with a version target in the tooltip.

### Step 4.5 — Cherry-pick/revert diff preview in modal
**Touch:** `src/ui/modals.rs:2839-2905` — render the in-memory merge diff
(already computed at `cherry_revert.rs:13`) in an expandable pane.

### Step 4.6 — "Mark resolved" per file + merge-commit editor
**Touch:** `src/ui/conflict_view.rs` — add "Mark resolved" calling
`stage_conflict_resolution`; `src/ui/operations/conflict.rs:435-444` — dedicated
merge-commit preview modal before `execute_merge_commit`.

### Step 4.7 — Localize plan-modal safety strings
**Touch:** Add `Msg` keys for discard/stash/conflict blockers + recovery;
route through `Msg::t`. (Large; can be staged per-op.)

---

## Phase 5 — Large structural moves (after Phases 1–2)

**Do NOT start until Phase 1 (safety) and Step 2.1 (RepoSession) are done.**
These are the ADR-0072/0075/0076/0077 deliverables.

### Step 5.1 — Decompose `KagiApp` into child `Entity<T>` panels
**Touch:** Promote `CommitPanel`, `ConflictEditor`, `FileHistoryView`,
`OpLogPanel`, `Toasts`, `Sidebar` from flat fields on `KagiApp` to child
`Entity<T>`s with their own `Render`. Each becomes its own `cx.notify()` scope.

**Risk:** High. Do one panel at a time; `Toasts` and `OpLogPanel` are the
lowest-risk starts. This is the change that kills the 329-notify repaints.

**Done:** `Toasts` → `Entity<ToastStack>` and `OpLogPanel` → `Entity<OpLogPanel>`
(ADR-0110): push / expire / dismiss / row-expand now re-render only their own
subtree.

**Prep done:** the flat per-panel fields are being consolidated into single
structs ahead of the Entity move (pure field moves, no behaviour change): the six
`sidebar_*` fields → `sidebar: sidebar::SidebarState`, and the thirteen
`conflict_*` fields → `conflict: conflict_view::ConflictState`. Both are the
groundwork for a future `Entity<…>` migration. (The conflict dashboard's
information hierarchy / per-file card actions were also reworked on top of the
consolidated state — a UX change, separate from the Entity work.)

Remaining: `CommitPanel`, `FileHistoryView` (already single-struct, ready for
Entity); `Sidebar`, `ConflictEditor` (state now consolidated — the Entity
migration itself is the next step). Note `FileHistoryView` / `CommitPanel` /
`Sidebar` / `ConflictEditor` interactions drive `Backend` (diff load, stage,
checkout, resolve), so — unlike the pure-UI `Toasts` / `OpLogPanel` — their
Entity move needs a child→parent event/callback path, not just a field hoist.

### Step 5.2 — Worker thread per RepoSession
**Touch:** New `src/git/worker.rs` — one thread holding the `git2::Repository`,
serializing ops via a channel. `RepoSession` sends `Operation`s and awaits
results. Kills the per-op open pattern permanently.

### Step 5.3 — Build real view-models
**Touch:** Grow `src/ui/view_models/` from 213 LOC to cover `CommitGraphVM`,
`InspectorVM`, `DiffVM`, `ConflictVM`, `CommitDraftVM`, `SidebarVM`. Each is
plain data, unit-testable without GPUI/git2.

### Step 5.4 — Retire `headless.rs`
**Touch:** Delete the ~30 `KAGI_*` hooks that duplicate `tests/` coverage;
keep ~10 UI-state hooks. Route remaining through `Backend`, not raw git2.
Decompose `run_repo_flow` (1456 LOC). Eventually delete once Step 5.3 lands.

### Step 5.5 — Crate split (`kagi-ui`, `kagi-app`, `kagi-git`)
**Touch:** The final ADR-0072 move: extract `src/ui` → `crates/kagi-ui` (no
`git2` in `Cargo.toml`), `src/git` → `crates/kagi-git`, app state →
`crates/kagi-app`. Makes the git2-free UI a compile error.

---

## What should NOT be changed yet

- **`ActiveModal` enum** (`src/ui/modals.rs:3752`) — this is one of the two
  architectural wins that actually landed (ADR-0093). Keep it; only delete the
  dead accessors around it (Step 0.1).
- **`active_view` + `tab_cache`** (ADR-0095) — the other real win. Keep the
  shape; only bound `tab_cache` (LRU) later.
- **The `cx.background_spawn` + `cx.spawn` marshal-back pattern** in
  `src/ui/operations/*` — this is correctly idiomatic; the bug is only that
  the generation check is discarded (Step 1.1 / GPUI fix).
- **The `commit_row_index: HashMap<CommitId, usize>`** (`src/ui/mod.rs:1199`)
  and pre-baked `SharedString` display fields on `CommitRow` — the right
  structures; extend the pre-baking (Step 3.4), don't replace.
- **`menu_overlay.rs` and `button_style.rs`** — verified earning their keep
  (15+ call sites, real de-duplication). Leave them.
- **The avatar disk-cache + off-thread HTTP** (`src/ui/avatar_fetch.rs`) —
  well-engineered; leave it.
- **The `[kagi] …` log contract lines** — these are a test contract
  (AGENTS.md). Do not reword existing lines; only add new ones in the
  established format for new features.
- **Forbidden-op policy** (no `reset --hard`/`push --force`/`git clean`/
  `unsafe`) — holds; do not regress. All current "hits" are doc/comment/
  recovery text.

---

## Risks and rollback strategy

| Step | Risk | Rollback |
|---|---|---|
| 1.1 (enforced pipeline) | Medium — signature ripple through `operations/*` + `headless.rs` | Additive at `run()` layer first; old `execute_*` remain until callers migrate. Revert per-op sub-commits independently. |
| 1.3 (block dirty merge) | Low-Medium — behavior change users may notice | Keep the old warning path behind a setting for one release. |
| 1.4 (atomic conflict save) | Low | Pure restructure of an internal fn. |
| 2.1 (RepoSession) | Medium — large touch count | Mechanical per-file; revert per-file commits. |
| 2.2 (domain extraction) | Medium — logic moves between crates | Keep the `src/git/<x>.rs` shims delegating until tests pass; delete last. |
| 3.x (perf) | Low individually | Each is independent and reversible. |
| 5.1 (entity decomposition) | **High** — core architecture | Do one panel at a time; each panel is independently mergeable. Do not attempt before Phase 1. |
| 5.5 (crate split) | High — workspace restructure | Keep `src/` shims re-exporting from `crates/` during migration (the existing strangler pattern). |

**Global rollback:** Every step is one PR (or a short series of small
commits). `git revert` per step. The plan never requires a flag day.

**Test gate at every step:** `cargo fmt --all && cargo clippy --workspace &&
cargo test --workspace` must be green before pushing. UI-behavior steps
require a human to launch the app and confirm (subagents cannot exercise GPUI).

---

## Suggested commit cadence

- **Week 1:** Phase 0 (cleanup) + Phase 1.1 (enforced pipeline, op-by-op) +
  Phase 1.3 (dirty merge block) + Phase 1.4 (atomic conflict) + Phase 1.5
  (two-stage discard). These are the safety-critical, small, localized wins.
- **Week 2:** Phase 1.6 (oplog restore) + Phase 2.1 (RepoSession) +
  Phase 2.3–2.4 (helpers + test harness).
- **Week 3:** Phase 3.1–3.4 (the high-ROI perf wins) + Phase 4.1 (error
  classification).
- **Ongoing:** Phase 2.2 (domain extraction), Phase 4.2–4.7 (UX), Phase 5
  (structural) — interleaved with feature work, not blocking.

The single most important commit in this entire plan is **Step 1.1**. It
converts the safety thesis from a UI convention into a backend guarantee and
closes nine git-safety findings at once.
