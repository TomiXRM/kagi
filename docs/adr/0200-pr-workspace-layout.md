# ADR-0200 — The pull-request workspace: navigator, list, swimlane, rail

- Status: accepted
- Date: 2026-09-18
- Scope: phases P1–P4 of the PR workspace refresh. P5 (the FILES tab's own
  tree and per-PR file history) and P6 (writes: review submission, new PR,
  checkout, worktree-from-PR) are not in this decision.

## Context

PR mode had the data but not the shape. The home screen was a wall of cards;
the navigator grouped PRs by the Focus Queue's *action* buckets with no folds
and no counts; the commit list was a 210px strip pinned above every view; the
detail rail carried the PR's stack and its changed files but none of the facts
GitHub's own sidebar carries. A mock (Claude Design, `git-swim-lane-client-ui`)
gave the target shape, and a second GPUI GitHub client (`e1`) gave a reference
for the interactions.

`e1` is a **design** reference only. Its transport is REST with a device-flow
token; kagi delegates authentication to `gh` and stores no token, which is the
whole reason it can claim that what kagi shows and what an agent sees never
differ. Its split-diff row model and its lane layout are also already solved
here — `src/ui/diff_split.rs` and `kagi_domain::graph` predate it and are
wired into this mode.

## Decision

### 1. The list's data comes from the fetch, not from opening a PR

`FIELDS` gained `assignees,labels,changedFiles,additions,deletions,createdAt,
updatedAt`. A row can now say how big a PR is and how old it is without opening
it; the rail can name assignees and labels without a second call.

Timestamps are kept as `gh` returns them — verbatim RFC-3339, UTC, fixed
width. In that form **lexicographic order is chronological order**, so
`kagi_domain::pr_list::sort_prs` compares text and this crate needs no date
parser; rendering an age uses the existing `iso_to_epoch` + `relative_time`. A
missing stamp sorts last: an empty string is the smallest value, and absence is
not evidence of age.

`PullRequest` gained a hand-written `Default` — hand-written for one field,
`cross_repository: true`, matching the parser's reading of an absent
`isCrossRepository` (#701). A PR whose provenance is unknown must not have
`--delete-branch` promised for it, and a `..Default::default()` fixture is
exactly such a PR.

### 2. Navigator sections are filters, not a partition

INBOX / MY PRS / REVIEW / ASSIGNED, each with a count and a fold. A PR that is
mine, awaiting my review and assigned to me appears in three of them — as it
does in GitHub's own views, and as the mock's own numbers say (4+6+2+3 against
twelve open PRs). So section counts need not add up, and a row is drawn once
per section it matches. `PrSection::accepts` decides membership and is pure;
`pr_nav::pr_sections` builds the rows once and both the renderer and ↑/↓
stepping read that list, so what the arrows walk is what is on screen.

Without a known login nothing is the viewer's: INBOX then keeps only what is
broken or ready on a locally checked-out branch, and the three viewer-relative
sections are empty rather than guessing whose they are.

### 3. The home screen is a table

NO / TITLE+BRANCH / STATE / AUTHOR / CHECKS / FILES / AGE under a strip with
the counts, the OPEN/DRAFT chips and one cycling sort chip. STATE shows the
Focus Queue's verdict rather than GitHub's word for it: "changes requested" is
what the reader acts on, "open" is not.

**No merged chip.** Merged PRs are a different fetch with a reduced field set
(`list_merged_prs`, deliberately cheap for Branch Cleanup), so a MERGED chip
would render half-empty columns. `PrListFilter` therefore has two variants, not
three — a dead variant is a promise the data does not keep.

### 4. The swimlane is its own layout, not the commit graph

`kagi_domain::pr_swimlane` gives one lane per open PR tab and orders rows by
committer time, newest first, stable on ties. This is deliberately **not**
`kagi_domain::graph`: that one assigns lanes across a commit DAG and requires a
topologically ordered slice, and a set of open PRs is a set of disjoint
`merge-base..head` ranges with no edges between them — precisely the input its
own contract calls unspecified. A commit shared by a stacked pair gets a row in
each lane, because it is in both and hiding one would leave that lane with a
gap it cannot explain.

The *drawing* does reuse `graph_view::graph_canvas`, so a lane line and a node
look the same here as in the commit list. Rows belonging to other PRs fade to
45% (mock 1d option A); recolouring their lanes instead would need a second
palette to say the same thing.

The pane adds no read: a tab carries its commit range from the moment it opens.

### 5. Commits are a tab; the rail carries facts

The tab row is 概要 / FILES n / 議論 n / COMMITS n, plus Conflicts when GitHub
says the merge conflicts (ADR-0145). The commit strip became the COMMITS tab:
210px pinned above every view was 210px spent on a list the reader consults
once per PR. Picking a commit there still switches to the diff.

The rail gained REVIEWERS / ASSIGNEES / LABELS / WORKTREE. WORKTREE answers
what no GitHub field can — whether a worktree here has this PR's head branch
checked out, and whether it is clean — matched on the head branch name from the
snapshot's worktrees. The merge action stays in the header, where it is visible
from every tab.

## Consequences

- Three files passed the 800-LOC ceiling and were split on feature boundaries
  rather than having their ceilings raised: `kagi_domain::pr_list` (views over
  a list of PRs), `kagi_git::github_issue` (the `gh issue` parsing), and
  `ui::pr_nav` (the navigator). The issue split also made `logins_at` /
  `labels_at` the single reader of GitHub's `{login}` arrays and label objects;
  the issue parser had its own inline copies.
- Still open, and deliberately not half-built:
  - The **LANES/LIST toggle** on the home screen (mock 4c) needs every open
    PR's `merge-base..head` range, not just the open tabs' — a new background
    read with owner guards. A toggle over one tab's worth of lanes would claim
    to show all of them.
  - The **FILES tab's own tree** (mock 2a) and the file list's move out of the
    rail: the list is wired to `PrFocus::Files` and is reused by the Conflicts
    view for a different, shorter set of files. Moving it is a focus-model
    change, and it belongs with the tree and the per-PR file-history strip.
