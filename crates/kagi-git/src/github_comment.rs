//! `gh pr comment` — posting a comment to a pull request.
//!
//! The write half of the PR conversation surface. Same three-part shape as the
//! merge path in `github_merge`: a pure argument builder, a private transport
//! that owns the subprocess, and a recorded boundary that turns the transport's
//! answer into a receipt *before* the result crosses back into the UI.
//!
//! Re-exported from `github` so the public path stays `kagi_git::github::*`.
//!
//! What is deliberately **not** here: a server re-read. A merge is a state the
//! server can be asked about ("is #501 merged?"), so `merge_pr` asks. A comment
//! has no such question — "did a comment get posted?" is only answerable by
//! listing comments and guessing which one is ours, and a guess is exactly what
//! an unproven termination must not be turned into. An unprovable post stays
//! [`GitError::TerminationUnknown`].

use std::path::Path;

use kagi_domain::github::PullRequest;
use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{GithubNote, GithubTitle, PlanDisposition, PlanNote, PlanTitle};

use crate::github_fetch::GH_TIMEOUT;
use crate::github_merge::repo_owner_name;
use crate::GitError;

/// Build the `gh pr comment` argument vector. Pure and unit-tested, so the two
/// properties that matter are checked without spawning `gh`:
///
/// - **The body is never an argument.** Arbitrary user text in argv is a
///   quoting hazard and runs into `ARG_MAX` on a long comment; `--body-file -`
///   makes the body stdin, which has neither problem.
/// - **`-R <base_repo>` is always present**, for the same reason
///   [`merge_args`](crate::github::merge_args) carries it: a PR number is not
///   an address. Without `-R`, `gh` resolves the repository from the working
///   directory's remotes, and the mutation can land somewhere the caller never
///   named.
pub fn comment_args(base_repo: &str, number: u64) -> Vec<String> {
    vec![
        "pr".into(),
        "comment".into(),
        "-R".into(),
        base_repo.to_string(),
        number.to_string(),
        "--body-file".into(),
        "-".into(),
    ]
}

/// Post the comment. The body goes in on stdin, never in argv.
///
/// Runs through the bounded runner ([`crate::proc::run_child`]) like every
/// other `gh` invocation: it owns the child, drains both pipes concurrently,
/// feeds stdin from its own thread (a body larger than a pipe buffer would
/// otherwise deadlock against a child that is not reading yet), and kills +
/// reaps on the deadline.
///
/// Three answers, and they are not interchangeable:
/// - exit 0 → the trimmed stdout, which is the new comment's URL.
/// - a clean non-zero exit → [`GitError::Other`] carrying gh's own words.
/// - a deadline, or an exit without complete I/O → [`GitError::TerminationUnknown`].
///   The request may have reached GitHub. Nothing here may pretend otherwise.
fn comment_transport(
    workdir: &Path,
    base_repo: &str,
    number: u64,
    body: &str,
) -> Result<String, GitError> {
    let mut cmd = crate::cli::gh_command();
    cmd.args(comment_args(base_repo, number))
        .current_dir(workdir);
    let out = crate::proc::run_child(&mut cmd, GH_TIMEOUT, Some(body.as_bytes()))
        // The only hard error: the child never started, so nothing was posted.
        .map_err(|e| GitError::Other(format!("gh: {e}")))?;
    let (stdout, stderr) = (
        out.stdout_lossy().trim().to_string(),
        out.stderr_lossy().trim().to_string(),
    );
    let status = match &out.status {
        Ok(status) => *status,
        // The wait was cut short. The child is not the only writer — see
        // `Termination::from_run`, which is why the state is derived from the
        // run rather than assumed stopped.
        Err(stop) => {
            let reason = format!("gh pr comment #{number} {stop}");
            return Err(GitError::TerminationUnknown(crate::Termination::from_run(
                reason, &out,
            )));
        }
    };
    // Exited, but the body may not have been delivered in full, or the reply
    // was not read in full: a partially written comment is exactly the state
    // nobody can name from here.
    if let Err(io) = &out.io {
        let reason = format!("gh pr comment #{number} exited with status {status} but {io}");
        return Err(GitError::TerminationUnknown(crate::Termination::from_run(
            reason, &out,
        )));
    }
    if status == 0 {
        // gh prints the new comment's URL; keep gh's own words either way.
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        // gh writes the actionable reason (no auth, PR locked, repo not found)
        // to stderr; surface it verbatim rather than a generic failure.
        Err(GitError::Other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))
    }
}

