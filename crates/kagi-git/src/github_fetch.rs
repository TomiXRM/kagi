//! The GitHub list/detail fetch contract (#506).
//!
//! Split out of `github.rs`: the PR list is read on a 60s ticker and by the
//! Branch Cleanup scan, and the question "is this an empty list or a failed
//! fetch?" is the whole reason both callers exist. `github.rs` re-exports
//! everything here, so the public path stays `kagi_git::github::*`.

use std::path::Path;
use std::time::Duration;

use kagi_domain::github::{Issue, IssueListSnapshot, PullRequest};
use kagi_domain::list_filter::StateFilter;

use crate::github::{
    parse_pr_body_detail, parse_pr_status_detail, PR_BODY_FIELDS, PR_MERGED_FIELDS,
    PR_STATUS_FIELDS,
};
use crate::github_issue::{
    issue_states, mentions_scope, parse_issue_detail, parse_issue_list_snapshot,
    ISSUE_DETAIL_FIELDS, ISSUE_LIST_QUERY,
};
use crate::github_pr_list::{parse_pr_list, parse_pr_list_page, pr_states, PR_LIST_QUERY};

/// The bound every `gh` invocation runs under — reads here, and the `gh pr
/// comment` write in `github_comment`. One definition, so a write can never
/// end up unbounded because it was spelled somewhere else.
pub(crate) const GH_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a read-only `gh` fetch produced no data.
///
/// `Ok(data)` — including an empty list — is the only answer that is evidence
/// about the repository. Every variant here means the request did not produce
/// the requested GitHub data, so callers keep successful data they already
/// hold. PR callers retain the historical exception that `Unavailable` means
/// their sidebar evidence may be cleared; Issue callers treat every error as a
/// failure and preserve their last success.
///
/// ADR-0177's rule applies: an unproven outcome stays unproven — [`Unknown`]
/// is never downgraded to a definite empty answer.
///
/// [`Unknown`]: PrFetchError::Unknown
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrFetchError {
    /// No GitHub remote / not a GitHub repository: an answer, and the answer is
    /// "there is nothing here to show". The only failure that legitimately
    /// empties the list.
    Unavailable(String),
    /// Logged out, token expired, missing scope.
    Auth(String),
    /// DNS, TLS, timeout, refused connection — the request never got an answer.
    Network(String),
    /// `gh` answered but the JSON did not parse.
    Invalid(String),
    /// GitHub refused the request because the API budget was exhausted.
    RateLimited(String),
    /// The selected issue or repository no longer exists or is not visible.
    NotFound(String),
    /// Non-zero exit we could not classify (server error or a future `gh`
    /// message). Treated exactly like a transport failure: keep the data.
    Unknown(String),
}

impl PrFetchError {
    /// The raw `gh` stderr (or parser message) behind this verdict.
    pub fn detail(&self) -> &str {
        match self {
            PrFetchError::Unavailable(m)
            | PrFetchError::Auth(m)
            | PrFetchError::Network(m)
            | PrFetchError::Invalid(m)
            | PrFetchError::RateLimited(m)
            | PrFetchError::NotFound(m)
            | PrFetchError::Unknown(m) => m,
        }
    }

    /// True for the one verdict that means "there is genuinely nothing here",
    /// so a cached list may be cleared.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, PrFetchError::Unavailable(_))
    }

    /// GitHub's GraphQL front end timed out. Only the L1 list caller uses this
    /// to make one delayed retry; rate-limit responses are classified earlier
    /// and therefore never enter this path.
    pub fn is_gateway_timeout(&self) -> bool {
        let detail = self.detail().to_ascii_lowercase();
        matches!(self, PrFetchError::Network(_) | PrFetchError::Unknown(_))
            && (detail.contains("http 504") || detail.contains("gateway timeout"))
    }

    fn kind(&self) -> &'static str {
        match self {
            PrFetchError::Unavailable(_) => "unavailable",
            PrFetchError::Auth(_) => "auth",
            PrFetchError::Network(_) => "network",
            PrFetchError::Invalid(_) => "invalid",
            PrFetchError::RateLimited(_) => "rate-limited",
            PrFetchError::NotFound(_) => "not-found",
            PrFetchError::Unknown(_) => "unknown",
        }
    }
}

impl std::fmt::Display for PrFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind(), self.detail())
    }
}

