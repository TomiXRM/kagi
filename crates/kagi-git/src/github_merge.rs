//! PR merge-lifecycle backend (#347): `gh` version detection, `mergeStateStatus`
//! + merge-queue position via `gh api graphql`, and recorded merge/queue mutations.
//!
//! Split out of `github.rs` on the merge-lifecycle feature boundary (that file
//! is at its LOC ceiling). Re-exported from `github` so the public path stays
//! `kagi_git::github::*`. The UI gates mutations behind confirmation.

use std::path::Path;
use std::sync::OnceLock;

use crate::github::MergeMethod;
use crate::GitError;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::RecoveryKind;

pub use kagi_domain::merge_state::{
    BypassCapability, MergeQueueEntryState, MergeStateStatus, MissingRequirements, QueuePosition,
};

// ── gh version detection (#347 §5: degrade gracefully, never hard-require) ──

/// Parse the version out of `gh --version` output.
///
/// The first line is `gh version 2.97.0 (2024-…)`; older/newer builds vary the
/// tail but always start with `gh version <semver>`. Returns `(major, minor,
/// patch)`, or `None` if the shape is unfamiliar (→ treat as "assume missing
/// features", the safe default).
pub fn parse_gh_version(out: &str) -> Option<(u32, u32, u32)> {
    let ver = out.split_whitespace().nth(2)?;
    let mut it = ver.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    // Patch may carry a suffix on nightly builds; take the leading digits.
    let patch = it
        .next()
        .map(|p| {
            p.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// The installed `gh` version, probed once per process.
pub fn gh_version() -> Option<(u32, u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let out = crate::cli::gh_command().arg("--version").output().ok()?;
        parse_gh_version(&String::from_utf8_lossy(&out.stdout))
    })
}

/// Whether the installed `gh` is at least `major.minor`. `false` when `gh` is
/// missing or its version could not be parsed — callers hide the version-gated
/// feature and show a note rather than failing (#347 §5, PM-locked: no hard
/// 2.99 requirement).
pub fn gh_at_least(major: u32, minor: u32) -> bool {
    match gh_version() {
        Some((maj, min, _)) => (maj, min) >= (major, minor),
        None => false,
    }
}

// ── mergeStateStatus + merge queue (via `gh api graphql`) ──────────────────

/// Everything the PR merge-status surface needs beyond the PR list: the
/// `mergeStateStatus`, this PR's merge-queue entry (if any), the unresolved
/// review-thread count, and the PR's GraphQL node id (needed to enqueue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrMergeStatus {
    pub node_id: String,
    pub state: MergeStateStatus,
    pub queue: Option<QueuePosition>,
    pub unresolved_threads: u32,
}

const MERGE_STATUS_QUERY: &str = "\
query($owner:String!,$name:String!,$number:Int!){\
 repository(owner:$owner,name:$name){\
  pullRequest(number:$number){\
   id mergeStateStatus\
   reviewThreads(first:100){nodes{isResolved}}\
   mergeQueueEntry{position estimatedTimeToMerge state\
    mergeQueue{nextEntryEstimatedTimeToMerge}}}}}";

