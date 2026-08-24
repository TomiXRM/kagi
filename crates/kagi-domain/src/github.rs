//! GitHub pull-request model (pure). Filled by `kagi_git::github` from
//! `gh pr list --json`; rendered by the sidebar's PULL REQUESTS section.

/// Aggregate CI state of a PR's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
    /// No checks reported (or `gh` returned none).
    None,
    Pending,
    Success,
    Failure,
}

/// GitHub's `reviewDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// No decision yet (also: no reviewers requested).
    None,
    ReviewRequired,
    ChangesRequested,
    Approved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    /// Head (source) branch name, without the remote prefix.
    pub head: String,
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
        let mk = |n, head: &str, base: &str| PullRequest {
            number: n,
            title: String::new(),
            head: head.into(),
            base: base.into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: String::new(),
            reviewers: Vec::new(),
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::default(),
        };
        let prs = vec![mk(1, "feat/a", "main"), mk(2, "feat/b", "feat/a")];
        assert!(!prs[0].is_stacked_on(&prs));
        assert!(prs[1].is_stacked_on(&prs));
    }

    #[test]
    fn stack_order_puts_children_under_their_base() {
        let mk = |n, head: &str, base: &str| PullRequest {
            number: n,
            title: String::new(),
            head: head.into(),
            base: base.into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: String::new(),
            reviewers: Vec::new(),
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::default(),
        };
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
        let mk = |n, head: &str, base: &str| PullRequest {
            number: n,
            title: String::new(),
            head: head.into(),
            base: base.into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: String::new(),
            reviewers: Vec::new(),
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::default(),
        };
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
            title: String::new(),
            head: "feat/x".into(),
            base: "main".into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: "alice".into(),
            reviewers: vec!["bob".into()],
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::default(),
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
}

/// An issue-level comment on the PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: String,
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

/// What the viewer should do about this PR — the Focus Queue's grouping.
/// Derived, never fetched: everything here comes from data the PR list
/// already carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrAttention {
    /// Something is wrong and it is yours to fix.
    NeedsYou,
    /// Work is happening; nothing to do but wait.
    InProgress,
    /// Green and yours — merge it.
    Ready,
    /// Someone else's move (your review is requested, or you're waiting on one).
    Waiting,
    /// Everything else.
    Dormant,
}

/// Why a PR landed in its [`PrAttention`] bucket — shown next to the state so
/// the user never has to decode a glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrReason {
    CiFailed(usize),
    ChangesRequested,
    Conflicting,
    CiRunning,
    ReadyToMerge,
    ReviewRequested,
    AwaitingReview,
    Draft,
    None,
}

impl PullRequest {
    /// Number of failing checks (0 when none / unknown).
    pub fn failed_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.state == CiState::Failure)
            .count()
    }

    /// Classify for the Focus Queue. `mine` is [`PullRequest::group_for`] ==
    /// `Mine`; a PR that is not yours can never be "yours to fix".
    pub fn attention(&self, mine: bool, review_requested: bool) -> (PrAttention, PrReason) {
        if mine {
            if self.mergeable == Mergeable::Conflicting {
                return (PrAttention::NeedsYou, PrReason::Conflicting);
            }
            let failed = self.failed_checks();
            if failed > 0 || self.ci == CiState::Failure {
                return (PrAttention::NeedsYou, PrReason::CiFailed(failed.max(1)));
            }
            if self.review == ReviewState::ChangesRequested {
                return (PrAttention::NeedsYou, PrReason::ChangesRequested);
            }
            if self.ci == CiState::Pending {
                return (PrAttention::InProgress, PrReason::CiRunning);
            }
            if self.is_draft {
                return (PrAttention::InProgress, PrReason::Draft);
            }
            if self.review == ReviewState::Approved || self.mergeable == Mergeable::Clean {
                return (PrAttention::Ready, PrReason::ReadyToMerge);
            }
            return (PrAttention::Waiting, PrReason::AwaitingReview);
        }
        if review_requested {
            return (PrAttention::Waiting, PrReason::ReviewRequested);
        }
        (PrAttention::Dormant, PrReason::None)
    }
}

#[cfg(test)]
mod attention_tests {
    use super::*;

    fn pr() -> PullRequest {
        PullRequest {
            number: 1,
            title: "t".into(),
            head: "feat".into(),
            base: "main".into(),
            is_draft: false,
            ci: CiState::Success,
            review: ReviewState::None,
            url: String::new(),
            author: "me".into(),
            reviewers: Vec::new(),
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::Clean,
        }
    }

    fn check(state: CiState) -> Check {
        Check {
            name: "test".into(),
            workflow: "ci".into(),
            state,
            url: String::new(),
        }
    }

    /// The queue is ordered by what the user must do, and every bucket
    /// carries a concrete reason (no glyph decoding).
    #[test]
    fn mine_failing_ci_needs_you_with_a_count() {
        let mut p = pr();
        p.ci = CiState::Failure;
        p.checks = vec![
            check(CiState::Failure),
            check(CiState::Success),
            check(CiState::Failure),
        ];
        assert_eq!(
            p.attention(true, false),
            (PrAttention::NeedsYou, PrReason::CiFailed(2))
        );
    }

    #[test]
    fn conflicts_outrank_ci_and_reviews() {
        let mut p = pr();
        p.mergeable = Mergeable::Conflicting;
        p.ci = CiState::Failure;
        assert_eq!(
            p.attention(true, false),
            (PrAttention::NeedsYou, PrReason::Conflicting)
        );
    }

