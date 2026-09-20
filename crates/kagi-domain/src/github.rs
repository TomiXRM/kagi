//! Pure GitHub pull-request and read-only Issue display models. Filled by
//! `kagi_git::github`; independent of Git and UI state.

pub use crate::github_attention::{PrAttention, PrReason};
pub use crate::github_detail::{
    apply_pr_body, apply_pr_list, apply_pr_status, PrBodyDetail, PrDetailAvailability,
    PrStatusDetail,
};
pub use crate::github_edit::PrFieldEdit;

/// Aggregate CI state of a PR's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CiState {
    /// No checks reported (or `gh` returned none).
    #[default]
    None,
    Pending,
    Success,
    Failure,
}

/// GitHub's `reviewDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewState {
    /// No decision yet (also: no reviewers requested).
    #[default]
    None,
    ReviewRequired,
    ChangesRequested,
    Approved,
}

/// The verdict a review is submitted with — the write-side counterpart of
/// [`ReviewState`], which is what GitHub reports back afterwards.
///
/// Pure data: the flag is the one `gh pr review` understands, and
/// [`ReviewVerdict::requires_body`] is GitHub's own rule, written down once
/// here rather than re-derived at each call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
    Comment,
}

impl ReviewVerdict {
    /// The `gh pr review` flag that submits this verdict.
    pub fn flag(self) -> &'static str {
        match self {
            ReviewVerdict::Approve => "--approve",
            ReviewVerdict::RequestChanges => "--request-changes",
            ReviewVerdict::Comment => "--comment",
        }
    }

    /// Stable slug for the oplog receipt and the operation outcome. Not the
    /// flag: a receipt is read by people, and `--approve` reads like an
    /// invocation detail rather than what happened.
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::RequestChanges => "request-changes",
            ReviewVerdict::Comment => "comment",
        }
    }

    /// GitHub refuses a `REQUEST_CHANGES` or `COMMENT` review with no body —
    /// only an approval may be wordless.
    pub fn requires_body(self) -> bool {
        !matches!(self, ReviewVerdict::Approve)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    /// Head (source) branch name, without the remote prefix.
    pub head: String,
    /// Head commit SHA (`headRefOid`). Passed to `gh pr merge` as
    /// `--match-head-commit` so a merge is refused if someone pushed to the
    /// head branch after the plan was shown (PR-side force-with-lease, #347).
    /// Empty when the caller requested a reduced field set.
    pub head_sha: String,
    /// Base (target) branch name.
    pub base: String,
    pub is_draft: bool,
    pub ci: CiState,
    pub review: ReviewState,
    pub url: String,
    /// Login of the PR author.
    pub author: String,
    /// Logins with a pending review request.
    pub reviewers: Vec<String>,
    /// PR description (markdown), possibly empty.
    pub body: String,
    /// Individual CI checks on the head commit (folded into `ci`).
    pub checks: Vec<Check>,
    /// Mergeability, as GitHub computes it.
    pub mergeable: Mergeable,
    /// `isCrossRepository` — the head branch lives in a fork, not in the
    /// repository the PR targets. `gh pr merge` deliberately skips the remote
    /// head deletion for these, so `--delete-branch` promises something it
    /// will not do (#701 final review 2).
    pub cross_repository: bool,
    /// `<host>/<owner>/<repo>` of the repository the PR targets — the one
    /// `gh` operates on — read out of `url`. The host is part of it: the same
    /// `owner/repo` on github.com and on an Enterprise host are different
    /// repositories. Which local remote points at it is a separate question;
    /// the plan freezes this identity, not a remote name. Empty when `url`
    /// could not be read that way.
    pub base_repo: String,
    /// Logins assigned to the PR (`assignees`), for the detail rail.
    pub assignees: Vec<String>,
    /// Labels with their GitHub colours. Same shape as an Issue's labels —
    /// one label model, not two.
    pub labels: Vec<IssueLabel>,
    /// `changedFiles` / `additions` / `deletions`, so the list can say how big
    /// a PR is without opening it (opening one computes the diff locally).
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    /// `createdAt` / `updatedAt`, verbatim RFC-3339 as `gh` returns them
    /// (always UTC, zero-padded, fixed width). Kept as text on purpose: in
    /// that form lexicographic order *is* chronological order, so sorting
    /// needs no date parsing in this crate; rendering an age uses
    /// `kagi_ui_core::time_parse::iso_to_epoch`, which already exists.
    pub created_at: String,
    pub updated_at: String,
}

