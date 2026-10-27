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

## #1091 scope note (2026-10-09)

The accepted Issue Thread ownership/virtualization and SDK managed-selection
cutover is recorded in [ADR-0198](0198-read-only-issues-workspace.md#1091-2026-10-09-accepted-issue-conversation-and-managed-selection).
Its Issue-only native consumer results do not prove PR conversation
virtualization, remove PR data-cloning costs, or change this ADR's review/merge
boundary. Shared GitHub Markdown preparation/privacy policy remains ADR-0142.


## Amendment — 2026-10-14: PR merge danger and scope (#1529/#1531)

Branch deletion is a destructive **interaction role**, even though its guarded
execution and recovery contract remains unchanged. A merge plan that can delete
a remote or local head branch sets the shared destructive bit: the named Merge
button becomes danger, root Return leaves the sheet unchanged, and the existing alert focus trap
applies. Explicit focused Space/Return or pointer approval remains the only write entry;
retaining branches leaves the ordinary merge role.
The `pr_detail_a11y_1256` keyboard regression asserts that deleting-merge plans
retain this destructive role and transfer focus into the modal alert's focus
scope, rather than requiring the workspace root to stay focused. Escape still
cancels without reactivating Merge or authorizing any repository/transport write.

The card keeps local CURRENT separate from the GitHub AFTER target repository
and base ref. Typed EN/JA notes show the approved source branch/full SHA and
GitHub file/addition/deletion totals. A loaded PR tab can supply its immutable
whole-PR diff base only when its repository/number and loaded head match the
approved PR. The existing background plan computes paths from those immutable
commits, not from the currently selected commit's file list. Missing or failed
path reads are explicitly unavailable; they never imply zero changed files.
The shared preview/Copy all renderer displays the frozen paths. Local cleanup
names/tips remain separate, and `--match-head-commit` still refuses head drift.

Review clarification: file/line totals are optional evidence, not L1 default
zeros. Only the existing Body-detail controller's Fresh or same-head Stale
payload supplies totals; Missing, Loading, failed-without-success and moved-head
details disclose unavailable in EN/JA and Copy all. Frozen path inclusion in
Copy all is restricted to PR merge cards, leaving the established bespoke
Amend, Cherry-pick, Stash and Discard row formatting unchanged.