/// Auth is checked first: a logged-out `gh` also prints "could not determine
/// base repository", and reading that as "no GitHub remote" is exactly the
/// mistake that clears the list on an expired token.
const AUTH_MARKERS: &[&str] = &[
    "gh auth login",
    "authentication",
    "not logged in",
    "unauthorized",
    "bad credentials",
    "http 401",
    "http 403",
    "insufficient scope",
    "sso",
];

const NETWORK_MARKERS: &[&str] = &[
    "dial tcp",
    "no such host",
    "connection refused",
    "connection reset",
    "network is unreachable",
    "i/o timeout",
    "timeout",
    "tls handshake",
    "could not resolve host",
    "temporary failure in name resolution",
];

const UNAVAILABLE_MARKERS: &[&str] = &[
    "none of the git remotes",
    "no git remotes found",
    "not a git repository",
    "could not determine base repository",
    "no default remote repository",
];

const RATE_LIMIT_MARKERS: &[&str] = &[
    "rate limit",
    "http 429",
    "api rate limit exceeded",
    "secondary rate limit",
];

const NOT_FOUND_MARKERS: &[&str] = &["http 404", "not found", "could not resolve to an issue"];

/// Classify a non-zero `gh` exit. Pure; unit-tested below.
///
/// Exit code 4 is `gh`'s documented "authentication required". Everything else
/// is decided on the message, and anything unrecognised stays [`Unknown`] —
/// guessing "no pull requests" is the failure mode this whole type exists for.
///
/// [`Unknown`]: PrFetchError::Unknown
pub fn classify_gh_failure(code: Option<i32>, stderr: &str) -> PrFetchError {
    let lower = stderr.to_ascii_lowercase();
    let has = |markers: &[&str]| markers.iter().any(|m| lower.contains(m));
    let detail = stderr.trim().to_string();
    if code == Some(4) {
        PrFetchError::Auth(detail)
    } else if has(RATE_LIMIT_MARKERS) {
        PrFetchError::RateLimited(detail)
    } else if has(AUTH_MARKERS) {
        PrFetchError::Auth(detail)
    } else if has(NETWORK_MARKERS) {
        PrFetchError::Network(detail)
    } else if has(NOT_FOUND_MARKERS) {
        PrFetchError::NotFound(detail)
    } else if has(UNAVAILABLE_MARKERS) {
        PrFetchError::Unavailable(detail)
    } else {
        PrFetchError::Unknown(detail)
    }
}

/// What one fetch did to a cached PR list.
pub struct PrFetchOutcome {
    /// The cache contents changed, so the caller should redraw.
    pub changed: bool,
    /// `Some` when the fetch did not answer. [`PrFetchError::Unavailable`] did
    /// empty the cache; every other variant left the last good data in place.
    pub error: Option<PrFetchError>,
}

/// Fold a fetch into the caller's cached list — the single place the #506 rule
/// lives, shared by the sidebar refresh and the Branch Cleanup scan.
pub fn apply_pr_fetch(
    cache: &mut Vec<PullRequest>,
    fetched: Result<Vec<PullRequest>, PrFetchError>,
) -> PrFetchOutcome {
    match fetched {
        Ok(prs) => {
            let changed = kagi_domain::github::apply_pr_list(cache, prs);
            PrFetchOutcome {
                changed,
                error: None,
            }
        }
        // No GitHub remote: "nothing to show" is the truth here, so a stale
        // list must not linger.
        Err(e) if e.is_unavailable() => {
            let changed = !cache.is_empty();
            cache.clear();
            PrFetchOutcome {
                changed,
                error: Some(e),
            }
        }
        Err(e) => PrFetchOutcome {
            changed: false,
            error: Some(e),
        },
    }
}

/// Run one read-only `gh` JSON request through the shared hardened command and
/// bounded subprocess runner.
pub(crate) fn fetch_json<T>(
    workdir: &Path,
    args: &[&str],
    parse: impl FnOnce(&str) -> Result<T, crate::GitError>,
) -> Result<T, PrFetchError> {
    let mut command = crate::cli::gh_command();
    command.args(args).current_dir(workdir);
    let out = crate::proc::run_child(&mut command, GH_TIMEOUT, None)
        .map_err(|e| PrFetchError::Unknown(format!("gh: {e}")))?;
    if let Err(error) = &out.status {
        return Err(PrFetchError::Network(format!("gh: {error}")));
    }
    if let Err(error) = &out.io {
        return Err(PrFetchError::Network(format!("gh: {error}")));
    }
    let code = out.status.as_ref().copied().unwrap_or_default();
    if code != 0 {
        return Err(classify_gh_failure(Some(code), &out.stderr_lossy()));
    }
    parse(&out.stdout_lossy()).map_err(|e| PrFetchError::Invalid(e.to_string()))
}

