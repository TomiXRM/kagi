//! GitHub pull requests and read-only Issues via the `gh` CLI.
//!
//! Shelling out to `gh` (like `cli.rs` shells out to `git` for fetch/push)
//! rather than speaking the API directly: authentication is delegated to
//! `gh auth` (tokens, SSO, Enterprise hosts all just work), and it is the same
//! tool AI agents use, so what kagi shows and what an agent sees never differ.
//! `--json` keeps the output stable. Everything here is read-only.

use std::path::Path;
use std::sync::OnceLock;

use kagi_domain::github::{
    fold_ci, Check, Comment, Issue, IssueComment, IssueLabel, IssueState, Mergeable, PullRequest,
    Review, ReviewComment, ReviewState,
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
        crate::cli::gh_command()
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Every name here must be a field `gh pr list --json` accepts: `gh` rejects
/// an unknown one *before* the API call, which would break every PR fetch.
/// `field_names_are_accepted_by_gh` guards that. There is no `baseRepository`
/// field — the base repository's identity comes out of `url` instead.
pub(crate) const FIELDS: &str =
    "number,title,headRefName,headRefOid,baseRefName,isDraft,reviewDecision,\
statusCheckRollup,url,author,reviewRequests,body,mergeable,isCrossRepository,\
assignees,labels,changedFiles,additions,deletions,createdAt,updatedAt";

/// Fields requested from `gh issue list`. `gh issue list` excludes pull
/// requests server-side; the parser also rejects PR-shaped values defensively.
pub(crate) const ISSUE_LIST_FIELDS: &str =
    "number,title,state,url,author,assignees,labels,createdAt,updatedAt";

/// The list metadata plus the conversation loaded for a selected issue.
pub(crate) const ISSUE_DETAIL_FIELDS: &str =
    "number,title,state,url,author,assignees,labels,body,comments,createdAt,updatedAt";

/// The authenticated `gh` user's login, or `None` when logged out. One call;
/// callers cache it (the sidebar's "Mine" grouping keys on it).
pub fn current_login() -> Option<String> {
    let out = crate::cli::gh_command()
        .args(["api", "user", "--jq", ".login"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Parse `gh pr list --json <FIELDS>` output. Pure; unit-tested below.
pub fn parse_pr_list(json: &str) -> Result<Vec<PullRequest>, GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(pr_from_value).collect())
}

/// `assignees` / `reviewRequests`-style arrays of `{login}` → logins, empty
/// ones dropped. GitHub's shape is identical on an issue and a pull request,
/// so both parsers read it here rather than each spelling it out.
fn logins_at(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("login").and_then(serde_json::Value::as_str))
                .filter(|login| !login.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `labels` → labels with their GitHub colours; an entry with no name is not a
/// label. Shared by the issue and pull-request parsers for the same reason as
/// [`logins_at`].
fn labels_at(value: &serde_json::Value) -> Vec<IssueLabel> {
    value
        .get("labels")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let text = |key: &str| {
                        entry
                            .get(key)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string()
                    };
                    Some(IssueLabel {
                        name: entry.get("name")?.as_str()?.to_string(),
                        color: text("color"),
                        description: text("description"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pr_from_value(v: &serde_json::Value) -> Option<PullRequest> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    // Counts are display-only; an absent or negative one is 0 rather than a
    // reason to drop the whole PR from the list.
    let count = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    };
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
        head_sha: s("headRefOid"),
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
        reviewers: logins_at(v, "reviewRequests"),
        body: s("body"),
        checks,
        mergeable,
        // Absent (a reduced field set, or an older `gh`) reads as
        // cross-repository: the plan then refuses `--delete-branch` rather
        // than promising a deletion it cannot account for (#701).
        cross_repository: v
            .get("isCrossRepository")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        // `https://<host>/<owner>/<repo>/pull/<n>` already names the base
        // repository, host included — and `gh pr list --json` has no
        // `baseRepository` field to ask for it (#701 final review 3).
        base_repo: crate::backend::remote_ref::repo_identity(&s("url")).unwrap_or_default(),
        assignees: logins_at(v, "assignees"),
        labels: labels_at(v),
        changed_files: count("changedFiles"),
        additions: count("additions"),
        deletions: count("deletions"),
        created_at: s("createdAt"),
        updated_at: s("updatedAt"),
    })
}

/// Parse `gh issue list --json <ISSUE_LIST_FIELDS>` output.
pub fn parse_issue_list(json: &str) -> Result<Vec<Issue>, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let Some(values) = value.as_array() else {
        return Err(GitError::Other("gh json: expected issue array".into()));
    };
    Ok(values.iter().filter_map(issue_from_value).collect())
}

/// Parse `gh issue view --json <ISSUE_DETAIL_FIELDS>` output.
pub fn parse_issue_detail(json: &str) -> Result<Issue, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    issue_from_value(&value).ok_or_else(|| GitError::Other("gh json: missing issue number".into()))
}

fn issue_from_value(value: &serde_json::Value) -> Option<Issue> {
    if value
        .get("isPullRequest")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        || value.get("pullRequest").is_some_and(|v| !v.is_null())
    {
        return None;
    }
    let string = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let login = |entry: &serde_json::Value| {
        entry
            .get("login")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let state = IssueState::from_github(&string("state"));
    let assignees = value
        .get("assignees")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(login)
                .filter(|login| !login.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let labels = value
        .get("labels")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?.to_string();
                    Some(IssueLabel {
                        name,
                        color: entry
                            .get("color")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        description: entry
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let comments = value
        .get("comments")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let body = entry
                        .get("body")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if body.trim().is_empty() {
                        return None;
                    }
                    Some(IssueComment {
                        author: entry.get("author").map(login).unwrap_or_default(),
                        body,
                        created_at: entry
                            .get("createdAt")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        updated_at: entry
                            .get("updatedAt")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Issue {
        number: value.get("number")?.as_u64()?,
        title: string("title"),
        state,
        url: string("url"),
        author: value.get("author").map(login).unwrap_or_default(),
        assignees,
        labels,
        body: string("body"),
        comments,
        created_at: string("createdAt"),
        updated_at: string("updatedAt"),
    })
}

/// Reviews + issue comments for one PR — the "review chat". One `gh pr view`
/// call, made when a PR tab opens (not per list refresh).
pub fn pr_conversation(
    workdir: &Path,
    number: u64,
) -> Result<(Vec<Review>, Vec<Comment>), GitError> {
    let out = crate::cli::gh_command()
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
    let out = crate::cli::gh_command()
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
                // Multi-line anchor start (#351); falls back to the original
                // start line when the anchor is outdated.
                start_line: c
                    .get("start_line")
                    .and_then(|x| x.as_u64())
                    .or_else(|| c.get("original_start_line").and_then(|x| x.as_u64()))
                    .map(|n| n as u32),
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
    // `gh pr merge` skips the *remote* head deletion for a cross-repository
    // PR but still deletes the local branch, and nothing here can account for
    // that local effect yet (#705). Refuse the whole option rather than
    // freeze a promise the transport will not keep (#701 final review 2).
    let fork_delete = delete_branch && pr.cross_repository;
    if fork_delete {
        blockers.push(PlanNote::Github(GithubNote::ForkDeletesBranch {
            branch: pr.head.clone(),
        }));
    } else if delete_branch {
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
            kind: RecoveryKind::Github(GithubRecovery::MergePr {
                number: pr.number,
                // Freeze the *whole* promise: a merge that also deletes the
                // head branch is not confirmed by the merge alone (#701).
                // The repository identity, not a remote name — which local
                // remote points at it is resolved when the promise is checked.
                base_repo: pr.base_repo.clone(),
                delete_branch: (delete_branch && !fork_delete).then(|| pr.head.clone()),
            }),
            commands: Vec::new(),
        }),
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        // Not destructive in kagi's sense: nothing local is rewritten or
        // dropped, and GitHub keeps a Revert button.
        destructive: false,
        equivalent_command: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

// #506 fetch contract (typed failures, classification, cache fold, the two
// `gh pr list` calls) lives in `github_fetch`, re-exported here so the public
// path stays `kagi_git::github::*`.
pub use crate::github_fetch::{
    apply_pr_fetch, classify_gh_failure, issue_detail, list_issues, list_merged_prs, list_open_prs,
    PrFetchError, PrFetchOutcome,
};

// #347 merge-lifecycle backend (version detection, mergeStateStatus + merge
// queue, enqueue/dequeue) lives in `github_merge` and is re-exported here so
// the public path stays `kagi_git::github::*`.
pub use crate::github_merge::{
    dequeue_args, dequeue_pr, enqueue_args, enqueue_pr, gh_at_least, gh_version, merge_args,
    merge_pr, parse_gh_version, parse_merge_status, pr_merge_status, pr_merged_on_server,
    BypassCapability, MergeQueueEntryState, MergeStateStatus, MissingRequirements, PrMergeStatus,
    QueuePosition,
};

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
    fn parses_issue_list_without_pull_requests_and_with_nullable_fields() {
        let json = r#"[
          {"number":7,"title":"real issue","state":"OPEN",
           "url":"https://github.com/o/r/issues/7","author":{"login":"alice"},
           "assignees":null,
           "labels":[{"name":"bug","color":"d73a4a","description":null}],
           "createdAt":"t1","updatedAt":"t2"},
          {"number":8,"title":"a pull request","state":"OPEN",
           "isPullRequest":true,"url":"https://github.com/o/r/pull/8"},
          {"number":9,"title":"sparse","state":"CLOSED","author":null}
        ]"#;
        let issues = parse_issue_list(json).unwrap();
        assert_eq!(
            issues.iter().map(|issue| issue.number).collect::<Vec<_>>(),
            vec![7, 9],
            "PR-shaped values never enter the issue list"
        );
        assert_eq!(issues[0].labels[0].description, "");
        assert!(issues[0].assignees.is_empty());
        assert_eq!(issues[1].state, IssueState::Closed);
        assert_eq!(issues[1].author, "");
    }

    #[test]
    fn parses_issue_detail_comments_and_missing_optional_fields() {
        let json = r#"{
          "number":7,"title":"broken","state":"OPEN","body":null,
          "comments":[
            {"author":{"login":"bob"},"body":"confirmed","createdAt":"t3","updatedAt":null},
            {"author":null,"body":"  "}
          ]
        }"#;
        let issue = parse_issue_detail(json).unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.body, "");
        assert_eq!(issue.comments.len(), 1);
        assert_eq!(issue.comments[0].author, "bob");
        assert_eq!(issue.comments[0].updated_at, "");
    }

    #[test]
    fn issue_parsers_reject_wrong_top_level_shapes() {
        assert!(parse_issue_list("{}").is_err());
        assert!(parse_issue_detail("[]").is_err());
        assert!(parse_issue_detail(r#"{"title":"missing number"}"#).is_err());
    }

    #[test]
    fn merge_method_flags() {
        assert_eq!(MergeMethod::Squash.flag(), "--squash");
        assert_eq!(MergeMethod::Rebase.flag(), "--rebase");
        assert_eq!(MergeMethod::Merge.flag(), "--merge");
    }

    /// Acceptance §6, the core safety guarantee: `gh pr merge` ALWAYS carries
    /// `--match-head-commit <SHA>`, for every method, with or without
    /// `--delete-branch`.
    #[test]
    fn merge_always_matches_head_commit() {
        for method in [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase] {
            for delete in [false, true] {
                let args = merge_args("ghe.example/acme/widgets", 42, method, delete, "deadbeef");
                let i = args
                    .iter()
                    .position(|a| a == "--match-head-commit")
                    .expect("--match-head-commit is always present");
                assert_eq!(args.get(i + 1).map(String::as_str), Some("deadbeef"));
                // And it merges in the repository the plan named, not
                // whichever one the working directory resolves to (#701).
                let r = args
                    .iter()
                    .position(|a| a == "-R")
                    .expect("the merge is addressed to the frozen repository");
                assert_eq!(
                    args.get(r + 1).map(String::as_str),
                    Some("ghe.example/acme/widgets")
                );
            }
        }
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
}

#[cfg(test)]
mod merged_pr_tests {
    use super::parse_pr_list;

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
    }
}
