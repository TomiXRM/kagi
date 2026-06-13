# ADR-0079: Drag-and-Drop Branch Merge (Interaction Model + Layering)

- Status: Accepted / Date: 2026-06-14
- Context: New feature. Builds on the v1.0 layering (ADR-0072/0073/0078) and the
  existing merge pipeline (ADR-0052 merge/rebase direction, ADR-0056+ conflict mode).

## Decision

Add a **drag-and-drop gesture** in the sidebar `BRANCH / TAG` area to *start* (not
execute) a merge, GitKraken-style:

- A **local branch label** is draggable. During drag a ghost chip shows the dragged
  branch name.
- The **drop target (MVP)** is the **current (checked-out) branch** row. Dropping a
  different local branch onto it opens the **existing `MergePlanModal`** for
  "merge <dragged> into <current>" — i.e. `KagiApp::open_merge_modal(dragged)` →
  `Backend::plan_merge_branch(dragged)` (which already merges its argument into HEAD).
- **Drop only triggers the plan/preview.** Nothing is executed until the user clicks
  the explicit confirm button (`Merge <source> into <target>`). Cancel = no-op.
- Valid/invalid hover feedback: dragging a branch over a valid target highlights it;
  same-branch / non-current-target / remote / tag / detached HEAD are rejected — the
  drag layer pre-filters the obvious cases for hover styling, and
  `plan_merge_branch` produces blockers (execute button hidden) as the authoritative
  guard shown in the dialog.

### Layering (no Git in the view; reuse the safety pipeline)
1. **UI drag/drop** (`sidebar.rs`): branch labels `.on_drag(BranchDrag{name}, ghost)`;
   current-branch drop zone `.drag_over::<BranchDrag>(valid-style)` + `.on_drop::<BranchDrag>(…)`.
   The drop handler builds an intent and calls an app method — it does **not** call git.
2. **Action** (`KagiApp::start_merge_from_drag(source: String)`): the single entry the
   drop dispatches to. Pre-validates (source is a real local branch; source != current;
   not busy) and then delegates to `open_merge_modal(source)`.
3. **Planning** — **unchanged**: `Backend::plan_merge_branch` does the preflight
   (dirty WT, ff-vs-merge-commit, in-memory conflict prediction) and returns the plan.
4. **Execution / conflict** — **unchanged**: confirm → `execute_merge_branch` /
   `execute_merge_into_conflict` → on conflict, the existing Conflict Mode flow
   (3-pane resolver, abort/continue). Operation log + app-state refresh as today.

## なぜ
- Visual, direct branch operations (GitKraken/Zed-like) without sacrificing Kagi's
  safety thesis: the gesture is only a *trigger*; the proven plan→confirm→preflight→
  execute→verify pipeline still gates every write. Reusing `open_merge_modal` means
  zero new Git code in the view and no new execution path to make safe.

## 代替案 / 捨てた案
- **Execute on drop** — rejected outright (violates the safety thesis; the spec forbids it).
- **Drop onto an arbitrary branch label ("merge A into B")** — deferred (would require
  checking out B first or a detached merge; MVP targets the current branch only).
- **A new bespoke merge planner for DnD** — rejected; reuse `plan_merge_branch` so there
  is one merge path, one set of blockers, one tested pipeline.

## 将来の負債 / リスク
- MVP only merges into the current branch; branch→branch DnD (with target checkout) is
  future work. The `BranchDrag` payload + drop-target abstraction should generalize to
  rebase/cherry-pick DnD later.
- gpui 0.2.2 `drag_over`/`on_drop` exact signatures must be confirmed against the
  installed version (the codebase already uses `on_drag`/`on_drag_move`).
- Drag must not interfere with the existing right-click context menu or click-to-jump on
  branch rows (gesture disambiguation).

## Consequences
- New: `BranchDrag` payload type, a drag ghost view, a current-branch drop zone, and
  `KagiApp::start_merge_from_drag`. No change to the merge planning/execution/conflict
  layers. Branch/label rendering must not embed merge logic (only emit the intent).
- Tested: unit/integration coverage for `start_merge_from_drag` validation + the merge
  plan it produces (same-branch rejection, dirty-WT handling, ff vs merge-commit), on
  fixture repos.