/// Fetch `mergeStateStatus` + merge-queue position for one PR. `gh api graphql`
/// (same auth as everything else here). Read-only.
pub fn pr_merge_status(workdir: &Path, number: u64) -> Result<PrMergeStatus, GitError> {
    let (owner, name) = repo_owner_name(workdir)?;
    let out = crate::cli::gh_command()
        .args([
            "api",
            "graphql",
            "-f",
            &format!("query={MERGE_STATUS_QUERY}"),
            "-F",
            &format!("owner={owner}"),
            "-F",
            &format!("name={name}"),
            "-F",
            &format!("number={number}"),
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Err(GitError::Other(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    parse_merge_status(&String::from_utf8_lossy(&out.stdout))
}

/// `owner`/`name` for the repo at `workdir`, via `gh repo view`.
///
/// `pub(crate)`: the `gh pr comment` boundary in `github_comment` addresses
/// its mutation with the same identity, and resolving it a second way would
/// let two GitHub writes from the same worktree name two repositories.
pub(crate) fn repo_owner_name(workdir: &Path) -> Result<(String, String), GitError> {
    let out = crate::cli::gh_command()
        .args([
            "repo",
            "view",
            "--json",
            "owner,name",
            "-q",
            ".owner.login+\"/\"+.name",
        ])
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    if !out.status.success() {
        return Err(GitError::Other("not a GitHub repo".into()));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    s.split_once('/')
        .map(|(o, n)| (o.to_string(), n.to_string()))
        .ok_or_else(|| GitError::Other("unexpected repo view output".into()))
}

/// The `-R` identity for a GitHub write: the caller's frozen `base_repo` when
/// it has one, [`repo_owner_name`] only when it does not.
///
/// The order matters, and it is the fix for a real failure. `gh repo view` is
/// a **network** round trip, so resolving the identity that way made every
/// write depend on GitHub being reachable *twice* — an offline machine failed
/// with "not a GitHub repo" even though the PR snapshot in hand already
/// carried its own `<host>/<owner>/<repo>`. A PR knows where it lives; ask the
/// network only when nobody told us.
pub(crate) fn resolve_base_repo(workdir: &Path, base_repo: &str) -> Result<String, GitError> {
    let frozen = base_repo.trim();
    if !frozen.is_empty() {
        return Ok(frozen.to_string());
    }
    match repo_owner_name(workdir)? {
        (owner, name) if !owner.is_empty() && !name.is_empty() => Ok(format!("{owner}/{name}")),
        _ => Err(GitError::Other(
            "gh could not name the repository to write to".to_string(),
        )),
    }
}

/// Parse the `gh api graphql` merge-status response. Pure; unit-tested. A
/// missing `mergeQueueEntry` (non-MQ repo, or not queued) yields `queue: None`
/// so the UI hides the queue section — nothing to break.
pub fn parse_merge_status(json: &str) -> Result<PrMergeStatus, GitError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GitError::Other(format!("gh json: {}", e)))?;
    let pr = v
        .pointer("/data/repository/pullRequest")
        .ok_or_else(|| GitError::Other("no pullRequest in response".into()))?;
    let state = MergeStateStatus::from_graphql(
        pr.get("mergeStateStatus")
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );
    let unresolved_threads = pr
        .pointer("/reviewThreads/nodes")
        .and_then(|x| x.as_array())
        .map(|nodes| {
            nodes
                .iter()
                .filter(|n| {
                    !n.get("isResolved")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(true)
                })
                .count() as u32
        })
        .unwrap_or(0);
    let queue = pr.get("mergeQueueEntry").filter(|e| !e.is_null()).map(|e| {
        let s = |k: &str| e.get(k).and_then(|x| x.as_str()).map(str::to_string);
        QueuePosition {
            position: e.get("position").and_then(|x| x.as_u64()),
            estimated_time_to_merge: s("estimatedTimeToMerge"),
            state: MergeQueueEntryState::from_graphql(
                e.get("state").and_then(|x| x.as_str()).unwrap_or(""),
            ),
            next_entry_estimated_time_to_merge: e
                .pointer("/mergeQueue/nextEntryEstimatedTimeToMerge")
                .and_then(|x| x.as_str())
                .map(str::to_string),
        }
    });
    Ok(PrMergeStatus {
        node_id: pr
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        state,
        queue,
        unresolved_threads,
    })
}

/// Build the `gh api graphql` args for enqueue/dequeue. Pure so the mutation
/// shape (and the `jump` flag that jumps the queue) is testable without `gh`.
///
/// `jump`/solo are queue-cutting operations the UI gates behind a **second**
/// confirmation (they reorder other people's PRs); the backend just carries
/// the flag it is given.
pub fn enqueue_args(node_id: &str, jump: bool) -> Vec<String> {
    let mutation = "mutation($id:ID!,$jump:Boolean!){\
        enqueuePullRequest(input:{pullRequestId:$id,jump:$jump}){\
         mergeQueueEntry{position}}}";
    vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={mutation}"),
        "-F".into(),
        format!("id={node_id}"),
        "-F".into(),
        format!("jump={jump}"),
    ]
}

/// Args for removing this PR from the merge queue.
pub fn dequeue_args(node_id: &str) -> Vec<String> {
    let mutation = "mutation($id:ID!){\
        dequeuePullRequest(input:{pullRequestId:$id}){\
         mergeQueueEntry{position}}}";
    vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={mutation}"),
        "-F".into(),
        format!("id={node_id}"),
    ]
}

/// Add this PR to the repository's merge queue (`enqueuePullRequest`).
/// `jump = true` cuts to the front — the UI must have taken a second
/// confirmation first.
pub fn enqueue_pr(workdir: &Path, node_id: &str, jump: bool) -> Result<String, GitError> {
    run_gh(workdir, &enqueue_args(node_id, jump))
}

/// Remove this PR from the merge queue (`dequeuePullRequest`).
pub fn dequeue_pr(workdir: &Path, node_id: &str) -> Result<String, GitError> {
    run_gh(workdir, &dequeue_args(node_id))
}

fn run_gh(workdir: &Path, args: &[String]) -> Result<String, GitError> {
    let out = crate::cli::gh_command()
        .args(args)
        .current_dir(workdir)
        .output()
        .map_err(|e| GitError::Other(format!("gh: {}", e)))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
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

    #[test]
    fn parses_gh_versions() {
        assert_eq!(
            parse_gh_version("gh version 2.97.0 (2024-11-06)"),
            Some((2, 97, 0))
        );
        assert_eq!(parse_gh_version("gh version 2.99.1"), Some((2, 99, 1)));
        assert_eq!(parse_gh_version("gh version 3.0"), Some((3, 0, 0)));
        assert_eq!(parse_gh_version("garbage"), None);
    }

    /// Merge-queue present: the entry parses into a `QueuePosition`.
    #[test]
    fn parses_merge_status_with_queue() {
        let json = r#"{"data":{"repository":{"pullRequest":{
          "id":"PR_kw1","mergeStateStatus":"BLOCKED",
          "reviewThreads":{"nodes":[{"isResolved":false},{"isResolved":true},{"isResolved":false}]},
          "mergeQueueEntry":{"position":3,"estimatedTimeToMerge":"about 6 minutes","state":"QUEUED",
            "mergeQueue":{"nextEntryEstimatedTimeToMerge":"about 2 minutes"}}}}}}"#;
        let s = parse_merge_status(json).unwrap();
        assert_eq!(s.node_id, "PR_kw1");
        assert_eq!(s.state, MergeStateStatus::Blocked);
        assert_eq!(s.unresolved_threads, 2);
        let q = s.queue.expect("queued");
        assert_eq!(q.position, Some(3));
        assert_eq!(q.state, MergeQueueEntryState::Queued);
        assert_eq!(
            q.next_entry_estimated_time_to_merge.as_deref(),
            Some("about 2 minutes")
        );
    }

    /// Acceptance §6: merge queue absent (non-MQ repo) ⇒ `queue: None`, and the
    /// rest still parses. Nothing to render, nothing to break.
    #[test]
    fn parses_merge_status_without_queue() {
        let json = r#"{"data":{"repository":{"pullRequest":{
          "id":"PR_x","mergeStateStatus":"CLEAN",
          "reviewThreads":{"nodes":[]},"mergeQueueEntry":null}}}}"#;
        let s = parse_merge_status(json).unwrap();
        assert_eq!(s.state, MergeStateStatus::Clean);
        assert!(s.queue.is_none());
        assert_eq!(s.unresolved_threads, 0);
    }

    #[test]
    fn enqueue_dequeue_args_carry_jump_and_id() {
        let jump = enqueue_args("PR_1", true);
        assert!(jump.iter().any(|a| a == "jump=true"));
        assert!(jump.iter().any(|a| a == "id=PR_1"));
        let normal = enqueue_args("PR_1", false);
        assert!(normal.iter().any(|a| a == "jump=false"));
        let dq = dequeue_args("PR_1");
        assert!(dq.iter().any(|a| a.contains("dequeuePullRequest")));
        assert!(dq.iter().any(|a| a == "id=PR_1"));
    }
}

