//! Sync-to-remote (#536, ADR-0215): make a local branch — and, when it is
//! HEAD, the index and working tree — match its fetched upstream tip, with
//! every local commit and change retained under `refs/kagi/backups/` first.
//!
//! git2 only. Nothing here runs `git stash`, and no destructive git command
//! is involved: the working tree is replaced by `checkout_tree` of the target
//! (force, `overwrite_ignored(false)`, `remove_untracked(false)`), and the
//! untracked files that were retained are then removed one by one, each only
//! after its retained blob is verified. Ignored files are never touched.
//!
//! Order of writes: backups → checkout → untracked removal → ref update
//! (expected-old, under lock) → verify. The ref moves last so a refused
//! checkout (an ignored-file collision, say) leaves the branch where it was.

use super::*;
use kagi_domain::operation::SyncWorkBackup;
use kagi_domain::plan_note::{SyncNote, SyncRecovery, SyncTitle};
use kagi_domain::remote::shell_quote;

/// What the plan resolved; carried through preflight/execute via the plan's
/// title (names) and recovery (OIDs + backup ref names).
pub(crate) struct Resolved {
    upstream_name: String,
    target: git2::Oid,
    from: git2::Oid,
}

fn branch_refname(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

/// Upstream of `branch`: `Err(NoUpstream)` when none is configured,
/// `Err(UpstreamNotFetched)` when configured but the tracking ref is absent.
fn resolve_upstream(repo: &Repository, branch: &str) -> Result<(String, git2::Oid), SyncNote> {
    let refname = branch_refname(branch);
    let Ok(upstream_ref) = repo.branch_upstream_name(&refname) else {
        return Err(SyncNote::NoUpstream {
            branch: branch.to_string(),
        });
    };
    let full = upstream_ref.as_str().unwrap_or_default().to_string();
    let short = full.trim_start_matches("refs/remotes/").to_string();
    match repo.find_reference(&full).ok().and_then(|r| r.target()) {
        Some(oid) => Ok((short, oid)),
        None => Err(SyncNote::UpstreamNotFetched {
            branch: branch.to_string(),
            upstream: short,
        }),
    }
}

/// Analyse syncing `branch` to its upstream.
pub fn plan_sync_to_remote(repo: &Repository, branch: &str) -> Result<OperationPlan, GitError> {
    check_operand("branch", branch)?;
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty = status_summary_display(&status);
    let is_head = matches!(&head, Head::Attached { branch: b, .. } if b == branch);
    let operation_id = super::backup::operation_id();
    let tip_backup = format!("{}{}/0", super::backup::PREFIX, operation_id);
    let work_backup = format!("{}{}/1", super::backup::PREFIX, operation_id);

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    let from = repo
        .find_branch(branch, BranchType::Local)
        .ok()
        .and_then(|b| b.get().target());
    let Some(from) = from else {
        blockers.push(PlanNote::Sync(SyncNote::BranchMissing {
            branch: branch.to_string(),
        }));
        return Ok(finish(
            head, dirty, branch, "?", "?", "", blockers, warnings, None, None,
        ));
    };
    let (upstream_name, target) = match resolve_upstream(repo, branch) {
        Ok(u) => u,
        Err(note) => {
            blockers.push(PlanNote::Sync(note));
            return Ok(finish(
                head,
                dirty,
                branch,
                "?",
                "?",
                &from.to_string(),
                blockers,
                warnings,
                Some(tip_backup),
                None,
            ));
        }
    };
    let short_to = target.to_string()[..7].to_string();

    if matches!(head, Head::Detached { .. }) {
        blockers.push(PlanNote::Sync(SyncNote::DetachedHead));
    }
    if let Some(op) = super::in_progress_op(repo) {
        blockers.push(PlanNote::Sync(SyncNote::OperationInProgress { op }));
    }
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Sync(SyncNote::ConflictedFiles {
            count: status.conflicted.len(),
        }));
    }
    if !is_head {
        let repositories = super::branch_delete_safety::repositories(repo)?;
        if let Some(path) = super::branch_delete_safety::checked_out_at(&repositories, branch)? {
            blockers.push(PlanNote::Sync(SyncNote::CheckedOutElsewhere {
                branch: branch.to_string(),
                path: path.display().to_string(),
            }));
        }
    }
    if is_head {
        let target_tree = repo
            .find_commit(target)
            .and_then(|commit| commit.tree())
            .map_err(|e| {
                GitError::Other(format!("cannot read sync target tree: {}", e.message()))
            })?;
        blockers.extend(crate::special_repo::force_checkout_blockers(
            repo,
            &target_tree,
            &status,
        )?);
    }
    let work_dirty = is_head && status.is_dirty();
    if from == target && !work_dirty && blockers.is_empty() {
        blockers.push(PlanNote::Sync(SyncNote::AlreadyInSync {
            branch: branch.to_string(),
            upstream: upstream_name.clone(),
        }));
    }
    if from != target {
        let ahead = repo
            .graph_ahead_behind(from, target)
            .map(|(ahead, _)| ahead)
            .unwrap_or(0);
        if ahead > 0 {
            warnings.push(PlanNote::Sync(SyncNote::AbandonsCommits {
                branch: branch.to_string(),
                count: ahead,
            }));
        }
    }
    if work_dirty {
        warnings.push(PlanNote::Sync(SyncNote::PreservesWork {
            staged: status.staged.len(),
            unstaged: status.unstaged.len(),
            untracked: status.untracked.len(),
        }));
    }
    if is_head {
        warnings.push(PlanNote::Sync(SyncNote::KeepsIgnored));
    }
    Ok(finish(
        head,
        dirty,
        branch,
        &upstream_name,
        &short_to,
        &from.to_string(),
        blockers,
        warnings,
        Some(tip_backup),
        work_dirty.then_some(work_backup),
    ))
}

