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

`plan_pull` warns about actual overwrite risk, not the mere presence of staged,
unstaged or untracked entries. It computes the incoming checkout tree, including
the clean merge result for diverged history, and compares its changed paths with
local dirty paths. The UI obtains a fresh upstream first (ADR-0192); execution
fetches and repeats the safety checks before writing.

If there is overlap, execution stops before checkout/ref movement and reports
the paths that need protection. Without overlap, pull may fast-forward or create
a clean merge commit. Checkout is restricted to the incoming tree's exact changed
paths with literal path matching: unrelated index path / OID / mode and working
files, including split staged/unstaged edits, remain intact. The approved worktree
digest and typed upstream identity are rechecked; new risk refuses the operation
instead of silently adding a stash workflow (#1690).

Conflict state remains a blocker. Merge conflicts predicted by the in-memory
merge remain non-writing failures.

## Consequences

- A dirty editor-generated file no longer blocks pull unless the fetched update
  would touch that same path.
- Safety is checked after fetch because only then is the upstream tip exact.
- The working tree may remain dirty after a successful pull; this is expected
  and shown in the operation's verified after-state.
