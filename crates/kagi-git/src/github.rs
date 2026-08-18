//! GitHub pull requests via the `gh` CLI (Phase 1 of GitHub support).
//!
//! Shelling out to `gh` (like `cli.rs` shells out to `git` for fetch/push)
//! rather than speaking the API directly: authentication is delegated to
//! `gh auth` (tokens, SSO, Enterprise hosts all just work), and it is the same
//! tool AI agents use, so what kagi shows and what an agent sees never differ.
//! `--json` keeps the output stable. Everything here is read-only.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use kagi_domain::github::{fold_ci, PullRequest, ReviewState};

use crate::GitError;

/// Whether a usable `gh` binary is on PATH. Probed once per process — the
/// sidebar consults this every refresh and a `which` per frame is wasteful.
pub fn gh_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("gh")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

const FIELDS: &str = "number,title,headRefName,baseRefName,isDraft,reviewDecision,\
statusCheckRollup,url,author,reviewRequests";

/// The authenticated `gh` user's login, or `None` when logged out. One call;
/// callers cache it (the sidebar's "Mine" grouping keys on it).
pub fn current_login() -> Option<String> {
    let out = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Open PRs for the repository at `workdir`, newest-updated first.
///
/// `Ok(vec![])` when the repo has no GitHub remote or `gh` is not authenticated
/// for it — those are "nothing to show", not errors worth a toast. Only a
/// spawn failure (gh missing mid-session) or unparseable output errors.
pub fn list_open_prs(workdir: &Path) -> Result<Vec<PullRequest>, GitError> {
    let out = Command::new("gh")
        .args([
            "pr", "list", "--state", "open", "--limit", "100", "--json", FIELDS,
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        // No remote / not a GitHub repo / not logged in: gh writes the reason
        // to stderr and exits non-zero. Treat as empty.
        return Ok(Vec::new());
    }
    parse_pr_list(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `gh pr list --json <FIELDS>` output. Pure; unit-tested below.
pub fn parse_pr_list(json: &str) -> Result<Vec<PullRequest>, GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(pr_from_value).collect())
}

fn pr_from_value(v: &serde_json::Value) -> Option<PullRequest> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let conclusions: Vec<Option<String>> = v
        .get("statusCheckRollup")
        .and_then(|x| x.as_array())
        .map(|checks| {
            checks
                .iter()
                .map(|c| {
                    // CheckRun → `conclusion` (null while running);
                    // StatusContext → `state`.
                    c.get("conclusion")
                        .or_else(|| c.get("state"))
                        .and_then(|x| x.as_str())
                        .filter(|x| !x.is_empty())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    let refs: Vec<Option<&str>> = conclusions.iter().map(|c| c.as_deref()).collect();
    let review = match s("reviewDecision").as_str() {
        "APPROVED" => ReviewState::Approved,
        "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
        "REVIEW_REQUIRED" => ReviewState::ReviewRequired,
        _ => ReviewState::None,
    };
    Some(PullRequest {
        number: v.get("number")?.as_u64()?,
        title: s("title"),
        head: s("headRefName"),
        base: s("baseRefName"),
        is_draft: v.get("isDraft").and_then(|x| x.as_bool()).unwrap_or(false),
        ci: fold_ci(&refs),
        review,
        url: s("url"),
        author: v
            .get("author")
            .and_then(|a| a.get("login"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        reviewers: v
            .get("reviewRequests")
            .and_then(|x| x.as_array())
            .map(|rs| {
                rs.iter()
                    .filter_map(|r| r.get("login").and_then(|x| x.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::github::CiState;

    const SAMPLE: &str = r#"[
      {"number":236,"title":"feat(ui): stash peek","headRefName":"feat/stash-peek",
       "baseRefName":"main","isDraft":false,"reviewDecision":"",
       "statusCheckRollup":[
         {"__typename":"CheckRun","conclusion":"SUCCESS","status":"COMPLETED"},
         {"__typename":"CheckRun","conclusion":null,"status":"IN_PROGRESS"}],
       "url":"https://github.com/o/r/pull/236","author":{"login":"tomixrm"},
       "reviewRequests":[{"login":"bob"}]},
      {"number":240,"title":"wip","headRefName":"feat/b","baseRefName":"feat/stash-peek",
       "isDraft":true,"reviewDecision":"APPROVED",
       "statusCheckRollup":[{"__typename":"StatusContext","state":"FAILURE"}],
       "url":"https://github.com/o/r/pull/240","author":{"login":"bot"}}
    ]"#;

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
    fn empty_and_garbage_inputs() {
        assert!(parse_pr_list("[]").unwrap().is_empty());
        assert!(parse_pr_list("not json").is_err());
    }
}
