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

/// Run one open-PR fetch against `script` and fold it into the previous list.
fn fetch_into_cache(script: &str) -> (Vec<PullRequest>, Option<PrFetchError>) {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture(&pr_script(script));
    let mut cache = previous();
    let outcome = apply_pr_fetch(&mut cache, list_prs(&workdir, StateFilter::Open));
    (cache, outcome.error)
}

/// A page with no pull requests in it — the L1 shape, not gh's flat array.
const EMPTY_PAGE: &str = r#"echo '{"data":{"repository":{"pullRequests":{"nodes":[]}}}}'"#;
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
case "$*" in *"first: 100"*) ;; *) echo "unbounded page: $*" >&2; exit 1 ;; esac
case "$*" in *"comments { totalCount }"*) ;; *) echo "no comment total: $*" >&2; exit 1 ;; esac
case "$*" in *"states[]=OPEN"*) ;; *) echo "no state asked for: $*" >&2; exit 1 ;; esac
cat <<'JSON'
{"data":{"repository":{"pullRequests":{"nodes":[
  {"number":1,"title":"small","state":"OPEN","headRefName":"h","headRefOid":"sha",
   "baseRefName":"main","isDraft":false,"url":"https://github.com/o/r/pull/1",
   "comments":{"totalCount":3}}
]}}}}
JSON
"#;
    let (_root, workdir, _restore) = fixture(&pr_script(script));
    let prs = list_prs(&workdir, StateFilter::Open).expect("lightweight list");
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
    let error = list_prs(&workdir, StateFilter::Open).expect_err("both attempts fail");
    assert!(error.is_gateway_timeout());
    assert_eq!(std::fs::read_to_string(&count).unwrap().trim(), "2");

    std::fs::write(&count, "0\n").unwrap();
    fake_gh(&bin, &counting(AUTH));
    assert!(matches!(
        list_prs(&workdir, StateFilter::Open),
        Err(PrFetchError::Auth(_))
    ));
    assert_eq!(
        std::fs::read_to_string(&count).unwrap().trim(),
        "1",
        "only a gateway timeout is retried"
    );
}

/// (b) The one answer that may empty the list: `gh` said there are none.
#[test]
fn a_genuine_empty_response_is_the_only_thing_that_empties_the_list() {
    let (cache, error) = fetch_into_cache(EMPTY_PAGE);
    assert!(error.is_none(), "an empty list is not an error: {error:?}");
    assert!(cache.is_empty(), "a real empty response replaces the list");
}

#[test]
fn successful_l1_refresh_preserves_details_for_the_same_head() {
    let mut cache = kagi_git::github::parse_pr_list(
        r#"[{"number":7,"title":"old","headRefOid":"sha","body":"kept","changedFiles":4,"additions":9,"deletions":2,"mergeable":"CONFLICTING","statusCheckRollup":[{"name":"ci","conclusion":"FAILURE"}]}]"#,
    )
    .unwrap();
    let listed = kagi_git::github::parse_pr_list(
        r#"[{"number":7,"title":"new","headRefOid":"sha","updatedAt":"now"}]"#,
    )
    .unwrap();
    let outcome = apply_pr_fetch(&mut cache, Ok(listed));
    assert!(outcome.changed);
    assert_eq!(cache[0].title, "new");
    assert_eq!(cache[0].body, "kept");
    assert_eq!(cache[0].changed_files, 4);
    assert_eq!(cache[0].checks.len(), 1);
    assert_eq!(
        cache[0].mergeable,
        kagi_domain::github::Mergeable::Conflicting
    );
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
    let script = r#"
for arg in "$@"; do
  case "$arg" in query=*) ;; *) printf '%s ' "$arg" >> gh-args.log ;; esac
done
printf '\n' >> gh-args.log
cat <<'JSON'
{"data":{"repository":{"pullRequests":{"nodes":[
  {"number":5,"title":"landed","state":"MERGED","headRefName":"h","headRefOid":"s",
   "url":"https://github.com/o/r/pull/5","comments":{"totalCount":9}}
]}}}}
JSON
"#;
    let (_root, workdir, _restore) = fixture(&pr_script(script));
    for (state, expected) in [
        (StateFilter::Open, "-f states[]=OPEN "),
        (
            StateFilter::Closed,
            "-f states[]=CLOSED -f states[]=MERGED ",
        ),
        (
            StateFilter::All,
            "-f states[]=OPEN -f states[]=CLOSED -f states[]=MERGED ",
        ),
    ] {
        let prs = list_prs(&workdir, state).expect("page");
        assert_eq!(prs.len(), 1);
        assert_eq!(
            prs[0].state,
            kagi_domain::github::IssueState::Closed,
            "a merged PR reads as closed, the strip has no third state"
        );
        assert_eq!(prs[0].comment_count, 9);
        let recorded =
            std::fs::read_to_string(workdir.join("gh-args.log")).expect("recorded requests");
        let last = recorded.lines().last().expect("one request per fetch");
        assert!(last.contains(expected), "{state:?} asked for: {last}");
        assert!(
            last.contains("--hostname github.com") && last.contains("owner=o"),
            "{last}"
        );
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
    let outcome = apply_pr_fetch(&mut cache, list_prs(&workdir, StateFilter::Open));
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
    let error = issue_detail(&workdir, 404).expect_err("missing issue");
    assert!(matches!(error, PrFetchError::NotFound(_)), "{error:?}");
}
