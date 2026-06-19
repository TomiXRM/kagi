# ADR-0100: Dirty pull uses path-overlap preflight

- Status: Accepted
- Date: 2026-06-19
- Amends: ADR-0004, ADR-0009

## Context

ADR-0009 originally treated any staged or unstaged working-tree change as a
pull blocker. That was safe, but too coarse for editor-generated files such as
`.vscode/settings.json`: stashing those files can cause the editor to recreate
them immediately, so users get stuck in a stash/recreate loop even when the
incoming pull does not touch those paths.

The safety property we need is narrower than "working tree must be clean":
pull must not overwrite local dirty paths.

## Decision

Local pull remains a Guarded operation, but dirty working-tree entries are no
longer plan-time blockers by themselves.

`plan_pull` reports staged, unstaged, and untracked entries as warnings. The
execute phase fetches first, computes the exact tree that the pull would check
out, diffs it against the current HEAD tree, and refuses only when that changed
path set overlaps staged, unstaged, or untracked paths.

If there is overlap, execution stops before checkout/ref movement and reports
the paths that must be stashed or committed. If there is no overlap, pull may
fast-forward or create a clean merge commit while preserving the unrelated
dirty files.

Conflict state remains a blocker. Merge conflicts predicted by the in-memory
merge remain non-writing failures.

## Consequences

- A dirty editor-generated file no longer blocks pull unless the fetched update
  would touch that same path.
- Safety is checked after fetch because only then is the upstream tip exact.
- The working tree may remain dirty after a successful pull; this is expected
  and shown in the operation's verified after-state.
