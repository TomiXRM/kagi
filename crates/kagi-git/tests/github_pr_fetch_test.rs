//! #506 — an empty PR list and a failed PR fetch are different answers.
//!
//! Every non-zero `gh` exit used to become `Ok(vec![])`, so an expired token or
//! an offline machine was indistinguishable from "this repo has no pull
//! requests" — and the previously fetched list was wiped by the next tick.
//!
//! Each test drives the real transport against a stand-in `gh` (the fixture
//! pattern from `transport_recording_test`), then folds the result into a cache
//! that already holds a PR — exactly what the sidebar refresh and the Branch
//! Cleanup scan do. The assertion is on the surviving cache.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use kagi_domain::github::PullRequest;
use kagi_domain::list_filter::StateFilter;
use kagi_git::github::{
    apply_pr_fetch, issue_detail, list_issues, list_merged_prs, list_prs, pr_body_detail,
    pr_status_detail, PrFetchError,
};

/// PATH is process-global; the tests in this binary share it.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const PREVIOUS_JSON: &str = r#"[{"number":506,"title":"previous fetch",
  "headRefName":"feat/prev","headRefOid":"aaaa","baseRefName":"main",
  "isDraft":false,"mergeable":"MERGEABLE","author":{"login":"a"}}]"#;

struct Environment(Option<OsString>);

impl Drop for Environment {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

impl Environment {
    fn install(bin: &Path) -> Self {
        let restore = Environment(std::env::var_os("PATH"));
        let mut paths = vec![bin.to_path_buf()];
        paths.extend(std::env::split_paths(
            &restore.0.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        restore
    }
}

/// A stand-in `gh` whose body is the whole script.
fn fake_gh(bin: &Path, body: &str) {
    let gh = bin.join("gh");
    std::fs::write(&gh, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// bin + workdir under one tempdir, with the fake `gh` first on PATH.
fn fixture(script: &str) -> (tempfile::TempDir, std::path::PathBuf, Environment) {
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir) = (root.path().join("bin"), root.path().join("repo"));
    for dir in [&bin, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let restore = Environment::install(&bin);
    fake_gh(&bin, script);
    (root, workdir, restore)
}

/// The cache as it stands after a successful earlier fetch.
fn previous() -> Vec<PullRequest> {
    kagi_git::github::parse_pr_list(PREVIOUS_JSON).unwrap()
}

/// A stand-in `gh` that resolves the repository — every PR list read starts
/// with that, exactly as the Issue list does — and answers the L1 page with
/// `body`.
fn pr_script(body: &str) -> String {
    format!(
        r#"case "$*" in
"repo view --json url") echo '{{"url":"https://github.com/o/r"}}'; exit 0 ;;
esac
{body}"#
    )
}

fn pr_page(
    repo: &str,
    numbers: std::ops::Range<u64>,
    state: &str,
    head: &str,
    has_next: bool,
    cursor: Option<&str>,
) -> String {
    let nodes: Vec<_> = numbers
        .map(|number| {
            serde_json::json!({
                "number": number, "title": format!("PR {number}"), "state": state,
                "url": format!("https://{repo}/pull/{number}"), "headRefOid": head,
                "comments": {"totalCount": number}
            })
        })
        .collect();
    serde_json::json!({"data": {"repository": {"pullRequests": {
        "nodes": nodes,
        "pageInfo": {"hasNextPage": has_next, "endCursor": cursor}
    }}}})
    .to_string()
}

/// Run one open-PR fetch against `script` and fold it into the previous list.
fn fetch_into_cache(script: &str) -> (Vec<PullRequest>, Option<PrFetchError>) {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture(&pr_script(script));
    let mut cache = previous();
    let fetched = list_prs(&workdir, None, None, StateFilter::Open);
    let outcome = apply_pr_fetch(&mut cache, fetched.map(|snapshot| snapshot.prs));
    (cache, outcome.error)
}

/// A page with no pull requests in it — the L1 shape, not gh's flat array.
const EMPTY_PAGE: &str = r#"echo '{"data":{"repository":{"pullRequests":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}'"#;
/// The flat `gh pr list --json` shape Branch Cleanup's merged evidence reads.
const EMPTY: &str = "echo '[]'";
const AUTH: &str =
    "echo 'gh: To get started with GitHub CLI, please run: gh auth login' >&2; exit 4";
const NETWORK: &str = "echo 'dial tcp: lookup api.github.com: no such host' >&2; exit 1";
const INVALID: &str = "echo 'not json at all'";
const UNAVAILABLE: &str = "echo 'none of the git remotes configured for this repository point to a known GitHub host' >&2; exit 1";
const RATE_LIMITED: &str = "echo 'HTTP 429: API rate limit exceeded' >&2; exit 1";

#[test]
fn l1_requests_only_lightweight_fields() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
case "$*" in
  *statusCheckRollup*|*mergeable*|*body*|*changedFiles*|*additions*|*deletions*)
    echo "heavy field in L1: $*" >&2; exit 1 ;;
esac
cat <<'JSON'
{"data":{"repository":{"pullRequests":{"nodes":[
  {"number":1,"title":"small","state":"OPEN","headRefName":"h","headRefOid":"sha",
   "baseRefName":"main","isDraft":false,"url":"https://github.com/o/r/pull/1",
   "comments":{"totalCount":3}}
],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}
JSON
"#;
    let (_root, workdir, _restore) = fixture(&pr_script(script));
    let snapshot = list_prs(&workdir, None, None, StateFilter::Open).expect("lightweight list");
    assert_eq!(snapshot.base_repo, "github.com/o/r");
    assert_eq!(snapshot.next_cursor, None);
    let prs = snapshot.prs;
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].head_sha, "sha");
    assert_eq!(prs[0].state, kagi_domain::github::IssueState::Open);
    assert_eq!(
        prs[0].comment_count, 3,
        "the comments sort orders by a real total, not a guess"
    );
    assert!(prs[0].checks.is_empty(), "checks are the L2 read");
}

#[test]
fn l2_and_l3_are_individual_repo_addressed_reads() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
case "$*" in
  "pr view -R ghe.example/acme/widgets 42 --json number,headRefOid,statusCheckRollup,mergeable")
    echo '{"number":42,"headRefOid":"sha","statusCheckRollup":[],"mergeable":"MERGEABLE"}' ;;
  "pr view -R ghe.example/acme/widgets 42 --json number,headRefOid,updatedAt,body,changedFiles,additions,deletions")
    echo '{"number":42,"headRefOid":"sha","updatedAt":"t","body":"","changedFiles":0,"additions":0,"deletions":0}' ;;
  *) echo "wrong command: $*" >&2; exit 1 ;;
