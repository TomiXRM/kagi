//! The `gh pr list` fetch contract (#506).
//!
//! Split out of `github.rs`: the PR list is read on a 60s ticker and by the
//! Branch Cleanup scan, and the question "is this an empty list or a failed
//! fetch?" is the whole reason both callers exist. `github.rs` re-exports
//! everything here, so the public path stays `kagi_git::github::*`.

use std::path::Path;

use kagi_domain::github::PullRequest;

use crate::github::{parse_pr_list, FIELDS};

/// Why a `gh` PR fetch produced no list (#506).
///
/// `Ok(prs)` — including `Ok(vec![])` — is the **only** answer that is evidence
/// about the repository. Every variant here means "we did not learn what the
/// pull requests are", so callers keep the list they already had. Folding all
/// of these into `Ok(vec![])` made an expired token and an offline machine look
/// exactly like "no pull requests", and wiped the previously fetched list.
///
/// ADR-0177's rule applies: an unproven outcome stays unproven — [`Unknown`]
/// is never downgraded to a definite "there are none".
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
    /// Non-zero exit we could not classify (rate limit, server error, a future
    /// `gh` message). Treated exactly like a transport failure: keep the data.
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
            | PrFetchError::Unknown(m) => m,
        }
    }

    /// True for the one verdict that means "there is genuinely nothing here",
    /// so a cached list may be cleared.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, PrFetchError::Unavailable(_))
    }

    fn kind(&self) -> &'static str {
        match self {
            PrFetchError::Unavailable(_) => "unavailable",
            PrFetchError::Auth(_) => "auth",
            PrFetchError::Network(_) => "network",
            PrFetchError::Invalid(_) => "invalid",
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
    if code == Some(4) || has(AUTH_MARKERS) {
        PrFetchError::Auth(detail)
    } else if has(NETWORK_MARKERS) {
        PrFetchError::Network(detail)
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
            let changed = *cache != prs;
            *cache = prs;
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

/// One `gh pr list` call, classified. The shared half of [`list_open_prs`] and
/// [`list_merged_prs`] so both carry the same contract (#506).
fn fetch_prs(workdir: &Path, args: &[&str]) -> Result<Vec<PullRequest>, PrFetchError> {
    let out = crate::cli::gh_command()
        .args(args)
        .current_dir(workdir)
        .output()
        .map_err(|e| PrFetchError::Unknown(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Err(classify_gh_failure(
            out.status.code(),
            &String::from_utf8_lossy(&out.stderr),
        ));
    }
    parse_pr_list(&String::from_utf8_lossy(&out.stdout))
        .map_err(|e| PrFetchError::Invalid(e.to_string()))
}

/// Open PRs for the repository at `workdir`, newest-updated first.
///
/// `Ok(vec![])` means the repository really has no open pull requests — the
/// only answer that may replace a cached list. Every failure is classified
/// ([`PrFetchError`]) so an expired token or an offline machine keeps the last
/// good data instead of being shown as an empty inbox (#506).
pub fn list_open_prs(workdir: &Path) -> Result<Vec<PullRequest>, PrFetchError> {
    fetch_prs(
        workdir,
        &[
            "pr", "list", "--state", "open", "--limit", "100", "--json", FIELDS,
        ],
    )
}

/// Recently **merged** PRs, keyed by their head branch by the caller.
///
/// A separate call from [`list_open_prs`] because Branch Cleanup asks the
/// opposite question: its rows are branches that are already merged, so their
/// pull requests are by definition *not* open and never appear in that list.
///
/// Only the fields the cleanup table shows are requested — number, title,
/// author, head branch — so this stays one cheap call rather than the full
/// `FIELDS` set with its per-PR check rollup.
///
/// Same contract as [`list_open_prs`] (#506): Branch Cleanup shows this as
/// *evidence* that a branch was merged through a PR, so a failed fetch must not
/// arrive as "this branch has no PR".
pub fn list_merged_prs(workdir: &Path, limit: usize) -> Result<Vec<PullRequest>, PrFetchError> {
    let limit = limit.to_string();
    fetch_prs(
        workdir,
        &[
            "pr",
            "list",
            "--state",
            "merged",
            "--limit",
            &limit,
            "--json",
            "number,title,headRefName,author",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::parse_pr_list;

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
                PrFetchError::Unknown(String::new()),
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
