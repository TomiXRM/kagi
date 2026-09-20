//! Recorded Issue writes following `github_comment`'s transport contract.
//! Bodies travel on stdin. Uncertain posts stay Unknown without a blind retry.

use std::path::Path;

use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{GithubNote, GithubTitle, PlanDisposition, PlanNote, PlanTitle};

use crate::backend::recording::RunReport;
use crate::github_fetch::GH_TIMEOUT;
use crate::GitError;

pub fn issue_create_args(base_repo: &str, title: &str) -> Vec<String> {
    vec![
        "issue".into(),
        "create".into(),
        "-R".into(),
        base_repo.into(),
        "--title".into(),
        title.into(),
        "--body-file".into(),
        "-".into(),
    ]
}

pub fn issue_comment_args(base_repo: &str, number: u64) -> Vec<String> {
    vec![
        "issue".into(),
        "comment".into(),
        "-R".into(),
        base_repo.into(),
        number.to_string(),
        "--body-file".into(),
        "-".into(),
    ]
}

/// Pure over the frozen repository identity and composed text.
/// The caller derives the default title before freezing this plan.
pub fn plan_issue_create(base_repo: &str, title: &str, body: &str) -> OperationPlan {
    let blockers = if title.trim().is_empty() {
        vec![PlanNote::Github(GithubNote::IssueTitleEmpty)]
    } else {
        Vec::new()
    };
    issue_plan(
        GithubTitle::CreateIssue,
        StateSummary {
            head: base_repo.into(),
            dirty: "new issue".into(),
        },
        StateSummary {
            head: title.into(),
            dirty: "issue created".into(),
        },
        body,
        blockers,
    )
}

pub fn plan_issue_comment(number: u64, title: &str, body: &str) -> OperationPlan {
    let head = format!("#{number} {title}");
    issue_plan(
        GithubTitle::CommentIssue { number },
        StateSummary {
            head: head.clone(),
            dirty: format!("#{number} issue"),
        },
        StateSummary {
            head,
            dirty: format!("#{number} has one more comment"),
        },
        body,
        Vec::new(),
    )
}

fn issue_plan(
    title: GithubTitle,
    current: StateSummary,
    predicted: StateSummary,
    body: &str,
    mut blockers: Vec<PlanNote>,
) -> OperationPlan {
    if body.trim().is_empty() {
        blockers.push(PlanNote::Github(GithubNote::CommentBodyEmpty));
    }
    OperationPlan {
        disposition: if blockers.is_empty() {
            PlanDisposition::Ready
        } else {
            PlanDisposition::Blocked
        },
        title: PlanTitle::Github(title),
        current,
        predicted,
        warnings: vec![PlanNote::Github(GithubNote::RemoteSideEffect)],
        blockers,
        recovery: None,
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        destructive: false,
        equivalent_command: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

fn issue_transport(workdir: &Path, args: Vec<String>, body: &str) -> Result<String, GitError> {
    let mut cmd = crate::cli::gh_command();
    cmd.args(args).current_dir(workdir);
    let out = crate::proc::run_child(&mut cmd, GH_TIMEOUT, Some(body.as_bytes()))
        .map_err(|error| GitError::Other(format!("gh: {error}")))?;
    let status = match &out.status {
        Ok(status) => *status,
        Err(stop) => {
            return Err(GitError::TerminationUnknown(crate::Termination::from_run(
                format!("gh issue write {stop}"),
                &out,
            )));
        }
    };
    // A signal is not a clean refusal: the remote write may already exist.
    if status < 0 {
        return Err(GitError::TerminationUnknown(crate::Termination::from_run(
            "gh issue write terminated without an exit code",
            &out,
        )));
    }
    if let Err(io) = &out.io {
        return Err(GitError::TerminationUnknown(crate::Termination::from_run(
            format!("gh issue write exited with status {status} but {io}"),
            &out,
        )));
    }
    let stdout = out.stdout_lossy().trim().to_string();
    let stderr = out.stderr_lossy().trim().to_string();
    if status == 0 {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(GitError::Other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))
    }
}

/// Issue writes only accept the repository identity frozen by the Issues read.
/// Unlike the legacy PR fallback, this boundary must never run `gh repo view`
/// while dispatching a mutation (ADR-0201).
fn frozen_issue_repo(base_repo: &str) -> Result<&str, GitError> {
    let base_repo = base_repo.trim();
    if base_repo.is_empty() {
        Err(GitError::Other(
            "issue write has no frozen GitHub repository identity".into(),
        ))
    } else {
        Ok(base_repo)
    }
}

/// The receipt is finalized before the UI owner guard runs; the URL identifies
/// the new issue even when the user switched tabs while the request ran.
pub fn issue_create(
    workdir: &Path,
    base_repo: &str,
    title: &str,
    body: &str,
    plan: &OperationPlan,
) -> RunReport {
    let result = frozen_issue_repo(base_repo)
        .and_then(|repo| issue_transport(workdir, issue_create_args(repo, title), body));
    record_issue_write(workdir, "issue-create", plan, result, |detail| {
        crate::OperationOutcome::IssueCreate { detail }
    })
}

pub fn issue_comment(
    workdir: &Path,
    base_repo: &str,
    number: u64,
    body: &str,
    plan: &OperationPlan,
) -> RunReport {
    let result = frozen_issue_repo(base_repo)
        .and_then(|repo| issue_transport(workdir, issue_comment_args(repo, number), body));
    record_issue_write(workdir, "issue-comment", plan, result, |detail| {
        crate::OperationOutcome::IssueComment { number, detail }
    })
}

fn record_issue_write(
    workdir: &Path,
    op: &str,
    plan: &OperationPlan,
    result: Result<String, GitError>,
    success: impl FnOnce(String) -> crate::OperationOutcome,
) -> RunReport {
    let outcome = match &result {
        Ok(url) => crate::oplog::OpOutcome::Success {
            after: StateSummary {
                head: plan.predicted.head.clone(),
                dirty: if url.is_empty() {
                    plan.predicted.dirty.clone()
                } else {
                    format!("{} ({url})", plan.predicted.dirty)
                },
            },
        },
        Err(GitError::TerminationUnknown(termination)) => crate::oplog::OpOutcome::Unknown {
            after: StateSummary {
                head: plan.predicted.head.clone(),
                dirty: format!("{op} unconfirmed"),
            },
            evidence: format!(
                "{}; the issue write may or may not have been posted — it was not re-read, \
                 so do not retry blindly",
                termination.reason()
            ),
        },
        Err(error) => crate::oplog::OpOutcome::Failed {
            error: error.to_string(),
        },
    };
    let repo = workdir.display().to_string();
    let entry = crate::oplog::OpLogEntry::new(op, repo.clone(), plan.current.clone(), outcome)
        .with_worktree(Some(repo));
    RunReport {
        result: result.map(success),
        recording: crate::backend::recording::finalize(entry),
        stash: None,
    }
}
