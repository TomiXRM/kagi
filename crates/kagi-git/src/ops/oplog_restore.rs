//! Op-revert / restore-to-point (#334 slice 2b, ADR-0214 §5): put branches
//! back where Operation Log entries **recorded** them, by ref update only.
//!
//! plan → confirm (two-stage, destructive) → preflight (re-plan, same
//! restores) → execute (retain every moved tip under `refs/kagi/backups/`,
//! one `git update-ref --stdin` transaction in which git re-checks every old
//! value) → verify → oplog (via `Backend::run`, so the restore records its own
//! ref moves and can itself be reverted). No working tree, index or HEAD
//! target is touched: an entry that switched HEAD is a blocker.

use super::*;
use kagi_domain::operation::Operation;
use kagi_domain::plan_note::{OplogRestoreNote, OplogRestoreRecovery, OplogRestoreTitle};
use kagi_domain::ref_moves::RefSnapshot;
use kagi_domain::ref_restore::{
    self, EntryRepo, Observed, RecordedEntry, RefRestore, RestoreMode, RestoredRef,
};
use std::collections::{BTreeSet, HashMap};

/// How many Operation Log entries are searched for the target and its range.
const ENTRY_SCAN: usize = 1000;

/// HEAD of the worktree `repo` is (its symbolic target and commit) and every
/// `refs/heads/*` (#334 slice 2a). Other worktrees' HEADs are not read: a
/// branch they hold moving shows in `refs/heads/*`. `None` when the refs
/// cannot be read — callers then record (or plan) nothing rather than guess.
pub(crate) fn ref_snapshot(repo: &Repository) -> Option<RefSnapshot> {
    let head = repo.find_reference("HEAD").ok()?;
    let head_symbolic = head.symbolic_target().ok().flatten().map(str::to_string);
    // An unborn branch has a symbolic target but nothing to resolve.
    let head_oid = head
        .resolve()
        .ok()
        .and_then(|r| r.target())
        .map(|o| o.to_string());
    let mut branches = std::collections::BTreeMap::new();
    for reference in repo.references_glob("refs/heads/*").ok()? {
        let reference = reference.ok()?;
        let (Ok(name), Some(oid)) = (reference.name(), reference.target()) else {
            continue;
        };
        branches.insert(name.to_string(), oid.to_string());
    }
    Some(RefSnapshot {
        head_oid,
        head_symbolic,
        branches,
    })
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The whole Operation Log tail, oldest first, each entry classified as this
/// repository's (its worktree opens onto the same common dir — branches are
/// shared by every worktree), another's, or unknown (its worktree can no
/// longer be opened: deleted or pruned). The planner fails closed on unknown
/// entries and on a broken chain in the range (#878 review).
fn log_entries(repo: &Repository) -> Vec<RecordedEntry> {
    let mine = canonical(repo.commondir());
    let mut common_of: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut entries: Vec<RecordedEntry> = crate::oplog::read_oplog_tail(ENTRY_SCAN)
        .into_iter()
        .map(|e| {
            let path = e.worktree.clone().unwrap_or_else(|| e.repo.clone());
            let common = common_of
                .entry(path.clone())
                .or_insert_with_key(|path| {
                    Repository::open(path)
                        .ok()
                        .map(|r| canonical(r.commondir()))
                })
                .clone();
            let repo = match common {
                Some(common) if common == mine => EntryRepo::Mine,
                Some(_) => EntryRepo::Other,
                None => EntryRepo::Unknown(path),
            };
            RecordedEntry {
                id: e.id,
                parent: e.parent,
                timestamp: e.timestamp,
                op: e.op,
                repo,
                ref_moves: e.ref_moves,
            }
        })
        .collect();
    entries.sort_by_key(|e| e.id);
    entries
}

/// Local branches whose reflog records an update after `after` (unix
/// seconds): created or moved since the target entry was recorded. Which of
/// them the record explains is the planner's decision.
fn branches_changed_after(repo: &Repository, after: i64) -> Result<BTreeSet<String>, GitError> {
    let mut changed = BTreeSet::new();
    let refs = repo
        .references_glob("refs/heads/*")
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    for reference in refs.flatten() {
        let Ok(name) = reference.name() else {
            continue;
        };
        let Ok(log) = repo.reflog(name) else {
            continue;
        };
        if log
            .iter()
            .any(|line| line.committer().when().seconds() > after)
        {
            changed.insert(name.to_string());
        }
    }
    Ok(changed)
}

/// Plan undoing exactly the oplog entry `entry_id`.
pub fn plan_op_revert(repo: &Repository, entry_id: u64) -> Result<OperationPlan, GitError> {
    plan_oplog_restore(repo, RestoreMode::Revert, entry_id)
}

/// Plan putting every branch back where it was right after `entry_id`.
pub fn plan_restore_to_point(repo: &Repository, entry_id: u64) -> Result<OperationPlan, GitError> {
    plan_oplog_restore(repo, RestoreMode::RestoreTo, entry_id)
}

fn plan_oplog_restore(
    repo: &Repository,
    mode: RestoreMode,
    entry_id: u64,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let dirty = status_summary_display(&working_tree_status(repo)?);
    let entries = log_entries(repo);
    let target = entries
        .iter()
        .find(|e| e.id == entry_id && e.repo == EntryRepo::Mine);
    let op = target.map(|e| e.op.clone()).unwrap_or_default();
    let observed = match target {
        Some(t) if mode == RestoreMode::RestoreTo => Observed {
            changed_after_target: branches_changed_after(repo, t.timestamp)?,
        },
        _ => Observed::default(),
    };
    let current = ref_snapshot(repo)
        .ok_or_else(|| GitError::Other("cannot read the repository's branches".to_string()))?;
    let planned = ref_restore::plan(&entries, entry_id, mode, &current, &observed);

    let note = PlanNote::OplogRestore;
    let mut blockers: Vec<PlanNote> = planned.blockers.into_iter().map(note).collect();
    let mut warnings = Vec::new();
    let repositories = super::branch_delete_safety::repositories(repo)?;
    // Any worktree mid-operation: a branch it builds on must not move (#878).
    for wt in &repositories {
        if let Some(op) = in_progress_op(wt) {
            blockers.push(note(OplogRestoreNote::OperationInProgress {
                op,
                path: wt.workdir().unwrap_or(wt.path()).display().to_string(),
            }));
        }
    }
    for r in &planned.restores {
        warnings.push(note(OplogRestoreNote::Moves {
            refname: r.refname.clone(),
            from: r.expect.clone(),
            to: r.restore_to.clone(),
        }));
        if let Some(target) = &r.restore_to {
            let present = git2::Oid::from_str(target)
                .ok()
                .is_some_and(|oid| repo.find_commit(oid).is_ok());
            if !present {
                blockers.push(note(OplogRestoreNote::TargetGone {
                    refname: r.refname.clone(),
                    oid: target.clone(),
                }));
            }
        }
        let branch = r.refname.trim_start_matches("refs/heads/").to_string();
        let Some(path) = super::branch_delete_safety::checked_out_at(&repositories, &branch)?
        else {
            continue;
        };
        let shown = path.display().to_string();
        let checked_out_dirty = Repository::open(&path)
            .map_err(|e| GitError::Other(e.message().to_string()))
            .and_then(|wt| working_tree_status(&wt))?
            .is_dirty();
        // A soft move (ADR-0084): the index and files stay. A dirty worktree is
        // a warning, not a blocker — moving its branch back always leaves
        // that worktree "dirty", so blocking would make a restore impossible
        // to revert. Deleting a checked-out branch is still refused.
        match (&r.restore_to, checked_out_dirty) {
            (None, _) => blockers.push(note(OplogRestoreNote::DeletesCheckedOutBranch {
                branch,
                path: shown,
            })),
            (Some(_), true) => warnings.push(note(OplogRestoreNote::CheckedOutDirty {
                branch,
                path: shown,
            })),
            (Some(_), false) => warnings.push(note(OplogRestoreNote::MovesCheckedOutBranch {
                branch,
                path: shown,
            })),
        }
    }
    warnings.push(note(OplogRestoreNote::RefsOnly));

    let title = match mode {
        RestoreMode::Revert => OplogRestoreTitle::Revert { id: entry_id, op },
        RestoreMode::RestoreTo => OplogRestoreTitle::RestoreTo { id: entry_id, op },
    };
    let summary = StateSummary {
        head: head.display(),
        dirty,
    };
    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::OplogRestore(title),
        current: summary.clone(),
        predicted: summary,
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::OplogRestore(OplogRestoreRecovery::Restore),
            commands: Vec::new(),
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: ref_restore::to_lines(&planned.restores),
        // Rewriting where branches point is destructive (two-stage confirm).
        destructive: true,
        equivalent_command: Some(format!(
            "git update-ref --stdin <<'EOF'\n{}EOF",
            ref_restore::transaction(&planned.restores)
        )),
    })
}

