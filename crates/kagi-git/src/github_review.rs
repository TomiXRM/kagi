//! `gh pr review` — submitting a review verdict on a pull request.
//!
//! The verdict half of the PR conversation surface, and the same three-part
//! shape as `github_comment`: a pure argument builder, a private transport
//! that owns the subprocess, and a recorded boundary that turns the
//! transport's answer into a receipt *before* the result crosses back into
//! the UI.
//!
//! Re-exported from `github` so the public path stays `kagi_git::github::*`.
//!
//! What is deliberately **not** here, for the same reason as in
//! `github_comment`: a server re-read. "Did my review land?" is only
//! answerable by listing reviews and guessing which one is ours, and a guess
//! is exactly what an unproven termination must not become. An unprovable
//! submission stays [`GitError::TerminationUnknown`].
//!
//! Also not modelled: GitHub refuses a review on your own PR. That rule has
//! exceptions kagi cannot see from a PR snapshot (the token's identity is not
//! the PR author by construction — bots, org accounts, `gh` acting as an
//! app), so guessing it here would block legal reviews. `gh`'s own refusal is
//! the honest report.

use std::path::Path;

use kagi_domain::github::{PullRequest, ReviewVerdict};
use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{GithubNote, GithubTitle, PlanDisposition, PlanNote, PlanTitle};

use crate::github_fetch::GH_TIMEOUT;
use crate::github_merge::repo_owner_name;
use crate::GitError;

/// Build the `gh pr review` argument vector. Pure and unit-tested, so the
/// properties that matter are checked without spawning `gh`:
///
/// - **The body is never an argument**, exactly as in
///   [`comment_args`](crate::github::comment_args): arbitrary review text in
///   argv is a quoting hazard and runs into `ARG_MAX`. `--body-file -` makes
///   it stdin, which has neither problem.
/// - **`-R <base_repo>` is always present**: a PR number is not an address,
///   and without `-R` the mutation lands wherever the working directory's
///   remotes point.
/// - **`--body-file -` appears only when there is a body.** Passing it with
///   an empty stdin is not the same request: an approval with no words is
///   legal, and `gh` must be told that no body is coming rather than handed
///   an empty one.
pub fn review_args(
    base_repo: &str,
    number: u64,
    verdict: ReviewVerdict,
    has_body: bool,
) -> Vec<String> {
    let mut args = vec![
        "pr".into(),
        "review".into(),
        "-R".into(),
        base_repo.to_string(),
        number.to_string(),
        verdict.flag().to_string(),
    ];
    if has_body {
        args.push("--body-file".into());
        args.push("-".into());
    }
    args
}

/// Submit the review. The body goes in on stdin, never in argv.
///
/// Runs through the bounded runner ([`crate::proc::run_child`]) like every
/// other `gh` invocation: it owns the child, drains both pipes concurrently,
/// feeds stdin from its own thread (a review body larger than a pipe buffer
/// would otherwise deadlock against a child that is not reading yet), and
/// kills + reaps on the deadline.
///
/// Three answers, and they are not interchangeable:
/// - exit 0 → the trimmed stdout, which is the new review's URL.
/// - a clean non-zero exit → [`GitError::Other`] carrying gh's own words
///   (a self-review, a missing token, a locked PR).
/// - a deadline, or an exit without complete I/O → [`GitError::TerminationUnknown`].
///   The request may have reached GitHub. Nothing here may pretend otherwise.
fn review_transport(
    workdir: &Path,
    base_repo: &str,
    number: u64,
    verdict: ReviewVerdict,
    body: &str,
) -> Result<String, GitError> {
    let mut cmd = crate::cli::gh_command();
    cmd.args(review_args(base_repo, number, verdict, !body.is_empty()))
        .current_dir(workdir);
    let stdin = (!body.is_empty()).then_some(body.as_bytes());
    let out = crate::proc::run_child(&mut cmd, GH_TIMEOUT, stdin)
        // The only hard error: the child never started, so nothing was sent.
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
            let reason = format!("gh pr review {} #{number} {stop}", verdict.as_str());
            return Err(GitError::TerminationUnknown(crate::Termination::from_run(
                reason, &out,
            )));
        }
    };
    // Exited, but the body may not have been delivered in full, or the reply
    // was not read in full: a half-submitted review is exactly the state
    // nobody can name from here.
    if let Err(io) = &out.io {
        let reason = format!(
            "gh pr review {} #{number} exited with status {status} but {io}",
            verdict.as_str()
        );
        return Err(GitError::TerminationUnknown(crate::Termination::from_run(
            reason, &out,
        )));
    }
    if status == 0 {
        // gh prints the new review's URL; keep gh's own words either way.
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        // gh writes the actionable reason (self-review, no auth, PR locked)
        // to stderr; surface it verbatim rather than a generic failure.
        Err(GitError::Other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))
    }
}

