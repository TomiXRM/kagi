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
use kagi_git::github::{apply_pr_fetch, list_merged_prs, list_open_prs, PrFetchError};

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

/// Run one fetch against `script` and fold it into the previous list.
fn fetch_into_cache(script: &str) -> (Vec<PullRequest>, Option<PrFetchError>) {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture(script);
    let mut cache = previous();
    let outcome = apply_pr_fetch(&mut cache, list_open_prs(&workdir));
    (cache, outcome.error)
}

const EMPTY: &str = "echo '[]'";
const AUTH: &str =
    "echo 'gh: To get started with GitHub CLI, please run: gh auth login' >&2; exit 4";
const NETWORK: &str = "echo 'dial tcp: lookup api.github.com: no such host' >&2; exit 1";
const INVALID: &str = "echo 'not json at all'";
const UNAVAILABLE: &str = "echo 'none of the git remotes configured for this repository point to a known GitHub host' >&2; exit 1";
const RATE_LIMITED: &str = "echo 'HTTP 429: API rate limit exceeded' >&2; exit 1";

/// (b) The one answer that may empty the list: `gh` said there are none.
#[test]
fn a_genuine_empty_response_is_the_only_thing_that_empties_the_list() {
    let (cache, error) = fetch_into_cache(EMPTY);
    assert!(error.is_none(), "an empty list is not an error: {error:?}");
    assert!(cache.is_empty(), "a real empty response replaces the list");
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

/// ADR-0177's rule: an unclassified failure stays unproven. It must not be
/// downgraded into the definite "there are no pull requests".
#[test]
fn an_unclassified_failure_is_unknown_and_keeps_the_previous_list() {
    let (cache, error) = fetch_into_cache(RATE_LIMITED);
    assert!(matches!(error, Some(PrFetchError::Unknown(_))), "{error:?}");
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