// ── `gh pr merge` itself (#347 / #501 / #701) ──
//
// The execute half of the PR merge op. `github.rs` keeps the PR read model
// and `plan_pr_merge`; the mutation and the server re-read that decides its
// receipt live beside the rest of the merge lifecycle.

/// Build the `gh pr merge` argument vector. Pure and unit-tested so the
/// **safety invariant** — `--match-head-commit <SHA>` is *always* present — is
/// checked without spawning `gh`. That flag makes GitHub refuse the merge if
/// the head branch moved after the plan was shown (PR-side force-with-lease,
/// #347): the same principle as force-with-lease, applied to the merge button.
pub fn merge_args(
    base_repo: &str,
    number: u64,
    method: MergeMethod,
    delete_branch: bool,
    head_sha: &str,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["pr".into(), "merge".into()];
    // `-R <host>/<owner>/<repo>`: the mutation targets the repository the plan
    // froze, so the reconcile read of the same identity is a read of the same
    // thing. Without it `gh` resolves the repository from the working
    // directory's remotes, which can move after approval (#701 review 4).
    if !base_repo.is_empty() {
        args.push("-R".into());
        args.push(base_repo.to_string());
    }
    args.extend([
        number.to_string(),
        method.flag().into(),
        // ALWAYS present — never gate this behind a flag or a branch.
        "--match-head-commit".into(),
        head_sha.to_string(),
    ]);
    if delete_branch {
        args.push("--delete-branch".into());
    }
    args
}

