//! The pull-request list reads: the L1 GraphQL page, and the reduced
//! `gh pr list --json` shape Branch Cleanup's merged evidence still uses.
//!
//! Split out of `github.rs` for the reason `github_issue.rs` was (that file's
//! 800-line ceiling): one parser has to answer both shapes, and the PR list is
//! the only place that needs it. Pure; the callers are in `github_fetch.rs`.

use kagi_domain::github::{IssueState, PullRequest, ReviewState};
use kagi_domain::list_filter::StateFilter;

use super::github::{
    checks_at, connection_nodes, count_at, graphql_failure, labels_anywhere, logins_anywhere,
    logins_at, mergeable_at, string_at,
};
use crate::GitError;

/// The L1 page: identity, review routing and the aggregate comment total in
/// one request.
///
/// Deliberately without `statusCheckRollup`, `mergeable` and `body`: those are
/// the L2 and L3 reads, and one expensive rollup per PR is exactly what must
/// not be able to time out the whole repository query (#506). `comments` is
/// asked for as `totalCount` only — `gh pr list --json comments` would fetch
/// every comment body of every PR just to count them.
///
/// `$states` is a real variable so the state chip cannot reshape the query
/// text, and the page stays bounded at 100 like the `--limit 100` it replaces.
pub(crate) const PR_LIST_QUERY: &str = r#"
query($owner: String!, $name: String!, $states: [PullRequestState!]) {
  repository(owner: $owner, name: $name) {
    pullRequests(first: 100, states: $states, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number title url state isDraft isCrossRepository createdAt updatedAt
        headRefName headRefOid baseRefName reviewDecision
        author { login }
        assignees(first: 20) { nodes { login } }
        labels(first: 20) { nodes { name color description } }
        reviewRequests(first: 20) { nodes { requestedReviewer { ... on User { login } } } }
        comments { totalCount }
      }
    }
  }
}
"#;

/// The server-side collection each filter names.
///
/// `Closed` includes `MERGED`: GitHub's pull-request lifecycle has three
/// states and the shared filter strip has two, so a merged PR is closed — the
/// same reduction [`pr_state`] applies to a fetched row.
pub(crate) fn pr_states(state: StateFilter) -> &'static [&'static str] {
    match state {
        StateFilter::Open => &["OPEN"],
        StateFilter::Closed => &["CLOSED", "MERGED"],
        StateFilter::All => &["OPEN", "CLOSED", "MERGED"],
    }
}

/// Parse `gh pr list --json <fields>` output — the reduced merged-evidence
/// field set, and the historical shape fixtures are written in. Pure.
pub fn parse_pr_list(json: &str) -> Result<Vec<PullRequest>, GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(pr_from_value).collect())
}

/// Parse one L1 GraphQL page.
///
/// A GraphQL `errors` block is a failed read, never a short list, and missing
/// `nodes` is a failed read too: both reach the caller as
/// [`PrFetchError::Invalid`] so the previous list survives (#506), exactly as
/// the Issue page does (#752).
///
/// [`PrFetchError::Invalid`]: crate::github_fetch::PrFetchError::Invalid
pub fn parse_pr_list_page(json: &str) -> Result<Vec<PullRequest>, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {e}")))?;
    if let Some(failure) = graphql_failure(&value) {
        return Err(failure);
    }
    let nodes = value
        .pointer("/data/repository/pullRequests/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| GitError::Other("gh json: expected repository pull request nodes".into()))?;
    Ok(nodes.iter().filter_map(pr_from_value).collect())
}

/// GitHub's pull-request lifecycle on the shared issue/PR state: `MERGED` is
/// closed. An absent field — the reduced merged-evidence set asks for no
/// `state` — stays [`IssueState::Unknown`] rather than guessing a verdict.
fn pr_state(value: &str) -> IssueState {
    match value {
        "MERGED" => IssueState::Closed,
        other => IssueState::from_github(other),
    }
}