esac
"#;
    let (_root, workdir, _restore) = fixture(script);
    let status = pr_status_detail(&workdir, "ghe.example/acme/widgets", 42).unwrap();
    let body = pr_body_detail(&workdir, "ghe.example/acme/widgets", 42).unwrap();
    assert_eq!(status.head_sha, "sha");
    assert!(status.checks.is_empty());
    assert_eq!(body.changed_files, 0);
    assert!(body.body.is_empty());
}

#[test]
fn l1_retries_a_504_once_and_no_other_failure() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir) = (root.path().join("bin"), root.path().join("repo"));
    for dir in [&bin, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let count = root.path().join("count");
    // Only the page request is counted: resolving the repository is not the
    // call the retry rule is about.
    let counting = |body: &str| {
        pr_script(&format!(
            r#"count=$(cat '{count}' 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > '{count}'
{body}"#,
            count = count.display()
        ))
    };
    let _restore = Environment::install(&bin);
    fake_gh(
        &bin,
        &counting("echo 'HTTP 504: Gateway Timeout' >&2\nexit 1"),
    );
    let error = list_prs(&workdir, None, None, StateFilter::Open).expect_err("both attempts fail");
    assert!(error.is_gateway_timeout());
    assert_eq!(std::fs::read_to_string(&count).unwrap().trim(), "2");

    for body in [AUTH, NETWORK, RATE_LIMITED, INVALID] {
        std::fs::write(&count, "0\n").unwrap();
        fake_gh(&bin, &counting(body));
        let error = list_prs(&workdir, None, None, StateFilter::Open).expect_err("failed page");
        assert!(!error.is_gateway_timeout());
        assert_eq!(std::fs::read_to_string(&count).unwrap().trim(), "1");
    }
}

/// (b) The one answer that may empty the list: `gh` said there are none.
#[test]
fn a_genuine_empty_response_is_the_only_thing_that_empties_the_list() {
    let (cache, error) = fetch_into_cache(EMPTY_PAGE);
    assert!(error.is_none(), "an empty list is not an error: {error:?}");
    assert!(cache.is_empty(), "a real empty response replaces the list");
}

#[test]
fn successful_l1_refresh_preserves_details_only_for_the_same_repository_and_head() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = kagi_git::github::parse_pr_list(
        r#"[{"number":7,"url":"https://github.com/o/r/pull/7","title":"old","headRefOid":"sha","body":"kept","changedFiles":4,"additions":9,"deletions":2,"mergeable":"CONFLICTING","statusCheckRollup":[{"name":"ci","conclusion":"FAILURE"}]}]"#,
    ).unwrap();
    let page = |repo: &str, head: &str| pr_page(repo, 7..8, "OPEN", head, false, None);
    let (_root, workdir, _restore) = fixture(&pr_script(&format!(
        "echo '{}'",
        page("github.com/o/r", "sha")
    )));
    let fetched = list_prs(&workdir, None, None, StateFilter::Open);
    let outcome = apply_pr_fetch(&mut cache, fetched.map(|snapshot| snapshot.prs));
    assert!(outcome.changed);
    assert_eq!(cache[0].title, "PR 7");
    assert_eq!(cache[0].body, "kept");
    assert_eq!(cache[0].changed_files, 4);
    assert_eq!(cache[0].checks.len(), 1);
    assert_eq!(
        cache[0].mergeable,
        kagi_domain::github::Mergeable::Conflicting
    );

    for (repo, head) in [("github.com/o/r", "new-head"), ("ghe.example/o/r", "sha")] {
        let mut changed = cache.clone();
        let (_root, workdir, _restore) = fixture(&format!("echo '{}'", page(repo, head)));
        let fetched = list_prs(&workdir, Some(repo), None, StateFilter::Open);
        let outcome = apply_pr_fetch(&mut changed, fetched.map(|snapshot| snapshot.prs));
        assert!(outcome.error.is_none());
        assert!(changed[0].body.is_empty());
        assert!(changed[0].checks.is_empty());
        assert_eq!(changed[0].changed_files, 0);
    }
}

