# ADR-0147: Preflight compares a working-tree digest, not just HEAD

- Status: Accepted
- Date: 2026-09-02
- Closes: #295

## Context

The `plan → confirm → preflight → execute` pipeline's preflight compared only
HEAD (`checkout.rs preflight_check`). The blockers a plan raises from the
working tree — discard refusing a conflicted target, merge refusing a dirty
tree, stash refusing to apply onto changes — were reasoned at plan time and
never re-checked. The gap is reachable without an adversary: the two-stage
confirm holds a modal open for the user's whole thinking time, and none of the
dangerous transitions move HEAD.

- A conflicting merge does not move HEAD; `git rm --cached` does not; an
  interrupting edit does not. So a HEAD-only preflight passes a plan whose
  premises are gone.
- #280, #281, #282 each patched their own corner (stash's dirty re-check,
  discard's outcome plumbing). #295 is the shared floor those were standing on.

## Decision

`WorkingTreeStatus::digest()` (kagi-domain, pure) fingerprints the tree's
**classification**: each path with which group it is in (staged / unstaged /
untracked / conflicted) and its `ChangeKind`, sorted so the value is
order-independent. `OperationPlan` carries `worktree_digest: Option<…>` —
`Some` for the ops whose blockers depend on the tree (discard, merge, stash
apply/pop), `None` where only HEAD matters. `preflight_check` recomputes it and
refuses when it no longer matches.

Two design choices, held deliberately rather than taken from the issue text:

- **Classification, not content.** The digest has no file-content input, so
  editing a file that stays in the same group does not invalidate the plan —
  otherwise every keystroke in an open editor would. It changes exactly on the
  shifts that silently invalidate a blocker: a path entering/leaving a group,
  or moving between groups. All three of #295's reproductions are such shifts.
- **Whole tree, including untracked — accepting over-refusal.** A new untracked
  file that the operation would not touch still changes the digest and forces a
  re-plan. That is stricter than necessary for, say, a stash apply that ignores
  untracked files, but the cost is one re-plan and the alternative (a per-op
  notion of "which classifications this op cares about") is far more surface to
  get subtly wrong on a data-destroying path. Safety over convenience here.

`preflight_check_stash`'s hand-written dirty check (added for #280) is removed:
the digest subsumes it and additionally catches the untracked transition the
old check skipped. A standalone drop carries no digest, so it stays allowed on
a dirty tree, exactly as before.

Separately, `execute_discard` now records its target paths in the plan and
refuses any path the plan did not cover — a plan built for A can no longer be
replayed to discard B.

## Consequences

- The four #295 acceptance cases are enforced at the pipeline floor, so the
  per-op patches in #280/#281/#282 rest on something rather than each
  re-deriving it.
- Ops that carry a digest pay one extra `working_tree_status` walk at execute
  (already gated to those ops; a drop and every HEAD-only op skip it).
- Over-refusal on an unrelated untracked file is possible and intended; the
  message tells the user to re-plan.
- The digest uses `DefaultHasher` (deterministic within a build), which is all
  a same-process plan→execute needs; it is not stored across runs.