/// Hand-written, not derived, for one field: `cross_repository` defaults to
/// **true**, matching the parser's reading of an absent `isCrossRepository`
/// (#701). A PR whose provenance is unknown must not have `--delete-branch`
/// promised for it, and a `..Default::default()` fixture is exactly such a PR.
impl Default for PullRequest {
    fn default() -> Self {
        Self {
            number: 0,
            title: String::new(),
            head: String::new(),
            head_sha: String::new(),
            base: String::new(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: String::new(),
            reviewers: Vec::new(),
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::Unknown,
            cross_repository: true,
            base_repo: String::new(),
            assignees: Vec::new(),
            labels: Vec::new(),
            changed_files: 0,
            additions: 0,
            deletions: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

/// Which sidebar group a PR belongs to, from the viewer's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrGroup {
    /// Authored by me, or its head branch exists locally.
    Mine,
    /// My review was requested.
    ReviewRequested,
    Others,
}

impl PullRequest {
    /// Classify for the viewer `me` (login, if known) given the local branch
    /// names. Pure; the sidebar groups on this.
    pub fn group_for(&self, me: Option<&str>, local_branches: &[String]) -> PrGroup {
        let mine = me.is_some_and(|m| m.eq_ignore_ascii_case(&self.author))
            || local_branches.iter().any(|b| b == &self.head);
        if mine {
            PrGroup::Mine
        } else if me.is_some_and(|m| self.reviewers.iter().any(|r| r.eq_ignore_ascii_case(m))) {
            PrGroup::ReviewRequested
        } else {
            PrGroup::Others
        }
    }
}

impl PullRequest {
    /// Whether this PR's base is itself another open PR's head — i.e. it is
    /// stacked on `others`. Pure; used to mark stacked PRs in the list.
    pub fn is_stacked_on(&self, others: &[PullRequest]) -> bool {
        others
            .iter()
            .any(|o| o.number != self.number && o.head == self.base)
    }
}

/// Order PRs as a stack forest for display: roots (base is not another open
/// PR's head) in input order, each followed by its stacked children
/// depth-first. Returns `(index into prs, depth)`. Cycles (impossible on
/// GitHub, but the data is external) are broken by the visited set.
pub fn stack_order(prs: &[PullRequest]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(prs.len());
    let mut visited = vec![false; prs.len()];
    fn walk(
        prs: &[PullRequest],
        i: usize,
        depth: usize,
        visited: &mut [bool],
        out: &mut Vec<(usize, usize)>,
    ) {
        if visited[i] {
            return;
        }
        visited[i] = true;
        out.push((i, depth));
        for (j, child) in prs.iter().enumerate() {
            if !visited[j] && child.base == prs[i].head {
                walk(prs, j, depth + 1, visited, out);
            }
        }
    }
    for i in 0..prs.len() {
        if !prs[i].is_stacked_on(prs) {
            walk(prs, i, 0, &mut visited, &mut out);
        }
    }
    // Anything left is part of a cycle: emit flat.
    for i in 0..prs.len() {
        if !visited[i] {
            walk(prs, i, 0, &mut visited, &mut out);
        }
    }
    out
}

/// Fold per-check conclusions into one [`CiState`]: any failure wins, then
/// any pending, then success; no checks → `None`.
pub fn fold_ci(conclusions: &[Option<&str>]) -> CiState {
    if conclusions.is_empty() {
        return CiState::None;
    }
    let mut pending = false;
    for c in conclusions {
        match c.map(|s| s.to_ascii_uppercase()) {
            Some(ref s)
                if s == "FAILURE" || s == "TIMED_OUT" || s == "CANCELLED" || s == "ERROR" =>
            {
                return CiState::Failure
            }
            Some(ref s) if s == "SUCCESS" || s == "NEUTRAL" || s == "SKIPPED" => {}
            _ => pending = true,
        }
    }
    if pending {
        CiState::Pending
    } else {
        CiState::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PR that differs only in the fields the stacking rules read.
    fn mk(n: u64, head: &str, base: &str) -> PullRequest {
        PullRequest {
            number: n,
            head: head.into(),
            base: base.into(),
            cross_repository: false,
            base_repo: "o/r".into(),
            ..Default::default()
        }
    }

    #[test]
    fn ci_folds_failure_over_pending_over_success() {
        assert_eq!(fold_ci(&[]), CiState::None);
        assert_eq!(
            fold_ci(&[Some("SUCCESS"), Some("SKIPPED")]),
            CiState::Success
        );
        assert_eq!(fold_ci(&[Some("SUCCESS"), None]), CiState::Pending);
        assert_eq!(
            fold_ci(&[Some("SUCCESS"), None, Some("FAILURE")]),
            CiState::Failure
        );
    }

    /// Every conclusion GitHub can report, and the case-normalisation `gh`
    /// forces on us (the REST API lower-cases what GraphQL upper-cases).
    #[test]
    fn ci_folds_every_github_conclusion_case_insensitively() {
        // Failure-ish conclusions all fail the fold.
        for c in ["FAILURE", "TIMED_OUT", "CANCELLED", "ERROR"] {
            assert_eq!(fold_ci(&[Some(c)]), CiState::Failure, "{c}");
            assert_eq!(
                fold_ci(&[Some(&c.to_ascii_lowercase() as &str)]),
                CiState::Failure,
                "{c} (lowercase)"
            );
        }
        // Non-blocking conclusions leave the fold green.
        for c in ["SUCCESS", "NEUTRAL", "SKIPPED"] {
            assert_eq!(fold_ci(&[Some(c)]), CiState::Success, "{c}");
            assert_eq!(
                fold_ci(&[Some(&c.to_ascii_lowercase() as &str)]),
                CiState::Success,
                "{c} (lowercase)"
            );
        }
        // Anything unrecognised (in-flight checks report e.g. ACTION_REQUIRED
        // or nothing at all) counts as still pending.
        assert_eq!(fold_ci(&[Some("ACTION_REQUIRED")]), CiState::Pending);
        assert_eq!(fold_ci(&[None]), CiState::Pending);
        // A failure still outranks a pending.
        assert_eq!(
            fold_ci(&[Some("neutral"), None, Some("timed_out")]),
            CiState::Failure
        );
    }

    #[test]
    fn stacked_detection_uses_head_of_another_open_pr() {
        let prs = vec![mk(1, "feat/a", "main"), mk(2, "feat/b", "feat/a")];
        assert!(!prs[0].is_stacked_on(&prs));
        assert!(prs[1].is_stacked_on(&prs));
    }

    #[test]
    fn stack_order_puts_children_under_their_base() {
        // 3 stacked on 1; 2 independent; 4 stacked on 3.
        let prs = vec![
            mk(1, "a", "main"),
            mk(2, "b", "main"),
            mk(3, "c", "a"),
            mk(4, "d", "c"),
        ];
        let order: Vec<(u64, usize)> = stack_order(&prs)
            .into_iter()
            .map(|(i, d)| (prs[i].number, d))
            .collect();
        assert_eq!(order, vec![(1, 0), (3, 1), (4, 2), (2, 0)]);
    }

    /// The PR data is external: a base cycle (A's base is B's head and vice
    /// versa) means no PR is a root, so the root pass emits nothing. The
    /// cycle-recovery pass is what keeps those PRs on the dashboard instead
    /// of silently dropping them.
    #[test]
    fn stack_order_never_drops_prs_in_a_base_cycle() {
        // 1 ← 2 ← 1: a two-PR cycle, no root at all.
        let cycle = vec![mk(1, "a", "b"), mk(2, "b", "a")];
        let order = stack_order(&cycle);
        assert_eq!(order.len(), 2, "cycle dropped PRs: {order:?}");
        let mut nums: Vec<u64> = order.iter().map(|(i, _)| cycle[*i].number).collect();
        nums.sort_unstable();
        assert_eq!(nums, vec![1, 2]);

        // A cycle sitting alongside a normal stack: every PR still appears
        // exactly once.
        let mixed = vec![
            mk(1, "root", "main"),
            mk(2, "child", "root"),
            mk(3, "x", "y"),
            mk(4, "y", "x"),
        ];
        let order = stack_order(&mixed);
        let mut nums: Vec<u64> = order.iter().map(|(i, _)| mixed[*i].number).collect();
        nums.sort_unstable();
        assert_eq!(nums, vec![1, 2, 3, 4], "every PR listed once");
    }

    #[test]
    fn grouping_is_by_author_local_branch_then_review_request() {
        let mut pr = PullRequest {
            number: 1,
            head: "feat/x".into(),
            base: "main".into(),
            author: "alice".into(),
            reviewers: vec!["bob".into()],
            cross_repository: false,
            base_repo: "o/r".into(),
            ..Default::default()
        };
        let local = vec!["main".to_string()];
        assert_eq!(pr.group_for(Some("alice"), &local), PrGroup::Mine);
        assert_eq!(pr.group_for(Some("bob"), &local), PrGroup::ReviewRequested);
        assert_eq!(pr.group_for(Some("carol"), &local), PrGroup::Others);
        // A local branch of the same name makes it mine regardless of author.
        assert_eq!(
            pr.group_for(Some("carol"), &["feat/x".to_string()]),
            PrGroup::Mine
        );
        // Unknown viewer: only the local-branch rule can say "mine".
        pr.author = "someone".into();
        assert_eq!(pr.group_for(None, &local), PrGroup::Others);
    }
}

// ────────────────────────────────────────────────────────────
// Checks, reviews, and "what should I do next" (2026-08-19)
// ────────────────────────────────────────────────────────────

/// One CI check on a PR's head commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    /// Workflow the check belongs to (empty for a bare status context).
    pub workflow: String,
    pub state: CiState,
    pub url: String,
}

/// A review submitted on the PR (not a line comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub author: String,
    /// `APPROVED` / `CHANGES_REQUESTED` / `COMMENTED` …
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

/// A line-level review comment — where Copilot / Codex put their code
/// suggestions. Distinct from [`Comment`] (issue-level) and [`Review`]
/// (the submitted verdict).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub author: String,
    /// Repo-relative file the comment is anchored to.
    pub path: String,
    /// Anchor line in the new file (0 when the anchor is outdated).
    pub line: u32,
    /// First line of a multi-line anchor (GitHub `start_line`); `None` for a
    /// single-line comment. A multi-line ```suggestion replaces
    /// `[start_line, line]`.
    pub start_line: Option<u32>,
    /// Markdown body — may carry a ```suggestion fence.
    pub body: String,
    /// The diff hunk GitHub shows above the comment.
    pub diff_hunk: String,
    pub created_at: String,
    /// Set when this is a reply in an existing thread.
    pub in_reply_to: Option<u64>,
}

impl ReviewComment {
    /// Whether the body carries a GitHub ```suggestion block (an applyable
    /// code proposal rather than prose).
    pub fn has_suggestion(&self) -> bool {
        self.body
            .lines()
            .any(|l| l.trim_start().starts_with("```suggestion"))
    }
    // `ReviewComment::suggestion()` (the applyable Suggestion) lives in
    // `crate::suggestion` next to the parser (#351).
}

/// An issue-level comment on the PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: String,
}

/// GitHub's lifecycle state for an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
    Unknown,
}