/// (c) An expired token keeps the last good list — the #506 bug.
#[test]
fn an_auth_failure_keeps_the_previous_list() {
    let (cache, error) = fetch_into_cache(AUTH);
    assert!(matches!(error, Some(PrFetchError::Auth(_))), "{error:?}");
    assert_eq!(cache.len(), 1, "the previous fetch survives");
    assert_eq!(cache[0].number, 506);
}

/// (d) So does an offline machine.
#[test]
fn a_network_failure_keeps_the_previous_list() {
    let (cache, error) = fetch_into_cache(NETWORK);
    assert!(matches!(error, Some(PrFetchError::Network(_))), "{error:?}");
    assert_eq!(cache.len(), 1);
}

/// (e) And output that does not parse.
#[test]
fn unparseable_output_keeps_the_previous_list() {
    let (cache, error) = fetch_into_cache(INVALID);
    assert!(matches!(error, Some(PrFetchError::Invalid(_))), "{error:?}");
    assert_eq!(cache.len(), 1);
}

/// (a) No GitHub remote is an *answer*: there is nothing to show, so the stale
/// list must not linger — but it is not reported as a failure either.
#[test]
fn no_github_remote_is_unavailable_and_clears_the_list() {
    let (cache, error) = fetch_into_cache(UNAVAILABLE);
    let error = error.expect("unavailable is reported");
    assert!(error.is_unavailable(), "{error:?}");
    assert!(cache.is_empty(), "nothing to show here");
}

/// A rate-limit response is classified explicitly and still keeps evidence.
#[test]
fn a_rate_limit_failure_is_classified_and_keeps_the_previous_list() {
    let (cache, error) = fetch_into_cache(RATE_LIMITED);
    assert!(
        matches!(error, Some(PrFetchError::RateLimited(_))),
        "{error:?}"
    );
    assert_eq!(cache.len(), 1);
}

/// Branch Cleanup's merged-PR evidence carries the same contract: a failed
/// fetch must not arrive as "this branch was merged without a pull request".
#[test]
fn merged_pr_evidence_is_kept_on_failure_and_replaced_only_by_an_answer() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let (_root, workdir, _restore) = fixture(AUTH);
    let mut cache = previous();
    let outcome = apply_pr_fetch(&mut cache, list_merged_prs(&workdir, 50));
    assert!(
        matches!(outcome.error, Some(PrFetchError::Auth(_))),
        "{:?}",
        outcome.error
    );
    assert!(!outcome.changed, "the evidence column is untouched");
    assert_eq!(cache.len(), 1, "the last known merged PRs survive");

    let (_root, workdir, _restore) = fixture(EMPTY);
    let outcome = apply_pr_fetch(&mut cache, list_merged_prs(&workdir, 50));
    assert!(outcome.error.is_none());
    assert!(outcome.changed);
    assert!(cache.is_empty(), "a real answer does replace it");
}

/// #753 — the state chip decides *which collection is fetched*. A closed or
/// merged pull request is not in the open collection at all, so no local
/// filter over an open-only list could ever produce one.
#[test]
fn pr_state_filters_fetch_the_collection_they_name() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = pr_script(&format!(
        r#"states=""
for arg in "$@"; do
  case "$arg" in states\[\]=*) states="$states $arg" ;; esac
done
case "$states" in
  " states[]=OPEN") echo '{open}' ;;
  " states[]=CLOSED states[]=MERGED") echo '{closed}' ;;
  " states[]=OPEN states[]=CLOSED states[]=MERGED") echo '{all}' ;;
  *) echo "invalid collection: $states" >&2; exit 1 ;;
