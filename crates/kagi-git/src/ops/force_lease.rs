//! Force-with-lease push operation pipeline (branch-menu "Advanced /
//! Dangerous" group, "Force-with-lease push...").
//!
//! The ONE place in this codebase that passes `--force-with-lease` to `git
//! push` — see `push.rs`'s module doc for why every *other* push path never
//! forces. The lease value is always the local record of the remote-tracking
//! ref (`refs/remotes/<remote>/<branch>`) captured at plan time and passed
//! explicitly as `--force-with-lease=<branch>:<expected-oid>`, so the remote
//! rejects the push outright if it moved since our last fetch — this is not
//! a blind `--force`.

use super::remote_common::{local_branch_oid, resolve_upstream_info, resolve_upstream_oid};
use super::*;
use kagi_domain::plan_note::{ForceLeaseNote, ForceLeaseRecovery, ForceLeaseTitle, PlanOp};

// ────────────────────────────────────────────────────────────
// plan_force_with_lease_push
// ────────────────────────────────────────────────────────────

/// Analyse whether a force-with-lease push of the current branch is safe and
/// return an [`OperationPlan`]. `destructive: true` — the UI must require an
/// armed two-stage confirm (mirrors `discard` / `delete-remote-branch` /
/// `reset-current-to-head`), not a single click.
///
/// # Blocker conditions
///
/// - HEAD is detached or unborn.
/// - No upstream configured for the current branch.
/// - The local branch tip already matches the remote-tracking ref (nothing
///   to force-push).
pub fn plan_force_with_lease_push(repo: &Repository) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let head_display = head.display();
    let dirty_display = if status.is_dirty() {
        "dirty".to_string()
    } else {
        "clean".to_string()
    };
    let current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display.clone(),
    };
    let predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    let branch_name = match &head {
        Head::Attached { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    if branch_name.is_none() {
        blockers.push(PlanNote::Common(CommonNote::HeadDetached {
            op: PlanOp::Push,
        }));
    }

    let mut title_branch = branch_name.clone().unwrap_or_default();
    let mut title_remote = String::new();
    let mut recovery = None;
    // #353: faithful equivalent, set only in the successful-lease arm below.
    let mut equivalent_command: Option<String> = None;

    if let Some(branch) = branch_name.as_ref() {
        match resolve_upstream_info(repo, branch) {
            Ok((_, remote, _)) => {
                title_remote = remote.clone();
                let local_oid = local_branch_oid(repo, branch).ok();
                let lease_oid = resolve_upstream_oid(repo, branch, &remote).ok();

                match (local_oid, lease_oid) {
                    (Some(local), Some(lease)) if local == lease => {
                        blockers.push(PlanNote::ForceLease(ForceLeaseNote::NothingToPush {
                            branch: branch.clone(),
                        }));
                    }
                    (Some(local), Some(lease)) => {
                        warnings.push(PlanNote::ForceLease(
                            ForceLeaseNote::RewritesRemoteHistory {
                                branch: branch.clone(),
                            },
                        ));
                        warnings.push(PlanNote::ForceLease(ForceLeaseNote::LeaseValue {
                            remote: remote.clone(),
                            sha: lease.to_string()[..8].to_string(),
                        }));
                        recovery = Some(PlanRecovery {
                            kind: RecoveryKind::ForceLease(ForceLeaseRecovery::ForceLeasePush {
                                branch: branch.clone(),
                                remote: remote.clone(),
                                previous_remote_sha: lease.to_string(),
                                new_sha: local.to_string(),
                            }),
                            commands: vec![format!(
                                "git push --force-with-lease={}:{} {} {}:refs/heads/{}",
                                branch, local, remote, lease, branch
                            )],
                        });
                        // #353: the forward equivalent guards the remote tip
                        // (`lease`) and pushes the local tip. Faithful to what
                        // libgit2 does — same lease, same refspec.
                        equivalent_command = Some(format!(
                            "git push --force-with-lease={}:{} {} {}",
                            branch, lease, remote, branch
                        ));
                    }
                    _ => {
                        blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                            message: format!(
                                "could not resolve local or remote-tracking tip for '{}'",
                                branch
                            ),
                        }));
                    }
                }
                title_branch = branch.clone();
            }
            Err(_) => {
                blockers.push(PlanNote::ForceLease(ForceLeaseNote::NoUpstream {
                    branch: branch.clone(),
                }));
            }
        }
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::ForceLease(ForceLeaseTitle::ForceLeasePush {
            branch: title_branch,
            remote: title_remote,
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
        equivalent_command,
    })
}

