# ADR-0151: Absorb — fold uncommitted hunks into their mutable ancestors

- Status: Accepted
- Date: 2026-09-03
- Implements: issue #345 (tracking #359)
- Builds on: ADR-0045 (fixup concept), ADR-0143 (pushed-history judgment),
  ADR-0002 (git2 single backend), ADR-0149 (oplog in `Backend::run`)

## Context

Agents produce "fix a little bit everywhere" changes. Sorting those hunks into
the right existing commit by hand is drudgery. `git-absorb` / `jj absorb` /
`sl absorb` all automate it: each uncommitted hunk is folded into the ancestor
commit that last touched those lines. Kagi's `plan → confirm → execute` gives
this a shape no other client has — the distribution is shown as a table the user
inspects and confirms **before** any history is rewritten.

## Decision

Add an `absorb` operation living entirely in `kagi-git`
(`ops/absorb.rs`, the `plan_absorb` / `preflight_absorb` / `execute_absorb` /
`verify_absorb` triple), with pure plan/finding types in
`kagi-domain::absorb`.

### Attribution (self-implemented, no vendored code — PM §5)

For each uncommitted **unstaged** hunk (diff HEAD-tree → working tree, default
context), blame the hunk's **deleted lines only** against HEAD (git2's blame
API). A hunk is absorbed only when its deleted lines all resolve to a **single**
commit that is **mutable**. Everything else stays in the working tree:

- pure-addition hunk (no deleted lines) → `Keep(PureAddition)`;
- deleted lines split across commits → `Keep(Ambiguous)`;
- single owner that is not mutable → `Keep(Immutable)`.

We do **not** vendor `git-absorb` (avoids the ADR-0031 intake overhead) and add
**no** new dependency. Zero-context diffs would make blame cleaner but break
re-application to older trees, so we use default context and restrict blame to
deleted lines instead.

### `mutable` definition (PM §5)

A commit is a valid target iff it is, on HEAD's first-parent chain within the
last **N** commits (`DEFAULT_ABSORB_WINDOW = 10`, configurable), AND:

- **not pushed** — unreachable from the branch upstream, the exact
  `graph_descendant_of(upstream, commit)` test amend/undo use (ADR-0143); and
- **not a merge commit**.

Signed / tagged conditions are out of scope for v1. Protected branches
(`main` etc.) are refused outright, as in amend.

### Execute — in-memory rebuild, no destructive command

`execute_absorb` rebuilds the affected slice of history **entirely in memory**,
matching the codebase's existing in-memory ops (cherry-pick, pull) rather than
shelling out to `git rebase`:

For each commit `ci` from the oldest target up to HEAD, its new tree is
`apply_to_tree(ci.tree, diff, hunks whose target depth ≥ depth(ci))` — every
absorbed hunk owned by `ci` or an ancestor of `ci`, so an ancestor's absorbed
change propagates forward to its descendants. Fresh commit objects are chained
onto the oldest target's parent (a root target stays a root); the branch ref is
moved **last** with a reflog-logged `reference(..., force=true)` — the same
ref-order rule amend/undo follow. The index is then re-read from the new HEAD
tree (a `reset --mixed`, never `--hard`) so the absorbed hunks leave the index
while the kept hunks remain as unstaged working-tree changes.

Because the rebuild is a chain of new commits and nothing is deleted, the old
history stays reachable via the reflog. `push --force`, `reset --hard`, and
`git clean` appear nowhere. `git rebase` is not invoked — a dirty working tree
(the kept hunks) would block it anyway.

Why not the git 2.55 `git history fixup` (#344)? It would add a version
dependency for no safety gain here — the in-memory rebuild already touches no
working-tree state and leaves no interrupted sequencer.

### Confirm / reassign UI (v1)

The plan modal renders the distribution table. Reassignment is limited to a
per-hunk "keep in the working tree (don't absorb)" toggle — no dropdown
re-pick. Ambiguous hunks are never force-assigned. A signed target produces a
**warning** line ("signature will be dropped"), never a blocker.

### Oplog

Absorb is not an `Operation` enum variant — its plan carries the whole
distribution table, which is awkward to route through `Backend::run`. So
`Backend::execute_absorb` appends its own oplog entry (`op = "absorb"`),
producing exactly one record per run, consistent with ADR-0149.

## Consequences

- New public surface: `kagi_domain::absorb::*` (pure), and
  `plan_absorb` / `preflight_absorb` / `execute_absorb` / `verify_absorb` plus
  `Backend::{plan_absorb, execute_absorb}` in `kagi-git`.
- v1 restrictions, each a documented ceiling, not a silent gap:
  - unstaged hunks only (staged changes are a blocker — keeps the index out of
    the rewrite);
  - a hunk mixing a blameable modification and an addition in one diff hunk is
    absorbed as a unit to the modification's owner;
  - a merge commit inside the rebuild range refuses the whole absorb (linear
    history assumption);
  - signatures on rewritten commits are dropped (warned, not blocked).
- Acceptance (all covered by `tests/absorb_test.rs`): single hunk → correct
  ancestor; a pushed commit is never a target; ambiguous hunk stays in the tree;
  the plan produces the distribution table; post-execute the hunk is gone from
  the working tree and present in the target commit; the run is in the oplog.
