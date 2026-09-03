# ADR-0150: Local pre-verification of GitHub branch rulesets

- Status: **Accepted**
- Date: 2026-09-03
- Related: ADR-0043 (commit checklist), ADR-0129 (structured plan notes),
  ADR-0130 (force-with-lease), #346 (this feature), #347 ((B)-group deferral)

## Context

Kagi's `plan → confirm → preflight → execute → verify → oplog` pipeline shows
the user *what will happen* before a write. Until now "what will happen" stopped
at **local** safety (dirty tree, conflict markers, secrets, large blobs). It did
not include the **remote contract**: a commit or branch name that GitHub's
branch ruleset will reject is only discovered on `git push`, after the work is
done. That is a hole in the product's central promise.

The 2026Q3 survey found the enabling fact: `GET /repos/{o}/{r}/rules/branches/{branch}`
needs only `repo` scope (no admin), and **13 of the 23 rule types can be verified
with zero network round-trips** against the change the user is about to make:

`commit_message_pattern`, `commit_author_email_pattern`, `committer_email_pattern`,
`branch_name_pattern`, `max_file_size`, `file_extension_restriction`,
`file_path_restriction`, `max_file_path_length`, `required_signatures`,
`required_linear_history`, `non_fast_forward`, `creation`, `update`, `deletion`.

This ADR covers only that (A) group. Rules needing server state
(`pull_request.*`, `allowed_merge_methods`, `merge_queue`) are deferred to #347.

## Decision

### Layering

- **`kagi-domain::ruleset`** (pure) holds the model — a `Ruleset` struct of the
  13 rule parameters, `Pattern`/`PatternOp`, `Bypass`/`Severity`, `Finding`,
  and the `validate_*` functions. No git2, no gpui, **no JSON**: the domain
  crate stays dependency-free (invariant #2). The findings render through a new
  `PlanNote::Ruleset(RulesetNote)` category (ADR-0129), with EN in
  `plan_note/ruleset.rs` and JA in `i18n/plan/ruleset.rs`.
- **`kagi-git::ruleset`** does the impure work: fetches via `gh api`, parses the
  JSON into the plain `Ruleset` (keeping the domain JSON-free), caches per
  `(workdir, branch)`, and folds findings into an `OperationPlan`.
- **`kagi-git::backend`** exposes the facade the UI uses (`ruleset_for`,
  `ruleset_cached`, `refresh_ruleset`, `ruleset_message_findings`,
  `ruleset_branch_findings`) so `src/ui/` never touches git2 or `gh` (invariant #1).

Integration points reuse the existing plan producers: `plan_commit` (message /
author+committer email / signatures / staged-file size·extension·path·length)
and `plan_create_branch` (branch name / creation). Both read the **cache only**
at plan time, so planning never blocks on the network.

### Empty response = "unknown", never "unconstrained" (PM-locked, §5)

The survey could not confirm whether `/rules/branches/{branch}` includes classic
branch protection, and an empty `[]` could not be distinguished from "no admin".
Therefore an empty or unparseable response maps to `RulesetStatus::Unknown`,
**never** to an empty/unconstrained ruleset. `RulesetStatus::from_fetch(0, _)`
is the single choke point that enforces this, and nothing in Kagi presents a
branch as having no rules on the strength of an empty response.

### Bypass → severity

GitHub's branch-*rules* endpoint does not report `current_user_can_bypass`, so
`Bypass` is `Unknown` in practice and every finding surfaces as a **warning**
("surface, don't hard-block on incomplete info", §5). The
`Denied → blocker` / `Allowed → warning` machinery is implemented and tested for
when bypass becomes available (#347) but is dormant today.

### `gh` absent / unauthenticated

`fetch_ruleset` returns `RulesetStatus::Disabled` when `gh` is missing or the
API call fails (not a GitHub repo, logged out). Disabled contributes no
findings, so the conventional flow runs unchanged and **no error is thrown**.

### Caching

Cache refresh happens on `git fetch` (`Backend::fetch_remote` refreshes the
current branch's ruleset) and on explicit refresh only — **no TTL timer**. A
cache hit is a zero round-trip answer; the `cached_or` seam is unit-tested with
an injected counting fetcher.

### `regex` operator

`kagi-domain` may not depend on a regex crate (invariant #2), so pattern rules
using the `regex` operator (and any unknown operator) are classified
`Uncheckable` and surfaced as an explicit "cannot verify locally — GitHub will
check on push" **warning**. They are never silently treated as satisfied. The
literal operators (`starts_with` / `ends_with` / `contains`, with `negate`) are
fully evaluated.

## Consequences

- A push GitHub would reject for an (A)-group rule is now caught at commit /
  branch-create time, extending preflight from local safety to the remote
  contract — with no new runtime dependency and `kagi-domain` still pure.
- Everything surfaces as a warning today (bypass unknown), so the feature never
  hard-blocks a user out of an operation on incomplete information.
- The live commit-message / branch-name badges have their data path
  (`Backend::ruleset_message_findings` / `ruleset_branch_findings`) and their
  i18n strings (`Msg::RulesetBadge` / `RulesetBadgeTooltip`) in place; the GPUI
  render is the remaining UI wiring (needs human verification per the repo's
  GUI-testing rule).
- (B)-group server-state rules, `require_extra_approval_for_unattributed_changes`
  / #337 coupling, and non-GitHub hosts (GitLab/Gitea) are out of scope (#347).
