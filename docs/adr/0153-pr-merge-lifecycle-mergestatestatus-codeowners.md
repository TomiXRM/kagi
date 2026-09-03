# ADR-0153: PR merge lifecycle — mergeStateStatus, merge queue, `--match-head-commit`, CODEOWNERS

- Status: **Accepted**
- Date: 2026-09-03
- Related: ADR-0136 (PR merge + review conversation), ADR-0145 (PR conflict preview, `DIRTY`), ADR-0119 (Analyze ownership), #347, #346, #355

## Context

Kagi could list PRs, preview conflicts, review, and merge — but the *back half*
of a PR's life was invisible:

- The PR conflict preview (ADR-0145) handled only `mergeStateStatus == DIRTY`.
  GitHub reports **four** actionable states — `DIRTY`, `BEHIND`, `BLOCKED`,
  `UNSTABLE` — and the other three had no surface at all.
- Once a PR entered a **merge queue**, kagi showed nothing: no position, no ETA.
- `gh pr merge` was invoked **without** `--match-head-commit`, so a merge
  confirmed while someone else pushed to the head branch would merge commits the
  user never saw — the PR-side analogue of the force-push footgun kagi exists to
  prevent.
- CODEOWNERS review requirements were invisible. GitHub's API does **not** return
  which owners a PR's files require, so this cannot be read; it must be computed.

## Decision

**1. Pure `mergeStateStatus` → action mapping** (`kagi-domain::merge_state`).
`MergeStateStatus` (the full GitHub enum, unknowns degraded, never guessed) maps
to one `RecommendedAction`: `DIRTY` → open conflict view, `BEHIND` → update
branch, `BLOCKED` → show what is missing, `UNSTABLE` → show the failing
non-required check, `CLEAN` → ready. `MergeStatusView::build` assembles the
render-ready model; the GPUI layer only reads fields off it.

**2. `--match-head-commit` is ALWAYS present.** `github::merge_args` (pure,
unit-tested) unconditionally appends `--match-head-commit <headRefOid>`. There is
no flag or branch that omits it. `PullRequest` gained a `head_sha` field
(`headRefOid`) carried from the list into the merge modal.

**3. Merge queue via `gh api graphql`.** `github_merge::pr_merge_status` fetches
`mergeStateStatus`, the `mergeQueueEntry` (position / ETA / state /
nextEntryEstimatedTimeToMerge), and the unresolved-thread count. A **missing**
merge queue (non-MQ repo, or not queued) yields `queue: None`, and the UI simply
renders no queue section — the MQ-absent case is structurally intact, not grayed
out. `enqueue`/`dequeue` mutations are provided; `jump`/solo are two-step
confirmed in the UI (they reorder other people's PRs).

**4. `gh` version is detected, never hard-required** (#347 §5, PM-locked).
`parse_gh_version` / `gh_at_least` let callers hide a 2.99-gated affordance with
a note; an older `gh` keeps working. We do **not** force an upgrade.

**5. Hand-rolled CODEOWNERS parser + matcher** (`kagi-domain::codeowners`).
CODEOWNERS patterns are a small subset of gitignore, and `kagi-domain` is
dependency-free by invariant, so the `ignore` crate is **not** added. A ~40-line
segment glob (`*`, `**`, `?`, leading-`/` anchor, trailing-`/` directory, `!`
negation, last-match-wins) covers it, with golden tests for each construct and
`@org/team` owners.

**6. No `--admin` / rule-override button — ever.** Offering a bypass contradicts
kagi's "no destructive, no override" ethos. `MergeStatusView::show_admin_button`
is hardwired `false` with no input that can flip it — not even when the API
reports the viewer *could* bypass (`BypassCapability::Allowed`). We show *what*
is missing (approvals / CODEOWNERS / unresolved threads), never a way past it.

## Consequences

- The 4-state action surface, queue position, and missing-requirements list are
  a new card in the PR overview (`src/ui/pr_merge_status.rs`, a pure renderer
  over the view-model, split out because `pr_mode.rs` is at its LOC ceiling).
- The safety guarantee (`--match-head-commit` always) and the MQ-absent /
  no-admin invariants are asserted by unit tests, not left to the GPUI layer.
- **Follow-ups:** wiring the CODEOWNERS *file* read off the base ref into the PR
  tab so `codeowner_reviews` is populated live (the matcher is done and tested);
  `gh pr checks --watch` background job is deferred to #355; a precise
  required-approval count (the current heuristic is 0/1 from `reviewDecision`).
