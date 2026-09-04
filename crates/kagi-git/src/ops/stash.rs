use super::*;

// ADR-0129 Phase 2: this file's plan text is now structured (`StashNote` /
// `StashTitle` / `StashRecovery`), not English prose. `message_en()` in
// kagi-domain renders the exact legacy strings for oplog/klog/EN display
// (golden-tested there); JA lives in `kagi-ui-core::i18n::plan::stash`.
use kagi_domain::plan::StashPopOutcome;
use kagi_domain::plan_note::stash::StashDirtyOp;
use kagi_domain::plan_note::{
    CommonNote, DirtyParts, OpPhrase, StashNote, StashRecovery, StashTitle,
};

// ────────────────────────────────────────────────────────────
// plan_stash_push
// ────────────────────────────────────────────────────────────

/// Analyse whether a stash push is safe and return an [`OperationPlan`].
///
/// Stash push is a **Guarded-class** operation (ADR-0004): it modifies the
/// working tree and index by saving all local modifications to a new stash
/// entry, leaving the working tree clean.
///
/// # Blocker conditions
///
/// - There are no local modifications (staged, unstaged, untracked all empty) —
///   nothing to stash.
/// - The repository is in a conflict state — stash cannot be created during
///   a merge conflict.
///
/// # Warning conditions
///
/// - Untracked files are included in the stash (equivalent to `git stash -u`).
///   This is intentional for convenience but is surfaced as a warning.
///
/// # Predicted state
///
/// - Working tree will be clean after the push.
/// - Stash count will increase by 1.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Count existing stashes ────────────────────────────
    let stash_count = count_stashes(repo)?;

    // ── 3. Build current StateSummary ────────────────────────
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

    // ── 4. Check blockers ────────────────────────────────────
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    // Nothing to stash.
    // When include_untracked=false, untracked files don't count as "something to stash".
    let has_something_to_stash = if include_untracked {
        status.is_dirty()
    } else {
        !status.staged.is_empty() || !status.unstaged.is_empty()
    };
    if !has_something_to_stash {
        blockers.push(PlanNote::Stash(StashNote::NothingToStash));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::Stashing,
        }));
    }

    // Untracked files included in stash (warning, not blocker) — only when include_untracked=true.
    if include_untracked && !status.untracked.is_empty() {
        warnings.push(PlanNote::Stash(StashNote::UntrackedIncluded {
            count: status.untracked.len(),
        }));
    }

    // When include_untracked=false, warn that untracked files will NOT be stashed.
    if !include_untracked && !status.untracked.is_empty() {
        warnings.push(PlanNote::Stash(StashNote::UntrackedExcluded {
            count: status.untracked.len(),
        }));
    }

    // ── 5. Predicted StateSummary ─────────────────────────────
    // After push: working tree is clean, stash count +1.
    let msg_label = message.unwrap_or("(no message)");
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: "clean".to_string(),
    };

    // ── 6. Recovery guidance ──────────────────────────────────
    let recovery = PlanRecovery {
        kind: RecoveryKind::Stash(StashRecovery::Push {
            message: msg_label.to_string(),
        }),
        commands: vec![
            "git stash list".to_string(),
            "git stash apply stash@{0}".to_string(),
        ],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Stash(StashTitle::Push {
            next_count: stash_count + 1,
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_push
// ────────────────────────────────────────────────────────────

/// Execute a stash push: save local modifications to a new stash entry.
///
/// When `include_untracked` is `true`, uses
/// `repo.stash_save2(&sig, message, Some(StashFlags::INCLUDE_UNTRACKED))`
/// (equivalent to `git stash push -u`).  When `false`, uses `StashFlags::DEFAULT`
/// so untracked files remain in the working tree.
///
/// The signature is read from the repository config (`user.name` / `user.email`);
/// if either is absent, falls back to `"kagi <kagi@local>"`.
///
/// **stash_drop is only called internally by `execute_stash_pop` — it is never
/// called from this function.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<(), GitError> {
    // Build the signature from repo config, with fallback.
    let sig = build_signature(repo)?;

    let flags = if include_untracked {
        Some(StashFlags::INCLUDE_UNTRACKED)
    } else {
        Some(StashFlags::DEFAULT)
    };

    repo.stash_save2(&sig, message, flags)
        .map_err(|e| GitError::Other(format!("stash push failed: {}", e.message())))?;

    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_stash_apply
// ────────────────────────────────────────────────────────────

/// Analyse whether applying stash entry at `index` is safe and return an
/// [`OperationPlan`].
///
/// Stash apply is a **Guarded-class** operation (ADR-0004): applying to a
/// dirty working tree risks mixing changes, so we require a clean tree.
///
/// # Blocker conditions
///
/// - `index` is out of range (no stash entry at that position).
/// - The repository is in a conflict state.
/// - The working tree is dirty (staged or unstaged changes exist) — applying
///   to a dirty tree risks unexpected merge conflicts mixing two sets of
///   changes.
///
/// # Predicted state
///
/// - Working tree will contain the stashed changes (dirty again).
/// - The stash entry **remains** in the stash list (apply, not pop).
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_stash_apply(repo: &mut Repository, index: usize) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Collect stash entries ─────────────────────────────
    let stashes = collect_stash_entries(repo)?;
    let stash_count = stashes.len();

    // ── 3. Build current StateSummary ────────────────────────
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
        dirty: dirty_display.clone(),
    };

    // ── 4. Check blockers ────────────────────────────────────
    let mut blockers: Vec<PlanNote> = Vec::new();

    // Index out of range.
    if index >= stash_count {
        blockers.push(PlanNote::Stash(StashNote::IndexOutOfRange {
            index,
            count: stash_count,
        }));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::ApplyingAStash,
        }));
    }

    // Dirty working tree (staged or unstaged) — MVP policy: clean only.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        blockers.push(PlanNote::Stash(StashNote::DirtyBlocksApply {
            parts: DirtyParts {
                staged: status.staged.len(),
                modified: status.unstaged.len(),
            },
            op: StashDirtyOp::Apply,
        }));
    }

    // ── 5. Predicted StateSummary ─────────────────────────────
    // After apply: working tree will reflect the stash content.
    // The stash entry **remains** (apply, not pop).
    let stash_message = stashes
        .get(index)
        .map(|(_, msg)| msg.clone())
        .unwrap_or_else(|| format!("stash@{{{}}}", index));

    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: format!("restored from stash@{{{}}}", index),
    };

    // ── 6. Recovery guidance ──────────────────────────────────
    let recovery = PlanRecovery {
        kind: RecoveryKind::Stash(StashRecovery::Apply {
            index,
            message: stash_message,
        }),
        commands: vec!["git stash list".to_string()],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Stash(StashTitle::Apply { index }),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        // #295: pin the tree so a dirty/conflict transition after planning is refused.
        worktree_digest: Some(status.digest()),
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_apply
// ────────────────────────────────────────────────────────────

/// Apply the stash entry at `index` to the working tree.
///
/// Uses `repo.stash_apply(index, None)`.
///
/// **This function does NOT remove the stash entry** — the stash is preserved
/// after apply.  For apply + drop, use [`execute_stash_pop`] instead.
/// The stash entry at `index` is preserved after this call.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure (including apply
/// conflicts — in that case the stash entry remains intact).
pub fn execute_stash_apply(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    repo.stash_apply(index, None)
        .map_err(|e| GitError::Other(format!("stash apply failed: {}", e.message())))?;
    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_stash_pop  (T-HT-007, ADR-0009)
// ────────────────────────────────────────────────────────────

/// Analyse whether a stash pop at `index` is safe and return an [`OperationPlan`].
///
/// Stash pop is a **Destructive-class (緩和付き)** operation (ADR-0009): on success
/// it applies the stash entry AND removes it from the stash list.  This is
/// irreversible — unlike apply, which preserves the stash entry.
///
/// # Design (ADR-0009)
///
/// The pop is blocked when a conflict is **predicted** via an in-memory merge of
/// `stash_commit` with the current HEAD.  The stash commit structure is:
///
/// ```text
/// stash@{N}  (the stash commit itself)
///   parent[0] = stash base commit (HEAD at stash-push time)
///   parent[1] = index snapshot commit
///   parent[2] = untracked files commit  (if INCLUDE_UNTRACKED was used)
/// ```
///
/// Conflict prediction: `repo.merge_commits(&head_commit, &stash_commit, None)`.
/// If the in-memory index has conflicts → blocker with a message recommending
/// `stash apply` instead, so the user can resolve conflicts without losing the
/// stash entry.
///
/// # Blocker conditions
///
/// - `index` out of range.
/// - Repository is in a conflict state.
/// - Working tree is dirty (staged or unstaged changes).
/// - Conflict **predicted** by in-memory merge of stash commit with HEAD
///   ("use apply instead, stash will not be consumed").
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_stash_pop(repo: &mut Repository, index: usize) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Collect stash entries with OIDs for conflict prediction ───────────
    let stashes_with_oid = collect_stash_entries_with_oid(repo)?;
    let stash_count = stashes_with_oid.len();
    let stashes: Vec<(usize, String)> = stashes_with_oid
        .iter()
        .map(|(i, msg, _)| (*i, msg.clone()))
        .collect();
    let stash_oid_for_index: Option<git2::Oid> = stashes_with_oid
        .iter()
        .find(|(i, _, _)| *i == index)
        .map(|(_, _, oid)| *oid);

    // ── 3. Build current StateSummary ────────────────────────
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
        dirty: dirty_display.clone(),
    };

    // ── 4. Check blockers ────────────────────────────────────
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    // Index out of range.
    if index >= stash_count {
        blockers.push(PlanNote::Stash(StashNote::IndexOutOfRange {
            index,
            count: stash_count,
        }));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::ApplyingAStash,
        }));
    }

    // Dirty working tree (staged or unstaged) — same policy as stash apply.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        blockers.push(PlanNote::Stash(StashNote::DirtyBlocksApply {
            parts: DirtyParts {
                staged: status.staged.len(),
                modified: status.unstaged.len(),
            },
            op: StashDirtyOp::Pop,
        }));
    }

    // ── 5. Stash info + conflict prediction (only when index is valid) ────
    let stash_message = stashes
        .get(index)
        .map(|(_, msg)| msg.clone())
        .unwrap_or_else(|| format!("stash@{{{}}}", index));

    // Predict conflicts via in-memory merge of stash commit with HEAD.
    // Only run when we have no blockers so far (index valid, not dirty, no conflict state).
    //
    // A predicted conflict is a WARNING, not a blocker (GUI report: the modal
    // had only a Cancel button, so a conflicting stash could never be popped
    // at all). Blocking was the right call while execute_stash_pop dropped
    // the stash unconditionally; now a conflicted apply returns
    // ConflictedStashKept and the entry survives, so confirming through the
    // conflict is exactly real `git stash pop` behaviour. The prediction
    // FAILING is still a blocker (fail-closed): an apply we cannot reason
    // about means the repo state is not understood — re-plan.
    if blockers.is_empty() {
        if let Some(stash_oid) = stash_oid_for_index {
            match predict_stash_pop_conflict(repo, &head, stash_oid) {
                Some(note @ PlanNote::Stash(StashNote::PopWouldConflict { .. })) => {
                    warnings.push(note);
                }
                Some(note) => blockers.push(note),
                None => {}
            }
        }
    }

    // ── 6. Predicted StateSummary ─────────────────────────────
    // After pop: working tree reflects stash content; stash entry is REMOVED.
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: format!(
            "restored from stash@{{{}}} (stash entry will be removed)",
            index
        ),
    };

    // ── 7. Recovery guidance ──────────────────────────────────
    let recovery = PlanRecovery {
        kind: RecoveryKind::Stash(StashRecovery::Pop {
            index,
            message: stash_message,
        }),
        commands: vec!["git stash list".to_string()],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Stash(StashTitle::Pop { index }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        // #295: pin the tree so a dirty/conflict transition after planning is refused.
        worktree_digest: Some(status.digest()),
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        // issue #280: pop irreversibly deletes the stash entry — Destructive class,
        // so the confirm UI treats it like drop/reset rather than a plain apply.
        destructive: true,
        equivalent_command: None,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_pop  (T-HT-007, ADR-0009)
// ────────────────────────────────────────────────────────────

/// Execute a stash pop: apply the stash entry at `index`, then drop it **only
/// when the apply came out clean**.
///
/// # Design (ADR-0009 — Destructive 緩和付き / issue #280)
///
/// 1. `repo.stash_apply(index, None)` — same as `execute_stash_apply`.
/// 2. Re-read the index. libgit2's `git_stash_apply` deliberately writes
///    conflicts into the index and still returns 0 (`GIT_ECONFLICT` is only
///    returned on the `REINSTATE_INDEX` path), so `Ok(())` means "the apply
///    ran", **not** "the content was restored cleanly".
/// 3. Conflicts present → return [`StashPopOutcome::ConflictedStashKept`]
///    **without** dropping: the stashed content is in the working tree with
///    markers and the stash entry survives, exactly like real `git stash pop`.
/// 4. Clean → drop the entry and return [`StashPopOutcome::Applied`].
///
/// A hard apply error also skips the drop (the `?` returns early).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_stash_pop(repo: &mut Repository, index: usize) -> Result<StashPopOutcome, GitError> {
    // Step 1: Apply the stash.
    repo.stash_apply(index, None)
        .map_err(|e| GitError::Other(format!("stash apply (pop phase) failed: {}", e.message())))?;

    // Step 2: Did the apply write conflicts into the index? (issue #280)
    let conflicts = applied_conflict_files(repo)?;
    if !conflicts.is_empty() {
        return Ok(StashPopOutcome::ConflictedStashKept { files: conflicts });
    }

    // Step 3: Drop ONLY after a clean apply.
    stash_drop_internal(repo, index)?;

    Ok(StashPopOutcome::Applied)
}

/// Conflicting paths the just-run `stash_apply` left in the index, if any.
fn applied_conflict_files(repo: &mut Repository) -> Result<Vec<String>, GitError> {
    let has_conflicts = repo
        .index()
        .map_err(|e| GitError::Other(format!("index read after stash apply: {}", e.message())))?
        .has_conflicts();
    if !has_conflicts {
        return Ok(Vec::new());
    }
    // ponytail: the status walk only runs once has_conflicts() is set, so the
    // clean path stays a single index read.
    Ok(working_tree_status(repo)?
        .conflicted
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Drop stash entry at `index`.
///
/// # ADR-0004 / ADR-0009 — Why this is private
///
/// `stash_drop` is a **Destructive** operation (ADR-0004): it permanently removes
/// a stash entry with no recovery path.  ADR-0009 permits stash_drop **only as
/// the second step of a pop**, and **only when the preceding `stash_apply` has
/// already succeeded**.  Exposing it as a standalone public API would allow callers
/// to drop a stash entry without first verifying that the content was successfully
/// restored to the working tree — exactly the "stash lost, conflict unresolved"
/// footgun that ADR-0009 was designed to prevent (and that issue #280 hit,
/// because a conflicted `stash_apply` still returns `Ok`).
///
/// This function is therefore intentionally `fn` (private to this module), not `pub fn`.
/// The only caller is [`execute_stash_pop`].
fn stash_drop_internal(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    repo.stash_drop(index)
        .map_err(|e| GitError::Other(format!("stash drop (pop phase) failed: {}", e.message())))
}

// ────────────────────────────────────────────────────────────
// plan_stash_drop / execute_stash_drop  (ADR-0087)
// ────────────────────────────────────────────────────────────

/// Analyse a standalone stash **drop** (delete the entry without applying it).
///
/// # ADR-0087 — standalone drop (amends ADR-0009)
///
/// ADR-0009 kept `stash_drop` private to prevent the "drop without apply"
/// footgun. ADR-0087 re-exposes it as an **explicit, user-initiated Destructive
/// op** gated behind a danger-confirmation modal (same class as discard / reset
/// --hard). It does NOT touch the working tree — only the stash entry is removed
/// — so the only blocker is an out-of-range index. The dropped stash commit
/// stays reachable from the stash reflog until gc, so the recovery guidance
/// records its OID for `git stash store`.
/// Build the danger-confirm plan for dropping a **remote** stash over SSH
/// (ADR-0089 Phase 3). The remote read path has no local `Repository`, so this
/// synthesises the [`OperationPlan`] the confirm modal needs — `destructive`,
/// with an irreversible-action warning — without a git2 dry run. The actual drop
/// runs via `kagi::remote::remote_stash_drop` in the UI layer. `head_summary` is
/// taken from the remote snapshot (e.g. `"branch: master"`) for display only.
pub fn plan_stash_drop_remote(stash_label: &str, head_summary: String) -> OperationPlan {
    OperationPlan {
        disposition: PlanDisposition::Ready,
        title: PlanTitle::Stash(StashTitle::DropRemote {
            label: stash_label.to_string(),
        }),
        current: StateSummary {
            head: head_summary.clone(),
            dirty: "remote (read-only view)".to_string(),
        },
        predicted: StateSummary {
            head: head_summary,
            dirty: "stash entry removed".to_string(),
        },
        warnings: vec![PlanNote::Stash(StashNote::RemoteDropIrreversible)],
        blockers: Vec::new(),
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Stash(StashRecovery::DropRemote),
            commands: Vec::new(),
        }),
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
        equivalent_command: None,
    }
}

pub fn plan_stash_drop(repo: &mut Repository, index: usize) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let stashes = collect_stash_entries_with_oid(repo)?;
    let stash_count = stashes.len();

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

    let mut blockers: Vec<PlanNote> = Vec::new();
    if index >= stash_count {
        blockers.push(PlanNote::Stash(StashNote::IndexOutOfRange {
            index,
            count: stash_count,
        }));
    }

    let (stash_message, stash_oid) = stashes
        .iter()
        .find(|(i, _, _)| *i == index)
        .map(|(_, msg, oid)| (msg.clone(), Some(*oid)))
        .unwrap_or_else(|| (format!("stash@{{{}}}", index), None));

    let predicted = StateSummary {
        head: head_display,
        dirty: format!("working tree unchanged (stash@{{{}}} entry deleted)", index),
    };

    let recovery = PlanRecovery {
        kind: RecoveryKind::Stash(StashRecovery::Drop {
            message: stash_message.clone(),
            oid: stash_oid.map(|oid| oid.to_string()),
        }),
        commands: match stash_oid {
            Some(oid) => vec![
                format!("git stash store -m \"{}\" {}", stash_message, oid),
                "git stash list".to_string(),
            ],
            None => Vec::new(),
        },
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Stash(StashTitle::Drop { index }),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
        equivalent_command: None,
    })
}

