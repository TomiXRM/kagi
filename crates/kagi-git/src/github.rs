//! GitHub pull requests and read-only Issues via the `gh` CLI.
//!
//! Shelling out to `gh` (like `cli.rs` shells out to `git` for fetch/push)
//! rather than speaking the API directly: authentication is delegated to
//! `gh auth` (tokens, SSO, Enterprise hosts all just work), and it is the same
//! tool AI agents use, so what kagi shows and what an agent sees never differ.
//! `--json` keeps read output stable; recorded mutations live in feature siblings.

pub use crate::github_issue_write::{
    issue_comment, issue_comment_args, issue_create, issue_create_args, plan_issue_comment,
    plan_issue_create,
};
use std::path::Path;
use std::sync::OnceLock;

use kagi_domain::github::{
    fold_ci, Check, Comment, IssueLabel, Mergeable, PrBodyDetail, PrStatusDetail, PullRequest,
    Review, ReviewComment,
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
/// an unknown one *before* the API call, which would break that fetch.
/// `field_names_are_accepted_by_gh` guards that.
///
/// Branch Cleanup's merged evidence: the four columns its table shows, and
/// nothing else. The L1 list is a GraphQL page ([`crate::github_pr_list`])
/// because `gh pr list --json comments` would download every comment body of
/// every PR just to show a count.
pub(crate) const PR_MERGED_FIELDS: &str = "number,title,headRefName,author";

/// L2: volatile merge/check state for one PR. Kept separate from the list so
/// one expensive rollup cannot make the entire repository query time out.
pub(crate) const PR_STATUS_FIELDS: &str = "number,headRefOid,statusCheckRollup,mergeable";

/// L3: the selected PR's large body and aggregate diff statistics.
pub(crate) const PR_BODY_FIELDS: &str =
    "number,headRefOid,updatedAt,body,changedFiles,additions,deletions";

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

/// Parse one `gh pr view --json PR_STATUS_FIELDS` response.
pub fn parse_pr_status_detail(json: &str) -> Result<PrStatusDetail, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {e}")))?;
    let (ci, checks) = checks_at(&value);
    Ok(PrStatusDetail {
        number: required_number(&value)?,
        head_sha: string_at(&value, "headRefOid"),
        ci,
        checks,
        mergeable: mergeable_at(&value),
    })
}

/// Parse one `gh pr view --json PR_BODY_FIELDS` response.
pub fn parse_pr_body_detail(json: &str) -> Result<PrBodyDetail, GitError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {e}")))?;
    Ok(PrBodyDetail {
        number: required_number(&value)?,
        head_sha: string_at(&value, "headRefOid"),
        updated_at: string_at(&value, "updatedAt"),
        body: string_at(&value, "body"),
        changed_files: count_at(&value, "changedFiles"),
        additions: count_at(&value, "additions"),
        deletions: count_at(&value, "deletions"),
    })
}

fn required_number(value: &serde_json::Value) -> Result<u64, GitError> {
    value
        .get("number")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| GitError::Other("gh json: missing PR number".into()))
}

pub(crate) fn string_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub(crate) fn count_at(value: &serde_json::Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

/// `assignees` / `reviewRequests`-style arrays of `{login}` → logins, empty
/// ones dropped. GitHub's shape is identical on an issue and a pull request,
/// so both parsers read it here rather than each spelling it out.
pub(crate) fn logins_at(value: &serde_json::Value, key: &str) -> Vec<String> {
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

/// `labels` entries → labels with their GitHub colours; an entry with no name
/// is not a label.
fn labels_from(entries: &[serde_json::Value]) -> Vec<IssueLabel> {
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
}

/// `labels` → labels, from gh's flat array. Shared by the issue and
/// pull-request parsers for the same reason as [`logins_at`].
pub(crate) fn labels_at(value: &serde_json::Value) -> Vec<IssueLabel> {
    value
        .get("labels")
        .and_then(serde_json::Value::as_array)
        .map(|entries| labels_from(entries))
        .unwrap_or_default()
}

/// A GraphQL `{nodes: [...]}` connection, or `None` for anything else —
/// including the flat array `gh --json` produces for the same field.
pub(crate) fn connection_nodes<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Option<&'a Vec<serde_json::Value>> {
    value.get(key)?.get("nodes")?.as_array()
}

/// Logins from either shape: a GraphQL connection, or gh's flat `[{login}]`.
/// The Issue and pull-request list reads differ only in that wrapper, and a
/// row means the same thing whichever transport produced it.
pub(crate) fn logins_anywhere(value: &serde_json::Value, key: &str) -> Vec<String> {
    match connection_nodes(value, key) {
        Some(nodes) => nodes
            .iter()
            .filter_map(|entry| entry.get("login").and_then(serde_json::Value::as_str))
            .filter(|login| !login.is_empty())
            .map(str::to_string)
            .collect(),
        None => logins_at(value, key),
    }
}

/// Labels from either shape, for the same reason as [`logins_anywhere`].
pub(crate) fn labels_anywhere(value: &serde_json::Value) -> Vec<IssueLabel> {
    match connection_nodes(value, "labels") {
        Some(nodes) => labels_from(nodes),
        None => labels_at(value),
    }
}

/// A GraphQL `errors` block as the failure it is: a partial response is never
/// a short list. Shared by the Issue page (#752) and the pull-request page.
pub(crate) fn graphql_failure(value: &serde_json::Value) -> Option<GitError> {
    let errors = value
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .filter(|errors| !errors.is_empty())?;
    let detail = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join("; ");
    Some(GitError::Other(if detail.is_empty() {
        "gh graphql: partial response".into()
    } else {
        format!("gh graphql: {detail}")
    }))
}

pub(crate) fn checks_at(v: &serde_json::Value) -> (kagi_domain::github::CiState, Vec<Check>) {
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
    (fold_ci(&refs), checks)
}

pub(crate) fn mergeable_at(v: &serde_json::Value) -> Mergeable {
    match string_at(v, "mergeable").as_str() {
        "MERGEABLE" => Mergeable::Clean,
        "CONFLICTING" => Mergeable::Conflicting,
        _ => Mergeable::Unknown,
    }
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

/// Build the display half after Backend has frozen the local cleanup inputs.
/// No extra `gh` call is needed to show the confirmation.
pub(crate) fn plan_pr_merge(
    pr: &PullRequest,
    method: MergeMethod,
    delete_branch: bool,
    head_summary: String,
    local_branch: Option<kagi_domain::plan::PrMergeLocalBranch>,
) -> OperationPlan {
    use kagi_domain::github::{CiState, Mergeable, ReviewState};
    use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};

    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();
    if pr.head_sha.trim().is_empty() {
        blockers.push(PlanNote::Github(GithubNote::HeadUnavailable {
            number: pr.number,
        }));
    }
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
    if delete_branch {
        if pr.cross_repository {
            warnings.push(PlanNote::Github(GithubNote::ForkKeepsRemoteBranch));
        } else {
            warnings.push(PlanNote::Github(GithubNote::DeletesBranch {
                branch: pr.head.clone(),
            }));
        }
        if let Some(branch) = &local_branch {
            warnings.push(PlanNote::Github(match &branch.keep_reason {
                // Plan time already knows the deletion would be refused, so
                // the modal says the branch stays rather than promising a
                // deletion whose receipt then contradicts it (#705 review P2).
                Some(reason) => GithubNote::KeepsLocalBranch {
                    branch: branch.name.clone(),
                    reason: reason.clone(),
                },
                None => GithubNote::DeletesLocalBranch {
                    branch: branch.name.clone(),
                    tip: branch.tip.clone(),
                },
            }));
        }
    } else {
        warnings.push(PlanNote::Github(GithubNote::RemoteSideEffect));
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
                // Execution and reconciliation use this exact repository
                // identity, never a mutable local remote name.
                base_repo: pr.base_repo.clone(),
                delete_branch: delete_branch.then(|| pr.head.clone()),
                cross_repository: pr.cross_repository,
                local_branch: local_branch.map(Box::new),
            }),
            commands: Vec::new(),
        }),
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        // Local cleanup uses its own guarded ref-only deletion and mandatory
        // backup root, and only follows server confirmation of the PR merge.
        destructive: false,
        equivalent_command: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