/// Preflight: the plan had no blockers, HEAD is unchanged, and re-planning now
/// yields exactly the restores that were confirmed (so every ref is still
/// where the record left it).
pub fn preflight_oplog_restore(
    repo: &Repository,
    plan: &OperationPlan,
    op: &Operation,
) -> Result<Vec<RefRestore>, GitError> {
    let refused = |why: &str| GitError::Other(format!("{} refused: {why}", op.oplog_name()));
    if !plan.blockers.is_empty() {
        return Err(refused("plan has blockers"));
    }
    preflight_check(repo, plan)?;
    let confirmed = ref_restore::from_lines(&plan.preview_commits).map_err(|e| refused(&e))?;
    if confirmed.is_empty() {
        return Err(refused("the plan carries no ref restores"));
    }
    let fresh = match op {
        Operation::OpRevert { entry_id } => plan_op_revert(repo, *entry_id)?,
        Operation::RestoreToPoint { entry_id } => plan_restore_to_point(repo, *entry_id)?,
        _ => return Err(refused("not an Operation Log restore")),
    };
    if let Some(blocker) = fresh.blockers.first() {
        return Err(refused(&format!("at preflight: {}", blocker.message_en())));
    }
    let now = ref_restore::from_lines(&fresh.preview_commits).map_err(|e| refused(&e))?;
    if now != confirmed {
        return Err(refused(
            "the branches changed since planning; please re-plan",
        ));
    }
    Ok(confirmed)
}

