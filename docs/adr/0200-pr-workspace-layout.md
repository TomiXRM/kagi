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
gave the target shape; **github.com itself is the behavioural reference** -
what kagi shows for a PR should read the way that PR reads on GitHub.

Kagi's own constraints do not move for it: authentication is delegated to `gh`
and no token is stored, which is the whole reason kagi can claim that what it
shows and what an agent sees never differ; the split-diff row model and the
lane layout are already solved here (`src/ui/diff_split.rs`,
`kagi_domain::graph`) and are wired into this mode.

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

### 4. The swimlane is the PR's lane in its neighbourhood

A PR read in isolation says nothing about where it branched from or what has
landed since, which is the whole point of showing a lane at all. The pane
therefore shows the repository's **own** history windowed around the PR — its
commits plus ten rows either side — with only the PR's rows lit and the
context faded to 45%. A context row is inert: clicking it would either
navigate away from the PR being read or imply the commit is part of it.

It reuses the rows the commit list already built, so lanes, colours and edges
come from the one graph layout in `render` and a lane here is the *same* lane
as in the main graph rather than a second opinion about it. Nothing is laid
out, fetched or read in the pane.

Two earlier attempts are recorded because both were user-reported bugs, and
both came from the same mistake — treating "loaded" as "on screen":

- Gating on *any open tab* left the previous PR's lane standing beside the
  home list, because Home keeps its tabs (that is what makes going back into a
  PR instant). The gate is the **active** tab.
- Drawing *one lane per open tab* grew the rail a lane every time another PR
  was opened, which says "these are related" about ranges that are merely both
  loaded. There is exactly one lane: the PR's.

A dedicated `pr_swimlane` layout existed for the per-tab version and was
deleted with it: the windowed view needs no layout of its own, and the commit
DAG's lanes are the right ones once the rows come from the DAG.

The pane is `None` only when no PR is on screen. With no commits, or with none
of them in the loaded history window, there is no neighbourhood to place a
lane in and none is drawn - but the pane stays, because its lower third is
where the files are picked (§5).

### 5. Commits are a tab; the detail lives under the swimlane, not in a pane

The tab row is 概要 / FILES n / 議論 n / COMMITS n, plus Conflicts when GitHub
says the merge conflicts (ADR-0145). The commit strip became the COMMITS tab:
210px pinned above every view was 210px spent on a list the reader consults
once per PR. Picking a commit there still switches to the diff.

**There is no detail pane.** The outer right rail (stack + checks + files) is
gone, and the checks, the facts and the file list moved into the lower third of
the swimlane pane, which is already on screen for the PR being read. Two
earlier shapes were tried and both were wrong for the same reason: a 320px
right pane and then the mock's 268px in-body column each put a description of
the PR between the reader and the diff, leaving the diff a sliver (user
report). The body is now full width, and the window is navigator | swimlane |
body.

The **stack** went with that pane. It was inferred from open PRs' `base`/`head`
links, not from `gh`, and it was the one thing in the rail that was neither a
fact about the PR nor a way into its files. `PrFocus::Stack` went with it, so
←/→ now cycles List → Commits → Files.

What the lower third shows follows the view: the files of the diff (or of the
conflict set) on the file views, and otherwise CHECKS / REVIEWERS / ASSIGNEES /
LABELS / WORKTREE. WORKTREE answers what no GitHub field can — whether a
worktree here has this PR's head branch checked out, and whether it is clean —
matched on the head branch name from the snapshot's worktrees. The merge action
stays in the header, where it is visible from every tab.

A PR whose commits are not in the loaded history window has no lane to draw.
The pane is still drawn: the detail takes all of it, because the file list is
how a file is picked and it cannot depend on how much history happens to be
loaded.

### 6. The navigator's row is the title over `● state … #N`

The row is the title on its own line, with air under it, then a second, smaller
line: the state dot at the left and the PR number hard against the right, so a
column of rows ends in a column of numbers. The **head branch is not on the
row** — it repeated what the title already says, in the room the number now
uses (user request).

