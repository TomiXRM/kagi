# ADR-0141: The delete-branch plan is built off the UI thread

- Status: Accepted
- Date: 2026-08-24

## Context

`open_delete_branch_modal` called `plan_delete_branch` synchronously from a
`&mut self` GPUI handler. On an unmerged branch that plan runs the squash-merge
patch-id probe (ADR-0138): `squash_merged_as` walks `base..HEAD` computing a
per-commit tree diff, up to `SQUASH_SCAN_LIMIT = 500` diffs. ADR-0139 measured
the whole-repo variant at ~600ms on a 1100-commit repo; the single-branch probe
is a fraction of that, but it still lands in the tens-to-hundreds of ms — a
visible freeze, on every open of the modal, for exactly the branches (old fork
point, not reachable from HEAD) where the feature matters.

The probe is not cosmetic: it is what downgrades the `DeleteUnmerged`
**blocker** to a `DeleteSquashMerged` **warning**. A plan built without it is a
*blocked* plan.

## Decision

Build the whole plan on a background thread and open the modal only when it is
ready, exactly as `open_merge_modal` already does for the in-memory merge
dry-run (ADR-0079). `busy_op = Some("delete-branch-plan")` +
`FooterStatus::Busy` drive the existing spinner and block re-entry;
`cx.background_spawn` opens its own `Backend` (the per-tab `RepoSession` is not
`Send`), `cx.spawn` moves the finished plan back to the main thread and calls
`set_delete_branch_modal`.

### The "probe not finished" race cannot happen

The `DeleteBranchModal` is *constructed from* the completed `OperationPlan`.
There is no moment at which a modal exists holding a plan whose probe has not
run, so there is nothing for the user to confirm early:

- No modal → `start_delete_branch` / `confirm_delete_branch` both return at
  their `delete_branch_modal()` lookup. Enter/click do nothing.
- Modal present → its `plan` is the final one, probe included. The blocker
  check in `start_delete_branch` therefore sees the probe's verdict, never a
  provisional one.

The unsafe direction would be a plan that is *missing* a blocker; here the
probe can only *remove* one, and it has always finished before the plan is
reachable. If the background plan fails, no modal opens at all and the footer
shows the error — refusal, not execution.

`busy_op` also serialises: a second open while one is in flight is rejected
with `OpInProgress`, so two plans cannot race to set the modal.

## Alternatives rejected

- **Plan without the probe, show the modal, replan in the background.** The
  intermediate plan is blocked, so a fast Enter is *refused* rather than
  wrongly executed — but it means a user who acts quickly gets a spurious "this
  branch is unmerged" refusal recorded in the oplog for a branch that is in
  fact deletable, and the modal's blocker list visibly flips under the cursor.
  The `replan_X` convention in this codebase is for input-driven modals
  (`replan_create_branch`, `replan_rename_branch`), where the user is expected
  to keep typing; a confirm-only modal has no such editing phase to hide the
  latency in. More states, more code, worse UX.
- **Speculatively probing at refresh time and caching per branch.** Pays the
  cost for every branch instead of the one the user asked about, and needs
  invalidation on every ref change.

## Consequences

- The modal now appears after a short busy state instead of instantly. On
  merged branches (the common case) the probe does not run at all and the delay
  is imperceptible.
- One new contract line, `[kagi] async: delete-branch plan started for <name>`,
  matching the `async: merge plan started for …` precedent. The existing
  `[kagi] plan: delete-branch <name> blockers=N` line is unchanged in format
  and still precedes the modal.