    #[test]
    fn running_ci_and_drafts_are_in_progress_not_actionable() {
        let mut p = pr();
        p.ci = CiState::Pending;
        assert_eq!(p.attention(true, false).0, PrAttention::InProgress);
        let mut d = pr();
        d.is_draft = true;
        assert_eq!(
            d.attention(true, false),
            (PrAttention::InProgress, PrReason::Draft)
        );
    }

    #[test]
    fn green_and_approved_is_ready_to_merge() {
        let mut p = pr();
        p.review = ReviewState::Approved;
        assert_eq!(
            p.attention(true, false),
            (PrAttention::Ready, PrReason::ReadyToMerge)
        );
    }

    /// Someone else's PR is never "yours to fix" — a failing CI on it is
    /// their problem; only a review request puts it in your queue.
    #[test]
    fn other_peoples_prs_only_surface_when_your_review_is_requested() {
        let mut p = pr();
        p.ci = CiState::Failure;
        assert_eq!(p.attention(false, false).0, PrAttention::Dormant);
        assert_eq!(
            p.attention(false, true),
            (PrAttention::Waiting, PrReason::ReviewRequested)
        );
    }
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
mod comment_tag_tests {
    use super::*;

    /// Codex's real shape: a shields.io image inside nested `<sub>`.
    #[test]
    fn codex_shields_badge_becomes_a_tag_and_leaves_the_prose() {
        let body = "**<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Preserve KEEP plates**\n\nWhen an operator…";
        let (tag, rest) = extract_comment_tag(body);
        let tag = tag.expect("tag");
        assert_eq!(tag.label, "P1");
        assert_eq!(tag.severity, TagSeverity::High);
        assert!(!rest.contains("shields.io"), "image removed: {rest}");
        assert!(!rest.contains("<sub>"), "wrapper removed: {rest}");
        assert!(rest.starts_with("Preserve KEEP plates"), "{rest}");
    }

    #[test]
    fn p2_is_medium_and_p3_is_low() {
        let mk =
            |p: &str| format!("![{p} Badge](https://img.shields.io/badge/{p}-yellow?style=flat) x");
        assert_eq!(
            extract_comment_tag(&mk("P2")).0.unwrap().severity,
            TagSeverity::Medium
        );
        assert_eq!(
            extract_comment_tag(&mk("P3")).0.unwrap().severity,
            TagSeverity::Low
        );
    }

    /// With a label `severity_for` does not know, the badge **colour** decides
    /// — the only arm that exercises the colour heuristic at all, since a
    /// known label matches first and returns.
    #[test]
    fn unknown_label_falls_back_to_the_badge_colour() {
        let sev = |url_label: &str, colour: &str| {
            let body = format!("![X Badge](https://img.shields.io/badge/{url_label}-{colour})");
            extract_comment_tag(&body).0.unwrap().severity
        };
        assert_eq!(sev("X", "red"), TagSeverity::High);
        assert_eq!(sev("X", "orange"), TagSeverity::High);
        assert_eq!(sev("X", "critical"), TagSeverity::High);
        assert_eq!(sev("X", "important"), TagSeverity::High);
        assert_eq!(sev("X", "yellow"), TagSeverity::Medium);
        assert_eq!(sev("X", "yellowgreen"), TagSeverity::Medium);
        assert_eq!(sev("X", "green"), TagSeverity::Low);
        assert_eq!(sev("X", "blue"), TagSeverity::Low);
        // The label wins over a contradicting colour.
        assert_eq!(sev("NIT", "red"), TagSeverity::Low);
        assert_eq!(sev("P1", "green"), TagSeverity::High);
        assert_eq!(sev("SHOULD", "green"), TagSeverity::Medium);
        // Colour matching is case-insensitive (`to_ascii_lowercase`).
        assert_eq!(sev("X", "RED"), TagSeverity::High);
    }

    /// Copilot's shape.
    #[test]
    fn copilot_bracket_prefix_becomes_a_tag() {
        let (tag, rest) = extract_comment_tag("[MUST] `f()` changed shape");
        assert_eq!(tag.unwrap().label, "MUST");
        assert_eq!(rest, "`f()` changed shape");
    }

    /// A markdown link or ordinary prose must not be mistaken for a tag.
    #[test]
    fn links_and_prose_are_left_alone() {
        for body in [
            "[see docs](https://x) then fix",
            "plain prose",
            "[a very long bracketed phrase] no",
        ] {
            let (tag, rest) = extract_comment_tag(body);
            assert!(tag.is_none(), "{body} → {tag:?}");
            assert_eq!(rest, body);
        }
    }

    #[test]
    fn has_suggestion_detects_only_real_suggestion_fences() {
        let c = |body: &str| ReviewComment {
            author: "copilot".into(),
            path: "src/lib.rs".into(),
            line: 10,
            body: body.into(),
            diff_hunk: String::new(),
            created_at: String::new(),
            in_reply_to: None,
        };
        assert!(c("nit\n\n```suggestion\nlet x = 1;\n```\n").has_suggestion());
        // Indented inside a list item still counts.
        assert!(c("- see below\n  ```suggestion\n  ok\n  ```").has_suggestion());
        // Language-tagged suggestion fences (```suggestion rust) count too.
        assert!(c("```suggestion rust\nok\n```").has_suggestion());
        // Prose and plain code fences do not gate the apply affordance.
        assert!(!c("looks good to me").has_suggestion());
        assert!(!c("```rust\nlet x = 1;\n```").has_suggestion());
        assert!(!c("the word suggestion appears mid-line ```suggestion").has_suggestion());
    }
}
