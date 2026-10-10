//! Commit approval and execution bind to the index content, not file status.
use super::*;
use crate::checklist::checklist;
use kagi_domain::plan_note::commit::{
    CommitLeftoverParts, CommitNote, CommitRecovery, CommitTitle,
};

/// Read the disk index rather than Repository's cached staging snapshot.
pub(crate) fn read_commit_index(repo: &Repository) -> Result<git2::Index, GitError> {
    let mut index = repo
        .index()
        .map_err(|error| GitError::Other(error.message().into()))?;
    index
        .read(true)
        .map_err(|error| GitError::Other(error.message().into()))?;
    Ok(index)
}

/// Stat-cache metadata is not staged content. Raw paths, OIDs, modes and
/// conflict stages are; length-prefixing paths makes the encoding unambiguous.
fn index_digest(index: &git2::Index) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for entry in index.iter() {
        digest.update((entry.path.len() as u64).to_be_bytes());
        digest.update(&entry.path);
        digest.update(entry.id.as_bytes());
        digest.update(entry.mode.to_be_bytes());
        digest.update((entry.flags & 0x3000).to_be_bytes());
    }
    hex::encode(digest.finalize())
}

pub(crate) fn staged_set_digest(repo: &Repository) -> Result<String, GitError> {
    Ok(index_digest(&read_commit_index(repo)?))
}

/// Missing identity is not permission to commit the current index.
fn check_commit_index(index: &git2::Index, plan: &OperationPlan) -> Result<(), GitError> {
    if plan.approved_index_digest.as_deref() != Some(index_digest(index).as_str()) {
        return Err(GitError::Blocked(Box::new(PlanNote::Commit(
            CommitNote::StagedContentChanged,
        ))));
    }
    Ok(())
}

pub fn preflight_commit(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    check_commit_index(&read_commit_index(repo)?, plan)
}

