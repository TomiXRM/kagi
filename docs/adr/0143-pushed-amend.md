# ADR-0143: Amending a pushed commit

- Status: Accepted
- Date: 2026-08-26
- Implements: ADR-0040 案C (the amend half); builds on ADR-0130 (force-with-lease push)

## Context

ADR-0040 shipped amend with 案B for the MVP: a commit already reachable from
its upstream was a **blocker**, because allowing the amend without a way to
publish it would leave the user stuck — amended locally, unable to push from
the GUI. It named 案C, "amend + force-with-lease as one guarded flow", as the
future answer and reserved T-COMMIT-019 for it.

ADR-0130 then delivered the publishing half: `Force-with-lease push...` in the
branch menu, isolated to `ops/force_lease.rs`, with a lease that is never
refreshed by an automatic fetch. Its Consequences claim ADR-0040 案C is "fully
delivered", but only the push existed — `plan_amend` still refused outright, so
the flow it unblocked had no way to start.

## Decision

A pushed commit may be amended on an ordinary branch, and may not on a shared
one.

- **Protected branches stay a blocker.** `main` / `master` / `develop` /
  `development` / `trunk`, and the `release` / `hotfix` series
  (`kagi_domain::refs::is_protected_branch`). The reason is not that these
  branches are important, it is that other people's work is built on them:
  a rewrite strands every clone that already fetched it, and no confirmation
  dialog makes that recoverable for the people who are not looking at this
  screen. ADR-0040 案C named this exclusion; it had never been implemented
  anywhere, including in the force-with-lease push itself (see Consequences).
- **Elsewhere it becomes a warning**, `HistoryNote::AmendDivergesFromRemote`,
  which states that the branch will diverge, that a plain push will be
  refused, and that the result is published with `Force-with-lease push...`.
  The user is not left stuck, which was 案B's whole reason for existing.
- **The two steps stay separate**, against 案C's original single combined
  flow. Amend and force-with-lease push each keep their own confirmation
  (the push's is the two-stage armed confirm of ADR-0130), and a divergent
  branch between them is a legible, inspectable state — the user can look at
  the graph before publishing. Chaining them would mean one confirmation
  standing in for two irreversible acts.
- `--force` remains absent from the codebase. The only rewriting push is
  still the lease-protected one in `ops/force_lease.rs`.

## Consequences

- ADR-0040's 案B row is superseded for amend; its MVP note ("未 push commit の
  amend のみ実装") no longer describes the code.
- Undo of a pushed commit is **unchanged** — still a blocker, still pointing
  at `git revert`. Undo drops a commit rather than replacing it, so the
  divergence it creates is a different shape and wants its own decision.
- **Known gap, deliberately not closed here:** `plan_force_with_lease_push`
  does not consult `is_protected_branch`, so a force-with-lease push to `main`
  is still possible from the branch menu even though ADR-0040 案C forbade it.
  This ADR does not change existing behaviour of a shipped feature; closing
  it is a follow-up, and it is recorded here so the inconsistency is not
  mistaken for a decision.