/// Pull requests for the repository at `workdir`, newest-updated first, in one
/// bounded 100-PR page.
///
/// `state` is the server-side half of the shared filter strip (#753): a closed
/// or merged pull request is not in the open collection at all, so the state
/// chip has to change *which collection is fetched*, never merely which rows
/// are drawn.
///
/// `Ok(vec![])` means the repository really has no pull requests in that
/// collection — the only answer that may replace a cached list. Every failure
/// is classified ([`PrFetchError`]) so an expired token or an offline machine
/// keeps the last good data instead of being shown as an empty inbox (#506).
pub fn list_prs(workdir: &Path, state: StateFilter) -> Result<Vec<PullRequest>, PrFetchError> {
    // gh's default-repository rules decide which repository this is, for the
    // reason the Issue list resolves it the same way: on a fork, `origin` is
    // not the repository the pull requests live in.
    let base_repo = repository_identity(workdir)?;
    let (host, owner, name) = split_identity(&base_repo)?;
    let owner_field = format!("owner={owner}");
    let name_field = format!("name={name}");
    let query_field = format!("query={PR_LIST_QUERY}");
    let states: Vec<String> = pr_states(state)
        .iter()
        .map(|state| format!("states[]={state}"))
        .collect();
    let mut args = vec![
        "api",
        "graphql",
        "--hostname",
        host,
        "-F",
        owner_field.as_str(),
        "-F",
        name_field.as_str(),
    ];
    for state in &states {
        args.extend(["-f", state.as_str()]);
    }
    args.extend(["-f", query_field.as_str()]);
    let first = fetch_json(workdir, &args, parse_pr_list_page);
    if first.as_ref().is_err_and(PrFetchError::is_gateway_timeout) {
        std::thread::sleep(l1_retry_delay());
        fetch_json(workdir, &args, parse_pr_list_page)
    } else {
        first
    }
}

/// `<host>/<owner>/<name>`, split for a GraphQL request. Anything else is an
/// identity kagi cannot address, and refusing it is better than sending a
/// request that names half a repository.
fn split_identity(base_repo: &str) -> Result<(&str, &str, &str), PrFetchError> {
    let mut parts = base_repo.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(host), Some(owner), Some(name), None) => Ok((host, owner, name)),
        _ => Err(PrFetchError::Invalid(format!(
            "invalid repository identity: {base_repo}"
        ))),
    }
}

fn l1_retry_delay() -> Duration {
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_millis() as u64)
        .unwrap_or(0)
        % 1_001;
    Duration::from_millis(1_000 + jitter)
}

/// Volatile checks and mergeability for one PR (L2).
pub fn pr_status_detail(
    workdir: &Path,
    base_repo: &str,
    number: u64,
) -> Result<kagi_domain::github::PrStatusDetail, PrFetchError> {
    let number = number.to_string();
    fetch_json(
        workdir,
        &[
            "pr",
            "view",
            "-R",
            base_repo,
            &number,
            "--json",
            PR_STATUS_FIELDS,
        ],
        parse_pr_status_detail,
    )
}

/// Body and aggregate statistics for the selected PR (L3).
pub fn pr_body_detail(
    workdir: &Path,
    base_repo: &str,
    number: u64,
) -> Result<kagi_domain::github::PrBodyDetail, PrFetchError> {
    let number = number.to_string();
    fetch_json(
        workdir,
        &[
            "pr",
            "view",
            "-R",
            base_repo,
            &number,
            "--json",
            PR_BODY_FIELDS,
        ],
        parse_pr_body_detail,
    )
}

/// Recently **merged** PRs, keyed by their head branch by the caller.
///
/// A separate, deliberately cheaper call than [`list_prs`]: Branch Cleanup
/// needs four columns for rows it already knows are merged, not the L1 page's
/// identity, review routing and comment totals.
///
/// Same contract as [`list_prs`] (#506): Branch Cleanup shows this as
/// *evidence* that a branch was merged through a PR, so a failed fetch must not
/// arrive as "this branch has no PR".
pub fn list_merged_prs(workdir: &Path, limit: usize) -> Result<Vec<PullRequest>, PrFetchError> {
    let limit = limit.to_string();
    fetch_json(
        workdir,
        &[
            "pr",
            "list",
            "--state",
            "merged",
            "--limit",
            &limit,
            "--json",
            PR_MERGED_FIELDS,
        ],
        parse_pr_list,
    )
}

