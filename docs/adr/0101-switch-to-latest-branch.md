# ADR-0101 — "Switch to latest `<branch>`" operation

## Status

Accepted.

## Context

The existing "Checkout `origin/X` as local branch `X`" operation
(`plan_/execute_checkout_tracking_branch`, ADR T-BCM-061) is **create-only**: it
hard-blocks when a local branch of the same name already exists
(`"Local branch 'X' already exists."`). Users hitting that block expect the
intuitive `git checkout X && git pull --ff-only` behaviour — "just put me on the
up-to-date branch" — but the UI offered no path to it.

Overloading the create-only operation with update semantics would make a single
action mean two different things depending on hidden state. Instead we add a
dedicated, intention-revealing operation.

## Decision

Add a new guarded operation **`SwitchToLatestBranch { branch_name, remote_branch }`**
meaning "fetch the remote, switch to `branch_name`, and fast-forward it to
`remote_branch` when it is safe to do so".

Behaviour, by the state of the local branch (resolved internally; the user sees
one action):

| Local `branch_name` | Action |
|---|---|
| missing | create a tracking branch at the remote tip, switch to it |
| fast-forwardable | switch, then fast-forward the ref to the remote tip |
| ahead only | switch only; warn "ahead by N, not updated" |
| diverged | switch only; warn "diverged, merge/rebase needed" |

Invariants honoured:

- **No destructive commands.** Updates are fast-forward only (`graph_descendant_of`
  guard); never `reset --hard`, never force. A diverged or ahead branch is only
  switched to, never moved.
- **Guarded pipeline.** `plan → confirm → preflight → execute → verify → oplog`.
  A dirty working tree (staged/unstaged) or conflicts are **blockers** at plan
  time, because switching rewrites the working tree.
- **fetch first.** Execute runs `git fetch <remote>` (CLI, same path as
  `execute_pull_branch_ff`) so "latest" means the true remote tip, then
  re-resolves the tip before deciding the fast-forward.

The plan's behind/ahead counts are local knowledge (pre-fetch) and are
re-evaluated after fetch at execute time, mirroring `plan_pull`.

## Invocation

- Remote branch badge in the commit graph (`remote_branch` = the badge ref, e.g.
  `origin/master`; `branch_name` derived via `default_tracking_branch_name`).
- Sidebar / graph local-branch context menu (`branch_name` = the local branch;
  `remote_branch` = its configured upstream). Disabled when the local branch has
  no upstream.

## Consequences

- The create-only "Checkout as local" stays as-is for first-time checkouts.
- Users get a single safe "get me onto the up-to-date branch" action that never
  loses local work (worst case: switch without update + a warning).