#[allow(clippy::too_many_arguments)]
fn finish(
    head: Head,
    dirty: String,
    branch: &str,
    upstream: &str,
    to: &str,
    from: &str,
    blockers: Vec<PlanNote>,
    warnings: Vec<PlanNote>,
    tip_backup: Option<String>,
    work_backup: Option<String>,
) -> OperationPlan {
    let is_head = matches!(&head, Head::Attached { branch: b, .. } if b == branch);
    let recovery = tip_backup.map(|tip_backup| {
        let mut commands = vec![format!(
            "git update-ref {} {}",
            shell_quote(&format!("refs/heads/{branch}")),
            shell_quote(&tip_backup)
        )];
        if let Some(work) = &work_backup {
            commands.push(format!("git stash apply --index {}", shell_quote(work)));
        }
        PlanRecovery {
            kind: RecoveryKind::Sync(SyncRecovery::SyncToRemote {
                branch: branch.to_string(),
                from: from.to_string(),
                tip_backup,
                work_backup,
            }),
            commands,
        }
    });
    OperationPlan {
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Sync(SyncTitle::SyncToRemote {
            branch: branch.to_string(),
            upstream: upstream.to_string(),
            to: to.to_string(),
        }),
        current: StateSummary {
            head: head.display(),
            dirty: dirty.clone(),
        },
        predicted: StateSummary {
            head: if is_head {
                format!("{branch} @ {to}")
            } else {
                head.display()
            },
            dirty: if is_head { "clean".to_string() } else { dirty },
        },
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        // No digest: everything in the working tree is retained whatever its
        // state at execute time; a plan made on a clean tree refuses at
        // preflight if the tree became dirty (no work backup was planned).
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        // History + working tree are replaced: two-stage confirm (ADR-0023).
        destructive: true,
        equivalent_command: None,
    }
}

/// Everything preflight needs, re-derived from the repository and compared
/// with what the plan recorded.
fn resolve_for_execute(
    repo: &Repository,
    plan: &OperationPlan,
    branch: &str,
) -> Result<(Resolved, String, Option<String>), GitError> {
    let Some(PlanRecovery {
        kind:
            RecoveryKind::Sync(SyncRecovery::SyncToRemote {
                from,
                tip_backup,
                work_backup,
                ..
            }),
        ..
    }) = &plan.recovery
    else {
        return Err(GitError::Other(
            "sync-to-remote: plan carries no recovery record; please re-plan".to_string(),
        ));
    };
    let PlanTitle::Sync(SyncTitle::SyncToRemote { to, .. }) = &plan.title else {
        return Err(GitError::Other(
            "sync-to-remote: plan title mismatch; please re-plan".to_string(),
        ));
    };
    let now = repo
        .find_branch(branch, BranchType::Local)
        .ok()
        .and_then(|b| b.get().target())
        .ok_or_else(|| GitError::Other(format!("branch '{branch}' not found")))?;
    if now.to_string() != *from {
        return Err(GitError::Other(format!(
            "'{branch}' moved after planning ({} → {}); please re-plan",
            &from[..7.min(from.len())],
            &now.to_string()[..7]
        )));
    }
    let (upstream_name, target) = resolve_upstream(repo, branch).map_err(|note| {
        GitError::Other(format!("sync-to-remote refused: {}", note.message_en()))
    })?;
    if !target.to_string().starts_with(to.as_str()) {
        return Err(GitError::Other(format!(
            "'{upstream_name}' moved after planning ({to} → {}); please re-plan",
            &target.to_string()[..7]
        )));
    }
    Ok((
        Resolved {
            upstream_name,
            target,
            from: now,
        },
        tip_backup.clone(),
        work_backup.clone(),
    ))
}