impl IssueState {
    /// Reduce GitHub's external state string without inventing an open/closed
    /// verdict for a missing or future value.
    pub fn from_github(value: &str) -> Self {
        match value {
            "OPEN" => Self::Open,
            "CLOSED" => Self::Closed,
            _ => Self::Unknown,
        }
    }
}

/// One label attached to an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueLabel {
    pub name: String,
    /// Six-digit RGB value from GitHub, without a leading `#`.
    pub color: String,
    pub description: String,
}

/// One comment in an issue conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueComment {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Read-only GitHub Issue display model.
///
/// List results leave `body` and `comments` empty. `gh issue view` fills them
/// without changing the identity and metadata used by the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub url: String,
    pub author: String,
    pub assignees: Vec<String>,
    pub labels: Vec<IssueLabel>,
    pub body: String,
    pub comments: Vec<IssueComment>,
    /// Aggregate comment count from the list read. Detail reads also fill the
    /// comments themselves, but the sidebar must not fetch every conversation.
    pub comment_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

/// The four mutually-exclusive Issue navigator filters. The selected value is
/// session-owned UI intent; membership itself is a pure projection of the
/// latest successful list read and the process-wide viewer login cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IssueListTab {
    #[default]
    AssignedToMe,
    CreatedByMe,
    MentioningMe,
    RecentlyUpdated,
}

