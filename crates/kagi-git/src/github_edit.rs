//! `gh pr edit` — reviewers, assignees and labels on a pull request.
//!
//! The metadata half of the PR write surface, and the same three-part shape as
//! `github_comment` / `github_review`: a pure argument builder, a private
//! transport that owns the subprocess, and a recorded boundary that turns the
//! transport's answer into a receipt *before* the result crosses back into the
//! UI.
//!
//! Re-exported from `github` so the public path stays `kagi_git::github::*`.
//!
//! Two read helpers live here too ([`repo_labels`], [`repo_assignable_users`]):
//! they fill the picker the edit is composed in, so they belong next to the
//! write they feed rather than in the PR-list fetch module.
//!
//! What is deliberately **not** here, for the same reason as in the sibling
//! modules: a server re-read. "Did my label land?" is answerable only by
//! re-listing the PR and comparing, and a comparison against a list that other
//! people also edit is a guess. An unprovable edit stays
//! [`GitError::TerminationUnknown`].

use std::path::Path;

use kagi_domain::github::{IssueLabel, PrFieldEdit, PullRequest};
use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{GithubNote, GithubTitle, PlanDisposition, PlanNote, PlanTitle};

use crate::github_fetch::GH_TIMEOUT;
use crate::github_merge::resolve_base_repo;
use crate::GitError;

/// Build the `gh pr edit` argument vector. Pure and unit-tested, so the two
/// properties that matter are checked without spawning `gh`:
///
/// - **One flag per value.** `gh` also accepts a comma-joined list, and that
///   is a data-loss bug waiting to happen: a label like `needs: triage, docs`
///   would be split into two labels that do not exist. A repeated flag has no
///   such ambiguity.
/// - **`-R <base_repo>` is always present**, for the same reason
///   [`comment_args`](crate::github::comment_args) carries it: a PR number is
///   not an address, and without `-R` the mutation lands wherever the working
///   directory's remotes happen to point.
///
/// The order is fixed — reviewers, assignees, labels, adds before removes —
/// so the argv is a stable thing a test can assert.
pub fn edit_args(base_repo: &str, number: u64, edit: &PrFieldEdit) -> Vec<String> {
    let mut args = vec![
        "pr".to_string(),
        "edit".to_string(),
        "-R".to_string(),
        base_repo.to_string(),
        number.to_string(),
    ];
    for (flag, values) in [
        ("--add-reviewer", &edit.add_reviewers),
        ("--remove-reviewer", &edit.remove_reviewers),
        ("--add-assignee", &edit.add_assignees),
        ("--remove-assignee", &edit.remove_assignees),
        ("--add-label", &edit.add_labels),
        ("--remove-label", &edit.remove_labels),
    ] {
        for value in values {
            args.push(flag.to_string());
            args.push(value.clone());
        }
    }
    args
}

/// Apply the edit. No stdin: every value is a short identifier, so unlike a
/// comment body there is nothing here that argv cannot carry.
///
/// Runs through the bounded runner ([`crate::proc::run_child`]) like every
/// other `gh` invocation: it owns the child, drains both pipes concurrently,
/// and kills + reaps on the deadline.
///
/// Three answers, and they are not interchangeable:
/// - exit 0 → the trimmed stdout, which is the PR's URL.
/// - a clean non-zero exit → [`GitError::Other`] carrying gh's own words (an
///   unknown login, a label that does not exist, a missing token).
/// - a deadline, or an exit without complete I/O → [`GitError::TerminationUnknown`].
///   The request may have reached GitHub. Nothing here may pretend otherwise.
fn edit_transport(
    workdir: &Path,
    base_repo: &str,
    number: u64,
    edit: &PrFieldEdit,
) -> Result<String, GitError> {
    let mut cmd = crate::cli::gh_command();
    cmd.args(edit_args(base_repo, number, edit))
        .current_dir(workdir);
    let out = crate::proc::run_child(&mut cmd, GH_TIMEOUT, None)
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
            let reason = format!("gh pr edit #{number} {stop}");
            return Err(GitError::TerminationUnknown(crate::Termination::from_run(
                reason, &out,
            )));
        }
    };
    // Exited, but the reply was not read in full: `gh pr edit` sends the
    // whole change in one API call, yet an incomplete read cannot tell a
    // rejected request from an applied one.
    if let Err(io) = &out.io {
        let reason = format!("gh pr edit #{number} exited with status {status} but {io}");
        return Err(GitError::TerminationUnknown(crate::Termination::from_run(
            reason, &out,
        )));
    }
    if status == 0 {
        // gh prints the PR's URL; keep gh's own words either way.
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        // gh writes the actionable reason (no such user, no such label, no
        // auth) to stderr; surface it verbatim rather than a generic failure.
        Err(GitError::Other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))
    }
}