/// One page of Issues for the repository at `workdir`, newest-updated first.
///
/// Each call is a bounded 100-Issue page, not a count or an exhaustive
/// repository history. `cursor` is `None` for the first page and the previous
/// snapshot's [`IssueListSnapshot::next_cursor`] for every page after it, so a
/// repository with more than 100 Issues is reachable without ever widening one
/// request (#752). The repository list and the one `mentions:@me` search alias
/// share one GraphQL request; the parser rejects PR-shaped nodes defensively.
///
/// `state` selects the collection both halves of that request read (#753):
/// the repository page and the mention search always name the same states, so
/// the Mentioned tab can never mark a row the list did not fetch.
///
/// `frozen_base_repo` pins the identity across the whole pagination run: page
/// two of a repository must not be able to land on a different repository
/// because `gh`'s default resolved differently in between.
///
/// [`IssueListSnapshot::next_cursor`]: kagi_domain::github::IssueListSnapshot::next_cursor
pub fn list_issues(
    workdir: &Path,
    frozen_base_repo: Option<&str>,
    cursor: Option<&str>,
    state: StateFilter,
) -> Result<IssueListSnapshot, PrFetchError> {
    // A snapshot never carries an empty cursor, so an empty one here is a
    // caller bug. Silently restarting at page one would duplicate the first
    // page into the list instead of reporting it. Normalising once also means
    // the request and the advance check compare the same cursor.
    let cursor = match cursor.map(str::trim) {
        Some("") => return Err(PrFetchError::Invalid("empty issue page cursor".into())),
        cursor => cursor,
    };
    // Keep gh's canonical default-repository semantics (including
    // `gh repo set-default` and forks). Guessing from `origin` can address a
    // different repository and would freeze the wrong mutation destination.
    let base_repo = match frozen_base_repo
        .map(str::trim)
        .filter(|repo| !repo.is_empty())
    {
        Some(repo) => repo.to_string(),
        None => repository_identity(workdir)?,
    };
    let (host, owner, name) = split_identity(&base_repo)?;
    let owner_field = format!("owner={owner}");
    let name_field = format!("name={name}");
    let mentions_field = format!(
        "mentions=repo:{owner}/{name} is:issue{} mentions:@me",
        mentions_scope(state)
    );
    let query_field = format!("query={ISSUE_LIST_QUERY}");
    let states: Vec<String> = issue_states(state)
        .iter()
        .map(|state| format!("states[]={state}"))
        .collect();
    // `-F` types its value, which is how the first page sends a real JSON
    // `null` for the nullable `$cursor`. A real cursor goes through `-f`, which
    // never types anything: an opaque cursor that happened to read as a number
    // or as `null` must still arrive as the string GitHub issued.
    let (cursor_flag, cursor_field) = match cursor {
        Some(cursor) => ("-f", format!("cursor={cursor}")),
        None => ("-F", "cursor=null".to_string()),
    };
    let mut args = vec![
        "api",
        "graphql",
        "--hostname",
        host,
        "-F",
        owner_field.as_str(),
        "-F",
        name_field.as_str(),
        "-F",
        mentions_field.as_str(),
        cursor_flag,
        cursor_field.as_str(),
    ];
    for state in &states {
        args.extend(["-f", state.as_str()]);
    }
    args.extend(["-f", query_field.as_str()]);
    fetch_json(workdir, &args, |json| {
        parse_issue_list_snapshot(json, &base_repo, cursor)
    })
}

/// Resolve the GitHub destination with gh's default-repository rules — the
/// repository both list reads address. The Issue caller carries this identity
/// through the same owner/generation as the GraphQL list result; mutations
/// never re-resolve it at dispatch.
pub fn repository_identity(workdir: &Path) -> Result<String, PrFetchError> {
    fetch_json(workdir, &["repo", "view", "--json", "url"], |json| {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| crate::GitError::Other(e.to_string()))?;
        value
            .get("url")
            .and_then(|url| url.as_str())
            .and_then(crate::backend::remote_ref::repo_identity)
            .ok_or_else(|| crate::GitError::Other("missing GitHub repository identity".into()))
    })
}

