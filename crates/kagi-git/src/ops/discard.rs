use super::*;

// ────────────────────────────────────────────────────────────
// discard (W17-DISCARD, ADR-0046) — backup-then-discard
// ────────────────────────────────────────────────────────────

/// Normalise a user/UI-supplied path to the repository-relative, forward-slash
/// form that git status reports, so plan/execute and status comparisons line up.
///
/// **Never consults the process CWD** (issue #282). A *relative* input is already
/// repo-relative (all three UI call sites pass repo-relative strings) and is only
/// normalised lexically — feeding it to `fs::canonicalize` resolved it against the
/// process CWD, which discarded a different, same-named file when kagi was started
/// from inside the workdir. An *absolute* input has the workdir prefix stripped;
/// `canonicalize` is safe there because an absolute path never depends on the CWD.
fn discard_rel_path(workdir: &Path, raw: &str) -> String {
    let raw_path = Path::new(raw);
    let rel = if raw_path.is_absolute() {
        // Try canonical and lexical forms of both sides so a symlinked workdir
        // (e.g. /tmp → /private/tmp on macOS) still matches, and so an absolute
        // path to a *deleted* file (canonicalize fails) still strips correctly.
        let abs_forms = [
            std::fs::canonicalize(raw_path).ok(),
            Some(normalize_path(raw_path)),
        ];
        let wd_forms = [
            std::fs::canonicalize(workdir).ok(),
            Some(workdir.to_path_buf()),
        ];
        abs_forms
            .iter()
            .flatten()
            .find_map(|abs| {
                wd_forms
                    .iter()
                    .flatten()
                    .find_map(|wd| abs.strip_prefix(wd).ok().map(|p| p.to_path_buf()))
            })
            .unwrap_or_else(|| normalize_path(raw_path))
    } else {
        normalize_path(raw_path)
    };
    rel.to_string_lossy().replace('\\', "/")
}

