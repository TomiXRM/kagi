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

The lightweight L1 list carries identity, state, branches, participants,
labels, comment totals and timestamps. It does not fetch bodies, checks or
change statistics. Checks/status are lazy L2 reads; body/change statistics
are L3 for opened PRs only. Rows show retained facts from those levels, not
details fetched by rendering a row. The bounded list and detail budgets are
specified in the #1104 amendment below.

Timestamps are kept as `gh` returns them — verbatim RFC-3339, UTC, fixed
width. In that form **lexicographic order is chronological order**, so
`kagi_domain::list_filter` compares text and this crate needs no date
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

NO / TITLE+BRANCH / STATE / AUTHOR / CHECKS / FILES / AGE under the shared
Issue/PR filter strip (#753). State, labels, author and title compose with AND;
PRs additionally offer draft and already-fetched checks. The former OPEN/DRAFT
chips and cycling sort control are replaced, not kept as parallel state.
Open rows retain the Focus Queue's actionable verdict in STATE; closed rows
say Closed rather than being presented as open or ready.

Closed includes merged PRs, fetched through the same complete L1 projection
as Open and All. Branch Cleanup still uses its independent, reduced
`list_merged_prs` read; that reduced evidence is not the dashboard's data source.

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

kagi's 概要 and レビュー tabs now draw **one feed**
(`pr_conversation::render_feed`). Both tabs draw the same list; pressing one scrolls the tab's `ListState` so its section sits at the top (`ListState::scroll_to` with a one-shot `feed_anchor`, not `scroll_to_reveal_item`, whose minimal scroll left the reviews off-screen).
The lit chip is therefore "where you jumped", not "which body is mounted".

The feed is a **virtualized list** (`gpui::list`, the same element the diff
uses), not a scrolling column: its items are the headline, the properties,
the checks card, the merge card, the description, the conversation heading
and then one item per conversation entry (`FeedItem`). A PR with dozens of
Copilot line comments laid every card out on every frame and scrolled in
stutters (user report); now only the visible items exist. The entries are
flattened once, when the conversation lands (`PrTab::feed_entries`), and the
`ListState` lives on the tab so the scroll survives a switch. The tabs reveal
item 0 and the heading's index through `scroll_to_reveal_item`; that index is
looked up per frame because the optional cards shift it.

Three consequences worth stating:

- The heading is always an item, even with no entries (an `Empty` item follows
  it), so the レビュー tab always has something to reveal.
- The jump is **consumed once** (`PrModeState::feed_anchor` is `take`n by the
  renderer). A later frame - a fetch landing, a resize - must not drag the
  reader back to the heading.
- The animated loading row stays **above** the feed: `with_animation` does not
  tick inside a scroll pane. The description is readable while the
  conversation is still in flight, which is the point of merging them.

#### Parent render ownership (#1108)

Virtualizing the feed does not justify copying its backing conversation to
draw a header. `render_center` borrows the active tab's PR facts and commit
source; the Review badge counts raw reviews + issue comments + line comments
(including empty-body reviews, excluding the description). It does not copy
those collections or the complete PR before choosing the view. Commits builds
its own elements only when selected; Files and Conflicts take only their own
diff snapshots and scroll handles, with conflict files borrowed from the tab.

The headline, properties, checks and description cards also borrow that PR.
The description passes the original body slice to the existing Markdown path;
only the empty-description fallback needs a new string. This is not Markdown
preparation reuse, truncation, a new owning DTO, or another renderer cache.
The flattened `Rc` entries, per-tab `ListState`, one-shot section anchors,
selection/Copy and pinned composer's parked drafts keep their existing owners.

Header GitHub/merge listeners retain the session owner and full `PrKey`
(base repository + number), refuse a different active session, and look up
that owner's matching tab on click. Only then do they acquire the owned PR
needed by the existing browser/merge action; render does not freeze its body
into every listener. Refresh can therefore supply the current matching facts
without retargeting another repository's same-number PR. The existing merge
planning/admission pipeline and klog contract are unchanged.

Acceptance evidence is scoped: the filtered native lane
`pr_same_number,workspace_mode_toolbar,pr_threads_via_gh` passed, including the
toolbar's explicit PR-section-exercised marker. An isolated default app in
EN / Apple Light / 100% displayed actual PR12 description section 0001 through
0140, Review 66, review threads and large comment paragraphs, whole-PR files
and a selected-commit file, then PR280's empty conversation and exactly
restored unsent ASCII draft. These are content/ownership witnesses, not a
before/after speedup measurement. Final Review tail, terminal large-diff row,
split mode, JA / 167% and same-key two-session default smoke were not accepted
by that run. A -50000 wheel returned to the diff's top, not its tail; neither
a tail-reachability nor a product-cause claim follows from it.

The subsequent PM-owned final gate lane passed all seven stages (exit 0):
timed build, the same filtered native lane, workspace tests, workspace Clippy,
format check, CI invariant checks and the final default build. Its recorded
wall time was 429.369851 s; this is verification duration, not runtime
performance. Production remained unchanged. Formatting alone changed the
native test file's hash without changing its consumer assertions. These local
gates do not establish public CI status or remove the default-smoke and
matched-performance limits above.


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
  it. The bound `InputState` is the live text authority; `PrTab::comment_draft`
  parks that text while another PR owns the box. The binding is
  `(SessionId, PrKey)`, not PR number or repository identity alone: two local
  sessions can show the same GitHub PR without sharing drafts. Rebinding parks
  through the recorded owner, then loads the destination owner's draft. A
  posted comment clears only its owner's draft and re-reads the thread through
  the existing owner-frozen conversation load; it resets the live input only
  when that input is bound to the same owner and PR.

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

### 11. The properties are editable from their rows

Reviewers, assignees and labels each open a picker **from the value itself** -
"なし" is an invitation to add and an existing value an invitation to change.
A gear at the end of the row came first and was dropped: the value already
says what a gear would (user request). The picker carries a fuzzy filter box
(the command palette's matcher): a login is remembered by a few letters more
often than by its spelling. Selected values always stay listed whatever the
filter says, so a filter can never hide what is about to be sent. The box is
built while the picker is open and dropped with it, on the window-bearing
render pass like every other input. Each opens a picker
(`src/ui/pr_fields.rs`, `ActiveModal::PrFields`). The picker opens on what the
PR already carries and starts a background read of what the repository offers
(`gh label list`, `gh api repos/…/assignees`); the read only ever *adds*
candidates, so a value can be removed with no network at all. Confirming
sends the **diff** between the opening snapshot and the selection through
`kagi_git::github::pr_edit` (`gh pr edit`, one `--add-*`/`--remove-*` flag per
value, op name `pr-edit`), so two readers editing different values do not
overwrite each other's field wholesale. An empty diff is a plan blocker.

On success the owner's copy of the PR takes the new values at once
(`apply_pr_fields`, owner-scoped like every other write settlement) and the
list is re-fetched **for that owner's session and repository**
(`refresh_github_prs_for`) - not through `refresh_github_prs`, which reads the
active tab and would refresh the wrong list after a switch. The candidate read
is keyed by a per-opening `generation`: a read that returns for an earlier
opening of the same PR and field is dropped, so a stale failure cannot blank a
freshly loaded list (both review findings, `w5:p19`).

**No GitHub write derives the repository from a network call.** The first
comment write resolved it with `gh repo view`, which is a round trip, and so
commenting failed with "not a GitHub repo" whenever GitHub was unreachable -
while `PullRequest::base_repo` held the answer the whole time (user report).
Every `gh` mutation now takes that frozen `<host>/<owner>/<repo>` from its
caller; `repo_owner_name` is a fallback for an empty one only. A test keeps it
so: the fake `gh` exits loudly if it is ever asked for `repo view`.

### 12. Opening a PR fetches missing local refs

A PR row always opens its page on the first click. GitHub-owned body, checks
and conversation start loading immediately; missing local refs no longer stop
the transition with “Branch not fetched”. Kagi fetches the base branch and
`refs/pull/<number>/head` in the background, then fills commits and files into
that same owner-scoped tab. The synthetic PR ref is addressed through the one
configured remote whose raw URL matches the PR's frozen `base_repo`, so fork
PRs work without guessing a head repository or overwriting a same-named
remote-tracking branch. Its destination is Kagi's private
`refs/kagi/pr/<remote>/<number>/head` namespace, so it never appears as a
sidebar or Branch Cleanup branch. Zero or multiple matching remotes fail
explicitly.

The request freezes session, repository identity, PR number and head SHA.
Completion updates only that tab incarnation and rejects a fetched head whose
OID differs from the L1 `headRefOid`. Git I/O stays in `kagi-git` and runs off
the UI thread under the existing fetch lease. A second PR click waits with its
owner, visit and generation frozen while that lease is held, then starts
without requiring another click. File diff and conflict state are derived only
after those local refs land. Snapshot graph input pins fetched
`refs/kagi/pr/**` tips as required roots, while branch collection continues to
exclude that namespace; the swimlane therefore sees an unfetched PR's commits
after reload without exposing a synthetic branch in the sidebar.

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

## 追記 1 — PR ページは Issues のタイムライン chrome を共有する (#750)

ADR-0201 の Issues 行・composer の chrome を `src/ui/timeline_row.rs` に切り出し、
Issues（thread / home 一覧 / composer）と PR（feed / composer / home 表）が同じ 1
実装を使う。第二の実装は作らない。行は 40px avatar + 24px gutter + 14px gap、
区切りは 1px の panel line で、card の枠と背景は廃止する。

- **PR 会話 feed**: description / review / comment はいずれも共有 row。line
  comment の diff hunk、```suggestion marker、severity tag、`path:line` anchor、
  review の verdict は row の meta 行と本文の上に残る。時刻表示は生の ISO から
  Issues と同じ相対表記になる。markdown は既存の
  sanitize → pad_inline_code → flatten_html_blocks 経路のまま。
- **PR composer**: viewer avatar、単一 placeholder の box、eye ↔ square-pen の
  単一トグル、有効時のみ amber の送信。preview は input entity の現在値を読む
  だけで `set_value` しないので undo は保たれる。Approve / Request changes は
  従来の色と hold 判定のまま（空文でも Approve は可能）。
  preview は composer に属する状態なので、(a) その composer が表示していた
  tab の書き込みが settle したとき（`PrModeState::settle_composer_for`。active
  tab と一致する場合だけで、背景の PR A の完了が読み手の見ている PR B の
  composer を切り替えることはない）と、(b) 別の PR が box を取ったとき
  （`sync_pr_comment_input` の switch 経路。lane 選択・active tab を閉じた場合・
  home から別 PR を開いた場合はすべてここを通る）に false へ戻す。home から
  同じ PR に戻る場合は box の内容も preview も保持する（読み手が離れただけで、
  下書きは失われていない)。
- **PR home**: triage の表のまま。行は共有 row frame と共有 hover を使う。
  高さは固定せず、共有の 40px avatar + 上下 12px + 1px の hairline から 65px に
  決まる（固定 64px は avatar を 1px 切っていた）。header は avatar 列幅を
  確保して列を揃える。
- **下書き表示**: PR の下書きは従来どおり `PrTab::comment_draft` のメモリ保持で、
  永続化も新しい fetch も追加しない。composer の「下書き保存済み」はその保持を
  指し、Issues 側だけが `drafts.rs` への保存中/保存済みを出し分ける。
- **state**: 追加したのは `PrModeState::comment_preview`（`checks_open` と同じ
  view flag）1 つだけ。書き込み経路 `start_pr_comment` / `start_pr_review`、
  owner 固定、transport 記録は変更しない。
- **i18n**: 両方が使う文言は `ComposerWrite` / `ComposerPreview` /
  `ComposerDraftSaved` / `ComposerDraftSaving` に改名した（EN/JA の文字列は不変）。

## #753: shared Issue/PR filtering and sorting

- `kagi_domain::list_filter::{IssueFilter, PrFilter, apply_issues, apply_prs}` is
  the sole predicate/order policy. It returns source indices, retaining cached
  payload ownership instead of cloning every body/check into virtual rows.
  Empty predicates retain membership; stable equal-key order is preserved.
- Both lists support state Open/Closed/All, multiple labels (all selected
  labels must match), one author and a case-insensitive title substring.
  PR draft Ready/Draft/All and checks Passing/Failing/Pending/All are additional
  AND predicates. An active checks predicate requires fresh, fetched status;
  missing, loading, stale and fetched-no-checks are not Pending. Filtering
  never starts a check request.
- Updated, created, number and comment count each sort ascending or descending,
  defaulting to updated descending. Missing timestamps stay last in either
  direction. Navigator collection membership is ANDed with these predicates,
  and navigator counts/keyboard order and the home table use the chosen order.
- `TabUiState` owns each filter plus the retained input/menu resources. UI
  defaults to Open with draft/checks unrestricted. Filters are not persisted
  and are not copied into `TabViewState`, saved searches or a second cache.
- `src/ui/list_filter_strip.rs` extracts the old PR chip chrome. Both homes
  render it; Issues places it below Composer. Candidate labels/authors come
  from loaded rows; selection uses the existing `menu_overlay`. Clear restores
  defaults and filter/sort changes reset the corresponding viewport.
- `list_prs(workdir, frozen_base_repo, cursor, state)` returns a bounded
  `PrListSnapshot`, not an all-pages collection. Its L1 GraphQL page requests
  real state and `comments { totalCount }`, not comment bodies, check rollups
  or mergeability. The first read resolves gh's repository identity, preserving
  default-repository/enterprise behavior; continuation uses the returned
  host/repository and opaque cursor without resolving the destination again.
  Closed requests CLOSED and MERGED; both become `IssueState::Closed`.
  Unknown state remains unknown. Gateway retry stays confined to the existing
  single L1 retry.
- `workspace_mode_toolbar` clicks the actual shared label and sort menus in
  both homes, proves row membership changes, then proves created-desc changes
  the first row. The PR half remains mandatory when gh is available.

### Review follow-up: preserve the open-PR evidence contract

- `TabUiState::github_prs` remains the shared **Open-only** collection consumed
  by Graph sidebar counts, branch badges and inspector chips. Both periodic
  and owner-targeted shared refreshes always request Open, never strip intent.
- Closed/All responses live in the session-owned `github_prs_strip` read, with
  separate rows, loading, error and generation. Only PR workspace projections
  use `pr_list_rows`; Open uses the shared collection without copying it.
  Leaving PR mode explicitly restores Open and drops strip membership. A
  repository-tab round trip instead retains the chosen predicate, accepted
  rows, opened panes and drafts while revoking old visit/request publication
  rights. An interrupted selected first read keeps pending intent and is
  reauthorized through the ordinary selected-state read on activation; no
  unobserved empty collection becomes a completed zero.
- Closed PRs are `(Dormant, None)` regardless of CI, reviews, draft or status
  availability. L2 admission and queued-read reconciliation reject closed
  status work; opening a closed conversation may still request its L3 body.
  Fresh strip L1 rows inherit already-loaded L2/L3 details only for the same
  repository, PR number and head SHA, so switching state retains known checks
  without refetching closed status or changing shared Open evidence.
- State-switch loading says Refreshing rather than claiming an empty result.
  The old separate “N need your review” headline is intentionally removed:
  filtered attention buckets and navigator collection counts own those counts.
  Its unused `PrHomeNeedsReview` translation key is removed too.
- Native coverage holds a Closed response, checks the loading indicator,
  accepts Closed/All rows without changing shared Open rows, applies the
  ticker's shared refresh while Closed remains visible, and rejects a response
  completed after departure. New PR/menu measurement names allocate only in gui-e2e.

## #1104: bounded cursor pages and retained owner authority

- **One bounded L1 answer.** `PrListSnapshot` carries lightweight `prs`, the
  resolved `base_repo` and `next_cursor` from GraphQL `pageInfo`. A read asks
  for at most 100 nodes in server `UPDATED_AT DESC` order. The existing
  first100 → 120 fixture needs only one cursor-authorized continuation for its
  final 20 new rows; overlapping nodes are deduplicated by `PrKey` without
  replacing accepted rows. There is no increased first-page limit, eager
  all-pages drain, renderer I/O or second collection cache.
- **Shared Open is not selected membership.** Graph badges, inspector chips
  and the periodic/owner-targeted evidence refresh still consume the existing
  shared Open collection. The PR workspace selects Open from that owner and
  Closed/All from its existing session-owned strip. Loaded counts are not a
  remote total: a remaining cursor marks them as incomplete, and filtered
  matches are reported separately.
- **Demand is bounded and visible.** An unfiltered home-table tail can demand
  one page only when it intersects the actual clipped viewport, not virtual
  overdraw. Client membership predicates, including a zero-match filter,
  retain explicit Load more. The navigator retains all loaded rows and a
  reachable continuation control rather than silently truncating at 100.
  Append keeps the scroll anchor, opened panes and composing text. A preserved
  context menu must be dismissed normally before it permits workspace scroll.
- **One append authority.** A request freezes session, repository path,
  resolved host/repository, selected state, cursor, generation, visit and
  attempt. Only the matching current receiver can publish; repeated tail
  paints cannot overlap its append slot. State change, refresh, departure or
  detach revokes obsolete publication rights. Returned metadata must describe
  the frozen repository and make cursor progress; malformed, partial or
  nonadvancing unfinished pages are errors, not successful empty results.
- **Failure is not exhaustion.** Failed append retains accepted rows and its
  cursor with an explicit retry of that page. Failed first-page refresh keeps
  last-known rows but revokes the previous continuation and resets the accepted
  page count, permitting the next automatic first-page read to recover. Success
  replaces the first page or appends only the authorized next page; only an
  exhausted `pageInfo` removes continuation. Branch Cleanup's reduced
  `list_merged_prs` and Vec evidence/settlement remain independent of this
  paging contract.
- **#1107 automatic admission, not newest membership.** The ordinary 60-second
  ticker defers only its shared Open L1 first-page replacement while the active
  workspace is PRs, no explicit Closed/All strip is installed, and shared Open
  has accepted more than one page or has an append pending. This includes an
  opened PR's paged sidebar. The deferred tick still refreshes visible/opened
  volatile details; it does not freeze checks. Manual Refresh and owner-targeted
  refresh keep their existing first-page cutover and revoke old continuation
  authority. Closed/All (even with a pending strip append), background/Graph
  consumers and departure from PRs retain ordinary shared Open ticker reads.
  An automatic first-page read does not reset the dashboard scroll position;
  filter/sort changes retain their explicit viewport reset.
  The deliberate tradeoff is retained last-accepted membership while paging:
  newly opened, closed or deleted PRs require manual Refresh for an authoritative
  new membership read. Retention must never be described as the newest list.
- **Fresh means retained same-head payload.** L2/L3 values remain in the
  existing shared rows, strip rows and actually opened `PrTab`s; the detail
  controller owns scheduling metadata, not another payload cache. Those
  receivers inherit real details only for matching repository, number and
  head. Fresh authority is invalidated when the last matching payload owner
  disappears, while generations continue rejecting old in-flight answers.
  Successful empty details are still payload; a different or stale head is
  not. Selected membership governs visibility and pending demand even when
  another collection retains Fresh data. Checks predicates therefore neither
  lose retained passing rows nor bless dehydrated reappended rows as Fresh.
- **Detail budgets do not grow with pages.** L2 observes only visible rows
  with the existing two-row overscan, 200 ms debounce, 500 ms maximum wait
  and concurrency limit of two. Closed rows do not acquire status, and L3 is
  for actually opened PRs only. Keeping another collection's payload does not
  authorize off-screen status reads. Composer ownership remains the
  `(SessionId, PrKey)` binding described in §9.
- **Actual prepaint is visibility authority.** GPUI `uniform_list` invokes
  `render_items(0..1)` for layout measurement; that invocation must neither
  publish visible demand nor prune the tail's queued details. A non-layout-
  affecting absolute canvas on the first actually rendered table row reports
  once from clipped prepaint, deferred behind active-session, full list-revision
  and PR-workspace/home guards. Measurement never prepaints that canvas.
  The existing two-row overdraw and detail budgets remain unchanged; this adds
  no payload cache, controller or independent state.
- **Exhaustion removes the terminal slot.** The virtual table's item count is
  `rowcount + usize::from(has_more)`: only a selected collection with a cursor
  reserves a continuation row. Loading/retry and pending demand are unchanged.
  Keeping a blank full-height slot after `cursor=None` displaced the last real
  PR completely above its paint clip in the 940×660, 167% one-row viewport.
  The existing `pr_dashboard_responsive` consumer failed before this count fix
  and passed afterward across its eight width/locale/zoom cells; paging consumer
  assertions and the responsive matrix were not repinned or weakened.
  PM's local post-repair gate recorded seven passing stages (build, scoped
  native, workspace, clippy, format check, invariants and default build) in
  `/tmp/kagi-hig-audit-20261008/pr-paging-terminal-row-final-gates/manifest.json`.
  Its native selection covered PR list roles, dashboard responsiveness, Issues
  pagination, the six paging cases and both periodic cases. This records local
  acceptance of that source, not hosted CI, merge or subsequent documentation.
- **Consumer evidence.** Domain/transport/receiver/detail tests cover page
  metadata, frozen cursors, races, retry, dedupe and payload ownership. The ten
  scoped native scenarios are the six `pr_pagination` cases, `pr_same_number`,
  `pr_paging_survives_periodic_tick`,
  `pr_periodic_pending_sidebar_and_other_collections`, and
  `pr_single_page_periodic_scroll_retention` (single-page Open list, real wheel to mid-table, 61-s ticker, anchor/offset unchanged). They exercise actual
  table/sidebar demand, clipped retry, filters, retained Closed/All visits,
  interrupted first reads and live composing inputs. The periodic cases start
  the real producer/ticker and assert retained 120-row membership/anchor and
  opened owner/input while server-produced checks change SUCCESS → FAILURE;
  pending sidebar append, manual reset/stale rejection, Closed/All independence
  and departure are positive controls. Measurement must not steal demand from
  the actually prepainted tail. These fixture assertions are not screenshots,
  live GitHub evidence, FPS or timing improvements.
- **Historical Tier B retention evidence and limits.** The earlier default-build
  run recorded in
  `/tmp/kagi-hig-audit-20261008/pr-periodic-after-native/proof.json` used genuine
  first100 → explicit Load more → 120 membership and the server-ordered tail
  PR #28. An actual 65.26914-second no-input interval retained that membership,
  tail and viewport with zero L1 GraphQL replacements, 22 L2 status queries
  (including #28), and one opened-PR #265 body refresh. Viewed before/after
  photographs showed determining/ready aggregates 75/45 → 107/13 while the
  anchor remained. This proves this interval's automatic retention and ongoing
  volatile refresh, not newest authoritative membership or FPS/latency/CPU
  improvement. The earlier CPU sample did not overlap input. The proof records
  the source after the true-prepaint fix and nine-scenario run; temporary
  diagnostic reads were subsequently removed. It predates the terminal-slot
  repair and is not fresh evidence for that repair.
- **Fresh narrow default-build terminal evidence.**
  `/tmp/kagi-hig-audit-20261008/pr-paging-terminal-tierb/narrow/observed.json`
  records source hashes and exact owned PID 61715/window 6182. Startup at logical
  940×660, zoom 1667, Apple Light/English produced 1880×1320 window-only
  photographs (capture-to-bounds ratio 2, not an independently queried backing
  scale). Explicit visible Load more accepted 100 → 120 with `has_more=false`;
  eight wheel inputs reached the actual last PR #28 fully painted in the tiny
  table body, without an empty reserved continuation slot. A fresh periodic
  photograph still showed 120 and #28, with readiness changing 5 → 6 and no new
  list GraphQL after extension in the observed timeline. Its 176.033609 seconds
  is page-request-to-capture, not response-acceptance retention duration.
  An initial tick before explicit append added a third list GraphQL call;
  this run does not establish two total list calls. The #28 opening showed
  one commit, one file and conversation counts 0/0/0, but its photograph was
  swimlane-only and does not accept narrow Overview visibility. Original
  Cmd-Q/Popen exit was 0, with no remaining exact executable PID or owned app.
  This is narrow-startup and terminal-paint evidence only: no dynamic resize,
  wide-window, Copy/Paste/clipboard, CPU, FPS or latency claim is made.
- **Fresh wide default-build paging and ownership evidence.**
  `/tmp/kagi-hig-audit-20261008/pr-paging-terminal-tierb/wide/observed.json`
  records the same frozen source hashes, exact owned PID 69326/window 6200 and
  logical 1392×883/100%, Apple Light/English window-only captures (ratio 2).
  Eight table wheels caused natural clipped-tail/prepaint demand, without
  explicit Load more, accepting 100 → 120; four further wheels reached last
  PR #28. The settled 120-row tail photograph and a photograph 155.08867 seconds
  later retained the same #80..#28 viewport while Ready changed 15 → 13. Actual
  `pr view` receipts continued at roughly 60-second cadence; no list GraphQL
  occurred after extension through that periodic capture. There were four list
  GraphQL calls through extension: initial, two legitimate pre-extension ticks
  and one non-null cursor request. The 229.741505 seconds from cursor request
  to periodic capture is not exact response-acceptance timing.
  Real PR #12 Overview displayed its 48,002-byte body, with accepted transport
  data of 24 comments/288,048 comment-body bytes, two reviews/268 review-body
  bytes and 40 line comments (visible Review count 66), three commits and two
  files. Visible additions were 22,010; this does not accept full body/comment
  tails, diff/thread interaction or performance. Settled All PRs returned to
  the same 120-row tail; immediate capture preceded settlement and navigation
  latency was not measured. PR #28's 102-byte Overview was visible with no
  comments/reviews, one commit and one file. Physically typed unsent ASCII
  `PM_PR28_DRAFT` did not appear after switching to PR #12; six independent
  sidebar wheels reached #28 while #12 Overview remained, and clicking #28
  restored the exact draft. This is one session/two PrKeys, not the same PrKey
  across two sessions. No Submit, Copy/Paste, clipboard or foreground action
  was exercised. Cmd-Q/original Popen exited 0 with no exact executable PID or
  owned apps left.
  Matched same-PR #28 narrow/wide photographs and source triage identify a
  separate detail-geometry issue [#1118](https://github.com/TomiXRM/kagi/issues/1118):
  the narrow Overview body is missing while wide shows it. This does not accept
  narrow-detail #1095 behavior; independent baseline-dev reproduction is unrun.
  Same-PrKey/two-session default-click acceptance remains dependent on the
  separate overlay-baseline fix/integration. No dynamic resize, matched CPU/FPS
  sample, full diff/thread acceptance, hosted review/CI or merge is claimed.

