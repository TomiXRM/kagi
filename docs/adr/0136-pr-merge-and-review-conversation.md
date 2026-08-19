# ADR-0136: PR mode — status, review conversation, and merge

- Status: Accepted
- Date: 2026-08-19

## Context

PR mode (ADR-less, shipped over v0.20.0–v0.21.0) could list pull requests and
show their commits, files and description, but stopped at read-only browsing.
The user's ask: "PRのステータス表示やレビューのチャットなども表示したい。mergeを
kagiからできるようにもしておきたい。カードもリッチにしたい。"

Three separate problems hide in that sentence:

1. **Which PR needs me right now?** A flat list of open PRs answers "what
   exists", not "what is blocked on me". Deriving that must not cost extra API
   calls — `gh pr list --json` already returns everything needed.
2. **Review threads.** Codex and Copilot leave the highest-signal review
   comments in this repo, and both encode severity in the comment body (a
   shields.io badge, a `[MUST]` prefix) rather than in any API field.
3. **Merge is a write.** Everything else in PR mode is read-only; merge is the
   first PR operation that changes remote state.

## Decision

### Attention is derived, not fetched

`kagi_domain::github::PullRequest::attention(mine, review_requested)` returns a
`(PrAttention, PrReason)` pair computed from fields the list call already
carries — CI rollup, review decision, `mergeable`, draft, `reviewRequests`. The
left pane groups by `PrAttention` (NeedsYou / InProgress / Ready / Waiting /
Dormant) and each card shows its single most important `PrReason`.

Consequence: no per-PR API call to build the queue, so the list stays one
`gh pr list`. The cost is that a reason can only be as precise as the list
fields allow (e.g. "CI failed (3)" but not *which* three).

### Comment tags are parsed out of the body

`extract_comment_tag(body) -> (Option<CommentTag>, String)` recognises the
shields.io badge Codex emits and Copilot's `[MUST]`/`[NIT]` prefixes, returns a
`CommentTag { label, severity }` rendered as a chip, and hands back the prose
with the marker removed. Unrecognised bodies pass through untouched — the
parser is additive, never lossy.

Rejected: asking the GitHub API for a severity field. There isn't one; these
are bot conventions, and each bot will keep inventing its own. Parsing in
`kagi-domain` (pure, unit-tested against real bodies) is where a new
convention gets added.

### Merge follows the standard write path

`plan_pr_merge` / `merge_pr` in `crates/kagi-git/src/github.rs` are a normal
plan → confirm → preflight → execute → oplog triple, surfaced through
`ActiveModal::PrMerge` like every other write:

- **Blockers** (refuse): draft PR, `mergeable == Conflicting`.
- **Warnings** (proceed with confirmation): failing or pending checks,
  changes-requested reviews, "this changes the remote", "this deletes the
  branch".
- Plan notes are typed (`GithubNote` / `GithubTitle` / `GithubRecovery`, ADR-0129)
  with EN in `kagi-domain` and JA in `kagi-ui-core`.

`gh pr merge` does the work; its stderr is surfaced verbatim on failure rather
than being re-worded, because gh's messages name the branch-protection rule
that blocked the merge and kagi cannot.

This is not a destructive command (invariant 3): merge creates a commit, and
the deleted branch is the PR's own head, already merged, recoverable from the
remote's reflog and from the PR page.

## Consequences

- The Focus Queue is only as good as `gh pr list`'s fields; a PR whose CI
  finished between polls shows a stale reason until the next tick.
- Bot-tag parsing is a maintenance surface: a bot that changes its badge format
  silently degrades to "no chip" (prose still renders), which is the right
  failure mode.
- `gh` remains the only GitHub transport. No token handling, no HTTP client.