/// Execute a standalone stash drop: delete the stash entry at `index` (ADR-0087).
///
/// Does **not** touch the working tree. Returns the dropped stash commit OID
/// (as a hex string) so the caller can record it in the oplog as the recovery
/// handle (`git stash store <oid>`).
pub fn execute_stash_drop(repo: &mut Repository, index: usize) -> Result<String, GitError> {
    // Capture the OID before dropping so the oplog keeps a recovery handle.
    let oid = collect_stash_entries_with_oid(repo)?
        .into_iter()
        .find(|(i, _, _)| *i == index)
        .map(|(_, _, oid)| oid.to_string());

    repo.stash_drop(index)
        .map_err(|e| GitError::Other(format!("stash drop failed: {}", e.message())))?;

    Ok(oid.unwrap_or_default())
}

// ────────────────────────────────────────────────────────────
// Internal helper: stash pop conflict prediction
// ────────────────────────────────────────────────────────────

/// Predict whether applying the stash commit onto HEAD would produce conflicts.
///
/// Runs the same three-way merge `git_stash_apply` runs (issue #280):
/// **ancestor = the stash commit's `parent[0]` tree** (the HEAD the stash was
/// taken from), ours = the current HEAD tree, theirs = the stash tree. The old
/// implementation used `merge_commits(HEAD, stash)`, whose base is
/// `merge_base(HEAD, stash)` — a *different*, older base whenever the branch
/// the stash was taken on is not an ancestor of the current HEAD, which let
/// real apply-time conflicts slip past the blocker.
///
/// In-memory only: does NOT modify the working tree, index, or repo state.
///
/// **Fail-closed**: any internal error is reported as a blocker
/// ([`StashNote::PopPredictionUnavailable`]), never as "clean" — pop deletes
/// the stash entry, so an unverifiable pop must not proceed.
///
/// Returns `Some(blocker_note)` if a conflict is predicted or the prediction
/// could not be computed, `None` only when the merge is provably clean.
fn predict_stash_pop_conflict(
    repo: &Repository,
    head: &Head,
    stash_oid: git2::Oid,
) -> Option<PlanNote> {
    // Resolve HEAD OID.
    let head_oid = match head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            match git2::Oid::from_str(target) {
                Ok(oid) => oid,
                Err(e) => return Some(prediction_unavailable(e.message())),
            }
        }
        // No HEAD commit: there is nothing to merge against, so the apply is a
        // plain checkout of the stash tree — nothing to predict.
        Head::Unborn { .. } => return None,
    };

    let index_result = match stash_apply_dry_run(repo, head_oid, stash_oid) {
        Ok(index) => index,
        Err(e) => return Some(prediction_unavailable(e.message())),
    };

    if !index_result.has_conflicts() {
        return None;
    }

    // Collect conflicting file paths.
    let mut conflict_files: Vec<String> = Vec::new();
    match index_result.conflicts() {
        Ok(conflicts) => {
            for c in conflicts.flatten() {
                let path_bytes: Option<Vec<u8>> = c
                    .our
                    .as_ref()
                    .map(|e| e.path.clone())
                    .or_else(|| c.their.as_ref().map(|e| e.path.clone()))
                    .or_else(|| c.ancestor.as_ref().map(|e| e.path.clone()));
                if let Some(p) = path_bytes {
                    conflict_files.push(String::from_utf8_lossy(&p).into_owned());
                }
            }
        }
        // Still a conflict — we just could not name the files.
        Err(_) => conflict_files.clear(),
    }
    Some(PlanNote::Stash(StashNote::PopWouldConflict {
        count: conflict_files.len(),
        files: conflict_files,
    }))
}

