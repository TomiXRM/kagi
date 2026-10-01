//! Recorded Issue writes following `github_comment`'s transport contract.
//! Bodies travel on stdin. Uncertain posts stay Unknown without a blind retry.

use std::path::Path;

use kagi_domain::github::IssueCreateFields;
use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{GithubNote, GithubTitle, PlanDisposition, PlanNote, PlanTitle};

use crate::backend::recording::RunReport;
use crate::github_fetch::GH_TIMEOUT;
use crate::GitError;

/// `gh issue create` with one `--label` / `--assignee` flag per chosen value
/// (#866). The author is the authenticated `gh` user; there is no flag for it.
pub fn issue_create_args(base_repo: &str, title: &str, fields: &IssueCreateFields) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "issue".into(),
        "create".into(),
        "-R".into(),
        base_repo.into(),
        "--title".into(),
        title.into(),
        "--body-file".into(),
        "-".into(),
    ];
    for label in &fields.labels {
        args.push("--label".into());
        args.push(slice_flag_value(label));
    }
    for login in &fields.assignees {
        args.push("--assignee".into());
        args.push(slice_flag_value(login));
    }
    args
}

/// One value for a `gh` string-slice flag (#904 review). `gh` reads each
/// `--label` / `--assignee` value as a CSV record, so `needs: triage, docs`
/// would arrive as two labels and a stray `"` makes the flag fail to parse.
/// Such a value is sent as one quoted CSV field (inner quotes doubled); any
/// other value is sent as is.
fn slice_flag_value(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
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
/// The caller derives the default title before freezing this plan. Chosen
/// labels and assignees (#866) ride along as a warning note, so the receipt
/// records what was asked for.
pub fn plan_issue_create(
    base_repo: &str,
    title: &str,
    body: &str,
    fields: &IssueCreateFields,
) -> OperationPlan {
    let blockers = if title.trim().is_empty() {
        vec![PlanNote::Github(GithubNote::IssueTitleEmpty)]
    } else {
        Vec::new()
    };
    let mut plan = issue_plan(
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
    );
    if !fields.is_empty() {
        plan.warnings
            .push(PlanNote::Github(GithubNote::IssueCreateFields {
                labels: fields.labels.clone(),
                assignees: fields.assignees.clone(),
            }));
    }
    plan
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

/// Preflight for chosen labels and assignees (#866): read the repository's
/// labels and assignable users just before the write, and refuse — without
/// calling `gh issue create` — anything it cannot take. A read that fails is a
/// failure, not a pass: an unchecked label is exactly what this guards.
fn preflight_issue_fields(
    workdir: &Path,
    base_repo: &str,
    fields: &IssueCreateFields,
) -> Result<(), GitError> {
    let unreadable = |what: &str, error: GitError| {
        GitError::Other(format!(
            "could not read the repository's {what} to check the issue against them: {error}"
        ))
    };
    let labels: Vec<String> = if fields.labels.is_empty() {
        Vec::new()
    } else {
        crate::github_edit::repo_labels(workdir, base_repo)
            .map_err(|error| unreadable("labels", error))?
            .into_iter()
            .map(|label| label.name)
            .collect()
    };
    // A list as long as the read's limit may have been cut off: a label
    // missing from it is left for `gh issue create` to judge rather than
    // refused here on no evidence (#904 review).
    let labels_complete = labels.len() < crate::github_edit::REPO_LABEL_LIMIT;
    let assignable = if fields.assignees.is_empty() {
        Vec::new()
    } else {
        crate::github_edit::repo_assignable_users(workdir, base_repo)
            .map_err(|error| unreadable("assignable users", error))?
    };
    let (unknown, unassignable) = fields.missing(&labels, &assignable);
    if labels_complete && !unknown.is_empty() {
        return Err(GitError::Blocked(Box::new(PlanNote::Github(
            GithubNote::IssueUnknownLabels { names: unknown },
        ))));
    }
    if !unassignable.is_empty() {
        return Err(GitError::Blocked(Box::new(PlanNote::Github(
            GithubNote::IssueUnassignableUsers {
                names: unassignable,
            },
        ))));
    }
    Ok(())
}

/// The receipt is finalized before the UI owner guard runs; the URL identifies
/// the new issue even when the user switched tabs while the request ran.
pub fn issue_create(
    workdir: &Path,
    base_repo: &str,
    title: &str,
    body: &str,
    fields: &IssueCreateFields,
    plan: &OperationPlan,
) -> RunReport {
    let result = frozen_issue_repo(base_repo).and_then(|repo| {
        preflight_issue_fields(workdir, repo, fields)?;
        issue_transport(workdir, issue_create_args(repo, title, fields), body)
    });
    record_issue_write(workdir, "issue-create", plan, fields, result, |detail| {
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
    let none = IssueCreateFields::default();
    record_issue_write(workdir, "issue-comment", plan, &none, result, |detail| {
        crate::OperationOutcome::IssueComment { number, detail }
    })
}

/// `fields` are the labels and assignees the write asked for; they go on the
/// durable entry for every outcome (#904 review), since the plan's warning
/// note that carries them is not part of the receipt.
fn record_issue_write(
    workdir: &Path,
    op: &str,
    plan: &OperationPlan,
    fields: &IssueCreateFields,
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
        // #866: a preflight refusal; nothing was sent.
        Err(GitError::Blocked(note)) => crate::oplog::OpOutcome::Refused {
            blockers: vec![note.message_en()],
        },
        Err(error) => crate::oplog::OpOutcome::Failed {
            error: error.to_string(),
        },
    };
    let repo = workdir.display().to_string();
    // #885: `gh issue create|comment -R <repo> … --body-file -`
    // (`issue_create_args` / `issue_comment_args`) only calls the GitHub API;
    // no local ref can move, so: recorded, nothing moved.
    let entry = crate::oplog::OpLogEntry::new(op, repo.clone(), plan.current.clone(), outcome)
        .with_worktree(Some(repo))
        .with_issue_fields(fields)
        .with_nothing_moved();
    RunReport {
        result: result.map(success),
        recording: crate::backend::recording::finalize(entry),
        stash: None,
    }
}