/// Preflight: blockers empty, HEAD unchanged, branch and upstream where the
/// plan saw them, no operation/conflict appeared, and (when the plan kept no
/// work backup) the working tree is still clean.
pub(crate) fn preflight_sync_to_remote(
    repo: &Repository,
    plan: &OperationPlan,
    branch: &str,
) -> Result<(Resolved, String, Option<String>), GitError> {
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(
            "sync-to-remote refused: plan has blockers".to_string(),
        ));
    }
    preflight_check(repo, plan)?;
    if let Some(op) = super::in_progress_op(repo) {
        return Err(GitError::Other(format!(
            "sync-to-remote refused at preflight: {} is in progress",
            op.label_en()
        )));
    }
    let resolved = resolve_for_execute(repo, plan, branch)?;
    let status = working_tree_status(repo)?;
    if !status.conflicted.is_empty() {
        return Err(GitError::Other(
            "sync-to-remote refused at preflight: the index has unmerged paths".to_string(),
        ));
    }
    let is_head = matches!(resolve_head(repo)?, Head::Attached { branch: b, .. } if b == branch);
    if is_head {
        let target_tree = repo
            .find_commit(resolved.0.target)
            .and_then(|commit| commit.tree())
            .map_err(|e| {
                GitError::Other(format!("cannot read sync target tree: {}", e.message()))
            })?;
        if let Some(note) =
            crate::special_repo::force_checkout_blockers(repo, &target_tree, &status)?
                .into_iter()
                .next()
        {
            return Err(GitError::Blocked(Box::new(note)));
        }
    }
    if is_head && resolved.2.is_none() && status.is_dirty() {
        return Err(GitError::Other(
            "sync-to-remote refused at preflight: the working tree changed since planning; please re-plan"
                .to_string(),
        ));
    }
    Ok(resolved)
}

/// Retain the index and working tree as a stash-shaped commit: tree = the
/// working tree (`git add -A` view, untracked-not-ignored included), parent 1
/// = HEAD, parent 2 = an index commit (tree = index, parent HEAD). This is
/// exactly what `git stash apply --index <ref>` consumes.
fn retain_work(
    repo: &Repository,
    head: &git2::Commit<'_>,
    operation_id: &str,
) -> Result<SyncWorkBackup, GitError> {
    let sig = super::build_signature(repo)?;
    let index_tree = repo
        .index()
        .and_then(|mut i| i.write_tree())
        .map_err(|e| GitError::Other(format!("sync-to-remote: index tree: {}", e.message())))?;
    let index_tree = repo
        .find_tree(index_tree)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let index_commit = repo
        .commit(
            None,
            &sig,
            &sig,
            "kagi sync-to-remote: index",
            &index_tree,
            &[head],
        )
        .map_err(|e| GitError::Other(format!("sync-to-remote: index commit: {}", e.message())))?;
    let index_commit = repo
        .find_commit(index_commit)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let work_tree = super::snapshot::write_worktree_tree(repo)?;
    let work_tree = repo
        .find_tree(work_tree)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let work_commit = repo
        .commit(
            None,
            &sig,
            &sig,
            "kagi sync-to-remote: working tree",
            &work_tree,
            &[head, &index_commit],
        )
        .map_err(|e| GitError::Other(format!("sync-to-remote: work commit: {}", e.message())))?;
    let reference = super::backup::retain_object(repo, operation_id, 1, work_commit)?;
    Ok(SyncWorkBackup {
        reference,
        commit: work_commit.to_string(),
    })
}