impl IssueListTab {
    pub const ALL: [Self; 4] = [
        Self::AssignedToMe,
        Self::CreatedByMe,
        Self::MentioningMe,
        Self::RecentlyUpdated,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::AssignedToMe => 0,
            Self::CreatedByMe => 1,
            Self::MentioningMe => 2,
            Self::RecentlyUpdated => 3,
        }
    }

    fn accepts(self, issue: &Issue, viewer: Option<&str>, mentioned: &[u64]) -> bool {
        match self {
            Self::AssignedToMe => viewer.is_some_and(|viewer| {
                issue
                    .assignees
                    .iter()
                    .any(|login| login.eq_ignore_ascii_case(viewer))
            }),
            Self::CreatedByMe => {
                viewer.is_some_and(|viewer| issue.author.eq_ignore_ascii_case(viewer))
            }
            Self::MentioningMe => mentioned.contains(&issue.number),
            Self::RecentlyUpdated => true,
        }
    }
}

/// One successful Issue-list response. These fields move through the same
/// request generation so a failed refresh cannot combine old Issues with new
/// mention membership or a different write destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueListSnapshot {
    pub issues: Vec<Issue>,
    pub mentioned_numbers: Vec<u64>,
    pub base_repo: String,
}

/// Filter and sort the existing open-Issue list without another fetch.
pub fn filtered_issues<'a>(
    issues: &'a [Issue],
    tab: IssueListTab,
    viewer: Option<&str>,
    mentioned: &[u64],
) -> Vec<&'a Issue> {
    let mut filtered: Vec<&Issue> = issues
        .iter()
        .filter(|issue| tab.accepts(issue, viewer, mentioned))
        .collect();
    filtered.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    filtered
}