/// `+2 reviewers, -1 label` — what the plan's predicted state says changed.
///
/// Counts, not names: a picker can queue a dozen labels, and a confirm modal
/// that grows without bound is one nobody reads. The names are on the rows the
/// user just edited.
fn summarize(edit: &PrFieldEdit) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (sign, noun, values) in [
        ('+', "reviewer", &edit.add_reviewers),
        ('-', "reviewer", &edit.remove_reviewers),
        ('+', "assignee", &edit.add_assignees),
        ('-', "assignee", &edit.remove_assignees),
        ('+', "label", &edit.add_labels),
        ('-', "label", &edit.remove_labels),
    ] {
        let count = values.len();
        if count > 0 {
            let plural = if count == 1 { "" } else { "s" };
            parts.push(format!("{sign}{count} {noun}{plural}"));
        }
    }
    if parts.is_empty() {
        "no change".to_string()
    } else {
        parts.join(", ")
    }
}

/// Plan a PR field edit (the `plan_` half of the write-op triple). Pure over
/// the PR snapshot kagi already holds and the composed edit — no `gh` call, so
/// the confirm modal opens instantly; [`pr_edit`] is the `execute_` half.
///
/// Not destructive and with no recovery block: reviewers, assignees and labels
/// are metadata GitHub keeps an audit trail of, nothing local is rewritten,
/// and the "undo" is the inverse edit — which the same modal can compose.
///
/// An edit with nothing in it is a **blocker**, not a plan that sends an empty
/// request: `gh pr edit` with no field flags exits 0 having changed nothing,
/// which would leave a Success receipt for an operation that never happened.
pub fn plan_pr_edit(pr: &PullRequest, edit: &PrFieldEdit) -> OperationPlan {
    let mut blockers: Vec<PlanNote> = Vec::new();
    if edit.is_empty() {
        blockers.push(PlanNote::Github(GithubNote::FieldEditEmpty {
            number: pr.number,
        }));
    }
    OperationPlan {
        disposition: if blockers.is_empty() {
            PlanDisposition::Ready
        } else {
            PlanDisposition::Blocked
        },
        title: PlanTitle::Github(GithubTitle::EditPr { number: pr.number }),
        current: StateSummary {
            head: format!("#{} {}", pr.number, pr.title),
            dirty: format!(
                "#{} {} reviewer(s), {} assignee(s), {} label(s)",
                pr.number,
                pr.reviewers.len(),
                pr.assignees.len(),
                pr.labels.len()
            ),
        },
        predicted: StateSummary {
            head: format!("#{} {}", pr.number, pr.title),
            dirty: format!("#{} {}", pr.number, summarize(edit)),
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

/// Apply `edit` to PR `number` (execute step), recording the attempt **here**,
/// at the transport boundary, before the result crosses back into the UI's
/// tab-owned completion guard (#501). A stale completion
/// (`OpDisposition::DropStale`) drops the callback, not the record: an edit
/// that really reached GitHub can never leave no trace.
///
/// The returned `result` follows the receipt, not the raw `gh` exit:
///
/// | transport | oplog outcome | `result` |
/// |---|---|---|
/// | exit 0 | `Success` | `Ok(OperationOutcome::PrEdit { number, detail })` |
/// | clean non-zero exit | `Failed` | `Err(GitError::Other(..))` |
/// | unproven termination | `Unknown` | `Err(GitError::TerminationUnknown(..))` |
///
/// There is no server re-read (see the module docs): an `Unknown` stays
/// unknown, so `apply` keeps the write lease and parks a reconcile entry
/// instead of letting the UI offer a retry. Retrying blindly is not harmless
/// here either — a re-sent `--add-reviewer` re-requests a review the person
/// may have already finished.
pub fn pr_edit(
    workdir: &Path,
    base_repo: &str,
    number: u64,
    edit: &PrFieldEdit,
    plan: &OperationPlan,
) -> crate::backend::recording::RunReport {
    // The repository identity the PR already carries — see
    // [`resolve_base_repo`]. `gh repo view` is a network round trip, so
    // resolving it that way made a metadata edit depend on GitHub being
    // reachable before the edit was even sent.
    let result = match resolve_base_repo(workdir, base_repo) {
        Ok(repo) => edit_transport(workdir, &repo, number, edit),
        Err(error) => Err(error),
    };
    let outcome = match &result {
        // The PR's URL is gh's own handle for what it changed — keep it in the
        // receipt, not just in the UI's toast.
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
                dirty: format!("#{number} field edit unconfirmed"),
            },
            evidence: format!(
                "{}; the reviewers, assignees and labels on #{number} may or may not have \
                 changed — they were not re-read, so do not retry blindly",
                termination.reason()
            ),
        },
        Err(error) => crate::oplog::OpOutcome::Failed {
            error: error.to_string(),
        },
    };
    let result = match result {
        Ok(detail) => Ok(crate::OperationOutcome::PrEdit { number, detail }),
        // Keep the termination as the transport established it: only a run
        // whose process group is confirmed empty is `Stopped`, and downgrading
        // an `Unaccounted` here would release a lease on a writer that may
        // still be alive.
        Err(error) => Err(error),
    };
    let repo = workdir.display().to_string();
    let entry =
        crate::oplog::OpLogEntry::new("pr-edit", repo.clone(), plan.current.clone(), outcome)
            .with_worktree(Some(repo));
    crate::backend::recording::RunReport {
        result,
        recording: crate::backend::recording::finalize(entry),
        stash: None,
    }
}

/// One bounded read-only `gh` call, typed as [`GitError`].
///
/// The sibling reads in `github_fetch` answer "is this an empty list or a
/// failed fetch?" with [`crate::github::PrFetchError`] because a PR list is
/// cached and a failure must not clear it. These two feed a picker that is
/// built fresh each time it opens, so the simpler typed error is enough — what
/// matters is the rule they share: **a failure is never an empty `Vec`**. A
/// picker that silently shows no labels is a picker that removes labels.
///
/// An unproven termination is a plain [`GitError::Other`] here, not
/// `TerminationUnknown`: nothing was written, so there is no lease to hold and
/// no state to reconcile — only data we do not have.
fn read_gh(workdir: &Path, args: &[String], what: &str) -> Result<String, GitError> {
    let mut cmd = crate::cli::gh_command();
    cmd.args(args).current_dir(workdir);
    let out = crate::proc::run_child(&mut cmd, GH_TIMEOUT, None)
        .map_err(|e| GitError::Other(format!("gh: {e}")))?;
    let status = match &out.status {
        Ok(status) => *status,
        Err(stop) => return Err(GitError::Other(format!("gh {what} {stop}"))),
    };
    if let Err(io) = &out.io {
        return Err(GitError::Other(format!(
            "gh {what} exited with status {status} but {io}"
        )));
    }
    if status != 0 {
        let stderr = out.stderr_lossy().trim().to_string();
        return Err(GitError::Other(if stderr.is_empty() {
            format!("gh {what} exited with status {status}")
        } else {
            stderr
        }));
    }
    Ok(out.stdout_lossy())
}

/// `owner/repo` out of a `<host>/<owner>/<repo>` identity, for `gh api` paths.
///
/// `-R` takes the host and is happier for it (the same `owner/repo` on
/// github.com and on an Enterprise host are different repositories), but an
/// API *path* is host-relative: `gh api github.com/acme/widgets/assignees`
/// would be a 404. Pure; unit-tested.
fn owner_repo(base_repo: &str) -> Option<String> {
    let parts: Vec<&str> = base_repo
        .trim()
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    match parts.len() {
        0 | 1 => None,
        n => Some(format!("{}/{}", parts[n - 2], parts[n - 1])),
    }
}

/// Sort case-insensitively (a picker reads alphabetically, and GitHub's own
/// order is by id), with an exact tiebreak so `Bug` and `bug` — two real,
/// distinct labels — keep a stable relative order.
fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Every label defined in the repository, for the label picker.
///
/// Bounded at 200 — `gh label list`'s own paging limit here, and well past the
/// point a flat picker is usable. Sorted and de-duplicated so the picker's
/// order does not depend on GitHub's.
pub fn repo_labels(workdir: &Path, base_repo: &str) -> Result<Vec<IssueLabel>, GitError> {
    let repo = resolve_base_repo(workdir, base_repo)?;
    let args = vec![
        "label".to_string(),
        "list".to_string(),
        "-R".to_string(),
        repo,
        "--json".to_string(),
        "name,color".to_string(),
        "--limit".to_string(),
        "200".to_string(),
    ];
    let stdout = read_gh(workdir, &args, "label list")?;
    let mut labels = parse_labels(&stdout)?;
    labels.sort_by(|a, b| by_name(&a.name, &b.name));
    labels.dedup_by(|a, b| a.name == b.name);
    Ok(labels)
}

/// Parse `gh label list --json name,color`. Pure; unit-tested.
///
/// A top-level array, not the `{"labels": [...]}` shape
/// [`labels_at`](crate::github) reads off a PR — the same model, a different
/// envelope. An entry with no name is not a label.
fn parse_labels(json: &str) -> Result<Vec<IssueLabel>, GitError> {
    let value: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| GitError::Other(format!("gh label list json: {e}")))?;
    let entries = value
        .as_array()
        .ok_or_else(|| GitError::Other("gh label list: expected a JSON array".to_string()))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(IssueLabel {
                name: entry.get("name")?.as_str()?.to_string(),
                color: entry
                    .get("color")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                description: String::new(),
            })
        })
        .collect())
}