/// Remove the untracked files that are retained in `work_tree`: each path is
/// removed only if its current content hashes to the blob the backup holds.
/// Returns how many were removed; anything else (ignored files, files that
/// changed after the backup) is left in place.
fn remove_retained_untracked(
    repo: &Repository,
    work_tree: &git2::Tree<'_>,
    untracked: &[PathBuf],
) -> Result<usize, GitError> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("sync-to-remote: bare repository".to_string()))?;
    let mut removed = 0;
    for rel in untracked {
        let Ok(entry) = work_tree.get_path(rel) else {
            continue;
        };
        if entry.kind() != Some(git2::ObjectType::Blob) {
            continue;
        }
        let abs = workdir.join(rel);
        let Ok(meta) = std::fs::symlink_metadata(&abs) else {
            continue;
        };
        let actual = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)
                .map_err(|e| GitError::Other(format!("{}: {e}", rel.display())))?;
            git2::Oid::hash_object(git2::ObjectType::Blob, target.to_string_lossy().as_bytes())
        } else if meta.is_file() {
            git2::Oid::hash_file(git2::ObjectType::Blob, &abs)
        } else {
            continue;
        }
        .map_err(|e| GitError::Other(format!("{}: {}", rel.display(), e.message())))?;
        if actual != entry.id() {
            continue;
        }
        std::fs::remove_file(&abs)
            .map_err(|e| GitError::Other(format!("remove {}: {e}", rel.display())))?;
        removed += 1;
    }
    Ok(removed)
}

/// Execute: preflight → retain tip (and work, when HEAD is the branch and it
/// is dirty) → `checkout_tree` target (HEAD branch only) → remove retained
/// untracked → move the ref under lock with expected-old → verify.
pub(crate) fn execute_sync_to_remote(
    repo: &Repository,
    plan: &OperationPlan,
    branch: &str,
    backup_refs: &mut Vec<String>,
    partial_after: &mut Option<StateSummary>,
) -> Result<crate::OperationOutcome, GitError> {
    let (resolved, tip_backup_name, work_backup_name) =
        preflight_sync_to_remote(repo, plan, branch)?;
    let operation_id = tip_backup_name
        .strip_prefix(super::backup::PREFIX)
        .and_then(|rest| rest.strip_suffix("/0"))
        .ok_or_else(|| GitError::Other("sync-to-remote: malformed backup ref in plan".to_string()))?
        .to_string();
    let is_head = matches!(resolve_head(repo)?, Head::Attached { branch: b, .. } if b == branch);

    // 1. Backups first; they reach the oplog even if anything below fails.
    let tip_backup = super::backup::retain_object(repo, &operation_id, 0, resolved.from)?;
    backup_refs.push(tip_backup.clone());
    let head_commit = repo
        .find_commit(resolved.from)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let status_before = working_tree_status(repo)?;
    let work_backup = if is_head && work_backup_name.is_some() {
        let kept = retain_work(repo, &head_commit, &operation_id)?;
        backup_refs.push(kept.reference.clone());
        Some(kept)
    } else {
        None
    };
    let restore_hint = |work: &Option<SyncWorkBackup>| {
        let mut s = format!("restore tip: git update-ref refs/heads/{branch} {tip_backup}");
        if let Some(w) = work {
            s.push_str(&format!(
                "; restore work: git stash apply --index {}",
                w.reference
            ));
        }
        s
    };

    let mut removed_untracked = 0;
    if is_head {
        // 2. Working tree + index → target. Force overwrites tracked changes
        // (retained above); ignored files are never overwritten (a collision
        // is an error before anything is written) and untracked files are
        // never removed here.
        let target_commit = repo
            .find_commit(resolved.target)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force().overwrite_ignored(false).remove_untracked(false);
        if let Err(e) = repo.checkout_tree(target_commit.as_object(), Some(&mut cb)) {
            *partial_after = Some(StateSummary {
                head: plan.current.head.clone(),
                dirty: format!(
                    "checkout of target refused or incomplete; {}",
                    restore_hint(&work_backup)
                ),
            });
            return Err(GitError::Other(format!(
                "sync-to-remote: checkout of '{}' failed: {}",
                resolved.upstream_name,
                e.message()
            )));
        }
        // 3. Untracked files that were retained are removed, each after its
        // retained blob is verified. Ignored files are not in this list.
        if let Some(work) = &work_backup {
            let work_tree = repo
                .find_commit(
                    git2::Oid::from_str(&work.commit)
                        .map_err(|e| GitError::Other(e.message().to_string()))?,
                )
                .and_then(|c| c.tree())
                .map_err(|e| GitError::Other(e.message().to_string()))?;
            removed_untracked =
                remove_retained_untracked(repo, &work_tree, &status_before.untracked)?;
        }
    }

    // 4. Move the branch, expected-old under lock.
    let refname = branch_refname(branch);
    let mut transaction = repo
        .transaction()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    transaction
        .lock_ref(&refname)
        .map_err(|e| GitError::Other(format!("lock {refname}: {}", e.message())))?;
    let now = repo
        .refname_to_id(&refname)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    if now != resolved.from {
        *partial_after = Some(StateSummary {
            head: plan.current.head.clone(),
            dirty: format!(
                "working tree moved to the target but '{branch}' changed under us and was not moved; {}",
                restore_hint(&work_backup)
            ),
        });
        return Err(GitError::Other(format!(
            "'{branch}' moved during execution; branch not updated"
        )));
    }
    let sig = super::build_signature(repo)?;
    transaction
        .set_target(
            &refname,
            resolved.target,
            Some(&sig),
            &format!(
                "kagi sync-to-remote: {} -> {}",
                resolved.from, resolved.target
            ),
        )
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    transaction
        .commit()
        .map_err(|e| GitError::Other(format!("commit ref update: {}", e.message())))?;

    // 5. Verify.
    verify_sync_to_remote(
        repo,
        branch,
        &resolved,
        is_head,
        &status_before.untracked,
        &tip_backup,
        work_backup.as_ref(),
    )
    .map_err(|e| {
        *partial_after = Some(StateSummary {
            head: plan.predicted.head.clone(),
            dirty: format!("verify failed: {e}; {}", restore_hint(&work_backup)),
        });
        e
    })?;

    Ok(crate::OperationOutcome::SyncToRemote {
        branch: branch.to_string(),
        from: resolved.from.to_string(),
        to: resolved.target.to_string(),
        tip_backup,
        work_backup,
        removed_untracked,
    })
}