#[cfg(test)]
mod issue_tests {
    use super::*;

    #[test]
    fn issue_state_does_not_guess_for_missing_or_future_values() {
        assert_eq!(IssueState::from_github("OPEN"), IssueState::Open);
        assert_eq!(IssueState::from_github("CLOSED"), IssueState::Closed);
        assert_eq!(IssueState::from_github(""), IssueState::Unknown);
        assert_eq!(IssueState::from_github("MERGED"), IssueState::Unknown);
    }

    fn issue(number: u64, author: &str, assignees: &[&str], updated_at: &str) -> Issue {
        Issue {
            number,
            title: format!("issue {number}"),
            state: IssueState::Open,
            url: String::new(),
            author: author.into(),
            assignees: assignees.iter().map(|name| (*name).into()).collect(),
            labels: Vec::new(),
            body: String::new(),
            comments: Vec::new(),
            comment_count: 0,
            created_at: String::new(),
            updated_at: updated_at.into(),
        }
    }

    #[test]
    fn issue_tabs_filter_client_side_and_sort_newest_first() {
        let issues = vec![
            issue(1, "alice", &["bob"], "2026-01-01T00:00:00Z"),
            issue(2, "bob", &["alice"], "2026-01-03T00:00:00Z"),
            issue(3, "ALICE", &[], "2026-01-02T00:00:00Z"),
        ];
        let numbers = |tab| {
            filtered_issues(&issues, tab, Some("Alice"), &[1])
                .into_iter()
                .map(|issue| issue.number)
                .collect::<Vec<_>>()
        };
        assert_eq!(numbers(IssueListTab::AssignedToMe), vec![2]);
        assert_eq!(numbers(IssueListTab::CreatedByMe), vec![3, 1]);
        assert_eq!(numbers(IssueListTab::MentioningMe), vec![1]);
        assert_eq!(numbers(IssueListTab::RecentlyUpdated), vec![2, 3, 1]);
        assert!(filtered_issues(&issues, IssueListTab::AssignedToMe, None, &[]).is_empty());
    }
}

/// GitHub's `mergeable` / `mergeStateStatus`, reduced to what a merge button
/// needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mergeable {
    /// Not computed yet (GitHub computes asynchronously) — retry.
    #[default]
    Unknown,
    Clean,
    /// Mergeable, but blocked by branch protection (checks / reviews).
    Blocked,
    Conflicting,
}

