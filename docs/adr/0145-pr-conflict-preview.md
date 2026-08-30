# ADR-0145: Showing what a PR conflicts on

- Status: Accepted
- Date: 2026-08-30

## Context

The PR pane shows "this branch has conflicts" because GitHub says
`mergeable: CONFLICTING`. That is the entire payload — the API does not say
which files, and there is no endpoint that does. So the warning is enough to
worry about and useless for deciding what to do, and the only way to find out
was to check the branch out and merge it, which is exactly the disruption the
user was trying to decide whether to accept.

## Decision

A **Conflicts** tab beside Diff, showing which files conflict and what the
conflict looks like. Read-only: no accept/reject, no editing, no entry into
Conflict Mode.

- **Computed locally, not fetched.** GitHub has no answer to give. The PR's
  base and head are already local — the Diff tab renders from them — so the
  merge is run in memory (`merge_commits`) and the conflicted entries are read
  off the resulting index.
- **The text comes from `merge_file_from_index`**, which returns exactly what
  git would have written into the working tree, and it is parsed by
  `HunkModel::from_marker_text` — the same parser the conflict editor uses.
  Hand-rolling a second marker format for a read-only view would be a way for
  the two to disagree about what a hunk is.
- **Delete/modify shows the fact, not a text box.** There is no three-way
  content when a side deleted the file; an empty code panel under the filename
  would read as a bug.
- **The tab only appears when GitHub reports a conflict.** A tab that is
  present but empty six times out of seven teaches people to ignore it.
- **Computed once per tab, off the UI thread.** It is a full three-way tree
  merge — the same work `plan_merge_branch` does — and re-running it per frame
  of a tab the user is looking at would be the worst possible cadence.
- **Not interactive, by design.** Resolving conflicts needs a working tree, and
  the working tree belongs to whatever branch is checked out. The value of this
  tab is that it can be opened from somewhere else, and an editor here would
  have to either move the user or lie about what it was doing.

## Consequences

- The one write: the both-added case writes an **unreferenced empty blob**.
  `merge_file_from_index` needs a readable ancestor, both-added has none, and a
  repository where no file has ever been empty does not contain `e69de29…` to
  point at. Nothing references it, `git gc` collects it, and no command a user
  runs will show it — but the module claimed "nothing here writes" before this
  was noticed, so it is recorded rather than left to be rediscovered.
- The preview can disagree with GitHub, in both directions: `mergeable` is
  computed against the base as GitHub last saw it, and this is computed against
  the objects present locally. A stale local base is the likely cause when they
  differ, and "no conflicts" is shown as a real answer rather than an error.
- `pr_conflict_preview` is not wired into `Backend::run`: it is a query, not an
  operation, and has no plan to preflight.