/// Plan a PR comment (the `plan_` half of the write-op triple). Pure over the
/// PR snapshot kagi already holds and the composed text — no `gh` call, so the
/// confirm modal opens instantly; [`pr_comment`] is the `execute_` half.
///
/// Not destructive and with no recovery block: a comment rewrites nothing and
/// drops nothing, and the only "undo" is deleting it on GitHub, which is not
/// something kagi can promise on the user's behalf. An empty body is a
/// **blocker**, not a plan that posts nothing: `gh` would accept it.
pub fn plan_pr_comment(pr: &PullRequest, body: &str) -> OperationPlan {
    let mut blockers: Vec<PlanNote> = Vec::new();
    if body.trim().is_empty() {
        blockers.push(PlanNote::Github(GithubNote::CommentBodyEmpty));
    }
    OperationPlan {
        disposition: if blockers.is_empty() {
            PlanDisposition::Ready
        } else {
            PlanDisposition::Blocked
        },
        title: PlanTitle::Github(GithubTitle::CommentPr { number: pr.number }),
        current: StateSummary {
            head: format!("#{} {}", pr.number, pr.title),
            dirty: format!("#{} open ({} → {})", pr.number, pr.head, pr.base),
        },
        predicted: StateSummary {
            head: format!("#{} {}", pr.number, pr.title),
            dirty: format!("#{} has one more comment", pr.number),
        },
        // The same sentence the merge plan shows: this happens on GitHub, and
        // the local clone is untouched by it.
        warnings: vec![PlanNote::Github(GithubNote::RemoteSideEffect)],
        blockers,
        recovery: None,
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        // Nothing local is rewritten or dropped.
        destructive: false,
        equivalent_command: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

/// Post `body` as a comment on PR `number` (execute step), recording the
/// attempt **here**, at the transport boundary, before the result crosses back
/// into the UI's tab-owned completion guard (#501). A stale completion
/// (`OpDisposition::DropStale`) drops the callback, not the record: a comment
/// that really reached GitHub can never leave no trace.
///
/// The returned `result` follows the receipt, not the raw `gh` exit:
///
/// | transport | oplog outcome | `result` |
/// |---|---|---|
/// | exit 0 | `Success` | `Ok(OperationOutcome::PrComment { number, detail })` |
/// | clean non-zero exit | `Failed` | `Err(GitError::Other(..))` |
/// | unproven termination | `Unknown` | `Err(GitError::TerminationUnknown(..))` |
///
/// There is no server re-read (see the module docs): an `Unknown` stays
/// unknown, so `apply` keeps the write lease and parks a reconcile entry
/// instead of letting the UI offer a retry that could post the comment twice.
pub fn pr_comment(
    workdir: &Path,
    number: u64,
    body: &str,
    plan: &OperationPlan,
) -> crate::backend::recording::RunReport {
    // The repository identity, resolved the same way the merge path resolves
    // it, so two GitHub writes from one worktree cannot address two
    // repositories. Failing to resolve it is a clean failure: nothing ran.
    let result = match repo_owner_name(workdir) {
        Ok((owner, name)) if !owner.is_empty() && !name.is_empty() => {
            comment_transport(workdir, &format!("{owner}/{name}"), number, body)
        }
        Ok(_) => Err(GitError::Other(
            "gh could not name the repository to comment on".to_string(),
        )),
        Err(error) => Err(error),
    };
    let outcome = match &result {
        // The URL is the only handle that identifies what was posted — keep it
        // in the receipt, not just in the UI's toast.
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
                dirty: format!("#{number} comment unconfirmed"),
            },
            evidence: format!(
                "{}; the comment may or may not have been posted — it was not re-read, \
                 so do not retry blindly",
                termination.reason()
            ),
        },
        Err(error) => crate::oplog::OpOutcome::Failed {
            error: error.to_string(),
        },
    };
    let result = match result {
        Ok(detail) => Ok(crate::OperationOutcome::PrComment { number, detail }),
        // Keep the termination as the transport established it: only a run
        // whose process group is confirmed empty is `Stopped`, and downgrading
        // an `Unaccounted` here would release a lease on a writer that may
        // still be alive.
        Err(error) => Err(error),
    };
    let repo = workdir.display().to_string();
    let entry =
        crate::oplog::OpLogEntry::new("pr-comment", repo.clone(), plan.current.clone(), outcome)
            .with_worktree(Some(repo));
    crate::backend::recording::RunReport {
        result,
        recording: crate::backend::recording::finalize(entry),
        stash: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PR_JSON: &str = r#"[{"number":812,"title":"comment transport",
      "headRefName":"feat/x","baseRefName":"main","isDraft":false,
      "reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":[],
      "url":"https://example.invalid/acme/widgets/pull/812","author":{"login":"a"},
      "reviewRequests":[],"body":""}]"#;

    fn pr() -> PullRequest {
        crate::github::parse_pr_list(PR_JSON).unwrap().remove(0)
    }

    /// The body is a quoting and length hazard in argv, so it goes in on
    /// stdin — and the mutation names the repository it targets.
    #[test]
    fn comment_args_address_the_repo_and_keep_the_body_off_argv() {
        let args = comment_args("github.com/acme/widgets", 812);
        assert_eq!(
            args,
            vec![
                "pr",
                "comment",
                "-R",
                "github.com/acme/widgets",
                "812",
                "--body-file",
                "-"
            ]
        );
        let args = comment_args("github.com/acme/widgets", 812);
        assert!(
            !args.iter().any(|a| a.contains("LGTM")),
            "the body must never appear in the argument vector"
        );
        assert!(args.iter().any(|a| a == "--body-file"));
    }

    /// `gh` accepts an empty comment. kagi does not: there is nothing to say,
    /// and the only undo is deleting it by hand on GitHub.
    #[test]
    fn an_empty_body_is_a_blocker_not_an_empty_comment() {
        for body in ["", "   ", "\n\t \n"] {
            let plan = plan_pr_comment(&pr(), body);
            assert_eq!(plan.disposition, PlanDisposition::Blocked, "body {body:?}");
            assert!(plan
                .blockers
                .iter()
                .any(|n| matches!(n, PlanNote::Github(GithubNote::CommentBodyEmpty))));
        }
    }

    #[test]
    fn a_real_body_plans_ready_and_not_destructive() {
        let plan = plan_pr_comment(&pr(), "LGTM, shipping it");
        assert_eq!(plan.disposition, PlanDisposition::Ready);
        assert!(plan.blockers.is_empty());
        assert!(!plan.destructive, "a comment rewrites nothing");
        assert!(plan.recovery.is_none(), "nothing local to recover");
        assert!(
            plan.warnings
                .iter()
                .any(|n| matches!(n, PlanNote::Github(GithubNote::RemoteSideEffect))),
            "the user must be told this lands on GitHub"
        );
        assert!(plan.current.dirty.contains("#812"));
    }
}