// ────────────────────────────────────────────────────────────
// execute_force_with_lease_push
// ────────────────────────────────────────────────────────────

/// Push the current branch with `--force-with-lease=<branch>:<lease-oid>`,
/// where `lease-oid` is **the value the plan captured and showed the user**
/// (the `LeaseValue` note), not a fresh read.
///
/// That distinction is the entire safety property. If someone else pushed in
/// the meantime, the lease is stale on purpose and the remote rejects the push
/// instead of silently overwriting unseen work. Leasing against whatever the
/// remote holds at execute time is indistinguishable from a blind `--force`.
///
/// This used to re-read the remote-tracking ref here, and the module doc
/// claimed an automatic fetch could not interfere. It could: kagi's own
/// auto-fetch ticker updates `refs/remotes/<remote>/<branch>` in the
/// background, so a colleague's push landing while the confirm modal sat open
/// was fetched, adopted as the new lease, and overwritten. The preflight does
/// not catch it — it compares local HEAD, which a fetch does not move.
/// `a_fetch_between_plan_and_execute_does_not_refresh_the_lease` in
/// `tests/force_lease_push_test.rs` is the guard.
///
/// This is the **only** call site in the codebase that passes
/// `--force-with-lease` (or any force flag) to `git push`.
///
/// The lease comes from `plan`, so git enforces the exact remote tip the user
/// was shown. If the remote moved since, the push is rejected by git rather
/// than silently re-leased against the newer value.
/// The remote tip the plan recorded, i.e. what the confirm modal told the user
/// it would lease against.
fn plan_lease_sha(plan: &OperationPlan) -> Option<&str> {
    match &plan.recovery.as_ref()?.kind {
        RecoveryKind::ForceLease(ForceLeaseRecovery::ForceLeasePush {
            previous_remote_sha,
            ..
        }) => Some(previous_remote_sha.as_str()),
        _ => None,
    }
}

pub fn execute_force_with_lease_push(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is not on a branch. Force-with-lease push requires an attached HEAD.".to_string(),
        ));
    }
    let branch = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();

    let (_, remote, _) = resolve_upstream_info(repo, &branch)?;

    // The lease is the value the plan showed the user — never a fresh read.
    //
    // This used to call `resolve_upstream_oid` again here, which quietly
    // undid the entire point of `--force-with-lease`: kagi's own auto-fetch
    // ticker updates `refs/remotes/<remote>/<branch>` in the background, so a
    // colleague's push landing while the confirm modal sat open was fetched,
    // adopted as the new lease, and then overwritten. The preflight cannot
    // catch it — it compares local HEAD, which a fetch does not move.
    let lease_oid = plan_lease_sha(plan).ok_or_else(|| {
        GitError::Other(
            "force-with-lease: the plan carries no lease value — re-plan the push".to_string(),
        )
    })?;

    // #291: `branch` is also interpolated into the `--force-with-lease=` value,
    // where `--` cannot protect it — the leading-dash reject is the guard there.
    check_operand("remote", &remote)?;
    check_operand("branch", &branch)?;

    let lease_arg = format!("--force-with-lease={}:{}", branch, lease_oid);
    let out = run_git(repo_path, &["push", &lease_arg, "--", &remote, &branch])
        .map_err(|e| GitError::Other(format!("push failed: {}", e)))?;

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "push failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }
    Ok(())
}
