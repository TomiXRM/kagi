# ADR-0189: Auto-stash dirty Pull and persistent failure modal

- Status: Accepted
- Date: 2026-09-08
- Related: ADR-0009, ADR-0093, ADR-0176

## Context

The current-branch Pull plan warns about a dirty working tree but still offers
Pull. Fetch can then reveal that incoming changes overlap local paths. Execution
refuses safely, but the fetch also triggers the filesystem watcher: its 500 ms
reload clears the newly reopened Pull failure modal. The operation log retains
the error, while the confirmation surface disappears before it can be read.

A confirmation must describe the operation that will actually run. Offering a
plain Pull and only discovering after confirmation that it cannot preserve local
changes violates that requirement.

## Decision

For a local current-branch Pull planned while the working tree is dirty, the
existing Pull modal confirms one explicit workflow:

1. stash staged, unstaged, and untracked changes;
2. run the already-approved Pull through `Backend::run`;
3. pop the newly created stash;
4. keep that stash if reapplication conflicts.

Each mutation remains an existing planned Backend operation. The Pull UI owns
only the sequential intent and presentation; it does not add a second stash or
Pull implementation. The modal uses a structured Pull plan note and recovery
text to state these steps before confirmation. Clean and remote Pull behavior is
unchanged.

A Pull modal containing an execution error survives repository reloads. Normal
confirmation modals still close on reload. The user dismisses the failed Pull
modal explicitly; retry remains guarded by Backend preflight.

## Failure semantics

- Stash failure: Pull does not start; report Failed.
- Pull failure: attempt to pop the temporary stash before reporting Failed.
- Pull succeeds and stash pop succeeds: report Success.
- Pull succeeds but stash pop conflicts or fails: report Partial, keep/recover
  the stash where Git permits, reload conflict state, and show a persistent
  modal error.
- Pull fails and restoration conflicts or fails: report Partial because the
  repository no longer matches the pre-confirmation state.

## Consequences

- Dirty current-branch Pull is deterministic from the first modal: confirmation
  means auto-stash, Pull, then restore.
- Staged state is restored according to the existing stash-pop behavior; no new
  `REINSTATE_INDEX` semantics are introduced.
- The watcher can refresh fetched refs without erasing the failure explanation.
- The operation log records the constituent stash and Pull operations at their
  existing Backend execution boundaries.