/// Requested reviewers as logins. GraphQL wraps each request in a
/// `requestedReviewer` union; gh's flat shape is already `[{login}]`. A team
/// review request carries no login and is dropped by both, as it always was.
fn requested_reviewers(value: &serde_json::Value) -> Vec<String> {
    match connection_nodes(value, "reviewRequests") {
        Some(nodes) => nodes
            .iter()
            .filter_map(|node| node.get("requestedReviewer")?.get("login")?.as_str())
            .filter(|login| !login.is_empty())
            .map(str::to_string)
            .collect(),
        None => logins_at(value, "reviewRequests"),
    }
}

/// One PR row, from either shape: the GraphQL node of [`PR_LIST_QUERY`] or a
/// `gh pr list --json` entry. Absent fields default instead of failing — a
/// reduced field set is a legitimate request, not malformed output.
fn pr_from_value(v: &serde_json::Value) -> Option<PullRequest> {
    let s = |k: &str| string_at(v, k);
    let (ci, checks) = checks_at(v);
    let mergeable = mergeable_at(v);
    let review = match s("reviewDecision").as_str() {
        "APPROVED" => ReviewState::Approved,
        "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
        "REVIEW_REQUIRED" => ReviewState::ReviewRequired,
        _ => ReviewState::None,
    };
    Some(PullRequest {
        number: v.get("number")?.as_u64()?,
        title: s("title"),
        state: pr_state(&s("state")),
        // `comments { totalCount }` is the aggregate the comments sort orders
        // by. A field set that did not ask for it reads as 0, never as a
        // fabricated count.
        comment_count: v
            .pointer("/comments/totalCount")
            .and_then(serde_json::Value::as_u64)
            .map(|count| usize::try_from(count).unwrap_or(usize::MAX))
            .unwrap_or(0),
        head: s("headRefName"),
        head_sha: s("headRefOid"),
        base: s("baseRefName"),
        is_draft: v.get("isDraft").and_then(|x| x.as_bool()).unwrap_or(false),
        ci,
        review,
        url: s("url"),
        author: v
            .get("author")
            .and_then(|a| a.get("login"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        reviewers: requested_reviewers(v),
        body: s("body"),
        checks,
        mergeable,
        // Unknown topology is conservatively treated as a fork: never promise
        // a head deletion in the base repository without same-repo evidence.
        cross_repository: v
            .get("isCrossRepository")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        // `https://<host>/<owner>/<repo>/pull/<n>` already names the base
        // repository, host included — and neither shape has a
        // `baseRepository` field to ask for it (#701 final review 3).
        base_repo: crate::backend::remote_ref::repo_identity(&s("url")).unwrap_or_default(),
        assignees: logins_anywhere(v, "assignees"),
        labels: labels_anywhere(v),
        changed_files: count_at(v, "changedFiles"),
        additions: count_at(v, "additions"),
        deletions: count_at(v, "deletions"),
        created_at: s("createdAt"),
        updated_at: s("updatedAt"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::github::{CiState, Mergeable};

    const SAMPLE: &str = r#"[
      {"number":236,"title":"feat(ui): stash peek","headRefName":"feat/stash-peek",
       "baseRefName":"main","isDraft":false,"reviewDecision":"",
       "mergeable":"MERGEABLE",
       "statusCheckRollup":[
         {"__typename":"CheckRun","name":"build","workflowName":"ci","conclusion":"SUCCESS","status":"COMPLETED"},
         {"__typename":"CheckRun","name":"test","workflowName":"ci","conclusion":null,"status":"IN_PROGRESS"}],
       "url":"https://github.com/o/r/pull/236","author":{"login":"tomixrm"},
       "reviewRequests":[{"login":"bob"}]},
      {"number":240,"title":"wip","headRefName":"feat/b","baseRefName":"feat/stash-peek",
       "isDraft":true,"reviewDecision":"APPROVED","mergeable":"CONFLICTING",
       "statusCheckRollup":[{"__typename":"StatusContext","context":"legacy","state":"FAILURE"}],
       "url":"https://github.com/o/r/pull/240","author":{"login":"bot"}}
    ]"#;

    /// One L1 page in the shape [`PR_LIST_QUERY`] asks for: connections rather
    /// than flat arrays, a lifecycle `state`, and a comment total.
    const PAGE: &str = r#"{"data":{"repository":{"pullRequests":{"nodes":[
      {"number":236,"title":"stash peek","url":"https://github.com/o/r/pull/236",
       "state":"OPEN","isDraft":false,"isCrossRepository":false,
       "createdAt":"t0","updatedAt":"t1","headRefName":"feat/stash-peek",
       "headRefOid":"sha236","baseRefName":"main","reviewDecision":"APPROVED",
       "author":{"login":"tomixrm"},
       "assignees":{"nodes":[{"login":"ann"}]},
       "labels":{"nodes":[{"name":"bug","color":"ff0000","description":"broken"}]},
       "reviewRequests":{"nodes":[{"requestedReviewer":{"login":"bob"}},{"requestedReviewer":{"name":"team"}}]},
       "comments":{"totalCount":7}},
      {"number":240,"title":"landed","url":"https://github.com/o/r/pull/240",
       "state":"MERGED","isDraft":true,"isCrossRepository":true,
       "headRefName":"feat/b","headRefOid":"sha240","baseRefName":"main",
       "author":{"login":"bot"},"assignees":{"nodes":[]},"labels":{"nodes":[]},
       "reviewRequests":{"nodes":[]},"comments":{"totalCount":0}}
    ]}}}}"#;

    #[test]
    fn parses_gh_json_into_domain_prs() {
        let prs = parse_pr_list(SAMPLE).unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 236);
        assert_eq!(prs[0].head, "feat/stash-peek");
        assert_eq!(prs[0].ci, CiState::Pending, "one check still running");
        assert_eq!(prs[0].review, ReviewState::None);
        assert!(prs[1].is_draft);
        assert_eq!(
            prs[1].ci,
            CiState::Failure,
            "StatusContext state is honoured"
        );
        assert_eq!(prs[1].review, ReviewState::Approved);
        assert!(prs[1].is_stacked_on(&prs));
        assert_eq!(prs[0].reviewers, vec!["bob".to_string()]);
    }

    #[test]
    fn parses_checks_and_mergeable() {
        let prs = parse_pr_list(SAMPLE).unwrap();
        assert_eq!(prs[0].checks.len(), 2);
        assert_eq!(prs[0].checks[0].name, "build");
        assert_eq!(prs[0].checks[0].workflow, "ci");
        assert_eq!(prs[0].checks[0].state, CiState::Success);
        assert_eq!(prs[0].checks[1].state, CiState::Pending, "null conclusion");
        assert_eq!(prs[0].mergeable, Mergeable::Clean);
        assert_eq!(prs[1].mergeable, Mergeable::Conflicting);
        assert_eq!(prs[1].failed_checks(), 1);
    }

    #[test]
    fn head_sha_parsed_from_list() {
        let json = r#"[{"number":1,"title":"t","headRefName":"h","headRefOid":"abc123",
          "baseRefName":"main","isDraft":false,"mergeable":"MERGEABLE"}]"#;
        let prs = parse_pr_list(json).unwrap();
        assert_eq!(prs[0].head_sha, "abc123");
    }

    #[test]
    fn empty_and_garbage_inputs() {
        assert!(parse_pr_list("[]").unwrap().is_empty());
        assert!(parse_pr_list("not json").is_err());
    }

    /// `list_merged_prs` asks for only four fields, so the shared parser has to
    /// survive the absence of `statusCheckRollup`, `mergeable`, `url` and the
    /// rest. Real `gh pr list --state merged` output, trimmed to two entries.
    #[test]
    fn parses_the_reduced_merged_field_set() {
        let json = r#"[
          {"author":{"id":"MDQ6VXNlcjI=","is_bot":false,"login":"TomiXRM","name":"D T"},
           "headRefName":"chore/bump-0.24.0","number":255,"title":"chore: bump to 0.24.0"},
          {"author":{"id":"MDQ6VXNlcjI=","is_bot":false,"login":"TomiXRM","name":"D T"},
           "headRefName":"fix/audit-bugs","number":254,"title":"fix+refactor: races"}
        ]"#;
        let prs = parse_pr_list(json).expect("parse");
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 255);
        assert_eq!(prs[0].title, "chore: bump to 0.24.0");
        assert_eq!(prs[0].head, "chore/bump-0.24.0");
        assert_eq!(prs[0].author, "TomiXRM");
        // Fields the reduced query does not ask for must default, not panic.
        assert!(prs[0].url.is_empty());
        assert!(prs[0].checks.is_empty());
        assert_eq!(
            prs[0].state,
            IssueState::Unknown,
            "a field set with no state must not claim one"
        );
        assert_eq!(prs[0].comment_count, 0);
    }

    #[test]
    fn graphql_page_carries_state_comment_totals_and_connection_fields() {
        let prs = parse_pr_list_page(PAGE).expect("page");
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 236);
        assert_eq!(prs[0].state, IssueState::Open);
        assert_eq!(prs[0].comment_count, 7);
        assert_eq!(prs[0].head_sha, "sha236");
        assert_eq!(prs[0].assignees, vec!["ann".to_string()]);
        assert_eq!(prs[0].labels[0].name, "bug");
        assert_eq!(prs[0].labels[0].color, "ff0000");
        assert_eq!(
            prs[0].reviewers,
            vec!["bob".to_string()],
            "a team review request has no login to show"
        );
        assert_eq!(prs[0].base_repo, "github.com/o/r");
        assert!(!prs[0].cross_repository);
        // L1 stays lightweight: no rollup, no mergeability, no body.
        assert!(prs[0].checks.is_empty());
        assert_eq!(prs[0].ci, CiState::None);
        assert_eq!(prs[0].mergeable, Mergeable::Unknown);
        assert!(prs[0].body.is_empty());
    }

    #[test]
    fn a_merged_pull_request_reads_as_closed() {
        let prs = parse_pr_list_page(PAGE).expect("page");
        assert_eq!(
            prs[1].state,
            IssueState::Closed,
            "the strip has two states; MERGED belongs to Closed"
        );
        assert!(prs[1].cross_repository);
    }

    #[test]
    fn a_partial_or_shapeless_page_is_a_failure_not_an_empty_list() {
        let errors = r#"{"data":{"repository":null},"errors":[{"message":"Could not resolve to a Repository"}]}"#;
        let error = parse_pr_list_page(errors).expect_err("partial response");
        assert!(
            error
                .to_string()
                .contains("Could not resolve to a Repository"),
            "{error}"
        );
        let missing = r#"{"data":{"repository":{}}}"#;
        assert!(parse_pr_list_page(missing).is_err(), "no nodes to read");
        assert!(parse_pr_list_page("not json").is_err());
        let empty = r#"{"data":{"repository":{"pullRequests":{"nodes":[]}}}}"#;
        assert!(
            parse_pr_list_page(empty).expect("an answer").is_empty(),
            "an empty page is an answer, not a failure"
        );
    }

    #[test]
    fn each_filter_names_the_collection_it_asks_for() {
        assert_eq!(pr_states(StateFilter::Open), ["OPEN"]);
        assert_eq!(
            pr_states(StateFilter::Closed),
            ["CLOSED", "MERGED"],
            "a merged PR is not in the open collection and must still be listed"
        );
        assert_eq!(pr_states(StateFilter::All), ["OPEN", "CLOSED", "MERGED"]);
    }

    /// The same guard the Issue query carries: an unused declared variable is
    /// a query GitHub refuses, and no fixture would notice.
    #[test]
    fn every_declared_variable_is_used_by_the_pr_query() {
        let (declaration, body) = PR_LIST_QUERY
            .split_once(") {")
            .expect("a variable declaration");
        let declared: Vec<&str> = declaration
            .split('$')
            .skip(1)
            .map(|variable| variable.split(':').next().unwrap_or_default().trim())
            .collect();
        assert_eq!(declared, ["owner", "name", "states"]);
        for name in declared {
            assert!(
                body.contains(&format!("${name}")),
                "${name} is declared but never used"
            );
        }
    }
}
