# Git Operation Safety Checklist

> Per-operation safety contract for every destructive or risky Git operation in
> Kagi. Each entry documents the **preflight checks**, **required user
> confirmation**, **expected safe behavior**, **error handling**, **recovery
> path**, and **UX requirements**.
>
> **Status legend**
> - ✅ = implemented and verified in code
> - ⚠️ = partially implemented / known gap (with `file:line`)
> - ❌ = missing (must be added)
> - 🚫 = intentionally absent by policy (the operation is forbidden)

> **Global invariant (must hold for every row below):** the operation flows
> through `Backend::run(op, &confirmed_plan)` which enforces
> `preflight_check` → dispatch → `verify_outcome` → `append_oplog`. See
> `/docs/codebase-review.md` §4 Finding 2 and `/docs/refactor-plan.md` Step 1.1
> — today this is a UI convention, not a backend guarantee. **Every ⚠️/❌ in
> the "preflight" column below is a symptom of that gap.**

---

## Operation index

| # | Operation | File | Current safety |
|---|---|---|---|
| 1 | [checkout (branch)](#1-checkout-branch) | `src/git/ops/checkout.rs` | ⚠️ |
| 2 | [checkout (detached commit)](#2-checkout-detached-commit) | `src/git/ops/checkout.rs` | ⚠️ |
| 3 | [reset](#3-reset) | — | 🚫 (policy) |
| 4 | [revert](#4-revert) | `src/git/ops/cherry_revert.rs` | ⚠️ |
| 5 | [merge](#5-merge) | `src/git/ops/merge.rs` | ⚠️ |
| 6 | [rebase](#6-rebase) | — | ❌ (stub) |
| 7 | [cherry-pick](#7-cherry-pick) | `src/git/ops/cherry_revert.rs` | ⚠️ |
| 8 | [stash push](#8-stash-push) | `src/git/ops/stash.rs` | ✅ / ⚠️ |
| 9 | [stash apply](#9-stash-apply) | `src/git/ops/stash.rs` | ⚠️ |
| 10 | [stash pop](#10-stash-pop) | `src/git/ops/stash.rs` | ⚠️ |
| 11 | [stash drop](#11-stash-drop) | `src/git/ops/stash.rs` | ⚠️ |
| 12 | [branch delete](#12-branch-delete) | `src/git/ops/branch.rs` | ✅ |
| 13 | [discard file changes](#13-discard-file-changes) | `src/git/ops/discard.rs` | ⚠️ |
| 14 | [amend commit](#14-amend-commit) | `src/git/ops/history.rs` | ⚠️ |
| 15 | [undo commit (soft reset of HEAD)](#15-undo-commit) | `src/git/ops/history.rs` | ⚠️ |
| 16 | [force push / force-with-lease](#16-force-push--force-with-lease) | — | 🚫 (policy) |
| 17 | [push](#17-push) | `src/git/ops/pull_push.rs` | ⚠️ |
| 18 | [pull](#18-pull) | `src/git/ops/pull_push.rs` | ⚠️ |
| 19 | [conflict resolution save/apply](#19-conflict-resolution-save--apply) | `src/git/conflicts.rs` | ⚠️ |
| 20 | [conflict abort](#20-conflict-abort) | `src/git/ops/merge.rs` | ⚠️ |
| 21 | [create worktree](#21-create-worktree) | `src/git/ops/worktree.rs` | ⚠️ |
| 22 | [create branch](#22-create-branch) | `src/git/ops/branch.rs` | ✅ |

---

## 1. checkout (branch)

**Current state: ⚠️** — plan exists and is accurate; execute has no preflight
and no verify. Safe-mode checkout is used (no force).

### Preflight checks
- ✅ Plan-time dirty-tracked-file prediction via `predict_checkout_conflict`
  (`checkout.rs:362-415`).
- ✅ In-progress conflict/rebase/cherry-pick state blocks
  (`plan_checkout` via `ops/mod.rs`).
- ⚠️ **Untracked-file overwrite is NOT predicted.** `predict_checkout_conflict`
  explicitly excludes untracked files (`checkout.rs:127-132`); the plan says
  "will remain" but if the target tracks the same path, `cb.safe()` aborts at
  execute with a raw libgit2 error.
- ⚠️ **`predict_checkout_conflict` fails open** — every step uses `.ok()?`
  (`checkout.rs:362-415`), so any analysis error suppresses the blocker.
- ❌ **No execute-time preflight.** `execute_checkout(repo, branch)`
  (`checkout.rs:216-243`) takes no `&OperationPlan`; `head_at_plan` is never
  compared to reality at execute time.

### Required user confirmation
- ✅ Plan modal (current → predicted state, warnings/blockers/recovery).
- ✅ Execute button hidden when `!blockers.is_empty()`.

### Expected safe behavior
- ✅ `CheckoutBuilder::safe()` is the only strategy (`checkout.rs:232`). Never
  `cb.force()`.
- ✅ Conflicted-state blocks the op.
- ✅ Detached/unborn HEAD handled.

### Error handling
- ⚠️ Safe-mode abort surfaces a raw libgit2 string (e.g. `checkout_tree
  failed`) with no remediation guidance.

### Recovery path
- ✅ None needed normally — safe-mode aborts before any WT change.
- ⚠️ Oplog entry written only by the UI path; **not** by `Backend::execute`
  (see global invariant).

### UX requirements
- ⚠️ Untracked-overlap should be a plan-time **blocker** naming the path, not
  a soft warning.
- ⚠️ Errors need friendly text ("would overwrite untracked file X; move or
  delete it first").

### Failure scenario this must handle
> User has untracked `notes.txt`. Target branch tracks `notes.txt`. Plan says
> "1 untracked file(s) will remain." Click Execute → `checkout_tree failed`
> with no guidance. **Fix:** predict the overlap, emit a blocker.

---

## 2. checkout (detached commit)

**Current state: ⚠️** — same shape as branch checkout. `execute_checkout_commit`
(`checkout.rs`) takes no plan; safe-mode used.

### Preflight checks
- ⚠️ Same gaps as branch checkout (no execute-time preflight, fail-open
  prediction).
- ✅ Warns that HEAD will detach.

### Required user confirmation
- ✅ Plan modal with detachment warning.

### Expected safe behavior
- ✅ `cb.safe()` only.
- ⚠️ No auto-return-to-branch affordance after detached work (a common
  footgun: user commits on detached HEAD, loses the ref).

### Error handling / Recovery / UX
- Same as §1.
- ⚠️ **Should warn at plan time if the target is already reachable from a
  branch tip** and offer "checkout branch X instead".

---

## 3. reset

**Current state: 🚫 (intentionally absent by policy).**

- `reset --hard`, `reset --soft`, `reset --mixed` are **not implemented**.
  All "hits" in `src/` are documentation, comments, or recovery text (verified
  by grep + manual review — see `/docs/codebase-review.md` §1).
- ADR-0024 documents reset semantics as a future, carefully-scoped feature.
  The menu stub `ResetCurrentToHead` / `ResetToCommit` is permanently
  disabled (`BcmNotImplementedYet`).

### When (if ever) implemented, it must:
- Preflight: dirty WT (for `--hard`), unborn HEAD, conflict state, **pushed
  commits** (block `--hard`/`--soft` that rewrite pushed history).
- Confirmation: **two-stage** (it is `destructive: true`).
- Recovery: pre-op HEAD SHA written to oplog; "Restore" action runs
  `git reset <sha>`.
- UX: red modal, explicit "this moves the branch pointer back by N commits,
  commits X..Y become unreachable."

---

## 4. revert

**Current state: ⚠️** — plan blocks dirty tree and conflicts; execute has no
preflight and risks a dangling commit object.

### Preflight checks
- ✅ Plan-time dirty-tree blocker (`cherry_revert.rs:84-104`, shared with
  cherry-pick).
- ✅ Conflict-state block.
- ⚠️ **No execute-time preflight.** `execute_revert(repo, id)`
  (`cherry_revert.rs:817-928`) takes no plan; plan-time dirty check is never
  re-evaluated.

### Required user confirmation
- ✅ Plan modal.
- ⚠️ No diff preview in the modal — only a file list (`modals.rs:2839-2905`).

### Expected safe behavior
- ✅ Uses `repo.revert_commit(...)` (in-memory) → commit → `checkout_head(safe())`.
- ⚠️ Commit object is created **before** the safe-checkout; if safe-checkout
  aborts (dirty file appeared), the revert commit is **dangling** and the ref
  is never moved.

### Error handling
- ⚠️ `checkout_head failed` surfaces raw; user doesn't know their dirty file
  blocked it.

### Recovery path
- ⚠️ Dangling commit is recoverable via reflog/fsck but the UI doesn't say so.

### UX requirements
- ❌ **Add diff preview** to the modal (the negation is already computed
  in-memory).
- ⚠️ Localize blocker/recovery strings.

### Failure scenario this must handle
> User plans revert. A file is modified by another tool before confirm.
> Execute: commit created → safe-checkout aborts → ref not moved → "failed"
> toast. User cleans and retries → may revert twice. **Fix:** preflight +
  dirty re-read at top of execute, before creating any object.

---

## 5. merge

**Current state: ⚠️** — **the most dangerous path.** Plan only *warns* on
dirty WT; execute writes conflict markers into uncommitted files.

### Preflight checks
- ⚠️ **Dirty WT only warns** (`merge.rs:26` `merge_dirty_warnings`). Cherry-pick
  *blocks* on dirty; merge does not — inconsistency.
- ✅ Conflict-state block.
- ✅ Ancestry checks prevent no-op / already-merged.
- ⚠️ No execute-time preflight; no dirty-tree re-read.

### Required user confirmation
- ✅ Plan modal (warnings for dirty WT).
- ⚠️ No merge-commit message editor before the 2-parent commit lands
  (`conflict.rs:435-444` uses the default message).

### Expected safe behavior
- ✅ FF merge when possible (`execute_merge_branch`, `merge.rs:227-343`).
- ✅ Writes `ORIG_HEAD` before real-merge path (`merge.rs:393-400`).
- ⚠️ `execute_merge_into_conflict` writes conflict markers into a possibly-dirty
  WT. `ORIG_HEAD` restores the **ref**, not the WT.

### Error handling
- ⚠️ Mid-merge libgit2 errors leave `MERGE_HEAD` + stage entries; `git merge
  --abort` is the recovery and is **not surfaced**.

### Recovery path
- ⚠️ `execute_conflict_abort` exists — verify it does a full WT/index reset to
  ORIG_HEAD, not just a ref move (`merge.rs` comment overstates).

### UX requirements
- ❌ **Block dirty WT** (mirror cherry-pick) — at minimum for the
  into-conflict path.
- ❌ **Surface `git merge --abort`** as the abort action.
- ⚠️ Merge-commit message editor.

### Failure scenario this must handle
> User has unstaged edits to `auth.rs`. Clicks Merge on a branch whose changes
> also touch `auth.rs`. Plan shows a yellow warning. On confirm, conflict
> markers are written *into the user's unsaved `auth.rs` edits*. `git merge
> --abort` would discard both the merge AND the pre-merge edits. **Fix:**
> block dirty merge; surface `--abort`.

---

## 6. rebase

**Current state: ❌ (stub).** Menu item `RebaseCurrentOnto` permanently
disabled (`branch_menu.rs:197,477-487`).

### When implemented, it must:
- Preflight: dirty WT (block), conflict state (block), detached HEAD (block).
- Confirmation: plan modal showing the rebase plan (which commits will be
  replayed, which dropped if already applied).
- Expected behavior: use `git rebase` via the CLI adapter (the existing
  conflict editor already handles `ConflictOp::Rebase`, `conflict_view.rs:191`).
- Error handling: surface `git rebase --abort` / `--skip` / `--continue`
  explicitly in conflict mode.
- Recovery: pre-rebase HEAD SHA in oplog; "Undo rebase" runs `git reset
  --hard <pre-sha>` (the one justified use of reset --hard, behind a confirm).
- UX: interactive rebase todo list (reorder/squash/drop/reword) with preview
  — the flagship differentiator for a safety-first client.

---

## 7. cherry-pick

**Current state: ⚠️** — plan is solid; execute has the same dangling-commit
risk as revert.

### Preflight checks
- ✅ Plan-time dirty-tree blocker (`cherry_revert.rs:84-104`).
- ✅ Conflict-state block.
- ⚠️ No execute-time preflight (same as revert).

### Required user confirmation
- ✅ Plan modal.
- ⚠️ No diff preview (`modals.rs:2839-2905`).

### Expected safe behavior
- ✅ Uses `repo.cherrypick_commit(...)` (in-memory, no CHERRYPICK state).
- ⚠️ Same dangling-commit risk as revert (`cherry_revert.rs:423-505`):
  commit created → safe-checkout → ref moved last.

### Error handling / Recovery / UX
- Same as §4 (revert).

---

## 8. stash push

**Current state: ✅ / ⚠️** — generally safe (creates a stash, non-destructive
to WT aside from the intended save).

### Preflight checks
- ✅ Clean WT → warning ("nothing to stash").
- ✅ Conflict state handling.
- ⚠️ No execute-time preflight.

### Required user confirmation
- ✅ Plan modal (shows what will be stashed).

### Expected safe behavior
- ✅ `repo.stash_save(...)` with `StashFlags` — default flags save
  tracked modifications + staged; untracked opt-in.

### Error handling / Recovery / UX
- ✅ Stash is always recoverable (`stash@{0}`..`stash@{n}`).
- ⚠️ Localize blocker/recovery.

---

## 9. stash apply

**Current state: ⚠️** — non-destructive (no drop), but no conflict prediction
in plan.

### Preflight checks
- ⚠️ **No conflict prediction** in `plan_stash_apply` (exists only for pop via
  `predict_stash_pop_conflict`). Apply onto a dirty tree that overlaps the
  stash → surprise conflicts at execute.
- ⚠️ No execute-time preflight.

### Required user confirmation
- ✅ Plan modal.

### Expected safe behavior
- ✅ `repo.stash_apply(index, None)` — no force flag; conflicts abort, stash
  retained.
- ⚠️ **Index not reinstated** (`StashApplyOptions` is `None`) — staged
  distinctions lost on apply.

### Error handling / Recovery / UX
- ✅ Apply never drops, so always recoverable.
- ❌ **Add "apply with `--index`"** option (`StashApplyOptions::
  with_reinstate_index(true)`) behind a checkbox.
- ❌ **Add stash preview** (`git stash show -p`).

---

## 10. stash pop

**Current state: ⚠️** — apply-then-drop-on-success is the right design, but
execute takes no plan and does not re-verify the stash index.

### Preflight checks
- ✅ Conflict prediction in `plan_stash_pop` (`predict_stash_pop_conflict`,
  `stash.rs:721`).
- ⚠️ **No execute-time preflight** (`execute_stash_pop(repo, index)`,
  `stash.rs:534-543`). `preflight_check_stash` exists but is not called.

### Required user confirmation
- ✅ Plan modal.

### Expected safe behavior
- ✅ Apply first, drop **only on success** (`stash.rs:516-528`). Conflicts →
  stash retained.

### Error handling
- ⚠️ If a stash was pushed between plan and execute, the index shifts and pop
  applies+drops the **wrong entry**.

### Recovery path
- ⚠️ Dropped stash is recoverable via `stash` reflog (`git stash list --reflog`
  equivalent) but not surfaced.

### UX requirements
- ❌ **Take `&OperationPlan`** + call `preflight_check_stash` first
  (`stash_count_at_plan`).
- ⚠️ Localize.

### Failure scenario this must handle
> User plans pop of `stash@{1}`. Another tool pushes a stash. User confirms.
> Execute pops what is now `stash@{1}` = a different entry, applies and drops
> it. **Fix:** preflight stash-count check.

---

## 11. stash drop

**Current state: ⚠️** — destructive, but the backend function takes no plan.

### Preflight checks
- ❌ `execute_stash_drop(repo, index)` (`stash.rs:693-704`) is public, takes no
  plan, no preflight. `plan_stash_drop` sets `destructive: true` (`stash.rs:684`)
  but execute does not enforce it.

### Required user confirmation
- ✅ UI modal (ADR-0087 danger-confirm). **UI-only** — backend enforces nothing.

### Expected safe behavior
- 🚫 No "safe" variant — drop is permanent deletion of the stash ref.

### Error handling
- ⚠️ Index shift risk (same as pop).

### Recovery path
- ⚠️ Stash reflog (`refs/stash@{n}` reflog) — not surfaced in UI.

### UX requirements
- ❌ **Take `&OperationPlan`** marked `destructive`; `preflight_check_stash`;
  two-stage confirm (mirror amend).
- ❌ **"Restore dropped stash"** oplog action (`git stash store`).

---

## 12. branch delete

**Current state: ✅** — the gold-standard implementation alongside discard.
Backup-then-mutate, preflight, verify.

### Preflight checks
- ✅ `plan_delete_branch` + `preflight_check` + `execute_delete_branch` all
  wired and take the plan.
- ✅ **Merged-only guard:** only branches whose tip is reachable from HEAD may
  be deleted; unmerged → blocker.
- ✅ Re-validates at execute (`backend.rs:416-420` re-plans inline).

### Required user confirmation
- ✅ Plan modal.

### Expected safe behavior
- ✅ `Branch::delete()` — ref-only, **never** touches WT. **Force delete
  intentionally absent.**

### Error handling / Recovery / UX
- ✅ Pre-delete tip SHA captured → recovery text "restore with
  `git branch <name> <sha>`".
- ⚠️ No in-app "Restore" button (recovery is CLI-only).
- ⚠️ Delete uses the same neutral card as safe ops — should escalate visually
  for unmerged-attempt (blocked anyway) and be red for merged-delete
  (destructive).

---

## 13. discard file changes

**Current state: ⚠️** — backup-then-delete design is sound; verify is
inconsistent and confirm is single-click.

### Preflight checks
- ✅ `plan_discard` + `preflight_check` + `execute_discard` wired (`discard.rs`).
- ✅ Conflicted targets rejected at plan time.
- ✅ Re-plans inline at execute (`backend.rs:421-425`).

### Required user confirmation
- ❌ **Single-click executes.** Only amend has the two-stage `confirm_armed`
  gate. Discard is strictly more destructive than amend.

### Expected safe behavior
- ✅ Tracked files: `checkout_index` path (the `git checkout -- <path>`
  semantic) — never force-checkout, never reset.
- ✅ Untracked files: backup as blob (`repo.blob(&content)`) →
  `std::fs::remove_file` → prune empty parents.
- ✅ Verify re-reads status (`discard.rs:269-302`).

### Error handling
- ⚠️ **Verify can return `Err` after files are already deleted** (for untracked
  targets). Outcome says "failed" but the WT is mutated.

### Recovery path
- ✅ Backup blobs written to ODB; oplog records the list (`discard.rs:110-114,
  192-219`).
- ❌ **No in-app restore** — requires `git cat-file -p <blob>` in a terminal.
- ⚠️ Empty-dir pruning (`discard.rs:259-267`) does not consult `.gitignore`.

### UX requirements
- ❌ **Two-stage confirm** (mirror amend's `confirm_armed`).
- ❌ **"Restore discarded files"** oplog action (`git cat-file -p <blob>` → write).
- ⚠️ Return `DiscardOutcome::Partial { deleted, remaining, backups }` instead
  of `Err` for partial success.
- ❌ **Per-hunk discard** (currently whole-file only).

### Failure scenario this must handle
> User clicks "Discard all" by mistake → every unstaged change gone. Backup
> exists but recovery is `git cat-file -p <sha>` in a terminal. **Fix:**
> two-stage confirm + in-app restore.

---

## 14. amend commit

**Current state: ⚠️** — two-stage confirm is the model for the codebase; but
the pushed-check is plan-time only.

### Preflight checks
- ✅ Plan-time "already pushed" blocker (`history.rs:448-471`).
- ⚠️ **No execute-time re-check.** A push between plan and execute amends
  published history.
- ⚠️ No execute-time preflight (`Backend::execute_amend` takes `mode` +
  `message`, no plan).

### Required user confirmation
- ✅ **Two-stage** `confirm_armed` (`modals.rs:103,732-784`). The model for
  discard (§13) and stash-drop (§11).
- ✅ `destructive: true` on the plan.

### Expected safe behavior
- ✅ Ref move only (no reset/clean); `history.rs:888` documents this.

### Error handling / Recovery
- ✅ Old HEAD SHA captured; recovery text "restore via reflog / `reset --hard
  <old>`" (the one place `reset --hard` appears — in user-facing recovery
  instructions).

### UX requirements
- ⚠️ **Re-verify pushed at execute** (or capture upstream tip in the plan).
- ⚠️ **Wrong recovery command** in `staging.rs:512-522` — says `git revert HEAD`
  to "undo while keeping staged"; correct is `git reset --soft HEAD~1`.

---

## 15. undo commit

**Current state: ⚠️** — soft-undo of HEAD; pushed-check is plan-time only.

### Preflight checks
- ✅ Plan-time "already pushed" blocker (`history.rs:146-171`).
- ✅ Root/merge-commit guard.
- ⚠️ **Not re-checked at execute** (`history.rs:269-336`).

### Required user confirmation
- ✅ Plan modal.

### Expected safe behavior
- ✅ Moves branch ref back one commit; WT/index untouched (soft semantics).
- ✅ ADR-0011 "never undo a pushed commit."

### Error handling / Recovery / UX
- ✅ Pre-undo HEAD SHA in oplog.
- ⚠️ Localize.
- ❌ **Re-verify pushed at execute.**

---

## 16. force push / force-with-lease

**Current state: 🚫 (intentionally absent by policy).**

- No `--force` or `--force-with-lease` in any `run_git` args (verified — the
  single `push ... --force` hit is a doc comment at `pull_push.rs:1401`).
- `ForceWithLeasePush` menu item is a permanent stub (`branch_menu.rs:276`).

### Rationale
Force push rewrites published history — the exact footgun a safety-first
client exists to prevent. Even `--force-with-lease` can lose a collaborator's
push if the lease base is stale.

### If ever reconsidered
- Default: **never**. If added, must be: two-stage confirm, `--force-with-lease`
  only (never bare `--force`), protected-branch block, behind a setting
  disabled by default, with a diff of "what the remote will lose."

---

## 17. push

**Current state: ⚠️** — force genuinely absent (good); no local divergence
pre-check.

### Preflight checks
- ✅ Force never used (`pull_push.rs:1482-1486,1954-1958` build plain `push`).
- ✅ Detached/unborn blocks.
- ⚠️ **No local ahead/behind check** — relies on the remote to reject
  non-fast-forward, surfacing raw stderr.

### Required user confirmation
- ✅ Plan modal with `preview_commits` (`pull_push.rs` plan).
- ⚠️ No "push will be rejected" plan-time blocker when `behind > 0`.

### Expected safe behavior
- ✅ Non-mutating to local state.
- ✅ Set-upstream flow for upstream-less branches.

### Error handling
- ⚠️ Raw CLI stderr surfaced (`pull_push.rs:1492-1498`).

### Recovery path
- N/A (push failure leaves local intact).

### UX requirements
- ❌ **Add plan-time blocker** when `behind > 0` ("branch is behind upstream;
  pull first").
- ⚠️ Friendly auth/network error classification.

---

## 18. pull

**Current state: ⚠️** — fetch-then-merge/FF; dirty-path guard exists; no
strategy choice.

### Preflight checks
- ✅ `ensure_pull_does_not_touch_dirty_paths` guard (`pull_push.rs:816`).
- ✅ Conflict-state block.
- ⚠️ Conflict prediction is advisory only ("Execute is NOT blocked") because
  fetch may change the tip.

### Required user confirmation
- ✅ Plan modal.
- ❌ **No strategy choice** (merge / rebase / ff-only). Hard-decides FF-else-merge.

### Expected safe behavior
- ✅ Fetch via CLI (non-mutating), then in-memory merge/FF (no MERGING state).
- ✅ Dirty-path overlap preflight (ADR-0100).

### Error handling / Recovery
- ⚠️ Post-merge recovery text recommends `git reset --hard HEAD~1`
  (`pull_push.rs:614`) — correct for undoing a merge commit.

### UX requirements
- ❌ **Strategy selector** defaulting to repo's `pull.rebase`.
- ⚠️ Localize.

---

## 19. conflict resolution save / apply

**Current state: ⚠️** — non-atomic write loop.

### Preflight checks
- ✅ All session files must have a resolution before continue (defensive
  marker check).

### Required user confirmation
- ✅ Continue modal.

### Expected safe behavior
- ⚠️ **Non-atomic write loop** (`conflicts.rs:931-984`): `fs::write` per file
  → `index.add_path` per file → `index.write()` once at end. A mid-loop write
  failure leaves WT ≠ index and loses the original markers for files already
  written.

### Error handling
- ⚠️ Partial-write error message doesn't distinguish "nothing happened" from
  "some files overwritten, index not written."

### Recovery path
- ❌ None for the partial-write case.

### UX requirements
- ❌ **Atomic save:** write all to temp paths, rename atomically, stage, write
  index once; roll back temps on any failure.
- ❌ **"Mark resolved"** per file (calls `stage_conflict_resolution`) —
  currently resolution is implicit.
- ⚠️ Localize marker-presence warnings.

### Failure scenario this must handle
> User resolves 5 files, clicks Continue. File 3's directory is read-only →
> `fs::write` fails after files 1–2 already overwritten. Index never written.
> WT now has partial resolutions + original markers; index shows all
> conflicting. **Fix:** atomic temp-write-then-rename.

---

## 20. conflict abort

**Current state: ⚠️** — exists; rollback scope unclear.

### Preflight checks
- N/A (abort is the recovery, not a new mutation).

### Required user confirmation
- ✅ Abort modal.

### Expected safe behavior
- ⚠️ **Verify `execute_conflict_abort` does a full WT/index reset to
  ORIG_HEAD**, not just a ref move. The comment at `merge.rs:355-358`
  overstates what `ORIG_HEAD` alone guarantees — it restores the *ref*, not
  WT files. True abort needs `git merge --abort` semantics (ref + WT + index +
  drop `MERGE_HEAD`).

### Error handling / Recovery / UX
- ⚠️ If abort is mid-conflict after manual edits, those edits are discarded —
  must be loud about it ("aborting discards your resolution edits").
- ⚠️ Abort during cherry-pick/rebase conflict must run the right `--abort`
  (`cherry-pick --abort` / `rebase --abort`), not just a merge abort.

---

## 21. create worktree

**Current state: ⚠️** — non-atomic (creates branch then worktree).

### Preflight checks
- ✅ `plan_create_worktree` + path validation (`worktree.rs`).
- ⚠️ No execute-time preflight.

### Required user confirmation
- ✅ Plan modal.

### Expected safe behavior
- ⚠️ **Non-atomic:** `execute_create_branch` then `repo.worktree(...)` — if
  the worktree creation fails (disk full, permission), the branch ref is left
  behind (`worktree.rs:301-337`).

### Error handling / Recovery
- ⚠️ Orphan branch left on partial failure; user must `git branch -d` manually.

### UX requirements
- ❌ **Roll back the branch** on worktree-creation failure if it did not
  pre-exist.

---

## 22. create branch

**Current state: ✅** — safe, simple, well-tested.

### Preflight checks
- ✅ `is_valid_name` rejects leading `-`, spaces, invalid refnames
  (`branch.rs:23-31`).
- ✅ Existing-name check.

### Required user confirmation
- ✅ Plan modal (safe class).

### Expected safe behavior
- ✅ `repo.branch(name, &commit, false)` — **`false` is a literal constant**
  (`ops/mod.rs:18-19`); force-create is impossible.

### Error handling / Recovery / UX
- ✅ Non-destructive (new ref only).
- ⚠️ `execute_create_branch` does not re-validate the name (trusts the plan);
  minor since the UI validates. Defense-in-depth: re-validate.

---

## Cross-cutting requirements (apply to every operation)

### Global pipeline (today a UI convention — see refactor Step 1.1)
Every mutating operation MUST flow through:
```
plan → confirm (modal) → preflight (HEAD/stash-count unchanged) →
  execute → verify → append_oplog
```
in **one** backend method (`Backend::run`), so no caller (UI, headless,
test, automation) can bypass it.

### Oplog
- Every mutating op appends an entry with: op kind, timestamp, pre-op HEAD SHA,
  recovery handle (blob SHA / ref SHA / stash reflog id).
- The Operation Log panel offers a **Restore** action for discard and
  delete-branch (and future destructive ops).

### Forbidden operations (policy — must never appear in code)
- `reset --hard`, `reset --mixed`, `reset --soft` (except as user-facing
  *recovery text* — the only acceptable form)
- `push --force`, `push -f`, `--force-with-lease`
- `git clean`, `clean -fd`
- `Branch::delete` with force=true
- `CheckoutBuilder::force()` / `cb.force(true)`
- `unsafe` blocks
- `cherrypick` (working-tree variant) — only `cherrypick_commit` (in-memory)

### Error reporting (UX)
- Every failure surfaces as a **persistent error toast** (not a footer line
  that gets overwritten).
- Raw `git2`/CLI strings are wrapped by an error-classification layer into
  typed messages with a suggested next action; the raw string is available
  in an expandable "details."

### Confirmation escalation
- `plan.destructive == true` → **two-stage confirm** (the amend `confirm_armed`
  pattern) and a red-bordered modal.
- Applies to: discard, amend, stash drop, branch delete, undo (history-rewriting).
- Plan-time blockers (red) hide the Execute button; warnings (yellow) do not.

### Dirty-tree policy (must be consistent across ops)
| Op | Dirty-tracked behavior | Dirty-untracked behavior |
|---|---|---|
| checkout | warn (predict conflict) | warn (will remain) — but block on overlap |
| cherry-pick | **block** | warn |
| revert | **block** | warn |
| merge | **block** (currently only warns — Fix) | warn |
| pull | block on path overlap (ADR-0100) | warn |
| stash push | n/a (that's the point) | opt-in |
| stash pop/apply | warn (predict conflict) | warn |
| discard | n/a (that's the point) | n/a |
| amend | n/a (WT-aware) | n/a |

The current inconsistency — **merge warns where cherry-pick blocks** — is the
single highest-priority safety fix.

---

## Quick reference: severity-ranked open items

| Priority | Item | Op | Fix |
|---|---|---|---|
| P0 | Enforce pipeline in `Backend::run` | all | refactor Step 1.1 |
| P0 | Block dirty merge | merge | add blocker in `plan_merge_branch` |
| P0 | Atomic conflict save | conflict §19 | temp-write-then-rename |
| P0 | Two-stage discard confirm | discard §13 | port `confirm_armed` |
| P1 | Stash pop/drop preflight | stash §10/11 | take `&OperationPlan` + `preflight_check_stash` |
| P1 | Cherry-pick/revert preflight + dangling fix | §4/§7 | take plan + dirty re-read before commit |
| P1 | Undo/amend pushed re-check | §14/§15 | re-verify at execute |
| P1 | Oplog Restore UI | discard/delete | oplog row action |
| P2 | Checkout untracked-overlap blocker | checkout §1 | extend `predict_checkout_conflict` |
| P2 | Error classification + persistent toasts | all | new `error_classify.rs` |
| P2 | Diff preview in cherry-pick/revert modal | §4/§7 | render in-memory diff |
| P2 | Per-hunk staging/discard | staging/discard | `git apply --cached` |
| P3 | Pull strategy selector | pull §18 | `PullStrategy` enum + modal dropdown |
| P3 | Stash `--index` + preview | stash §9/§10 | `StashApplyOptions` + `stash show -p` |
| P3 | Localize plan-modal strings | all | `Msg` keys |
| P3 | Worktree atomicity | worktree §21 | roll back branch on failure |
