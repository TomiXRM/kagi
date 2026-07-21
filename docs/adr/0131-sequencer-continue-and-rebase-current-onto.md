# ADR-0131: Sequencer Continue Advances via CLI + Rebase Current Onto

- Status: Accepted
- Date: 2026-07-21

## Context

Implementing the branch-menu "Rebase current onto <target>..." item (the
last of the 8 stubs from this round, after #196–#203) surfaced a pre-existing
gap: `execute_conflict_continue` for a sequencer op (rebase / cherry-pick /
revert) only staged the resolved buffer and returned `ContinueOutcome::Staged`
— it never actually committed the resolved step and advanced to the next
one. The doc comment said as much: "driving the sequencer forward... belongs
to the dedicated sequence executors, which a later lane wires." Shipping
"start a rebase" on top of that gap would mean a user who hit a conflict
mid-rebase had no way to finish it from kagi's UI — a genuinely broken state,
not merely an unimplemented convenience.

## Decision

### Sequencer continue now advances (fixes the gap)

`execute_conflict_continue`, after staging the resolution, shells out
`git <op> --continue` (`run_git`, matching every other CLI-driven op in this
codebase — push/pull/fetch) for rebase / cherry-pick / revert. Reasons:

- libgit2 exposes no "continue a sequence" API; reimplementing rebase's step
  machine (commit-with-original-message, advance msgnum, re-apply the next
  patch, detect a fresh conflict, finish and update the branch ref) on top of
  the lower-level `Rebase`/`RebaseOperation` types would duplicate real git's
  own sequencer for no benefit, and would be a second implementation to keep
  correct against edge cases (empty commits, `--merge` vs `--apply` backends,
  autosquash, etc.) that upstream git already handles.
- The outcome is read from `repo.state()` after the CLI call, not the exit
  code: a non-zero exit almost always means the sequence stopped again at a
  (possibly new) conflict, which is expected, not a failure. `state() ==
  Clean` means the whole sequence finished (`ContinueOutcome::Committed`);
  anything else means it's still in progress (`ContinueOutcome::Staged`), and
  the existing `detect_conflict_session` / reload path picks up whatever
  comes next — no new routing logic needed.
- `execute_conflict_continue` gained a `repo_path: &Path` parameter (mirrors
  `execute_push(repo, repo_path)`) since the CLI call needs a working
  directory the `git2::Repository` handle doesn't expose directly.

Verified by `tests/conflicts_test.rs::execute_continue_rebase_advances_and_finishes`
and `::execute_continue_cherry_pick_advances_and_finishes` (both build a real
conflict, resolve it, call continue, and assert the sequence actually
finished with the right content in the resulting commit — not just staged).

### Rebase current onto

- Guarded-class (ADR-0004), not Destructive: no armed two-stage confirm.
  Rebase only ever rewrites the *local* branch (before it's pushed anywhere),
  and — unlike reset/force-push/delete-remote-branch — a rebase that goes
  wrong routes into the existing conflict editor (with its own Abort) rather
  than silently discarding anything.
- Starts via `git rebase <onto>` (CLI), for the same "don't duplicate git's
  own machinery" reason as the continue fix above.
- Unlike `merge`'s in-memory-merge conflict prediction (a single merge is
  cheap to predict with `git2::Repository::merge_trees`), a multi-commit
  rebase's conflict — if any — can't be cheaply predicted ahead of time
  without literally running the rebase. `plan_rebase_current_onto` therefore
  carries an unconditional `RebaseNote::MayConflict` warning instead of a
  merge-style `MergeKind::Conflicts` prediction; `execute_rebase_current_onto`
  reports `RebaseOutcome::Conflicted` (not an `Err`) when the repo is left
  mid-rebase, and the UI's `reload()` — which unconditionally re-runs
  conflict-mode detection — picks it up exactly the way a conflicting merge
  already does.
- Scoped to the checked-out branch only, mirroring the menu's existing
  `rebase_label` ("Rebase `<current>` onto `<clicked row>`"); disabled when
  the clicked row *is* the current branch (rebasing onto self is a blocker
  in the plan anyway — `RebaseNote::AlreadyUpToDate`).

## Consequences

- Cherry-pick and revert conflicts also gained a working Continue as a side
  effect of fixing the shared code path — not just rebase.
- Any future sequencer feature (e.g. an interactive multi-commit rebase UI)
  can keep relying on the CLI's own step machine rather than needing to
  track libgit2's lower-level `Rebase` API.