esac"#,
        open = pr_page("github.com/o/r", 1..2, "OPEN", "s", false, None),
        closed = pr_page("github.com/o/r", 2..3, "MERGED", "s", false, None),
        all = pr_page("github.com/o/r", 3..4, "CLOSED", "s", false, None),
    ));
    let (_root, workdir, _restore) = fixture(&script);
    for (state, number, lifecycle) in [
        (StateFilter::Open, 1, kagi_domain::github::IssueState::Open),
        (
            StateFilter::Closed,
            2,
            kagi_domain::github::IssueState::Closed,
        ),
        (StateFilter::All, 3, kagi_domain::github::IssueState::Closed),
    ] {
        let snapshot = list_prs(&workdir, None, None, state).expect("collection page");
        assert_eq!(snapshot.prs.len(), 1);
        assert_eq!(snapshot.prs[0].number, number);
        assert_eq!(snapshot.prs[0].state, lifecycle);
        assert_eq!(snapshot.prs[0].comment_count, number as usize);
    }
}

/// A GraphQL `errors` block is a failed read, not a short list: the previous
/// list has to survive it like any other failure (#506).
#[test]
fn a_partial_page_keeps_the_previous_list() {
    let (cache, error) = fetch_into_cache(
        r#"echo '{"data":{"repository":null},"errors":[{"message":"Could not resolve to a Repository"}]}'"#,
    );
    assert!(
        matches!(&error, Some(PrFetchError::Invalid(detail)) if detail.contains("Could not resolve to a Repository")),
        "{error:?}"
    );
    assert_eq!(cache.len(), 1);

    let (cache, error) = fetch_into_cache(r#"echo '{"data":{"repository":{}}}'"#);
    assert!(matches!(error, Some(PrFetchError::Invalid(_))), "{error:?}");
    assert_eq!(cache.len(), 1, "a shapeless page is not an empty repo");
}

/// The PR list resolves its repository first. That leg carries the same
/// contract: a failure there is a failure, never an empty inbox.
#[test]
fn an_unresolvable_repository_keeps_the_previous_list() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture(AUTH);
    let mut cache = previous();
    let fetched = list_prs(&workdir, None, None, StateFilter::Open);
    let outcome = apply_pr_fetch(&mut cache, fetched.map(|snapshot| snapshot.prs));
    assert!(
        matches!(outcome.error, Some(PrFetchError::Auth(_))),
        "{:?}",
        outcome.error
    );
    assert_eq!(cache.len(), 1);
}

#[test]
fn issue_list_is_a_bounded_open_slice_and_excludes_pr_shaped_json() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
case "$*" in
"repo view --json url") echo '{"url":"https://github.com/o/r"}' ;;
api\ graphql*)
  case "$*" in *"mentions=repo:o/r is:issue is:open mentions:@me"*) ;; *) echo "missing mentions alias input: $*" >&2; exit 1 ;; esac
  case "$*" in *"states[]=OPEN"*) ;; *) echo "no state asked for: $*" >&2; exit 1 ;; esac
  cat <<'JSON'
{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
  {"number":12,"title":"issue","state":"OPEN","url":"https://github.com/o/r/issues/12","comments":{"totalCount":4}},
  {"number":13,"title":"pr","state":"OPEN","isPullRequest":true,"url":"https://github.com/o/r/pull/13"}
]}},"mentions":{"nodes":[{"number":12}]}}}
JSON
  ;;
*) echo "wrong command: $*" >&2; exit 1 ;;
esac
"#;
    let (_root, workdir, _restore) = fixture(script);
    let snapshot = list_issues(&workdir, None, None, StateFilter::Open).expect("issue list");
    assert_eq!(snapshot.issues.len(), 1);
    assert_eq!(snapshot.issues[0].number, 12);
    assert_eq!(snapshot.issues[0].comment_count, 4);
    assert_eq!(snapshot.mentioned_numbers, vec![12]);
    assert_eq!(snapshot.base_repo, "github.com/o/r");
    assert_eq!(
        snapshot.next_cursor, None,
        "a repository that fits in one page offers nothing more to load"
    );
}

#[test]
fn issue_refresh_reuses_frozen_repository_identity() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
case "$*" in
"repo view --json url") echo "unexpected identity refresh" >&2; exit 1 ;;
api\ graphql*)
  case "$*" in *"--hostname ghe.example"*"owner=acme"*"name=widgets"*) ;; *) echo "wrong frozen repository: $*" >&2; exit 1 ;; esac
  echo '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}},"mentions":{"nodes":[]}}}' ;;
*) echo "wrong command: $*" >&2; exit 1 ;;
esac
"#;
    let (_root, workdir, _restore) = fixture(script);
    let snapshot = list_issues(
        &workdir,
        Some("ghe.example/acme/widgets"),
        None,
        StateFilter::Open,
    )
    .expect("refresh uses frozen repository");
    assert!(snapshot.issues.is_empty());
    assert_eq!(snapshot.base_repo, "ghe.example/acme/widgets");
}

/// #753 — the Issue state chip selects the collection, and both halves of the
/// one request must name the same one: a `mentions:@me` alias still scoped to
/// open Issues would mark rows the closed list never fetched.
#[test]
fn issue_state_selects_the_collection_and_the_mention_alias_together() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
for arg in "$@"; do
  case "$arg" in query=*) ;; *) printf '%s ' "$arg" >> gh-args.log ;; esac
