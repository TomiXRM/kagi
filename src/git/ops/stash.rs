use super::*;

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
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Nothing to stash.
    // When include_untracked=false, untracked files don't count as "something to stash".
    let has_something_to_stash = if include_untracked {
        status.is_dirty()
    } else {
        !status.staged.is_empty() || !status.unstaged.is_empty()
    };
    if !has_something_to_stash {
        blockers.push(
            "Nothing to stash: working tree is already clean \
             (no staged, modified, or untracked files)."
                .to_string(),
        );
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve conflicts before stashing.",
            status.conflicted.len()
        ));
    }

    // Untracked files included in stash (warning, not blocker) — only when include_untracked=true.
    if include_untracked && !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will be included in the stash \
             (equivalent to `git stash push -u`).",
            status.untracked.len()
        ));
    }

    // When include_untracked=false, warn that untracked files will NOT be stashed.
    if !include_untracked && !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will NOT be included in the stash \
             (include_untracked=false). They will remain in the working tree.",
            status.untracked.len()
        ));
    }

    // ── 5. Predicted StateSummary ─────────────────────────────
    // After push: working tree is clean, stash count +1.
    let msg_label = message.unwrap_or("(no message)");
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: "clean".to_string(),
    };

    // ── 6. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "To inspect stash entries:  git stash list\n\
         To restore without removing the stash entry:  git stash apply stash@{{0}}\n\
         Stash message that will be used: \"{}\"",
        msg_label
    );

    Ok(OperationPlan {
        title: format!(
            "Stash push — save local modifications ({})",
            stash_count + 1
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
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
    let mut blockers: Vec<String> = Vec::new();

    // Index out of range.
    if index >= stash_count {
        blockers.push(format!(
            "Stash index {} is out of range (only {} stash entr{} exist).",
            index,
            stash_count,
            if stash_count == 1 { "y" } else { "ies" }
        ));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve conflicts before applying a stash.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged or unstaged) — MVP policy: clean only.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree is dirty ({}) — stash apply is only allowed on a clean \
             working tree to prevent accidental merge conflicts.",
            parts.join(", ")
        ));
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
    let recovery = format!(
        "The stash entry stash@{{{}}} is NOT removed by apply — it remains in the list.\n\
         If the apply caused conflicts, resolve them manually; the stash is safely preserved.\n\
         To see remaining stash entries:  git stash list\n\
         Stash message: \"{}\"",
        index, stash_message
    );

    Ok(OperationPlan {
        title: format!("Stash apply — restore stash@{{{}}}", index),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
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
    let mut blockers: Vec<String> = Vec::new();

    // Index out of range.
    if index >= stash_count {
        blockers.push(format!(
            "Stash index {} is out of range (only {} stash entr{} exist).",
            index,
            stash_count,
            if stash_count == 1 { "y" } else { "ies" }
        ));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve conflicts before applying a stash.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged or unstaged) — same policy as stash apply.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree is dirty ({}) — stash pop is only allowed on a clean \
             working tree to prevent accidental merge conflicts.",
            parts.join(", ")
        ));
    }

    // ── 5. Stash info + conflict prediction (only when index is valid) ────
    let stash_message = stashes
        .get(index)
        .map(|(_, msg)| msg.clone())
        .unwrap_or_else(|| format!("stash@{{{}}}", index));

    // Predict conflicts via in-memory merge of stash commit with HEAD.
    // Only run when we have no blockers so far (index valid, not dirty, no conflict state).
    if blockers.is_empty() {
        if let Some(stash_oid) = stash_oid_for_index {
            if let Some(conflict_blocker) = predict_stash_pop_conflict(repo, &head, stash_oid) {
                blockers.push(conflict_blocker);
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
    let recovery = format!(
        "WARNING: pop = apply + drop.  If apply succeeds, stash@{{{}}} is permanently removed.\n\
         The stash entry \"{}\" will be consumed.\n\
         To restore without removing the stash: use 'Stash Apply' instead.\n\
         To see remaining stash entries:  git stash list",
        index, stash_message
    );

    Ok(OperationPlan {
        title: format!("Stash pop — apply and remove stash@{{{}}}", index),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_pop  (T-HT-007, ADR-0009)
// ────────────────────────────────────────────────────────────

/// Execute a stash pop: apply the stash entry at `index`, then drop it **only on success**.
///
/// # Design (ADR-0009 — Destructive 緩和付き)
///
/// 1. `repo.stash_apply(index, None)` — same as `execute_stash_apply`.
/// 2. If and **only if** the apply succeeds, call `stash_drop_internal(repo, index)`
///    to remove the stash entry.
/// 3. If apply fails for **any** reason, the drop is **not called** — the stash
///    entry remains intact.
///
/// This "apply first, drop on success only" approach prevents the catastrophic
/// case of losing the stash entry when apply produces conflicts or other errors.
/// ADR-0009 mandates conflict prediction as a blocker in `plan_stash_pop` so
/// the execute path should rarely see conflict failures in practice.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_stash_pop(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    // Step 1: Apply the stash.
    repo.stash_apply(index, None)
        .map_err(|e| GitError::Other(format!("stash apply (pop phase) failed: {}", e.message())))?;

    // Step 2: Drop ONLY after successful apply.
    stash_drop_internal(repo, index)?;

    Ok(())
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
/// footgun that ADR-0009 was designed to prevent.
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
        title: format!("Drop {stash_label}"),
        current: StateSummary {
            head: head_summary.clone(),
            dirty: "remote (read-only view)".to_string(),
        },
        predicted: StateSummary {
            head: head_summary,
            dirty: "stash entry removed".to_string(),
        },
        warnings: vec![
            "This permanently removes the stash entry on the remote host. \
             It cannot be undone from Kagi."
                .to_string(),
        ],
        blockers: Vec::new(),
        recovery: "A dropped stash commit may remain reachable from the remote's \
                   stash reflog until gc, but Kagi does not manage remote recovery."
            .to_string(),
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
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

    let mut blockers: Vec<String> = Vec::new();
    if index >= stash_count {
        blockers.push(format!(
            "Stash index {} is out of range (only {} stash entr{} exist).",
            index,
            stash_count,
            if stash_count == 1 { "y" } else { "ies" }
        ));
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

    let recovery = match stash_oid {
        Some(oid) => format!(
            "Drop removes the stash entry only — the working tree is NOT touched.\n\
             The dropped stash commit {} stays reachable from the stash reflog until \
             gc; restore it with:\n  git stash store -m \"{}\" {}\n\
             To see remaining stash entries:  git stash list",
            oid, stash_message, oid
        ),
        None => "Drop removes the stash entry only — the working tree is NOT touched.".to_string(),
    };

    Ok(OperationPlan {
        title: format!("Stash drop — delete stash@{{{}}}", index),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
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

/// Predict whether applying a stash commit onto HEAD would produce merge conflicts.
///
/// Uses `repo.merge_commits(&head_commit, &stash_commit, None)` — an in-memory merge
/// that does NOT modify the working tree or repo state.
///
/// The `stash_oid` is the OID of the stash commit itself (parent[0] = base HEAD,
/// parent[1] = index snapshot, parent[2] = untracked files if applicable).
/// Merging the stash commit against the current HEAD predicts whether
/// `git stash apply` would conflict.
///
/// Returns `Some(blocker_message)` if a conflict is predicted, `None` if clean.
fn predict_stash_pop_conflict(
    repo: &Repository,
    head: &Head,
    stash_oid: git2::Oid,
) -> Option<String> {
    // Resolve HEAD OID.
    let head_oid = match head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            git2::Oid::from_str(target).ok()?
        }
        Head::Unborn { .. } => return None,
    };

    let head_commit = repo.find_commit(head_oid).ok()?;
    let stash_commit = repo.find_commit(stash_oid).ok()?;

    // In-memory merge of HEAD with the stash commit: does NOT set MERGING state,
    // does NOT touch the working tree.
    let index_result = repo.merge_commits(&head_commit, &stash_commit, None).ok()?;

    if index_result.has_conflicts() {
        // Collect conflicting file paths.
        let mut conflict_files: Vec<String> = Vec::new();
        if let Ok(conflicts) = index_result.conflicts() {
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
        Some(format!(
            "Stash pop would produce {} conflict(s): {}. \
             Pop is blocked to prevent losing the stash entry. \
             Use 'Stash Apply' instead: it applies the stash without removing it, \
             allowing you to resolve conflicts safely.",
            conflict_files.len(),
            if conflict_files.is_empty() {
                "(unknown files)".to_string()
            } else {
                conflict_files.join(", ")
            }
        ))
    } else {
        None
    }
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

    // 2. Stash count check.
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
        assert!(plan.title.contains("stash@{0}"), "title names the stash");
        assert_eq!(plan.current.head, "branch: main");
    }
}