/// Merge the frozen PR, then account for local cleanup through the guarded
/// delete-branch family. `-R` keeps gh from changing local branches or HEAD.
///
/// #501: the attempt is recorded **here**, before returning across the UI's
/// tab-owned completion guard, so a stale completion (`OpDisposition::DropStale`)
/// cannot lose the record of a merge that really happened on GitHub. The UI's
/// callback is presentation-only.
///
/// ADR-0196 Wave 3: the returned `result` **follows the receipt**, not the raw
/// `gh` exit. A merge the server confirms is `Ok` even when `gh` exited
/// non-zero, and a merge nobody can confirm or refute is
/// [`GitError::TerminationUnknown`] — so `apply` keeps the write lease and
/// parks a reconcile entry instead of releasing a merge that may be in flight.
pub fn merge_pr(
    workdir: &Path,
    number: u64,
    method: MergeMethod,
    delete_branch: bool,
    head_sha: &str,
    plan: &OperationPlan,
) -> crate::backend::recording::RunReport {
    // The identity the plan froze addresses both the mutation and the re-read,
    // so they cannot end up talking about different repositories (#701 review 4).
    let base_repo = frozen_base_repo(plan);
    let approved_request = matches!(
        plan.recovery.as_ref().map(|r| &r.kind),
        Some(RecoveryKind::Github(kagi_domain::plan_note::GithubRecovery::MergePr {
            number: approved_number, delete_branch: approved_delete, ..
        })) if *approved_number == number && approved_delete.is_some() == delete_branch
    );
    if !approved_request
        || !plan.blockers.is_empty()
        || (delete_branch && (base_repo.is_empty() || frozen_local_branch(plan).is_none()))
    {
        let reason = "merge plan has blockers or no frozen local deletion approval";
        let entry = crate::oplog::OpLogEntry::new(
            "pr-merge",
            workdir.display().to_string(),
            plan.current.clone(),
            crate::oplog::OpOutcome::Refused {
                blockers: vec![reason.into()],
            },
        )
        .with_worktree(Some(workdir.display().to_string()));
        return crate::backend::recording::RunReport {
            result: Err(GitError::Other(reason.into())),
            recording: crate::backend::recording::finalize(entry),
            stash: None,
        };
    }
    let result = merge_pr_transport(workdir, &base_repo, number, method, delete_branch, head_sha);
    // gh exit zero can also mean queued. Never delete the local branch until
    // GitHub confirms the merge itself, even when the command succeeded.
    let merged = if result.is_ok() && !delete_branch {
        Some(true)
    } else {
        pr_merged_on_server(workdir, &base_repo, number)
    };
    let after = StateSummary {
        head: plan.predicted.head.clone(),
        dirty: format!("{} (head {head_sha})", plan.predicted.dirty),
    };
    let mut outcome = match (merged, &result) {
        (Some(true), Err(error)) if delete_branch => crate::oplog::OpOutcome::Partial {
            after: after.clone(),
            error: format!("merged; gh failed after the merge (branch deletion unconfirmed): {error}"),
        },
        (Some(true), _) => crate::oplog::OpOutcome::Success { after: after.clone() },
        (Some(false), Err(error)) => crate::oplog::OpOutcome::Failed { error: error.to_string() },
        _ => crate::oplog::OpOutcome::Unknown {
            after: StateSummary {
                head: plan.predicted.head.clone(),
                dirty: format!("#{number} state unconfirmed (head {head_sha})"),
            },
            evidence: format!(
                "merge not confirmed by `gh pr view --json mergedAt` (transport: {result:?}); do not retry"
            ),
        },
    };
    let mut backup_refs = Vec::new();
    let local_branch = frozen_local_branch(plan)
        .filter(|_| merged == Some(true) && delete_branch)
        .map(|branch| {
            finish_local_cleanup(
                workdir,
                plan,
                branch,
                head_sha,
                &mut outcome,
                &mut backup_refs,
            )
        });
    let repo = workdir.display().to_string();
    // The receipt decides. `detail` keeps `gh`'s own words for the UI.
    let detail = match &result {
        Ok(out) => out.clone(),
        Err(error) => error.to_string(),
    };
    let result = match &outcome {
        crate::oplog::OpOutcome::Success { .. } => Ok(crate::OperationOutcome::PrMerge {
            number,
            detail,
            confirmed: true,
            local_branch,
        }),
        crate::oplog::OpOutcome::Partial { error, .. } => Ok(crate::OperationOutcome::PrMerge {
            number,
            detail: error.clone(),
            confirmed: false,
            local_branch,
        }),
        // Both `gh` invocations exited — what is unknown is the *repository*
        // state, not the child. `Stopped` says so, so the lease is released at
        // settlement and the reconcile entry can be read and acknowledged. The
        // other two states would be lies here: nothing is left to probe
        // (`Unaccounted`) and kagi did not lose its executor (`Abandoned`).
        crate::oplog::OpOutcome::Unknown { evidence, .. } => Err(GitError::TerminationUnknown(
            crate::Termination::stopped(evidence.clone()),
        )),
        _ => Err(result.err().unwrap_or(GitError::Other(detail))),
    };
    let mut entry =
        crate::oplog::OpLogEntry::new("pr-merge", repo.clone(), plan.current.clone(), outcome)
            .with_worktree(Some(repo));
    entry.backup_refs = backup_refs;
    crate::backend::recording::RunReport {
        result,
        recording: crate::backend::recording::finalize(entry),
        stash: None,
    }
}

