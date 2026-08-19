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

use kagi_domain::github::{
    fold_ci, Check, Comment, Mergeable, PullRequest, Review, ReviewComment, ReviewState,
};

use crate::GitError;
use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{PlanDisposition, PlanNote, PlanRecovery, PlanTitle, RecoveryKind};

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
statusCheckRollup,url,author,reviewRequests,body,mergeable";

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
    let checks: Vec<Check> = v
        .get("statusCheckRollup")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|c| {
                    let g = |k: &str| c.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let conclusion = c
                        .get("conclusion")
                        .or_else(|| c.get("state"))
                        .and_then(|x| x.as_str())
                        .filter(|x| !x.is_empty())
                        .map(str::to_string);
                    Check {
                        name: g("name"),
                        workflow: g("workflowName"),
                        state: fold_ci(&[conclusion.as_deref()]),
                        url: if g("detailsUrl").is_empty() {
                            g("targetUrl")
                        } else {
                            g("detailsUrl")
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mergeable = match s("mergeable").as_str() {
        "MERGEABLE" => Mergeable::Clean,
        "CONFLICTING" => Mergeable::Conflicting,
        _ => Mergeable::Unknown,
    };
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
        body: s("body"),
        checks,
        mergeable,
    })
}

/// Reviews + issue comments for one PR — the "review chat". One `gh pr view`
/// call, made when a PR tab opens (not per list refresh).
pub fn pr_conversation(
    workdir: &Path,
    number: u64,
) -> Result<(Vec<Review>, Vec<Comment>), GitError> {
    let out = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "reviews,comments",
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Ok((Vec::new(), Vec::new()));
    }
    parse_conversation(&String::from_utf8_lossy(&out.stdout))
}

/// Line-level review comments (`GET /pulls/{n}/comments`) — the Copilot /
/// Codex code-suggestion surface. `gh pr view --json` does not expose these,
/// so this goes through `gh api` (same auth, one call).
pub fn pr_review_comments(workdir: &Path, number: u64) -> Result<Vec<ReviewComment>, GitError> {
    let out = Command::new("gh")
        .args([
            "api",
            &format!("repos/{{owner}}/{{repo}}/pulls/{}/comments", number),
            "--paginate",
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    parse_review_comments(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `GET /pulls/{n}/comments`. Pure; unit-tested below.
pub fn parse_review_comments(json: &str) -> Result<Vec<ReviewComment>, GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr
        .iter()
        .map(|c| {
            let g = |k: &str| c.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            ReviewComment {
                author: c
                    .get("user")
                    .and_then(|u| u.get("login"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                path: g("path"),
                // `line` is null for an outdated anchor; fall back to the
                // original line so the comment still names a position.
                line: c
                    .get("line")
                    .and_then(|x| x.as_u64())
                    .or_else(|| c.get("original_line").and_then(|x| x.as_u64()))
                    .unwrap_or(0) as u32,
                body: g("body"),
                diff_hunk: g("diff_hunk"),
                created_at: g("created_at"),
                in_reply_to: c.get("in_reply_to_id").and_then(|x| x.as_u64()),
            }
        })
        .filter(|c| !c.body.trim().is_empty())
        .collect())
}

/// Parse `gh pr view --json reviews,comments`. Pure; unit-tested below.
pub fn parse_conversation(json: &str) -> Result<(Vec<Review>, Vec<Comment>), GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let login = |x: &serde_json::Value| {
        x.get("author")
            .and_then(|a| a.get("login"))
            .and_then(|l| l.as_str())
            .unwrap_or("")
            .to_string()
    };
    let str_at = |x: &serde_json::Value, k: &str| {
        x.get(k).and_then(|s| s.as_str()).unwrap_or("").to_string()
    };
    let reviews = v
        .get("reviews")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|r| Review {
                    author: login(r),
                    state: str_at(r, "state"),
                    body: str_at(r, "body"),
                    submitted_at: str_at(r, "submittedAt"),
                })
                // A review with no body and no verdict carries nothing.
                .filter(|r| !r.body.trim().is_empty() || r.state != "COMMENTED")
                .collect()
        })
        .unwrap_or_default();
    let comments = v
        .get("comments")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|c| Comment {
                    author: login(c),
                    body: str_at(c, "body"),
                    created_at: str_at(c, "createdAt"),
                })
                .filter(|c| !c.body.trim().is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok((reviews, comments))
}

/// How a PR should be merged. Mirrors `gh pr merge`'s three modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn flag(self) -> &'static str {
        match self {
            MergeMethod::Merge => "--merge",
            MergeMethod::Squash => "--squash",
            MergeMethod::Rebase => "--rebase",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            MergeMethod::Merge => "Merge commit",
            MergeMethod::Squash => "Squash and merge",
            MergeMethod::Rebase => "Rebase and merge",
        }
    }
}