/// Full read-only data for one selected issue, including body and comments.
pub fn issue_detail(workdir: &Path, number: u64) -> Result<Issue, PrFetchError> {
    let number = number.to_string();
    fetch_json(
        workdir,
        &["issue", "view", &number, "--json", ISSUE_DETAIL_FIELDS],
        parse_issue_detail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `gh` validates `--json` field names *before* it calls the API and exits
    /// on the first unknown one, so a bad name in any field set breaks that PR
    /// fetch above. Fixtures cannot catch that — they hand finished JSON
    /// straight to `parse_pr_list` (#701 final review 3, where
    /// `baseRepository` shipped). `--limit 0` is rejected *after* field
    /// validation, so this negotiates the names without touching the network.
    /// The L1 page is GraphQL and is not negotiable this way; its shape is
    /// covered by `parse_pr_list_page`'s tests and by the server itself.
    ///
    /// The negative control runs **first** and gates the whole test: without
    /// it a `gh` that never reports unknown fields would make the real
    /// assertion vacuously true, and where `gh` cannot answer at all — absent,
    /// or refusing before it validates anything, as on an unauthenticated CI
    /// runner — there is nothing to negotiate with and the test skips.
    #[test]
    fn field_names_are_accepted_by_gh() {
        const UNKNOWN: &str = "Unknown JSON field";
        let ask = |fields: &str| {
            std::process::Command::new("gh")
                .args(["pr", "list", "--json", fields, "--limit", "0"])
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stderr).into_owned())
        };
        if !ask("number,noSuchFieldAtAll").is_some_and(|out| out.contains(UNKNOWN)) {
            return; // this `gh` cannot tell us; a developer machine's can
        }
        for fields in [PR_MERGED_FIELDS, PR_STATUS_FIELDS, PR_BODY_FIELDS] {
            let stderr = ask(fields).expect("gh answered a moment ago");
            assert!(
                !stderr.contains(UNKNOWN),
                "PR fields name something gh does not have: {stderr}"
            );
        }
    }

    const SAMPLE: &str = r#"[
      {"number":236,"title":"a","headRefName":"feat/a","baseRefName":"main",
       "isDraft":false,"mergeable":"MERGEABLE","author":{"login":"x"}},
      {"number":240,"title":"b","headRefName":"feat/b","baseRefName":"main",
       "isDraft":true,"mergeable":"CONFLICTING","author":{"login":"y"}}
    ]"#;

    /// #506: the classification an empty-vs-failure decision rests on. Auth
    /// wins over the "no base repository" wording a logged-out `gh` also
    /// prints, and anything unrecognised stays `Unknown` — never "no PRs".
    #[test]
    fn classifies_gh_failures_without_guessing() {
        let cases: &[(Option<i32>, &str, PrFetchError)] = &[
            (
                Some(4),
                "could not determine base repository",
                PrFetchError::Auth(String::new()),
            ),
            (
                Some(1),
                "gh: To get started with GitHub CLI, please run: gh auth login",
                PrFetchError::Auth(String::new()),
            ),
            (
                Some(1),
                "dial tcp: lookup api.github.com: no such host",
                PrFetchError::Network(String::new()),
            ),
            (
                Some(1),
                "none of the git remotes configured for this repository point to a known GitHub host",
                PrFetchError::Unavailable(String::new()),
            ),
            (
                Some(1),
                "HTTP 429: API rate limit exceeded",
                PrFetchError::RateLimited(String::new()),
            ),
            (
                Some(1),
                "GraphQL: Could not resolve to an Issue with the number of 404.",
                PrFetchError::NotFound(String::new()),
            ),
        ];
        for (code, stderr, expected) in cases {
            let got = classify_gh_failure(*code, stderr);
            assert_eq!(
                std::mem::discriminant(&got),
                std::mem::discriminant(expected),
                "{stderr}"
            );
            assert_eq!(got.detail(), *stderr, "the reason is kept verbatim");
        }
    }

    #[test]
    fn only_an_answer_replaces_a_cached_list() {
        let cached = parse_pr_list(SAMPLE).unwrap();
        let mut cache = cached.clone();
        let kept = apply_pr_fetch(&mut cache, Err(PrFetchError::Network("offline".into())));
        assert!(!kept.changed);
        assert_eq!(cache, cached, "a failure never empties the list");

        let cleared = apply_pr_fetch(
            &mut cache,
            Err(PrFetchError::Unavailable("no remote".into())),
        );
        assert!(cleared.changed && cache.is_empty(), "no remote → nothing");

        let filled = apply_pr_fetch(&mut cache, Ok(cached.clone()));
        assert!(filled.changed && filled.error.is_none());
        assert_eq!(cache, cached);
    }
}