// ────────────────────────────────────────────────────────────
// Review-comment severity tags (Codex P1 badges, Copilot [MUST])
// ────────────────────────────────────────────────────────────

/// How loudly a review comment asks to be addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagSeverity {
    High,
    Medium,
    Low,
}

/// A severity tag lifted out of a review comment's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentTag {
    /// Display text, e.g. `"P1"` / `"MUST"`.
    pub label: String,
    pub severity: TagSeverity,
}

/// Pull a severity tag out of `body` and return it with the body it was
/// removed from.
///
/// Two producers in the wild:
/// * **Codex** emits a shields.io image badge —
///   `![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)`, usually
///   wrapped in `<sub>`. The image cannot load in kagi (and would be a
///   remote fetch anyway), so it is parsed into a native chip and stripped.
/// * **Copilot** prefixes prose with `[MUST]` / `[SHOULD]` / `[NIT]`.
///
/// Unrecognised bodies come back unchanged with `None`.
pub fn extract_comment_tag(body: &str) -> (Option<CommentTag>, String) {
    if let Some((tag, rest)) = shields_badge(body) {
        return (Some(tag), rest);
    }
    if let Some((tag, rest)) = bracket_prefix(body) {
        return (Some(tag), rest);
    }
    (None, body.to_string())
}

/// `![<label> Badge](https://img.shields.io/badge/<label>-<colour>...)`
fn shields_badge(body: &str) -> Option<(CommentTag, String)> {
    let start = body.find("![")?;
    let close_alt = body[start..].find("](")? + start;
    let close_url = body[close_alt..].find(')')? + close_alt;
    let url = &body[close_alt + 2..close_url];
    if !url.contains("img.shields.io/badge/") {
        return None;
    }
    // `.../badge/P1-orange?style=flat` → label "P1", colour "orange".
    let seg = url.rsplit("/badge/").next()?;
    let seg = seg.split(['?', '#']).next()?;
    let mut parts = seg.split('-');
    let label = parts.next()?.trim().to_string();
    if label.is_empty() {
        return None;
    }
    let colour = parts.next().unwrap_or("").to_ascii_lowercase();
    let severity = severity_for(&label, &colour);
    // Remove the image and the wrapper tags/whitespace it sat in.
    let mut rest = String::with_capacity(body.len());
    rest.push_str(&body[..start]);
    rest.push_str(&body[close_url + 1..]);
    let rest = rest
        .replace("<sub>", "")
        .replace("</sub>", "")
        .trim_start_matches(['*', ' ', '\n'])
        .to_string();
    Some((CommentTag { label, severity }, rest))
}

/// `[MUST] …` / `[NIT] …` at the very start of the body.
fn bracket_prefix(body: &str) -> Option<(CommentTag, String)> {
    let t = body.trim_start();
    if !t.starts_with('[') {
        return None;
    }
    let close = t.find(']')?;
    let label = t[1..close].trim().to_string();
    // A tag, not a markdown link (`[text](url)`) or a long sentence.
    if label.is_empty()
        || label.len() > 12
        || t[close + 1..].starts_with('(')
        || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    let severity = severity_for(&label, "");
    Some((
        CommentTag {
            label: label.to_ascii_uppercase(),
            severity,
        },
        t[close + 1..].trim_start().to_string(),
    ))
}

fn severity_for(label: &str, colour: &str) -> TagSeverity {
    match label.to_ascii_uppercase().as_str() {
        "P0" | "P1" | "MUST" | "BLOCKER" | "CRITICAL" => return TagSeverity::High,
        "P2" | "SHOULD" | "WARNING" => return TagSeverity::Medium,
        "P3" | "P4" | "NIT" | "NITPICK" | "INFO" | "note" => return TagSeverity::Low,
        _ => {}
    }
    match colour {
        "red" | "orange" | "critical" | "important" => TagSeverity::High,
        "yellow" | "yellowgreen" => TagSeverity::Medium,
        _ => TagSeverity::Low,
    }
}

#[cfg(test)]
#[path = "github_comment_tag_tests.rs"]
mod comment_tag_tests;
