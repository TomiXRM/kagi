use super::*;


// ────────────────────────────────────────────────────────────
// discard (W17-DISCARD, ADR-0046) — backup-then-discard
// ────────────────────────────────────────────────────────────

/// Normalise a user/UI-supplied path to the repository-relative, forward-slash
/// form that git status reports, so plan/execute and status comparisons line up.
fn discard_rel_path(repo: &Repository, raw: &str) -> String {
    let raw_path = Path::new(raw);
    // Strip a workdir prefix if an absolute path was given.
    let rel = repo
        .workdir()
        .and_then(|wd| std::fs::canonicalize(wd).ok())
        .and_then(|wd| {
            std::fs::canonicalize(raw_path)
                .ok()
                .and_then(|abs| abs.strip_prefix(&wd).ok().map(|p| p.to_path_buf()))
        })
        .unwrap_or_else(|| raw_path.to_path_buf());
    normalize_path(&rel).to_string_lossy().replace('\\', "/")
}

/// Analyse a discard of the given working-tree `paths` and return an
/// [`OperationPlan`] with `destructive: true` (ADR-0046).
///
/// **Semantics** (`git checkout -- <path>` equivalent): each target's working-tree
/// content is overwritten by the **index** content. The index (staged changes) and
/// all refs are left untouched.
///
/// # Blocker conditions
///
/// - `paths` is empty (nothing to discard).
/// - A target is a **conflicted** file (must be resolved via the conflict flow,
///   not stomped by discard).
/// - A target is an **untracked** file (discarding = deletion = `git clean`,
///   which is banned project-wide — the UI excludes these, this is the backstop).
/// - A target is not in the unstaged set at all (nothing to discard for it).
pub fn plan_discard(repo: &Repository, paths: &[String]) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = status_summary_display(&status);

    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Build the lookup sets from the current status (all repo-relative paths).
    let unstaged_set: std::collections::HashSet<String> = status
        .unstaged
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let untracked_set: std::collections::HashSet<String> = status
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let conflicted_set: std::collections::HashSet<String> = status
        .conflicted
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    let rels: Vec<String> = paths.iter().map(|p| discard_rel_path(repo, p)).collect();

    if rels.is_empty() {
        blockers.push("Nothing to discard: no files selected.".to_string());
    }

    // Count untracked targets — they are discarded by DELETING the file (after
    // an ODB backup), not by restoring from the index (ADR-0083).
    let mut untracked_targets = 0usize;
    for rel in &rels {
        if conflicted_set.contains(rel) {
            blockers.push(format!(
                "'{}' is conflicted. Resolve the conflict instead of discarding it.",
                rel
            ));
        } else if untracked_set.contains(rel) {
            untracked_targets += 1;
        } else if !unstaged_set.contains(rel) {
            blockers.push(format!("'{}' has no unstaged changes to discard.", rel));
        }
    }

    let target_count = rels.len();
    let predicted = StateSummary {
        head: head.display(),
        dirty: if blockers.is_empty() {
            format!("{} file(s) discarded", target_count)
        } else {
            dirty_display
        },
    };

    let title = if target_count == 1 {
        format!(
            "Discard changes to '{}'",
            rels.first().cloned().unwrap_or_default()
        )
    } else {
        format!("Discard changes to {} file(s)", target_count)
    };

    let recovery = "This discards your unstaged changes to the selected file(s): \
        tracked files are restored from the index, untracked files are deleted from \
        disk. Either way a backup blob of each file's current content is recorded in \
        the oplog (op=\"discard\") first; recover with `git cat-file -p <blob-sha>`."
        .to_string();

    // ADR-0083: untracked targets are DELETED (after an ODB backup). Surface this
    // as a warning so the confirm step is explicit about the irreversible-looking
    // (but recoverable) deletion.
    if untracked_targets > 0 {
        warnings.push(format!(
            "⚠️ {} untracked file(s) will be PERMANENTLY DELETED from disk (and any \
             now-empty folders removed). A backup blob is saved to the oplog first — \
             recover with `git cat-file -p <blob-sha>`.",
            untracked_targets
        ));
    }

    Ok(OperationPlan {
        title,
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
    })
}