fn frozen_local_branch(plan: &OperationPlan) -> Option<&kagi_domain::plan::PrMergeLocalBranch> {
    match plan.recovery.as_ref().map(|r| &r.kind) {
        Some(RecoveryKind::Github(kagi_domain::plan_note::GithubRecovery::MergePr {
            local_branch,
            ..
        })) => local_branch.as_deref(),
        _ => None,
    }
}

/// Compose cleanup into the merge's receipt; never append a second delete receipt.
fn finish_local_cleanup(
    workdir: &Path,
    plan: &OperationPlan,
    branch: &kagi_domain::plan::PrMergeLocalBranch,
    head_sha: &str,
    outcome: &mut crate::oplog::OpOutcome,
    backup_refs: &mut Vec<String>,
) -> kagi_domain::operation::PrMergeLocalOutcome {
    use crate::oplog::OpOutcome;
    use kagi_domain::operation::PrMergeLocalOutcome;
    let local = crate::Backend::open(workdir)
        .and_then(|backend| backend.execute_pr_merge_local_branch(branch, head_sha, backup_refs))
        .unwrap_or_else(|error| PrMergeLocalOutcome::NotDeleted {
            name: branch.name.clone(),
            reason: error.to_string(),
        });
    let note = local.note().message_en();
    let after = match outcome {
        OpOutcome::Success { after } | OpOutcome::Partial { after, .. } => after,
        _ => unreachable!("cleanup only follows a confirmed merge"),
    };
    after.dirty.push_str("; ");
    after.dirty.push_str(&note);
    for reference in backup_refs {
        after.dirty.push_str("; restorable from backup ref ");
        after.dirty.push_str(reference);
    }
    if matches!(local, PrMergeLocalOutcome::NotDeleted { .. }) {
        let after = after.clone();
        let error = match outcome {
            OpOutcome::Partial { error, .. } => format!("{error}; {note}"),
            _ => note,
        };
        *outcome = OpOutcome::Partial { after, error };
    } else if matches!(
        plan.recovery.as_ref().map(|r| &r.kind),
        Some(RecoveryKind::Github(
            kagi_domain::plan_note::GithubRecovery::MergePr {
                cross_repository: true,
                ..
            }
        ))
    ) {
        // The fork has no promised remote deletion left to verify.
        *outcome = OpOutcome::Success {
            after: after.clone(),
        };
    }
    local
}

