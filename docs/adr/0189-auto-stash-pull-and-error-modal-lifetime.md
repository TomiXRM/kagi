# ADR-0189: Auto-stash dirty Pull and persistent failure modal

- Status: Accepted
- Date: 2026-09-08
- Related: ADR-0009, ADR-0093, ADR-0176
- Amended: 2026-10-21, #1690 (risk-based confirmation); failure presentation is
  superseded by ADR-0196.

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

For a local current-branch Pull, dirty state alone does not authorize or require
a stash. After the owned freshness fetch, the plan classifies the actual incoming
checkout (ADR-0100 / ADR-0192):

- No overwrite risk or predicted history conflict: the user's Pull command
  authorizes the bounded operation without a second confirmation dialog. Preserve
  unrelated staged content and working files; do not stash.
- Incoming checkout overlaps dirty paths: show those paths and offer Cancel,
  Review changes, or the explicit Stash & Pull workflow below.
- Committed histories conflict: offer the existing Merge review / Conflict Mode
  path or Cancel. Stashing does not resolve a committed-history conflict.

The explicit Stash & Pull workflow stashes staged, unstaged and untracked changes,
runs the approved Pull through `Backend::run`, and restores the newly created
stash. Retain the stash when restoration conflicts. Each mutation remains an
existing planned Backend operation; the UI owns only intent and presentation.
Revalidate the approved identity and dirty digest before the first stash, and
never silently broaden a quiet Pull into this workflow.

A displayed choice takes modal keyboard focus. Confirmation removes the card
before admitting the background operation, including Return keydown/keyup from
the toolbar; repeated Return cannot start a second Pull. Failures use the durable
Operation Log and bounded notification policy in ADR-0196, not an automatically
reopened dismiss-only error modal. Remote Pull keeps its existing explicit plan.

## Failure semantics

- Stash failure: Pull does not start; report Failed.
- Pull fails before changing the checkout: restore the temporary stash before
  reporting the failure. A partially changed or unobservable checkout must retain
  the stash for recovery rather than blindly pop it into an uncertain state.
- Pull succeeds and stash pop succeeds: report Success.
- Pull succeeds but stash pop conflicts or fails: report Partial, keep/recover
  the stash where Git permits, reload conflict state, and retain the complete
  recovery detail in the Operation Log.
- Pull fails and restoration conflicts or fails: report Partial because the
  repository no longer matches the pre-confirmation state.

## Consequences

- A safe dirty Pull preserves unrelated staged path / OID / mode and split
  unstaged edits without a stash round trip.
- Only the explicit protection choice means stash, Pull, then restore. Its
  staged-state restoration still uses existing stash-pop behavior; no new
  `REINSTATE_INDEX` semantics are introduced.
- A watcher refresh cannot leave an already-confirmed Pull card over an executing
  operation or erase the durable failure explanation.
- The operation log records the constituent stash and Pull operations at their
  existing Backend execution boundaries.