// #506 fetch contract (typed failures, classification, cache fold, the list
// reads) lives in `github_fetch`, re-exported here so the public path stays
// `kagi_git::github::*`.
pub use crate::github_fetch::{
    apply_pr_fetch, classify_gh_failure, issue_detail, list_issues, list_merged_prs, list_prs,
    pr_body_detail, pr_status_detail, PrFetchError, PrFetchOutcome,
};
// The pull-request list shapes (the L1 GraphQL page and gh's flat `--json`
// array) are parsed in `github_pr_list`, re-exported for the same reason.
pub use crate::github_pr_list::{parse_pr_list, parse_pr_list_page};
pub use crate::github_status_batch::{
    parse_pr_status_batch, pr_status_details_batch, PrStatusBatchResult,
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

// The `gh pr comment` write (plan + recorded boundary) lives in
// `github_comment` and is re-exported here so the public path stays
// `kagi_git::github::*`.
pub use crate::github_comment::{comment_args, plan_pr_comment, pr_comment};

// The `gh pr review` write (plan + recorded boundary) lives in
// `github_review` and is re-exported here for the same reason.
pub use crate::github_review::{plan_pr_review, pr_review, review_args};

// The `gh pr edit` write (plan + recorded boundary) and the two picker reads
// it is composed from live in `github_edit`, re-exported here for the same
// reason.
pub use crate::github_edit::{
    edit_args, plan_pr_edit, pr_edit, repo_assignable_users, repo_labels,
};

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::github::CiState;
    #[test]
    fn parses_status_and_body_details_without_conflating_empty_with_missing() {
        let status = parse_pr_status_detail(
            r#"{"number":42,"headRefOid":"abc","mergeable":"CONFLICTING","statusCheckRollup":[]}"#,
        )
        .unwrap();
        assert_eq!(status.number, 42);
        assert_eq!(status.head_sha, "abc");
        assert!(status.checks.is_empty(), "a fetched empty rollup is valid");
        assert_eq!(status.ci, CiState::None);
        assert_eq!(status.mergeable, Mergeable::Conflicting);

        let body = parse_pr_body_detail(
            r#"{"number":42,"headRefOid":"abc","updatedAt":"t","body":"","changedFiles":0,"additions":0,"deletions":0}"#,
        )
        .unwrap();
        assert_eq!(body.number, 42);
        assert_eq!(body.head_sha, "abc");
        assert!(body.body.is_empty(), "a fetched empty body is valid");
        assert_eq!(
            (body.changed_files, body.additions, body.deletions),
            (0, 0, 0)
        );
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
    fn merge_plan_blocks_when_the_head_commit_is_unavailable() {
        let pr = PullRequest {
            number: 42,
            head_sha: String::new(),
            ..Default::default()
        };
        let plan = plan_pr_merge(&pr, MergeMethod::Merge, false, "main".into(), None);
        assert!(plan.blockers.iter().any(|note| matches!(
            note,
            PlanNote::Github(kagi_domain::plan_note::GithubNote::HeadUnavailable { number: 42 })
        )));
    }
}