/// Did GitHub actually merge the PR? `None` means the question could not be
/// answered (gh missing, offline, auth gone) — the honest outcome is then
/// `Unknown`, never an assumed failure. Server state is authoritative; the
/// exit status of the merge command is not.
///
/// `pub` because it is asked twice: once by [`merge_pr`] to decide the
/// receipt, and again by the reconcile read that observes a
/// [`RemoteExpectation::PullRequest`](crate::backend::remote_ref::RemoteExpectation)
/// — the same question, so the same answer, including the `None` that must
/// never pass for "not merged" (#701).
///
/// `base_repo` is the frozen `<host>/<owner>/<repo>`, passed as `-R`: a PR
/// number is not an address, and resolving one from the working directory's
/// remotes would let the answer come from a repository the plan never named
/// (#701 review 4). `workdir` is only `gh`'s working directory.
pub fn pr_merged_on_server(workdir: &Path, base_repo: &str, number: u64) -> Option<bool> {
    if base_repo.is_empty() {
        return None; // no address, no answer — never "not merged"
    }
    let out = crate::cli::gh_command()
        .args([
            "pr",
            "view",
            "-R",
            base_repo,
            &number.to_string(),
            "--json",
            "mergedAt",
        ])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).ok()?;
    let merged_at = value.get("mergedAt")?;
    if merged_at.is_null() {
        Some(false)
    } else {
        merged_at.as_str().map(|_| true)
    }
}

/// The `<host>/<owner>/<repo>` this plan froze, or empty when it named none.
fn frozen_base_repo(plan: &OperationPlan) -> String {
    match plan.recovery.as_ref().map(|recovery| &recovery.kind) {
        Some(RecoveryKind::Github(kagi_domain::plan_note::GithubRecovery::MergePr {
            base_repo,
            ..
        })) => base_repo.clone(),
        _ => String::new(),
    }
}

fn merge_pr_transport(
    workdir: &Path,
    base_repo: &str,
    number: u64,
    method: MergeMethod,
    delete_branch: bool,
    head_sha: &str,
) -> Result<String, GitError> {
    let args = merge_args(base_repo, number, method, delete_branch, head_sha);
    let out = crate::cli::gh_command()
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