/// Execute: preflight → retain every moved ref's current tip → one
/// `git update-ref --stdin` transaction → verify every ref.
pub(crate) fn execute_oplog_restore(
    repo: &Repository,
    repo_dir: &Path,
    plan: &OperationPlan,
    op: &Operation,
    backup_refs: &mut Vec<String>,
) -> Result<crate::OperationOutcome, GitError> {
    let restores = preflight_oplog_restore(repo, plan, op)?;
    let operation_id = super::backup::operation_id();
    let mut restored = Vec::with_capacity(restores.len());
    for (index, r) in restores.iter().enumerate() {
        let backup = match &r.expect {
            Some(tip) => {
                let oid = git2::Oid::from_str(tip)
                    .map_err(|e| GitError::Other(e.message().to_string()))?;
                let reference = super::backup::retain_object(repo, &operation_id, index, oid)?;
                backup_refs.push(reference.clone());
                Some(reference)
            }
            None => None,
        };
        restored.push(RestoredRef {
            refname: r.refname.clone(),
            from: r.expect.clone(),
            to: r.restore_to.clone(),
            backup,
        });
    }
    let script = ref_restore::transaction(&restores);
    let out = crate::cli::run_git_with_options(
        repo_dir,
        &["update-ref", "--stdin"],
        crate::cli::GitCliOptions {
            stdin: Some(script.as_bytes()),
            ..Default::default()
        },
    )?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "git update-ref refused the transaction (nothing was changed): {}",
            out.stderr.trim()
        )));
    }
    for r in &restores {
        let now = repo.refname_to_id(&r.refname).ok().map(|o| o.to_string());
        if now != r.restore_to {
            return Err(GitError::Other(format!(
                "{} is at {:?} after update-ref, expected {:?} — unexpected state",
                r.refname, now, r.restore_to
            )));
        }
    }
    Ok(crate::OperationOutcome::OplogRestore { restored })
}