/// Execute a discard following the **mandatory** ADR-0046 order:
///
/// 1. **backup** — write each target's CURRENT working-tree content into the ODB
///    via `repo.blob()`, collecting `path → blob SHA`. If **any** backup fails,
///    the whole discard is aborted (no working-tree change is made).
/// 2. **apply** — *tracked* targets are restored from the index with
///    `checkout_index` + `force()` (`git checkout -- <path>` semantics); *untracked*
///    targets are DELETED from disk (ADR-0083 — recoverable via the step-1 backup,
///    so this is not `git clean`). The index and refs are never touched.
/// 3. **verify** — re-read status and confirm each target left the unstaged set
///    (tracked) or is gone from disk (untracked).
///
/// Returns the [`DiscardOutcome`] (the path→blob backup list) so the caller can
/// record it in the oplog as the recovery handle. The caller MUST have rejected
/// conflicted targets at plan time.
pub fn execute_discard(
    repo: &Repository,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<DiscardOutcome, GitError> {
    // ── 0. Refuse to run a plan that has blockers. ───────────
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(format!(
            "discard refused: plan has {} blocker(s)",
            plan.blockers.len()
        )));
    }
    preflight_check(repo, plan)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?
        .to_path_buf();

    let rels: Vec<String> = paths.iter().map(|p| discard_rel_path(repo, p)).collect();
    if rels.is_empty() {
        return Err(GitError::Other("discard: no target paths".to_string()));
    }

    // Classify targets up front: untracked targets are deleted, tracked targets
    // are restored from the index (ADR-0083).
    let status_before = working_tree_status(repo)?;
    let untracked_set: std::collections::HashSet<String> = status_before
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    // ── 1. BACKUP — write each target's current WT content to the ODB. ──
    // Any failure aborts the whole discard BEFORE the working tree is touched.
    let mut backups: Vec<DiscardBackup> = Vec::with_capacity(rels.len());
    for rel in &rels {
        let abs = workdir.join(rel);
        // For an unstaged *deletion* the file is absent from the WT; back up an
        // empty blob so the recovery handle still exists and is uniform.
        let content: Vec<u8> = match std::fs::read(&abs) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(GitError::Other(format!(
                    "discard aborted: cannot read '{}' for backup: {}",
                    rel, e
                )));
            }
        };
        let oid = repo.blob(&content).map_err(|e| {
            GitError::Other(format!(
                "discard aborted: blob backup failed for '{}': {}",
                rel,
                e.message()
            ))
        })?;
        backups.push(DiscardBackup {
            path: rel.clone(),
            blob: oid.to_string(),
        });
    }

    // Partition into tracked (restore from index) vs untracked (delete).
    let (untracked_rels, tracked_rels): (Vec<&String>, Vec<&String>) =
        rels.iter().partition(|r| untracked_set.contains(*r));

    // ── 2a. checkout_index with path filter + force (restore WT from index). ──
    // update_index(false): the index (staged changes) is NEVER modified.
    if !tracked_rels.is_empty() {
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force();
        cb.update_index(false);
        cb.disable_pathspec_match(true);
        for rel in &tracked_rels {
            cb.path(rel.as_str());
        }
        repo.checkout_index(None, Some(&mut cb)).map_err(|e| {
            GitError::Other(format!("discard: checkout_index failed: {}", e.message()))
        })?;
    }

    // ── 2b. DELETE untracked targets (ADR-0083; content backed up in step 1). ──
    for rel in &untracked_rels {
        let abs = workdir.join(rel);
        match std::fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(GitError::Other(format!(
                    "discard: failed to delete untracked file '{}': {}",
                    rel, e
                )));
            }
        }
    }

    // ── 2c. Prune now-empty parent directories left by deleted untracked files
    // (the `-d` of `git clean -fd`), so discarding an untracked folder leaves no
    // empty husk. `remove_dir` only removes empty dirs; we walk up and stop at
    // the first non-empty dir, never touching the workdir root.
    for rel in &untracked_rels {
        let mut dir = std::path::Path::new(rel.as_str()).parent();
        while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
            if std::fs::remove_dir(workdir.join(d)).is_err() {
                break; // non-empty or already gone — stop ascending
            }
            dir = d.parent();
        }
    }

    // ── 3. VERIFY — tracked targets left the unstaged set; untracked are gone. ──
    let status = working_tree_status(repo)?;
    let still_unstaged: std::collections::HashSet<String> = status
        .unstaged
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let still_untracked: std::collections::HashSet<String> = status
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let mut leftover: Vec<&String> = tracked_rels
        .iter()
        .copied()
        .filter(|r| still_unstaged.contains(*r))
        .collect();
    leftover.extend(
        untracked_rels
            .iter()
            .copied()
            .filter(|r| still_untracked.contains(*r)),
    );
    if !leftover.is_empty() {
        return Err(GitError::Other(format!(
            "discard verify failed: {} target(s) not discarded: {}",
            leftover.len(),
            leftover
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    Ok(DiscardOutcome { backups })
}