done
printf '\n' >> gh-args.log
case "$*" in
"repo view --json url") echo "unexpected identity refresh" >&2; exit 1 ;;
esac
echo '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
  {"number":12,"title":"done","state":"CLOSED","url":"https://github.com/o/r/issues/12"}
]}},"mentions":{"nodes":[{"number":12}]}}}'
"#;
    let (_root, workdir, _restore) = fixture(script);
    let repo = "github.com/o/r";
    for (state, states, alias) in [
        (StateFilter::Open, "-f states[]=OPEN ", "is:issue is:open"),
        (
            StateFilter::Closed,
            "-f states[]=CLOSED ",
            "is:issue is:closed",
        ),
        (
            StateFilter::All,
            "-f states[]=OPEN -f states[]=CLOSED ",
            "is:issue mentions:@me",
        ),
    ] {
        let snapshot = list_issues(&workdir, Some(repo), None, state).expect("page");
        assert_eq!(snapshot.issues[0].number, 12);
        assert_eq!(
            snapshot.issues[0].state,
            kagi_domain::github::IssueState::Closed,
            "the row's own state is the server's, whatever was asked for"
        );
        assert_eq!(snapshot.mentioned_numbers, vec![12]);
        let recorded =
            std::fs::read_to_string(workdir.join("gh-args.log")).expect("recorded requests");
        let last = recorded.lines().last().expect("one request per page");
        assert!(last.contains(states), "{state:?} asked for: {last}");
        assert!(
            last.contains(alias),
            "the mention alias must scope to the same collection: {last}"
        );
    }
}

/// #752 — a repository with more than 100 Issues is reachable one page at a
/// time. Each request carries the previous page's cursor, the page size, the
/// state collection and the frozen repository identity never move, and the
/// run ends exactly when the server says there is no further page.
#[test]
fn issue_pages_walk_a_repository_past_the_hundred_issue_page_size() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
for arg in "$@"; do
  case "$arg" in query=*) ;; *) printf '%s ' "$arg" >> gh-args.log ;; esac
done
printf '\n' >> gh-args.log
case "$*" in
  "repo view"*) echo "unexpected identity refresh: $*" >&2; exit 1 ;;
esac
case "$*" in
  *"first: 100, after: \$cursor"*) ;;
  *) echo "page request is not a cursored 100-issue slice: $*" >&2; exit 1 ;;
esac
nodes() {
  n=$1
  out=""
  while [ "$n" -le "$2" ]; do
    out="$out{\"number\":$n,\"title\":\"issue $n\",\"state\":\"OPEN\"},"
    n=$((n + 1))
  done
  printf '%s' "${out%,}"
}
page() {
  printf '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":%s,"endCursor":%s},"nodes":[%s]}},"mentions":{"nodes":[]}}}\n' \
    "$1" "$2" "$(nodes "$3" "$4")"
}
case "$*" in
  *"-F cursor=null"*) page true '"c1"' 1 100 ;;
  *"-f cursor=c1"*) page true '"c2"' 101 103 ;;
  *"-f cursor=c2"*) page false null 104 104 ;;
  *) echo "unexpected cursor: $*" >&2; exit 1 ;;
esac
"#;
    let (_root, workdir, _restore) = fixture(script);
    let repo = "ghe.example/acme/widgets";
    let numbers = |snapshot: &kagi_domain::github::IssueListSnapshot| {
        snapshot
            .issues
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>()
    };

    let first = list_issues(&workdir, Some(repo), None, StateFilter::All).expect("first page");
    assert_eq!(first.issues.len(), 100);
    assert_eq!(first.next_cursor.as_deref(), Some("c1"));

    let next = list_issues(
        &workdir,
        Some(repo),
        first.next_cursor.as_deref(),
        StateFilter::All,
    )
    .expect("next page");
    assert_eq!(numbers(&next), vec![101, 102, 103]);
    assert_eq!(next.next_cursor.as_deref(), Some("c2"));

    let last = list_issues(
        &workdir,
        Some(next.base_repo.as_str()),
        next.next_cursor.as_deref(),
        StateFilter::All,
    )
    .expect("final page");
    assert_eq!(numbers(&last), vec![104]);
    assert_eq!(
        last.next_cursor, None,
        "the server's last page is what ends the run"
    );

    let recorded = std::fs::read_to_string(workdir.join("gh-args.log")).expect("recorded requests");
    let calls: Vec<&str> = recorded.lines().collect();
    assert_eq!(calls.len(), 3, "one request per page: {recorded}");
    assert!(calls[0].contains("-F cursor=null"), "{}", calls[0]);
    assert!(calls[1].contains("-f cursor=c1 "), "{}", calls[1]);
    assert!(calls[2].contains("-f cursor=c2 "), "{}", calls[2]);
    for call in calls {
        assert!(
            call.contains("--hostname ghe.example")
                && call.contains("owner=acme")
                && call.contains("name=widgets")
                && call.contains("mentions=repo:acme/widgets is:issue mentions:@me")
                && call.contains("-f states[]=OPEN -f states[]=CLOSED"),
            "every page addresses the frozen repository and keeps the state and mentions alias: {call}"
        );
    }
}