/// Plan a PR merge (the `plan_` half of the write-op triple). Pure over the
/// PR snapshot kagi already holds — no extra `gh` call, so the confirm modal
/// opens instantly; `merge_pr` is the `execute_` half.
pub fn plan_pr_merge(
    pr: &PullRequest,
    method: MergeMethod,
    delete_branch: bool,
    head_summary: String,
) -> OperationPlan {
    use kagi_domain::github::{CiState, Mergeable, ReviewState};
    use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};

    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();
    if pr.is_draft {
        blockers.push(PlanNote::Github(GithubNote::IsDraft { number: pr.number }));
    }
    if pr.mergeable == Mergeable::Conflicting {
        blockers.push(PlanNote::Github(GithubNote::NotMergeable {
            number: pr.number,
        }));
    }
    let failed = pr.failed_checks();
    if failed > 0 || pr.ci == CiState::Failure {
        warnings.push(PlanNote::Github(GithubNote::ChecksFailing {
            number: pr.number,
            failed: failed.max(1),
        }));
    } else if pr.ci == CiState::Pending {
        warnings.push(PlanNote::Github(GithubNote::ChecksPending {
            number: pr.number,
        }));
    }
    if pr.review == ReviewState::ChangesRequested {
        warnings.push(PlanNote::Github(GithubNote::ChangesRequested {
            number: pr.number,
        }));
    }
    warnings.push(PlanNote::Github(GithubNote::RemoteSideEffect));
    if delete_branch {
        warnings.push(PlanNote::Github(GithubNote::DeletesBranch {
            branch: pr.head.clone(),
        }));
    }
    OperationPlan {
        disposition: if blockers.is_empty() {
            PlanDisposition::Ready
        } else {
            PlanDisposition::Blocked
        },
        title: PlanTitle::Github(GithubTitle::MergePr {
            number: pr.number,
            method: method.label().to_string(),
        }),
        current: StateSummary {
            head: head_summary.clone(),
            dirty: format!("#{} open ({} → {})", pr.number, pr.head, pr.base),
        },
        predicted: StateSummary {
            head: head_summary,
            dirty: format!("#{} merged into {}", pr.number, pr.base),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Github(GithubRecovery::MergePr { number: pr.number }),
            commands: Vec::new(),
        }),
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        // Not destructive in kagi's sense: nothing local is rewritten or
        // dropped, and GitHub keeps a Revert button.
        destructive: false,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

/// Merge a PR through `gh pr merge` (execute step — the plan/confirm live in
/// the UI layer, per the write-op invariant). `delete_branch` maps to
/// `--delete-branch`; nothing here touches the local working tree.
pub fn merge_pr(
    workdir: &Path,
    number: u64,
    method: MergeMethod,
    delete_branch: bool,
) -> Result<String, GitError> {
    let mut args: Vec<String> = vec![
        "pr".into(),
        "merge".into(),
        number.to_string(),
        method.flag().into(),
    ];
    if delete_branch {
        args.push("--delete-branch".into());
    }
    let out = Command::new("gh")
        .args(&args)
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        // gh writes the actionable reason (blocked by review, checks, …) to
        // stderr; surface it verbatim rather than a generic failure.
        Err(GitError::Other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::github::CiState;

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
    fn parses_reviews_and_comments_dropping_empty_ones() {
        let json = r#"{
          "reviews":[
            {"author":{"login":"a"},"state":"APPROVED","body":"","submittedAt":"t1"},
            {"author":{"login":"b"},"state":"COMMENTED","body":"nit","submittedAt":"t2"},
            {"author":{"login":"c"},"state":"COMMENTED","body":"  ","submittedAt":"t3"}
          ],
          "comments":[
            {"author":{"login":"d"},"body":"hi","createdAt":"t4"},
            {"author":{"login":"e"},"body":"","createdAt":"t5"}
          ]}"#;
        let (rv, cm) = parse_conversation(json).unwrap();
        // The empty APPROVED review is kept (the verdict IS the content); the
        // empty COMMENTED one is not.
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].state, "APPROVED");
        assert_eq!(rv[1].body, "nit");
        assert_eq!(cm.len(), 1);
        assert_eq!(cm[0].author, "d");
    }

    #[test]
    fn parses_line_comments_with_suggestions_and_outdated_anchors() {
        let json = r#"[
          {"user":{"login":"Copilot"},"path":"a/b.py","line":872,"original_line":870,
           "body":"[MUST] fix this\n```suggestion\nx = 1\n```","diff_hunk":"@@ -1 +1 @@",
           "created_at":"t1","in_reply_to_id":null},
          {"user":{"login":"me"},"path":"a/b.py","line":null,"original_line":12,
           "body":"done","diff_hunk":"","created_at":"t2","in_reply_to_id":9},
          {"user":{"login":"x"},"path":"c.py","line":1,"body":"   ","created_at":"t3"}
        ]"#;
        let cs = parse_review_comments(json).unwrap();
        assert_eq!(cs.len(), 2, "the whitespace-only comment is dropped");
        assert_eq!(cs[0].author, "Copilot");
        assert_eq!(cs[0].line, 872);
        assert!(cs[0].has_suggestion(), "```suggestion detected");
        // Outdated anchor falls back to original_line, and the reply is linked.
        assert_eq!(cs[1].line, 12);
        assert_eq!(cs[1].in_reply_to, Some(9));
        assert!(!cs[1].has_suggestion());
    }

    #[test]
    fn merge_method_flags() {
        assert_eq!(MergeMethod::Squash.flag(), "--squash");
        assert_eq!(MergeMethod::Rebase.flag(), "--rebase");
        assert_eq!(MergeMethod::Merge.flag(), "--merge");
    }

    #[test]
    fn empty_and_garbage_inputs() {
        assert!(parse_pr_list("[]").unwrap().is_empty());
        assert!(parse_pr_list("not json").is_err());
    }
}