/// The workdir of `repo`, or an empty path for a bare repo (plan-time only —
/// `execute_discard` refuses bare repositories outright).
fn workdir_or_empty(repo: &Repository) -> PathBuf {
    repo.workdir().map(|p| p.to_path_buf()).unwrap_or_default()
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

    // ADR-0129: discard is the first structured producer — notes are typed
    // (`DiscardNote`), not English prose. `message_en()` renders the exact
    // legacy strings for oplog/klog/EN display (golden-tested in kagi-domain).
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

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

    let plan_workdir = workdir_or_empty(repo);
    let rels: Vec<String> = paths
        .iter()
        .map(|p| discard_rel_path(&plan_workdir, p))
        .collect();

    if rels.is_empty() {
        blockers.push(PlanNote::Discard(DiscardNote::NothingSelected));
    }

    // Count untracked targets — they are discarded by DELETING the file (after
    // an ODB backup), not by restoring from the index (ADR-0083).
    let mut untracked_targets = 0usize;
    for rel in &rels {
        if conflicted_set.contains(rel) {
            blockers.push(PlanNote::Discard(DiscardNote::TargetConflicted {
                path: rel.clone(),
            }));
        } else if untracked_set.contains(rel) {
            untracked_targets += 1;
        } else if !unstaged_set.contains(rel) {
            blockers.push(PlanNote::Discard(DiscardNote::NoUnstagedChanges {
                path: rel.clone(),
            }));
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

    let title = PlanTitle::Discard {
        single: (target_count == 1).then(|| rels.first().cloned().unwrap_or_default()),
        count: target_count,
    };

    let recovery = PlanRecovery {
        kind: RecoveryKind::Discard,
        commands: vec!["git cat-file -p <blob-sha>".to_string()],
    };

    // ADR-0083: untracked targets are DELETED (after an ODB backup). Surface this
    // as a warning so the confirm step is explicit about the irreversible-looking
    // (but recoverable) deletion.
    if untracked_targets > 0 {
        warnings.push(PlanNote::Discard(DiscardNote::UntrackedWillBeDeleted {
            count: untracked_targets,
        }));
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title,
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
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
///
/// **Failures after step 1** (issue #281) return `Ok` with
/// [`DiscardOutcome::error`] set instead of `Err`: the working tree has already
/// been mutated at that point, so the backup blob SHAs are the user's only route
/// back to their content and must never be dropped. Only failures *before* any
/// mutation (blockers, preflight, backup) return `Err`.
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

    let rels: Vec<String> = paths
        .iter()
        .map(|p| discard_rel_path(&workdir, p))
        .collect();
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
        // #281: the working tree may already be partly rewritten here, so a
        // failure returns the PARTIAL outcome (backups included) rather than an
        // `Err` that would drop the only handle on the overwritten content.
        if let Err(e) = repo.checkout_index(None, Some(&mut cb)) {
            return Ok(DiscardOutcome {
                backups,
                unverified: tracked_rels.iter().map(|r| (*r).clone()).collect(),
                error: Some(format!("discard: checkout_index failed: {}", e.message())),
            });
        }
    }

    // ── 2b. DELETE untracked targets (ADR-0083; content backed up in step 1). ──
    // #281: a failure at target N leaves 1..N-1 already deleted, so it must
    // report a PARTIAL outcome carrying the backups, not a bare `Err`.
    for (i, rel) in untracked_rels.iter().enumerate() {
        let abs = workdir.join(rel);
        match std::fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Ok(DiscardOutcome {
                    backups,
                    unverified: untracked_rels[i..].iter().map(|r| (*r).clone()).collect(),
                    error: Some(format!(
                        "discard: failed to delete untracked file '{}': {}",
                        rel, e
                    )),
                });
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
        // #281: verify runs AFTER the working tree was rewritten — return the
        // partial outcome so the backup blob SHAs reach the oplog.
        let unverified: Vec<String> = leftover.iter().map(|s| (*s).clone()).collect();
        return Ok(DiscardOutcome {
            backups,
            error: Some(format!(
                "discard verify failed: {} target(s) not discarded: {}",
                unverified.len(),
                unverified.join(", ")
            )),
            unverified,
        });
    }

    Ok(DiscardOutcome::complete(backups))
}

#[cfg(test)]
mod tests {
    use super::discard_rel_path;
    use std::path::Path;

    // Issue #282: a repo-relative input must resolve against the WORKDIR, never
    // the process CWD. The function takes the base dir explicitly, so the ambient
    // CWD is not even in the signature — the bug is unrepresentable, and the test
    // needs no (unsafe, in a parallel test process) chdir.
    #[test]
    fn rel_input_is_repo_relative_regardless_of_cwd() {
        let wd = Path::new("/repo");
        assert_eq!(discard_rel_path(wd, "a.txt"), "a.txt");
        assert_eq!(discard_rel_path(wd, "./a.txt"), "a.txt");
        assert_eq!(discard_rel_path(wd, "src/a.txt"), "src/a.txt");
    }

    // The bug's exact shape: the process CWD is INSIDE the workdir and a file of
    // the target's name exists there. The old code canonicalised the relative
    // input against the CWD and returned "<subdir>/Cargo.toml" — a different
    // file from the one the user selected. Reads the CWD but never changes it,
    // so it is safe in a parallel test process.
    #[test]
    fn rel_input_ignores_a_shadow_file_in_the_cwd() {
        let cwd = std::env::current_dir().unwrap();
        assert!(
            cwd.join("Cargo.toml").exists(),
            "precondition: a shadow Cargo.toml exists in the CWD"
        );
        // Workdir = an ancestor of the CWD, i.e. the reachability condition from
        // issue #282 (`cd <repo>/crates/kagi-git && kagi <repo>`).
        let workdir = cwd.parent().unwrap().parent().unwrap();
        assert_eq!(discard_rel_path(workdir, "Cargo.toml"), "Cargo.toml");
    }

    #[test]
    fn absolute_input_strips_the_workdir_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path();
        std::fs::create_dir_all(wd.join("src")).unwrap();
        std::fs::write(wd.join("a.txt"), b"x").unwrap();
        assert_eq!(
            discard_rel_path(wd, wd.join("a.txt").to_str().unwrap()),
            "a.txt"
        );
        // Absolute path to a file that no longer exists (an unstaged deletion):
        // canonicalize fails, the lexical fallback still strips the prefix.
        assert_eq!(
            discard_rel_path(wd, wd.join("src/gone.txt").to_str().unwrap()),
            "src/gone.txt"
        );
    }

    #[test]
    fn absolute_and_relative_forms_agree() {
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path();
        std::fs::write(wd.join("a.txt"), b"x").unwrap();
        assert_eq!(
            discard_rel_path(wd, "a.txt"),
            discard_rel_path(wd, wd.join("a.txt").to_str().unwrap())
        );
    }
}