/// The in-memory three-way merge `git_stash_apply` would perform.
fn stash_apply_dry_run(
    repo: &Repository,
    head_oid: git2::Oid,
    stash_oid: git2::Oid,
) -> Result<git2::Index, git2::Error> {
    let head_tree = repo.find_commit(head_oid)?.tree()?;
    let stash_commit = repo.find_commit(stash_oid)?;
    // parent[0] of a stash commit is the HEAD it was created from — the base
    // libgit2 uses for the apply.
    let base_tree = stash_commit.parent(0)?.tree()?;
    let stash_tree = stash_commit.tree()?;
    repo.merge_trees(&base_tree, &head_tree, &stash_tree, None)
}

fn prediction_unavailable(reason: &str) -> PlanNote {
    PlanNote::Stash(StashNote::PopPredictionUnavailable {
        reason: reason.to_string(),
    })
}

// ────────────────────────────────────────────────────────────
// preflight_check_stash
// ────────────────────────────────────────────────────────────

/// Extended preflight check for stash operations.
///
/// Verifies both:
/// 1. HEAD has not changed since the plan was generated (delegates to
///    [`preflight_check`]).
/// 2. The number of stash entries matches `expected_stash_count` — if another
///    process pushed or dropped a stash between planning and execution, abort.
/// 3. The working tree is still clean (issue #280): apply/pop are planned
///    against a clean tree, and a tree that turned dirty or conflicted between
///    plan and execute is exactly what makes `stash_apply` write conflicts.
///
/// # Errors
///
/// Returns [`GitError::Other`] when HEAD or stash count has changed, or on
/// unexpected failures.
pub fn preflight_check_stash(
    repo: &mut Repository,
    plan: &OperationPlan,
    expected_stash_count: usize,
) -> Result<(), GitError> {
    // 1. Head check (re-use existing).
    preflight_check(repo, plan)?;

    // 2. Working-tree TOCTOU is now covered by the digest inside
    // `preflight_check` (step 1) for apply/pop, which carry `worktree_digest`.
    // The hand-written dirty check that used to live here (#280) is subsumed:
    // the digest catches the same staged/unstaged/conflicted transitions, plus
    // untracked ones the old check skipped. A standalone drop carries no
    // digest, so it stays allowed on a dirty tree exactly as before.

    // 3. Stash count check.
    let current_count = count_stashes(repo)?;
    if current_count != expected_stash_count {
        return Err(GitError::Other(format!(
            "Stash list changed since planning: expected {} entr{}, \
             found {}. Please re-plan before proceeding.",
            expected_stash_count,
            if expected_stash_count == 1 {
                "y"
            } else {
                "ies"
            },
            current_count,
        )));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
// Internal helpers (stash)
// ────────────────────────────────────────────────────────────

/// Count the number of stash entries without allocating message strings.
fn count_stashes(repo: &mut Repository) -> Result<usize, GitError> {
    let mut count = 0usize;
    repo.stash_foreach(|_index, _message, _oid| {
        count += 1;
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(count)
}

/// Collect `(index, message)` pairs for all stash entries.
fn collect_stash_entries(repo: &mut Repository) -> Result<Vec<(usize, String)>, GitError> {
    let mut entries: Vec<(usize, String)> = Vec::new();
    repo.stash_foreach(|index, message, _oid| {
        entries.push((index, message.to_owned()));
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(entries)
}

/// Collect `(index, message, oid)` triples for all stash entries.
fn collect_stash_entries_with_oid(
    repo: &mut Repository,
) -> Result<Vec<(usize, String, git2::Oid)>, GitError> {
    let mut entries: Vec<(usize, String, git2::Oid)> = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        entries.push((index, message.to_owned(), *oid));
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(entries)
}

#[cfg(test)]
mod remote_drop_tests {
    use super::*;

    #[test]
    fn plan_stash_drop_remote_is_destructive_with_no_blockers() {
        let plan = plan_stash_drop_remote("stash@{0}: WIP on main: x", "branch: main".to_string());
        assert!(plan.destructive, "remote stash drop must be Destructive");
        assert!(
            plan.blockers.is_empty(),
            "no local blockers for a remote drop"
        );
        assert!(!plan.warnings.is_empty(), "must warn it is irreversible");
        assert!(
            plan.title.message_en().contains("stash@{0}"),
            "title names the stash"
        );
        assert_eq!(plan.current.head, "branch: main");
    }
}