/// Logins that may be assigned to an issue or PR in this repository — the
/// same set GitHub offers in its own assignee and reviewer pickers.
///
/// `gh api .../assignees` rather than `gh pr edit --add-assignee @me`-style
/// guessing: an assignee that is not assignable is refused by the API, and
/// finding that out after the confirm modal is worse than not offering it.
/// Sorted and de-duplicated so the picker's order does not depend on GitHub's.
pub fn repo_assignable_users(workdir: &Path, base_repo: &str) -> Result<Vec<String>, GitError> {
    let repo = resolve_base_repo(workdir, base_repo)?;
    let path = owner_repo(&repo)
        .ok_or_else(|| GitError::Other(format!("'{repo}' is not an <owner>/<repo> identity")))?;
    let args = vec![
        "api".to_string(),
        format!("repos/{path}/assignees"),
        "--jq".to_string(),
        ".[].login".to_string(),
    ];
    let stdout = read_gh(workdir, &args, "api assignees")?;
    let mut logins: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    logins.sort_by(|a, b| by_name(a, b));
    logins.dedup();
    Ok(logins)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PR_JSON: &str = r#"[{"number":812,"title":"edit transport",
      "headRefName":"feat/x","baseRefName":"main","isDraft":false,
      "reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":[],
      "url":"https://example.invalid/acme/widgets/pull/812","author":{"login":"a"},
      "reviewRequests":[],"body":""}]"#;

    fn pr() -> PullRequest {
        crate::github::parse_pr_list(PR_JSON).unwrap().remove(0)
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// One flag per value, in a fixed order, and the repository named. A
    /// comma-joined list would split a label that contains a comma into two
    /// labels that do not exist.
    #[test]
    fn edit_args_emit_one_flag_per_value_in_a_fixed_order() {
        let edit = PrFieldEdit {
            add_reviewers: v(&["bob", "carol"]),
            remove_reviewers: v(&["dave"]),
            add_assignees: v(&["erin"]),
            remove_assignees: v(&["frank"]),
            add_labels: v(&["needs: triage, docs"]),
            remove_labels: v(&["wip"]),
        };
        assert_eq!(
            edit_args("github.com/acme/widgets", 812, &edit),
            vec![
                "pr",
                "edit",
                "-R",
                "github.com/acme/widgets",
                "812",
                "--add-reviewer",
                "bob",
                "--add-reviewer",
                "carol",
                "--remove-reviewer",
                "dave",
                "--add-assignee",
                "erin",
                "--remove-assignee",
                "frank",
                "--add-label",
                "needs: triage, docs",
                "--remove-label",
                "wip",
            ]
        );
    }

    /// An edit that touches one field carries only that field's flags — a
    /// stray `--add-label ""` would be a label GitHub rejects the whole call
    /// for.
    #[test]
    fn edit_args_carry_only_the_fields_that_changed() {
        let edit = PrFieldEdit {
            add_labels: v(&["bug"]),
            ..Default::default()
        };
        let args = edit_args("acme/widgets", 7, &edit);
        assert_eq!(
            args,
            vec![
                "pr",
                "edit",
                "-R",
                "acme/widgets",
                "7",
                "--add-label",
                "bug"
            ]
        );
    }

    /// `gh pr edit` with no field flags exits 0 having changed nothing, which
    /// would leave a Success receipt for an operation that never happened.
    #[test]
    fn an_empty_edit_is_a_blocker_not_a_no_op_request() {
        let plan = plan_pr_edit(&pr(), &PrFieldEdit::default());
        assert_eq!(plan.disposition, PlanDisposition::Blocked);
        assert!(plan.blockers.iter().any(|n| matches!(
            n,
            PlanNote::Github(GithubNote::FieldEditEmpty { number: 812 })
        )));
    }

    #[test]
    fn a_real_edit_plans_ready_and_not_destructive() {
        let edit = PrFieldEdit {
            add_reviewers: v(&["bob", "carol"]),
            remove_labels: v(&["wip"]),
            ..Default::default()
        };
        let plan = plan_pr_edit(&pr(), &edit);
        assert_eq!(plan.disposition, PlanDisposition::Ready);
        assert!(plan.blockers.is_empty());
        assert!(!plan.destructive, "metadata edits rewrite nothing");
        assert!(plan.recovery.is_none(), "nothing local to recover");
        assert!(
            plan.warnings
                .iter()
                .any(|n| matches!(n, PlanNote::Github(GithubNote::RemoteSideEffect))),
            "the user must be told this lands on GitHub"
        );
        assert!(plan.current.dirty.contains("#812"));
        assert_eq!(plan.predicted.dirty, "#812 +2 reviewers, -1 label");
    }

    /// An API path is host-relative; `-R` is not.
    #[test]
    fn owner_repo_strips_the_host_from_the_identity() {
        assert_eq!(
            owner_repo("github.com/acme/widgets").as_deref(),
            Some("acme/widgets")
        );
        assert_eq!(owner_repo("acme/widgets").as_deref(), Some("acme/widgets"));
        assert_eq!(
            owner_repo("ghe.example.invalid/acme/widgets").as_deref(),
            Some("acme/widgets")
        );
        assert_eq!(owner_repo("widgets"), None);
        assert_eq!(owner_repo(""), None);
    }

    /// A malformed reply is an error, never an empty picker: a label list
    /// that silently comes back empty is a list that removes labels.
    #[test]
    fn a_malformed_label_reply_is_an_error_not_an_empty_list() {
        assert!(parse_labels("not json").is_err());
        assert!(parse_labels(r#"{"labels":[]}"#).is_err());
        assert_eq!(parse_labels("[]").unwrap(), vec![]);
    }

    #[test]
    fn labels_parse_with_their_colour_and_drop_nameless_entries() {
        let parsed =
            parse_labels(r#"[{"name":"bug","color":"d73a4a"},{"color":"ffffff"},{"name":"wip"}]"#)
                .unwrap();
        assert_eq!(
            parsed,
            vec![
                IssueLabel {
                    name: "bug".into(),
                    color: "d73a4a".into(),
                    description: String::new(),
                },
                IssueLabel {
                    name: "wip".into(),
                    color: String::new(),
                    description: String::new(),
                },
            ]
        );
    }

    #[test]
    fn the_summary_counts_each_field_and_names_the_direction() {
        assert_eq!(summarize(&PrFieldEdit::default()), "no change");
        assert_eq!(
            summarize(&PrFieldEdit {
                add_reviewers: v(&["a"]),
                remove_assignees: v(&["b", "c"]),
                add_labels: v(&["d"]),
                ..Default::default()
            }),
            "+1 reviewer, -2 assignees, +1 label"
        );
    }
}