/// A page that cannot advance is a failure, never a quietly truncated list: a
/// `hasNextPage` with no usable cursor, or a cursor that repeats the one just
/// requested, would otherwise append the same page forever.
#[test]
fn issue_pagination_rejects_pages_that_cannot_advance() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
printf 'call\n' >> gh-calls.log
case "$*" in
  *"-F cursor=null"*)
    echo '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":true,"endCursor":null},"nodes":[]}},"mentions":{"nodes":[]}}}' ;;
  *"-f cursor=stuck"*)
    echo '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":true,"endCursor":"stuck"},"nodes":[]}},"mentions":{"nodes":[]}}}' ;;
  *) echo "unexpected cursor: $*" >&2; exit 1 ;;
esac
"#;
    let (_root, workdir, _restore) = fixture(script);
    let repo = "ghe.example/acme/widgets";
    let calls = || {
        std::fs::read_to_string(workdir.join("gh-calls.log"))
            .map(|log| log.lines().count())
            .unwrap_or(0)
    };

    let missing =
        list_issues(&workdir, Some(repo), None, StateFilter::Open).expect_err("unusable endCursor");
    assert!(
        matches!(&missing, PrFetchError::Invalid(detail) if detail.contains("usable endCursor")),
        "{missing:?}"
    );
    let stuck = list_issues(&workdir, Some(repo), Some("stuck"), StateFilter::Open)
        .expect_err("repeated cursor");
    assert!(
        matches!(&stuck, PrFetchError::Invalid(detail) if detail.contains("did not advance")),
        "{stuck:?}"
    );

    assert_eq!(calls(), 2);
    let empty =
        list_issues(&workdir, Some(repo), Some("  "), StateFilter::Open).expect_err("empty cursor");
    assert!(
        matches!(&empty, PrFetchError::Invalid(detail) if detail.contains("empty issue page")),
        "{empty:?}"
    );
    assert_eq!(
        calls(),
        2,
        "an empty cursor is refused before it can refetch page one"
    );
}

#[test]
fn issue_detail_classifies_not_found_without_fabricating_data() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
test "$1 $2 $3" = "issue view 404" || { echo "wrong command: $*" >&2; exit 1; }
echo 'GraphQL: Could not resolve to an Issue with the number of 404.' >&2
exit 1
"#;
    let (_root, workdir, _restore) = fixture(script);
    let error = issue_detail(&workdir, Some("github.com/o/r"), 404).expect_err("missing issue");
    assert!(matches!(error, PrFetchError::NotFound(_)), "{error:?}");
}

/// #940 review P1: the detail of an issue is read from the repository the
/// list froze, even when `gh`'s default repository for the clone is another
/// one with an issue under the same number.
#[test]
fn issue_detail_reads_the_frozen_repository_not_the_default() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let script = r#"
case "$*" in
  "repo view --json url") echo '{"url":"https://github.com/acme/upstream"}' ;;
  "issue view 4 -R github.com/acme/local --json"*)
    echo '{"number":4,"title":"local four","state":"OPEN","url":"https://github.com/acme/local/issues/4","author":{"login":"a"},"assignees":[],"labels":[],"body":"","comments":[]}' ;;
  "issue view 4"*)
    echo '{"number":4,"title":"upstream four","state":"OPEN","url":"https://github.com/acme/upstream/issues/4","author":{"login":"a"},"assignees":[],"labels":[],"body":"","comments":[]}' ;;
  *) echo "unexpected gh $*" >&2; exit 1 ;;
esac
"#;
    let (_root, workdir, _restore) = fixture(script);
    let issue = issue_detail(&workdir, Some("github.com/acme/local"), 4).expect("issue #4");
    assert_eq!(issue.title, "local four");
}

#[test]
fn pr_pages_deliver_100_then_20_with_exact_cursor_and_frozen_host_and_state() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = "ghe.example/acme/widgets";
    for (state, states, cursor) in [
        (StateFilter::Open, " states[]=OPEN", "  opaque+/=  "),
        (
            StateFilter::Closed,
            " states[]=CLOSED states[]=MERGED",
            "null",
        ),
        (
            StateFilter::All,
            " states[]=OPEN states[]=CLOSED states[]=MERGED",
            "001",
        ),
    ] {
        let lifecycle = if state == StateFilter::Open {
            "OPEN"
        } else {
            "MERGED"
        };
        let script = format!(
            r#"case "$*" in
"repo view --json url")
  if [ -e resolved ]; then echo "continuation re-resolved destination" >&2; exit 1; fi
  touch resolved
  echo '{{"url":"https://{repo}"}}'
  exit 0 ;;
esac
host="" owner="" name="" states="" cursor="" cursor_flag="" previous=""
for arg in "$@"; do
  if [ "$previous" = "--hostname" ]; then host="$arg"; fi
  case "$arg" in
    owner=*) owner="$arg" ;;
    name=*) name="$arg" ;;
    states\[\]=*) states="$states $arg" ;;
    cursor=*) cursor="${{arg#cursor=}}"; cursor_flag="$previous" ;;
  esac
  previous="$arg"