The dot carries the attention colour, so the bucket is still per-row now that
the left edge marks the open PR rather than the bucket. The failed/total check
count and the agent badge (#337) ride between the state and the number: both
change what the reader does next. `PrListRow::why` went with the reason text;
the section header already names the bucket, and `pr_dashboard` computes its
own.

The row has **no fixed height** — it fits its two line boxes and its padding.
A fixed 42px survived the first reshape and then broke it, drawing the text
across the hairline below (user report); `github_evidence_restores` now
measures the row instead of trusting a constant.

### 7. 概要 and レビュー are one page; the tabs are navigation into it

Reading a PR on github.com is one motion: the properties, the checks, the
description and the conversation are a single scroll, and only the files are a
destination of their own. kagi's 概要/レビュー split had no counterpart there.

kagi's 概要 and レビュー tabs drew two separate scroll panes, so reading a
review meant losing the description. They are now **one feed**
(`pr_conversation::render_feed`) with exactly two children: the merge card plus
description, and the conversation. Both tabs draw the same feed; pressing one
scrolls the feed to its section through `ScrollHandle::scroll_to_top_of_item`.
The lit chip is therefore "where you jumped", not "which body is mounted".

Three consequences worth stating:

- The two children are a **contract**: an optional merge card rides *inside*
  the first child and an empty conversation still renders its heading, because
  an anchor index that moved with the data would scroll to the wrong place.
- The jump is **consumed once** (`PrModeState::feed_anchor` is `take`n by the
  renderer). A later frame - a fetch landing, a resize - must not drag the
  reader back to the heading.
- The animated loading row stays **above** the feed: `with_animation` does not
  tick inside a scroll pane. The description is readable while the
  conversation is still in flight, which is the point of merging them.

### 8. The PR's properties are the first rows of its page

github.com keeps Labels / Assignees / Reviewers in a right-hand column. kagi
does not: §5 removed the only column that could hold them, because a column of
metadata beside a diff is what the diff loses width to. They are instead the
first rows of the PR's own page - name/value rows with a fixed name column, so
they read as a table - ahead of the merge card and the description.

`render_pr_properties` (`src/ui/pr_mode.rs`) is the **only** renderer of those
facts; the swimlane pane's lower third keeps the checks and the file list. A
second copy in the rail would be a second thing to keep true.

### 9. Logins have faces, and a comment can be written

A reviewer list reads at a glance only with avatars beside the names, so the
avatar map gained a second key space: a **GitHub login** beside the commit
author emails it already held. Nothing else changed - a login has no `@` and an
email does, so the two cannot collide, and `commit_header::avatar_circle` is
now `pub` and renders both rather than growing a fourth hand-rolled copy of the
same circle. `ensure_pr_avatars` fetches one URL per login from GitHub's avatar
CDN, disk-cached, once per process per login: no API call and no token.

**Writing a comment** follows the write family exactly, with no new machinery:

- `kagi_git::github::pr_comment` is the recorded transport boundary. The body
  goes to `gh pr comment --body-file -` **on stdin**, never in argv: arbitrary
  user text as an argument is a quoting hazard and a length limit. The receipt
  is `Success` on exit 0, `Failed` on a clean non-zero exit, and `Unknown` on
  an unproven termination - there is no server re-read, so an unproven post is
  reported as unproven rather than guessed at, and the transport hold stops it
  being retried blindly.
- `KagiApp::start_pr_comment` gates on the transport hold and the in-flight
  latch and dispatches through `finish_run`, so the attempt is recorded before
  the completion crosses the tab guard - the record exists even if the reader
  has switched tabs.
- The explicit click is the approval; there is no confirmation modal, the same
  exemption staging has. An empty body is a plan **blocker**, not a plan that
  posts nothing.
- One composer exists per window, pinned under the feed where github.com keeps
  it. Its text belongs to the PR it was typed for: switching PRs parks the
  draft on the tab it came from (`PrTab::comment_draft`) and loads the new
  tab's. A posted comment clears that draft and re-reads the thread through the
  existing owner-frozen conversation load.

### 10. The checks card and the review verdicts (mock 7a/7b)

The mock's PR page answers three questions in order: what is this (title,
state, author, branch pair, size), who is on it (properties), **can it merge**
(checks), then what it says (description, conversation). Kagi's checks had
been filed in the swimlane pane's lower third, which is not where the second
question is asked. They are a card on the page now, folded to one line - "all
checks have passed / N successful checks" - and opening it lists the checks
in place with OPEN IN BROWSER per row. The fold is mode-wide state
(`PrModeState::checks_open`), because it is a reading preference and not a
property of one PR.

With the checks gone the swimlane pane's lower third exists only for the file
views; on every other view it is `None` and the lane takes the height back
instead of keeping an empty third.

The composer grew the mock's other two buttons. They are reviews, not
comments: `kagi_git::github::pr_review` is a second recorded transport
boundary (`gh pr review --approve` / `--request-changes`, op name
`pr-review`), with the same stdin body and the same Unknown-is-held rule as
`pr_comment`. GitHub requires words on a "request changes" review and allows a
wordless approval, so an empty box blocks REQUEST CHANGES and COMMENT but not
APPROVE - a plan blocker, not a disabled button with no reason.

A posted comment or review settles **before** the completion's tab guard, and
it settles against the `(session, repository)` pair the write was planned with
- never against what is on screen when `gh` answers:

- `settle_pr_write(owner, repo, number)` empties **that owner's** draft and
  re-reads **that repository's** thread. Both halves were review findings
  (`w5:p19`). Clearing the draft through the presentation path - which the
  guard drops - left the text in the box when the post landed while the reader
  was on another tab, so coming back and pressing the button sent the same
  comment twice. Reading `self.repo_path` in the reload ran `gh pr view #N`
  against whichever repository was active and filed the answer on the original
  PR, and on Welcome or a remote view (`repo_path == None`) skipped the reload
  entirely.
- `pr_mode_load_conversation_for` therefore takes the owner *and* the
  repository as parameters. That is the invariant: a completion acts on the tab
  and the repository it came from. Neither may be re-derived from current app
  state - `finish_run` already holds the frozen pair and passes it down.

Still not built from frame 7: the per-check **LOG** button and the CI-log
screen (7c) need a `gh run view` read that does not exist yet, and the FILES
tab's unified/split toggle with inline review comments (7d) is a change to the
diff row model, not to this page.

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
  - The **FILES tab's own tree** (mock 2a) and the per-PR file-history strip:
    the flat list moved out of the removed rail into the swimlane pane's lower
    third (§5), still wired to `PrFocus::Files` and still reused by the
    Conflicts view for its shorter set. A tree is a separate change.