/// Plan a PR review (the `plan_` half of the write-op triple). Pure over the
/// PR snapshot kagi already holds and the composed text — no `gh` call, so
/// the confirm modal opens instantly; [`pr_review`] is the `execute_` half.
///
/// Not destructive and with no recovery block: a review rewrites nothing and
/// drops nothing locally, and the only "undo" is dismissing it on GitHub,
/// which is not something kagi can promise on the user's behalf.
///
/// The one blocker is GitHub's own rule
/// ([`ReviewVerdict::requires_body`]): `--request-changes` and `--comment`
/// need words, so an empty body there is refused at plan time instead of
/// spending a round trip on a request that cannot succeed. `--approve` with
/// an empty body is a wordless approval — legal, and planned Ready.
pub fn plan_pr_review(pr: &PullRequest, verdict: ReviewVerdict, body: &str) -> OperationPlan {
    let mut blockers: Vec<PlanNote> = Vec::new();
    if verdict.requires_body() && body.trim().is_empty() {
        blockers.push(PlanNote::Github(GithubNote::ReviewBodyEmpty {
            verdict: verdict.as_str().to_string(),
        }));
    }
    OperationPlan {
        disposition: if blockers.is_empty() {
            PlanDisposition::Ready
        } else {
            PlanDisposition::Blocked
        },
        title: PlanTitle::Github(GithubTitle::ReviewPr {
            number: pr.number,
            verdict: verdict.as_str().to_string(),
        }),
        current: StateSummary {
            head: format!("#{} {}", pr.number, pr.title),
            dirty: format!("#{} open ({} → {})", pr.number, pr.head, pr.base),
        },
        predicted: StateSummary {
            head: format!("#{} {}", pr.number, pr.title),
            dirty: format!("#{} reviewed ({})", pr.number, verdict.as_str()),
        },
        // The same sentence the merge and comment plans show: this happens on
        // GitHub, and the local clone is untouched by it.
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

/// Submit `verdict` (with `body`) as a review on PR `number` (execute step),
/// recording the attempt **here**, at the transport boundary, before the
/// result crosses back into the UI's tab-owned completion guard (#501). A
/// stale completion (`OpDisposition::DropStale`) drops the callback, not the
/// record: a review that really reached GitHub can never leave no trace.
///
/// The returned `result` follows the receipt, not the raw `gh` exit:
///
/// | transport | oplog outcome | `result` |
/// |---|---|---|
/// | exit 0 | `Success` | `Ok(OperationOutcome::PrReview { number, verdict, detail })` |
/// | clean non-zero exit | `Failed` | `Err(GitError::Other(..))` |
/// | unproven termination | `Unknown` | `Err(GitError::TerminationUnknown(..))` |
///
/// There is no server re-read (see the module docs): an `Unknown` stays
/// unknown, so `apply` keeps the write lease and parks a reconcile entry
/// instead of letting the UI offer a retry that could leave two reviews.
pub fn pr_review(
    workdir: &Path,
    number: u64,
    verdict: ReviewVerdict,
    body: &str,
    plan: &OperationPlan,
) -> crate::backend::recording::RunReport {
    // The repository identity, resolved the same way the merge and comment
    // paths resolve it, so two GitHub writes from one worktree cannot address
    // two repositories. Failing to resolve it is a clean failure: nothing ran.
    let result = match repo_owner_name(workdir) {
        Ok((owner, name)) if !owner.is_empty() && !name.is_empty() => {
            review_transport(workdir, &format!("{owner}/{name}"), number, verdict, body)
        }
        Ok(_) => Err(GitError::Other(
            "gh could not name the repository to review in".to_string(),
        )),
        Err(error) => Err(error),
    };
    let outcome = match &result {
        // The URL is the only handle that identifies what was submitted —
        // keep it in the receipt, not just in the UI's toast.
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
                dirty: format!("#{number} {} review unconfirmed", verdict.as_str()),
            },
            evidence: format!(
                "{}; the '{}' review on #{number} may or may not have been submitted — \
                 it was not re-read, so do not retry blindly",
                termination.reason(),
                verdict.as_str()
            ),
        },
        Err(error) => crate::oplog::OpOutcome::Failed {
            error: error.to_string(),
        },
    };
    let result = match result {
        Ok(detail) => Ok(crate::OperationOutcome::PrReview {
            number,
            verdict: verdict.as_str().to_string(),
            detail,
        }),
        // Keep the termination as the transport established it: only a run
        // whose process group is confirmed empty is `Stopped`, and downgrading
        // an `Unaccounted` here would release a lease on a writer that may
        // still be alive.
        Err(error) => Err(error),
    };
    let repo = workdir.display().to_string();
    let entry =
        crate::oplog::OpLogEntry::new("pr-review", repo.clone(), plan.current.clone(), outcome)
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

    const PR_JSON: &str = r#"[{"number":812,"title":"review transport",
      "headRefName":"feat/x","baseRefName":"main","isDraft":false,
      "reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":[],
      "url":"https://example.invalid/acme/widgets/pull/812","author":{"login":"a"},
      "reviewRequests":[],"body":""}]"#;

    fn pr() -> PullRequest {
        crate::github::parse_pr_list(PR_JSON).unwrap().remove(0)
    }

    /// One verdict, one flag — the three are not interchangeable, and a
    /// mix-up would approve a PR the user asked to block.
    #[test]
    fn each_verdict_submits_its_own_flag() {
        for (verdict, flag) in [
            (ReviewVerdict::Approve, "--approve"),
            (ReviewVerdict::RequestChanges, "--request-changes"),
            (ReviewVerdict::Comment, "--comment"),
        ] {
            let args = review_args("github.com/acme/widgets", 812, verdict, true);
            assert_eq!(
                args[..6],
                ["pr", "review", "-R", "github.com/acme/widgets", "812", flag],
                "{verdict:?} must address the repo and carry {flag}"
            );
            assert_eq!(
                args.iter().filter(|a| a.starts_with("--")).count(),
                2,
                "{verdict:?} must carry exactly its own flag and --body-file"
            );
        }
    }

    /// The body is a quoting and length hazard in argv, so it goes in on
    /// stdin — and `--body-file` is present exactly when there is one to
    /// read. A wordless approval must not hand `gh` an empty stdin file.
    #[test]
    fn body_file_is_present_exactly_when_a_body_is() {
        let with = review_args("acme/widgets", 812, ReviewVerdict::RequestChanges, true);
        assert_eq!(with[6..], ["--body-file", "-"]);
        assert!(
            !with.iter().any(|a| a.contains("needs work")),
            "the body must never appear in the argument vector"
        );
        let without = review_args("acme/widgets", 812, ReviewVerdict::Approve, false);
        assert!(
            !without.iter().any(|a| a == "--body-file"),
            "a wordless approval sends no body file: {without:?}"
        );
        assert_eq!(without.len(), 6);
    }

    /// GitHub refuses `REQUEST_CHANGES` and `COMMENT` reviews with no body,
    /// so the plan refuses them first rather than spending a round trip.
    #[test]
    fn an_empty_body_blocks_the_two_verdicts_github_requires_words_for() {
        for verdict in [ReviewVerdict::RequestChanges, ReviewVerdict::Comment] {
            for body in ["", "   ", "\n\t \n"] {
                let plan = plan_pr_review(&pr(), verdict, body);
                assert_eq!(
                    plan.disposition,
                    PlanDisposition::Blocked,
                    "{verdict:?} with body {body:?}"
                );
                assert!(plan.blockers.iter().any(|n| matches!(
                    n,
                    PlanNote::Github(GithubNote::ReviewBodyEmpty { verdict: v })
                        if v == verdict.as_str()
                )));
            }
        }
    }

    /// An approval with no words is a real review GitHub accepts; refusing it
    /// would force people to type "LGTM" to say nothing.
    #[test]
    fn a_wordless_approval_is_ready_not_blocked() {
        let plan = plan_pr_review(&pr(), ReviewVerdict::Approve, "   ");
        assert_eq!(plan.disposition, PlanDisposition::Ready);
        assert!(plan.blockers.is_empty());
    }

    #[test]
    fn a_real_body_plans_ready_and_not_destructive() {
        let plan = plan_pr_review(&pr(), ReviewVerdict::RequestChanges, "needs work on line 3");
        assert_eq!(plan.disposition, PlanDisposition::Ready);
        assert!(plan.blockers.is_empty());
        assert!(!plan.destructive, "a review rewrites nothing");
        assert!(plan.recovery.is_none(), "nothing local to recover");
        assert!(
            plan.warnings
                .iter()
                .any(|n| matches!(n, PlanNote::Github(GithubNote::RemoteSideEffect))),
            "the user must be told this lands on GitHub"
        );
        assert!(plan.current.dirty.contains("#812"));
        assert!(
            plan.predicted.dirty.contains("request-changes"),
            "the prediction names the verdict: {}",
            plan.predicted.dirty
        );
    }
}