done
if [ "$host" != "ghe.example" ] || [ "$owner" != "owner=acme" ] ||
   [ "$name" != "name=widgets" ] || [ "$states" != "{states}" ]; then
  echo "wrong destination or collection" >&2; exit 1
fi
count=$(cat page-count 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > page-count
case "$count" in
  1)
    [ "$cursor_flag" = "-F" ] && [ "$cursor" = "null" ] || exit 1
    echo '{first}' ;;
  2)
    [ "$cursor_flag" = "-f" ] && [ "$cursor" = "{cursor}" ] || exit 1
    echo '{last}' ;;
  *) echo "unbounded page drain" >&2; exit 1 ;;
esac"#,
            first = pr_page(repo, 1..101, lifecycle, "sha", true, Some(cursor)),
            last = pr_page(repo, 101..121, lifecycle, "sha", false, Some("terminal")),
        );
        let (_root, workdir, _restore) = fixture(&script);
        let first = list_prs(&workdir, None, None, state).expect("first bounded page");
        assert_eq!(first.prs.len(), 100);
        assert_eq!(first.base_repo, repo);
        assert_eq!(first.next_cursor.as_deref(), Some(cursor));
        assert_eq!(
            std::fs::read_to_string(workdir.join("page-count"))
                .unwrap()
                .trim(),
            "1"
        );
        let last = list_prs(
            &workdir,
            Some(&first.base_repo),
            first.next_cursor.as_deref(),
            state,
        )
        .expect("frozen continuation");
        assert_eq!(last.prs.len(), 20);
        assert_eq!(last.base_repo, first.base_repo);
        assert_eq!(last.next_cursor, None);
        let numbers: Vec<_> = first
            .prs
            .iter()
            .chain(&last.prs)
            .map(|pr| pr.number)
            .collect();
        assert_eq!(numbers, (1..121).collect::<Vec<_>>());
        assert_eq!(
            std::fs::read_to_string(workdir.join("page-count"))
                .unwrap()
                .trim(),
            "2"
        );
    }
}

#[test]
fn invalid_pr_pages_preserve_last_good_rows_and_allow_same_request_retry() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = "github.com/o/r";
    let cursor = "  opaque  ";
    let valid: serde_json::Value =
        serde_json::from_str(&pr_page(repo, 507..508, "OPEN", "sha", false, None)).unwrap();
    let mut failures = Vec::new();
    for info in [
        serde_json::Value::Null,
        serde_json::json!({"hasNextPage": "true", "endCursor": "next"}),
        serde_json::json!({"hasNextPage": true, "endCursor": null}),
        serde_json::json!({"hasNextPage": true, "endCursor": ""}),
        serde_json::json!({"hasNextPage": true, "endCursor": " \t "}),
        serde_json::json!({"hasNextPage": true, "endCursor": cursor}),
        serde_json::json!({"hasNextPage": false, "endCursor": 7}),
        serde_json::json!({"hasNextPage": false}),
    ] {
        let mut page = valid.clone();
        page["data"]["repository"]["pullRequests"]["pageInfo"] = info;
        failures.push(page);
    }
    let mut missing = valid.clone();
    missing["data"]["repository"]["pullRequests"]
        .as_object_mut()
        .unwrap()
        .remove("pageInfo");
    failures.push(missing);
    for url in [
        "https://github.com/other/r/pull/507",
        "https://ghe.example/o/r/pull/507",
        "",
    ] {
        let mut page = valid.clone();
        page["data"]["repository"]["pullRequests"]["nodes"][0]["url"] = url.into();
        failures.push(page);
    }
    let mut partial = valid.clone();
    partial["errors"] = serde_json::json!([{"message": "partial failure"}]);
    failures.push(partial);
    let mut invalid_node = valid.clone();
    invalid_node["data"]["repository"]["pullRequests"]["nodes"][0]["number"] = "bad".into();
    failures.push(invalid_node);
    for page in failures {
        let script = format!(
            r#"count=$(cat page-count 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > page-count
case "$count" in 1) echo '{page}' ;; 2) echo '{valid}' ;; *) exit 1 ;; esac"#
        );
        let (_root, workdir, _restore) = fixture(&script);
        let mut cache = previous();
        let fetched = list_prs(&workdir, Some(repo), Some(cursor), StateFilter::Open);
        let outcome = apply_pr_fetch(&mut cache, fetched.map(|snapshot| snapshot.prs));
        assert!(
            matches!(outcome.error, Some(PrFetchError::Invalid(_))),
            "{page}"
        );
        assert!(!outcome.changed);
        assert_eq!(cache, previous());
        assert_eq!(
            std::fs::read_to_string(workdir.join("page-count"))
                .unwrap()
                .trim(),
            "1"
        );
        let retry = list_prs(&workdir, Some(repo), Some(cursor), StateFilter::Open).expect("retry");
        assert_eq!(retry.prs[0].number, 507);
        assert_eq!(retry.base_repo, repo);
        assert_eq!(retry.next_cursor, None);
        assert_eq!(
            std::fs::read_to_string(workdir.join("page-count"))
                .unwrap()
                .trim(),
            "2"
        );
    }
}