/// The branch is at the target; when it is HEAD the index and tracked tree
/// are clean and no retained untracked path remains; both backups resolve.
fn verify_sync_to_remote(
    repo: &Repository,
    branch: &str,
    resolved: &Resolved,
    is_head: bool,
    untracked_before: &[PathBuf],
    tip_backup: &str,
    work_backup: Option<&SyncWorkBackup>,
) -> Result<(), GitError> {
    let now = repo
        .refname_to_id(&branch_refname(branch))
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    if now != resolved.target {
        return Err(GitError::Other(format!(
            "'{branch}' is at {} after sync, expected {}",
            &now.to_string()[..7],
            &resolved.target.to_string()[..7]
        )));
    }
    if is_head {
        let status = working_tree_status(repo)?;
        if !status.staged.is_empty() || !status.unstaged.is_empty() || !status.conflicted.is_empty()
        {
            return Err(GitError::Other(format!(
                "working tree is not clean after sync ({} staged, {} unstaged, {} conflicted)",
                status.staged.len(),
                status.unstaged.len(),
                status.conflicted.len()
            )));
        }
        // A retained untracked path may remain only if the target tracks it
        // now (it was overwritten by the checkout, not left behind).
        if work_backup.is_some() {
            if let Some(left) = status
                .untracked
                .iter()
                .find(|p| untracked_before.contains(p))
            {
                return Err(GitError::Other(format!(
                    "retained untracked file still present after sync: {}",
                    left.display()
                )));
            }
        }
    }
    let tip = repo
        .refname_to_id(tip_backup)
        .map_err(|e| GitError::Other(format!("{tip_backup}: {}", e.message())))?;
    if tip != resolved.from {
        return Err(GitError::Other(format!(
            "{tip_backup} does not hold the pre-sync tip"
        )));
    }
    if let Some(work) = work_backup {
        let oid = repo
            .refname_to_id(&work.reference)
            .map_err(|e| GitError::Other(format!("{}: {}", work.reference, e.message())))?;
        if oid.to_string() != work.commit {
            return Err(GitError::Other(format!(
                "{} does not hold the work backup",
                work.reference
            )));
        }
    }
    Ok(())
}