/// The executor writes the tree from this same checked in-memory snapshot;
/// another process changing the disk index cannot replace its approved blobs.
pub(crate) fn approved_commit_index(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<git2::Index, GitError> {
    let index = read_commit_index(repo)?;
    check_commit_index(&index, plan).map_err(|error| GitError::Preflight(Box::new(error)))?;
    Ok(index)
}

// ────────────────────────────────────────────────────────────
// plan_commit
// ────────────────────────────────────────────────────────────

/// Analyse whether creating a commit is safe and return an [`OperationPlan`].
///
/// # Blocker conditions
///
/// - `message` is empty after trimming whitespace.
/// - No files are staged in the index.
/// - The repository has conflicted files.
///
/// # Warning conditions
///
/// - Unstaged or untracked changes exist that will **not** be included in
///   this commit.
///
/// # Predicted state
///
/// - HEAD branch advances by one commit.
/// - Staged files become empty (committed).
/// - Unstaged / untracked changes remain.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_commit(repo: &Repository, message: &str) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let approved_index_digest = Some(staged_set_digest(repo)?);
    let status = working_tree_status(repo)?;

    // ── 2. Build current StateSummary ────────────────────────
    let head_display = head.display();

    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    let dirty_display = if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    };

    let current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display,
    };

    // ── 3. Check blockers ────────────────────────────────────
    // ADR-0129: plan_commit is a structured producer — notes are typed
    // (`CommitNote`), not English prose. `message_en()` renders the exact
    // legacy strings for oplog/klog/EN display (golden-tested in kagi-domain).
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    // Empty message.
    if message.trim().is_empty() {
        blockers.push(PlanNote::Commit(CommitNote::EmptyMessage));
    }

    // Nothing staged.
    if status.staged.is_empty() {
        blockers.push(PlanNote::Commit(CommitNote::NothingStaged));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Commit(CommitNote::ConflictedFiles {
            count: status.conflicted.len(),
        }));
    }

    // Unstaged / untracked changes remain (warning, not blocker).
    let leftover_count = status.unstaged.len() + status.untracked.len();
    if leftover_count > 0 {
        warnings.push(PlanNote::Commit(CommitNote::LeftoverNotIncluded {
            count: leftover_count,
            parts: CommitLeftoverParts {
                modified: status.unstaged.len(),
                untracked: status.untracked.len(),
            },
        }));
    }

    // Staged-content checklist (ADR-0043 rules 4/5/6): conflict markers (block),
    // secret/.env (warn), large binary (warn).  Inspects index BLOBs, not WT.
    // Rules 1–3 (staged empty / message empty / repo conflicted) are handled
    // above; the checklist adds the content-level rules, already typed as
    // `PlanNote::Checklist(ChecklistNote)` (ADR-0129 Phase 3).
    let (check_blockers, check_warnings) = checklist(repo, &status)?;
    blockers.extend(check_blockers);
    warnings.extend(check_warnings);

    // ── 4. Predicted StateSummary ─────────────────────────────
    // After commit: staged becomes empty; unstaged/untracked remain.
    let msg_summary: String = message.trim().chars().take(72).collect();

    let remaining_parts: Vec<String> = [
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    let predicted_dirty = if remaining_parts.is_empty() {
        "clean".to_string()
    } else {
        remaining_parts.join(", ")
    };

    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        Head::Unborn { branch } => branch.clone(),
        Head::Detached { target } => target.get(..8).unwrap_or(target).to_string(),
    };

    let predicted = StateSummary {
        head: format!("branch: {} (+1 commit: \"{}\")", branch_name, msg_summary),
        dirty: predicted_dirty,
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let staged_files: Vec<String> = status
        .staged
        .iter()
        .map(|f| f.path.display().to_string())
        .collect();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Commit(CommitRecovery::AfterCommit { staged_files }),
        commands: vec![
            "git commit --amend".to_string(),
            "git revert HEAD".to_string(),
        ],
    };

    // Use staged file list as preview_files.
    let preview_files: Vec<FileStatus> = status.staged.clone();

    let mut plan = OperationPlan {
        approved_index_digest,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Commit(CommitTitle::Commit {
            summary: msg_summary,
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files,
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    };

    // GitHub ruleset pre-verification (#346, ADR-0150): fold in any local
    // findings from the cached branch ruleset. Cache-only — never a network
    // call at plan time; a no-op when nothing is cached / `gh` is unavailable.
    crate::ruleset::augment_commit_plan(&mut plan, repo, &status, &branch_name, message);
    preflight_commit(repo, &plan)?;

    Ok(plan)
}

// ────────────────────────────────────────────────────────────
// execute_commit
// ────────────────────────────────────────────────────────────

/// Commit the rechecked approved tree through Git, preserving project hooks
/// and the user's signing configuration. The real index is not passed to Git.
pub(crate) fn execute_commit(
    repo: &Repository,
    plan: &OperationPlan,
    message: &str,
) -> Result<CommitId, GitError> {
    let mut index = approved_commit_index(repo, plan)?;
    let tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree() failed: {}", e.message())))?;
    let parent = match resolve_head(repo)? {
        Head::Unborn { .. } => None,
        Head::Attached { target, .. } | Head::Detached { target } => {
            Some(git2::Oid::from_str(&target).map_err(|e| GitError::Other(e.to_string()))?)
        }
    };
    execute_git_commit(repo, tree_oid, parent.as_slice(), message, false)
        .map(|oid| CommitId(oid.to_string()))
}

/// All interactive commit variants share one CLI policy and read-back check.
/// A private index prevents external changes of the real index from replacing
/// the approved snapshot, and preserves the real index on hook/signing failure.
pub(crate) fn execute_git_commit(
    repo: &Repository,
    tree_oid: git2::Oid,
    parents: &[git2::Oid],
    message: &str,
    amend: bool,
) -> Result<git2::Oid, GitError> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("commit requires a worktree".into()))?;
    let before = resolve_head(repo)?;
    let signing_required = match repo
        .config()
        .and_then(|config| config.get_bool("commit.gpgsign"))
    {
        Ok(required) => required,
        Err(error) if error.code() == git2::ErrorCode::NotFound => false,
        Err(error) => {
            return Err(GitError::Other(format!(
                "cannot read commit signing policy: {error}"
            )))
        }
    };
    let scratch = tempfile::Builder::new()
        .prefix("kagi-commit-")
        .tempdir_in(repo.path())
        .map_err(|e| GitError::Other(format!("cannot create approved commit index: {e}")))?;
    let index_path = scratch.path().join("index");
    let mut index = git2::Index::open(&index_path).map_err(|e| GitError::Other(e.to_string()))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| GitError::Other(e.to_string()))?;
    index
        .read_tree(&tree)
        .map_err(|e| GitError::Other(e.to_string()))?;
    index.write().map_err(|e| GitError::Other(e.to_string()))?;
    let args = ["commit", "-m", message, "--amend"];
    let args = if amend { &args[..] } else { &args[..3] };
    let output = crate::cli::run_git_commit(workdir, &index_path, args)?;
    if output.status != 0 {
        let detail = format!(
            "git commit failed (exit {}): {}{}",
            output.status, output.stderr, output.stdout
        );
        if resolve_head(repo).as_ref() != Ok(&before) {
            return Err(GitError::TerminationUnknown(crate::Termination::stopped(
                detail,
            )));
        }
        return Err(GitError::Other(detail));
    }
    verify_git_commit(repo, &before, tree_oid, parents, signing_required)
}

fn verify_git_commit(
    repo: &Repository,
    before: &Head,
    tree: git2::Oid,
    parents: &[git2::Oid],
    signing_required: bool,
) -> Result<git2::Oid, GitError> {
    let verification = (|| {
        let head = repo
            .head()
            .and_then(|head| head.peel_to_commit())
            .map_err(|e| format!("cannot read committed HEAD: {e}"))?;
        if resolve_head(repo).as_ref() == Ok(before)
            || head.tree_id() != tree
            || !head.parent_ids().eq(parents.iter().copied())
        {
            return Err("HEAD tree or parents differ from the approved commit; inspect hooks and repository before continuing".into());
        }
        if signing_required {
            repo.extract_signature(&head.id(), None)
                .map_err(|error| format!("cannot verify the required commit signature in HEAD: {error}; inspect signing settings and hooks before continuing"))?;
        }
        Ok(head.id())
    })();
    verification
        .map_err(|reason: String| GitError::TerminationUnknown(crate::Termination::stopped(reason)))
}