#[test]
fn pr_continuations_reject_empty_or_unfrozen_identity_before_transport() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture("touch unexpected-transport; exit 1");
    for (repo, cursor) in [
        (None, Some("next")),
        (Some(""), Some("next")),
        (Some(" \t "), Some("next")),
        (Some("github.com/o/r"), Some("")),
        (Some("github.com/o/r"), Some(" \t ")),
    ] {
        assert!(matches!(
            list_prs(&workdir, repo, cursor, StateFilter::Open),
            Err(PrFetchError::Invalid(_))
        ));
    }
    assert!(!workdir.join("unexpected-transport").exists());
}

#[test]
fn a_504_continuation_retries_the_identical_frozen_request_and_delivers_the_page() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = "ghe.example/acme/widgets";
    let cursor = "  opaque+/=  ";
    let script = format!(
        r#"case "$*" in
repo*) echo "frozen continuation re-resolved" >&2; exit 1 ;;
esac
count=$(cat page-count 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > page-count
printf '%s\n' "$@" > "request-$count"
case "$count" in
  1) echo "HTTP 504: Gateway Timeout" >&2; exit 1 ;;
  2) echo '{page}' ;;
  *) exit 1 ;;
esac"#,
        page = pr_page(repo, 101..121, "MERGED", "sha", false, None),
    );
    let (_root, workdir, _restore) = fixture(&script);
    let page =
        list_prs(&workdir, Some(repo), Some(cursor), StateFilter::Closed).expect("one retry");
    assert_eq!(page.prs.len(), 20);
    assert_eq!(page.prs[0].number, 101);
    assert_eq!(page.base_repo, repo);
    assert_eq!(page.next_cursor, None);
    assert_eq!(
        std::fs::read_to_string(workdir.join("page-count"))
            .unwrap()
            .trim(),
        "2"
    );
    let first = std::fs::read_to_string(workdir.join("request-1")).unwrap();
    let second = std::fs::read_to_string(workdir.join("request-2")).unwrap();
    assert_eq!(first, second, "the retry must use the identical request");
}

#[test]
fn an_empty_final_pr_continuation_is_a_success_but_a_mixed_repository_page_is_not() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = "github.com/o/r";
    let (_root, workdir, _restore) = fixture(&format!(
        "echo '{}'",
        pr_page(repo, 0..0, "OPEN", "sha", false, None)
    ));
    let final_page = list_prs(&workdir, Some(repo), Some("after-100"), StateFilter::Open)
        .expect("a genuine empty final page");
    assert!(final_page.prs.is_empty());
    assert_eq!(final_page.base_repo, repo);
    assert_eq!(final_page.next_cursor, None);

    let mut mixed: serde_json::Value =
        serde_json::from_str(&pr_page(repo, 507..509, "OPEN", "sha", false, None)).unwrap();
    mixed["data"]["repository"]["pullRequests"]["nodes"][1]["url"] =
        "https://github.com/other/r/pull/508".into();
    let (_root, workdir, _restore) = fixture(&format!("echo '{mixed}'"));
    let mut cache = previous();
    let fetched = list_prs(&workdir, Some(repo), Some("after-100"), StateFilter::Open);
    let outcome = apply_pr_fetch(&mut cache, fetched.map(|snapshot| snapshot.prs));
    assert!(matches!(outcome.error, Some(PrFetchError::Invalid(_))));
    assert!(!outcome.changed);
    assert_eq!(
        cache,
        previous(),
        "do not publish the valid prefix of a mixed page"
    );
}

#[test]
fn an_empty_nonfinal_pr_page_preserves_last_good_rows_as_a_failed_read() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = "github.com/o/r";
    let (_root, workdir, _restore) = fixture(&format!(
        "echo '{}'",
        pr_page(repo, 0..0, "OPEN", "sha", true, Some("advanced"))
    ));
    let mut cache = previous();
    let fetched = list_prs(&workdir, Some(repo), Some("after-100"), StateFilter::Open);
    let outcome = apply_pr_fetch(&mut cache, fetched.map(|snapshot| snapshot.prs));
    assert!(matches!(outcome.error, Some(PrFetchError::Invalid(_))));
    assert!(!outcome.changed);
    assert_eq!(cache, previous());
}
