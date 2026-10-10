use super::*;

// ────────────────────────────────────────────────────────────
// discard (W17-DISCARD, ADR-0046) — backup-then-discard
// ────────────────────────────────────────────────────────────

/// Resolve legacy absolute/`./` inputs against the workdir, never the CWD.
/// Repository-relative filename bytes are otherwise preserved, including POSIX
/// backslashes. Do not canonicalize the target: a symlink is itself the target.
fn discard_rel_path(workdir: &Path, raw: &str) -> String {
    let raw_path = Path::new(raw);
    if !raw_path.is_absolute() && !raw.starts_with("./") {
        return raw.to_owned();
    }
    let rel = if raw_path.is_absolute() {
        // Only resolve workdir aliases (e.g. /tmp → /private/tmp on macOS).
        // The target may be a symlink or a deleted file and is never resolved.
        let abs_forms = [
            Some(normalize_path(raw_path)),
            raw_path.parent().and_then(|parent| {
                std::fs::canonicalize(parent)
                    .ok()
                    .map(|p| raw_path.file_name().map(|name| p.join(name)).unwrap_or(p))
            }),
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
    // Only these legacy normalized forms need conversion from platform path
    // components to Git separators. Raw relative inputs returned above remain
    // byte-identical, including literal POSIX backslashes.
    let mut git_path = String::with_capacity(raw.len());
    for component in rel.components() {
        let std::path::Component::Normal(part) = component else {
            return rel.to_str().unwrap_or(raw).to_owned();
        };
        let Some(part) = part.to_str() else {
            return raw.to_owned();
        };
        if !git_path.is_empty() {
            git_path.push('/');
        }
        git_path.push_str(part);
    }
    git_path
}

/// The workdir of `repo`, or an empty path for a bare repo (plan-time only —
/// `execute_discard` refuses bare repositories outright).
fn workdir_or_empty(repo: &Repository) -> PathBuf {
    repo.workdir().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// git file mode for a gitlink / submodule entry (GIT_FILEMODE_COMMIT).
const GITLINK_MODE: u32 = 0o160000;

/// Whether the repo-relative `rel` is a submodule / gitlink (#324). The discard
/// backup step does `fs::read(workdir/rel)`, which is a directory for a
/// submodule (EISDIR) — so these must be blocked at plan time, never reaching
/// execute. Detected from the index entry mode (a submodule is mode 160000).
pub fn is_submodule_path(repo: &Repository, rel: &str) -> bool {
    let Ok(index) = repo.index() else {
        return false;
    };
    index
        .get_path(Path::new(rel), 0)
        .map(|e| e.mode == GITLINK_MODE)
        .unwrap_or(false)
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
    //
    // #454 review: the unstaged lookup carries the `ChangeKind` too, because
    // the discard card renders a per-row A/M/D badge from `preview_files`.
    // A `HashMap` rather than a per-row `find`: the card exists for the
    // hundreds-of-files case, and the old scan allocated a `String` per
    // comparison (O(targets x unstaged)).
    let unstaged_kinds: std::collections::HashMap<&Path, kagi_domain::status::ChangeKind> = status
        .unstaged
        .iter()
        .map(|f| (f.path.as_path(), f.change.clone()))
        .collect();
    let untracked_set: std::collections::HashSet<&Path> =
        status.untracked.iter().map(PathBuf::as_path).collect();
    let conflicted_set: std::collections::HashSet<&Path> =
        status.conflicted.iter().map(PathBuf::as_path).collect();

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
    for (raw, rel) in paths.iter().zip(&rels) {
        if !discard_path_is_safe(rel)
            || Path::new(raw)
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            blockers.push(PlanNote::Discard(DiscardNote::UnsafePath {
                path: raw.clone(),
            }));
        } else if conflicted_set.contains(Path::new(rel)) {
            blockers.push(PlanNote::Discard(DiscardNote::TargetConflicted {
                path: rel.clone(),
            }));
        } else if is_submodule_path(repo, rel) {
            // #324: a dirty submodule shows as ` M sub` in the unstaged set but
            // fs::read(workdir/sub) in execute would hit EISDIR and abort the
            // whole batch. Reject it here so sibling targets still discard.
            blockers.push(PlanNote::Discard(DiscardNote::TargetSubmodule {
                path: rel.clone(),
            }));
        } else if untracked_set.contains(Path::new(rel)) {
            untracked_targets += 1;
        } else if !unstaged_kinds.contains_key(Path::new(rel)) {
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
        commands: vec!["git cat-file blob <backup-ref>:file".to_string()],
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
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title,
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        // #295: pin the classification so execute refuses if a target became
        // conflicted or moved tracked→untracked between plan and execute.
        worktree_digest: Some(status.digest()),
        // The targets this plan is FOR, so execute can refuse a path the plan
        // never covered (a plan for A must not be replayed to discard B).
        //
        // The change kind here is NOT a display value: this vector is the
        // authorization set (execute refuses a path the plan never covered),
        // and #454's review showed why encoding badge kinds here was wrong —
        // a target absent from `unstaged` (conflicted, staged-only, clean)
        // would be labelled with a kind the status never reported. The card
        // takes its A/M/D badges from the working-tree status it already holds
        // (`DiscardModal::kinds`) and shows none where the kind is unknown.
        preview_files: rels
            .iter()
            .map(|r| kagi_domain::status::FileStatus {
                path: std::path::PathBuf::from(r),
                change: unstaged_kinds
                    .get(Path::new(r))
                    .cloned()
                    .unwrap_or(kagi_domain::status::ChangeKind::Modified),
            })
            .collect(),
        preview_commits: Vec::new(),
        destructive: true,
        equivalent_command: None,
    })
}

/// Refuse unrepresentable or escaping targets before backup creates any refs.
/// Windows cannot express a literal Git backslash filename without treating it
/// as a separator, so do not reinterpret it as a neighboring path.
fn discard_path_is_safe(rel: &str) -> bool {
    if rel.is_empty() || rel.contains('\0') {
        return false;
    }
    if cfg!(windows) && rel.contains('\\') {
        return false;
    }
    Path::new(rel).components().all(|c| {
        matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    })
}

fn preflight_discard(
    repo: &Repository,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<Vec<String>, GitError> {
    preflight_check(repo, plan)?;
    // Validate raw inputs too: lexical normalization must not hide `..`.
    let workdir = workdir_or_empty(repo);
    let rels: Vec<String> = paths
        .iter()
        .map(|p| discard_rel_path(&workdir, p))
        .collect();
    if rels.is_empty() {
        return Err(GitError::Other("discard: no target paths".to_string()));
    }
    for (raw, rel) in paths.iter().zip(&rels) {
        if !discard_path_is_safe(rel)
            || Path::new(raw)
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err(GitError::Other(
                DiscardNote::UnsafePath { path: raw.clone() }.message_en(),
            ));
        }
    }
    let planned: std::collections::HashSet<&Path> = plan
        .preview_files
        .iter()
        .map(|f| f.path.as_path())
        .collect();
    let requested: std::collections::HashSet<&Path> = rels.iter().map(Path::new).collect();
    if requested != planned {
        return Err(GitError::Other(
            "discard refused: target paths differ from the approved plan. Please re-plan before proceeding.".into(),
        ));
    }
    Ok(rels)
}

fn discard_uses_symlinks(repo: &Repository) -> Result<bool, git2::Error> {
    match repo.config()?.get_bool("core.symlinks") {
        Ok(value) => Ok(value),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(true),
        Err(e) => Err(e),
    }
}

/// Empty-baseline checkout sees selected entries as additions, not typechanges.
/// libgit2 only unlinks their old destination when core.ignorecase is true;
/// otherwise its regular-file writer can follow and truncate a symlink.
fn remove_discard_typechanges(repo: &Repository, workdir: &Path) -> Result<(), git2::Error> {
    let index = repo.index()?;
    let symlinks = discard_uses_symlinks(repo)?;
    for entry in index.iter() {
        let rel =
            std::str::from_utf8(&entry.path).map_err(|e| git2::Error::from_str(&e.to_string()))?;
        let abs = workdir.join(rel);
        let metadata = match std::fs::symlink_metadata(&abs) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(git2::Error::from_str(&format!(
                    "discard: inspect '{rel}': {e}"
                )))
            }
        };
        let want_symlink = entry.mode == 0o120000 && symlinks;
        if metadata.file_type().is_symlink() != want_symlink {
            // No recursive deletion or dereferencing: directories fail closed.
            std::fs::remove_file(&abs)
                .map_err(|e| git2::Error::from_str(&format!("discard: unlink '{rel}': {e}")))?;
        }
    }
    Ok(())
}

/// Verify actual entry types and bytes without consulting the real index cache.
/// Regular files still use libgit2's clean/EOL rules, but a fresh stat-less
/// index forces content hashing instead of trusting matching size/timestamps.
fn verify_discard_tracked(repo: &Repository, workdir: &Path) -> Result<Vec<String>, git2::Error> {
    let selected = repo.index()?;
    let symlinks = discard_uses_symlinks(repo)?;
    let mut regular = git2::Index::new()?;
    let mut leftover = Vec::new();
    for mut entry in selected.iter() {
        let rel =
            std::str::from_utf8(&entry.path).map_err(|e| git2::Error::from_str(&e.to_string()))?;
        let abs = workdir.join(rel);
        let metadata = match std::fs::symlink_metadata(&abs) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                leftover.push(rel.to_string());
                continue;
            }
            Err(e) => {
                return Err(git2::Error::from_str(&format!(
                    "discard verify: inspect '{rel}': {e}"
                )))
            }
        };
        let want_symlink = entry.mode == 0o120000 && symlinks;
        if metadata.file_type().is_symlink() != want_symlink
            || (!want_symlink && !metadata.is_file())
        {
            leftover.push(rel.to_string());
        } else if want_symlink {
            let target = std::fs::read_link(&abs).map_err(|e| {
                git2::Error::from_str(&format!("discard verify: read link '{rel}': {e}"))
            })?;
            #[cfg(unix)]
            let bytes = {
                use std::os::unix::ffi::OsStrExt;
                target.as_os_str().as_bytes()
            };
            #[cfg(not(unix))]
            let bytes = target
                .to_str()
                .ok_or_else(|| {
                    git2::Error::from_str(&format!("discard verify: non-UTF-8 link target '{rel}'"))
                })?
                .as_bytes();
            if git2::Oid::hash_object(git2::ObjectType::Blob, bytes)? != entry.id {
                leftover.push(rel.to_string());
            }
        } else {
            entry.ctime = git2::IndexTime::new(0, 0);
            entry.mtime = git2::IndexTime::new(0, 0);
            entry.dev = 0;
            entry.ino = 0;
            entry.file_size = 0;
            regular.add(&entry)?;
        }
    }
    let mut options = git2::DiffOptions::new();
    options.update_index(false);
    let diff = repo.diff_index_to_workdir(Some(&regular), Some(&mut options))?;
    for delta in diff.deltas() {
        if let Some(path) = delta.old_file().path() {
            leftover.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(leftover)
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
/// 3. **verify** — inspect each target's type and content against the index
///    (tracked) or confirm its directory entry is absent (untracked).
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
pub(crate) fn execute_discard(
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
    let rels = preflight_discard(repo, plan, paths)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?
        .to_path_buf();

    // Classify targets up front: untracked targets are deleted, tracked targets
    // are restored from the index (ADR-0083).
    let status_before = working_tree_status(repo)?;
    let untracked_set: std::collections::HashSet<&Path> = status_before
        .untracked
        .iter()
        .map(PathBuf::as_path)
        .collect();

    let (untracked_rels, tracked_rels): (Vec<&String>, Vec<&String>) = rels
        .iter()
        .partition(|r| untracked_set.contains(Path::new(r.as_str())));

    // #1125: libgit2 computes a glob-unescaped common prefix even with
    // DISABLE_PATHSPEC_MATCH, so a single backslash path can match nothing.
    // A separate handle uses only approved, byte-exact entries as its index.
    // Both target and baseline are then restricted: a detached in-memory index
    // has no on-disk baseline, so checkout cannot remove unselected HEAD files.
    // The actual repository's index and the caller's cached handle stay intact.
    let checkout_repo = if tracked_rels.is_empty() {
        None
    } else {
        let index = repo
            .index()
            .map_err(|e| GitError::Other(e.message().into()))?;
        let mut selected_index =
            git2::Index::new().map_err(|e| GitError::Other(e.message().into()))?;
        for rel in &tracked_rels {
            let entry = index.get_path(Path::new(rel.as_str()), 0).ok_or_else(|| {
                GitError::Other(format!(
                    "discard refused: '{}' is absent from the index",
                    rel
                ))
            })?;
            selected_index
                .add(&entry)
                .map_err(|e| GitError::Other(e.message().into()))?;
        }
        let checkout_repo =
            Repository::open(repo.path()).map_err(|e| GitError::Other(e.message().into()))?;
        checkout_repo
            .set_index(&mut selected_index)
            .map_err(|e| GitError::Other(e.message().into()))?;
        Some(checkout_repo)
    };

    // ── 1. BACKUP — write each target's current WT content to the ODB. ──
    // Any failure aborts the whole discard BEFORE the working tree is touched.
    let mut backups: Vec<DiscardBackup> = Vec::with_capacity(rels.len());
    let backup_id = super::backup::operation_id();
    let backup_index = repo
        .index()
        .map_err(|e| GitError::Other(e.message().into()))?;
    #[cfg(unix)]
    let trust_filemode = match repo
        .config()
        .map_err(|e| GitError::Other(e.message().into()))?
        .get_bool("core.filemode")
    {
        Ok(value) => value,
        Err(e) if e.code() == git2::ErrorCode::NotFound => true,
        Err(e) => return Err(GitError::Other(e.message().into())),
    };
    for rel in &rels {
        let abs = workdir.join(rel);
        // Inspect the entry itself, including dangling links; never ingest an
        // external target or turn arbitrary metadata failures into regular reads.
        let metadata = match std::fs::symlink_metadata(&abs) {
            Ok(metadata) => Some(metadata),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(GitError::Other(format!(
                    "discard aborted: cannot inspect '{}' for backup: {}",
                    rel, e
                )));
            }
        };
        let is_symlink = metadata
            .as_ref()
            .is_some_and(|m| m.file_type().is_symlink());
        let mode = if is_symlink {
            0o120000
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if trust_filemode {
                    if metadata
                        .as_ref()
                        .is_some_and(|m| m.permissions().mode() & 0o100 != 0)
                    {
                        0o100755
                    } else {
                        0o100644
                    }
                } else {
                    backup_index
                        .get_path(Path::new(rel), 0)
                        .map(|entry| {
                            if entry.mode == 0o100755 {
                                0o100755
                            } else {
                                0o100644
                            }
                        })
                        .unwrap_or(0o100644)
                }
            }
            #[cfg(not(unix))]
            {
                // Platforms without a Unix executable bit retain the index mode.
                backup_index
                    .get_path(Path::new(rel), 0)
                    .map(|entry| {
                        if entry.mode == 0o100755 {
                            0o100755
                        } else {
                            0o100644
                        }
                    })
                    .unwrap_or(0o100644)
            }
        };
        let content: Vec<u8> = if is_symlink {
            let target = std::fs::read_link(&abs).map_err(|e| {
                GitError::Other(format!(
                    "discard aborted: cannot read symlink '{}' for backup: {}",
                    rel, e
                ))
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                target.into_os_string().into_vec()
            }
            #[cfg(not(unix))]
            {
                // Do not silently substitute U+FFFD for an unrepresentable target.
                target.to_str().ok_or_else(|| {
                    GitError::Other(format!(
                        "discard aborted: non-UTF-8 symlink target '{}' cannot be backed up on this platform",
                        rel
                    ))
                })?.as_bytes().to_vec()
            }
        } else {
            // For an unstaged *deletion* the file is absent from the WT; back up an
            // empty blob so the recovery handle still exists and is uniform.
            match std::fs::read(&abs) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => {
                    return Err(GitError::Other(format!(
                        "discard aborted: cannot read '{}' for backup: {}",
                        rel, e
                    )));
                }
            }
        };
        backups.push(super::backup::write_file(
            repo,
            &backup_id,
            backups.len(),
            rel.clone(),
            &content,
            mode,
        )?);
    }

    // ── 2a. checkout only the approved index entries (restore WT). ──
    // update_index(false): the repository's staged content is NEVER modified.
    if let Some(checkout_repo) = &checkout_repo {
        // All backups are pinned before the first unlink. Treat failures here
        // like checkout failures: earlier selected entries may already be gone.
        if let Err(e) = remove_discard_typechanges(checkout_repo, &workdir) {
            return Ok(DiscardOutcome {
                backups,
                unverified: rels.clone(),
                error: Some(e.to_string()),
            });
        }
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force();
        cb.update_index(false);
        // #281: the working tree may already be partly rewritten here, so a
        // failure returns the PARTIAL outcome (backups included) rather than an
        // `Err` that would drop the only handle on the overwritten content.
        if let Err(e) = checkout_repo.checkout_index(None, Some(&mut cb)) {
            // The untracked targets are unverified too: step 2b never runs on
            // this path, so none of them were deleted (#280 review, item 7).
            let unverified = tracked_rels
                .iter()
                .map(|r| (*r).clone())
                .chain(untracked_rels.iter().map(|r| (*r).clone()))
                .collect();
            return Ok(DiscardOutcome {
                backups,
                unverified,
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

    // ── 3. VERIFY — check types/content, never stale real-index status. ──
    let mut unverified = if let Some(checkout_repo) = &checkout_repo {
        match verify_discard_tracked(checkout_repo, &workdir) {
            Ok(leftover) => leftover,
            Err(e) => {
                return Ok(DiscardOutcome {
                    backups,
                    unverified: rels.clone(),
                    error: Some(e.to_string()),
                });
            }
        }
    } else {
        Vec::new()
    };
    for rel in &untracked_rels {
        match std::fs::symlink_metadata(workdir.join(rel)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => unverified.push((*rel).clone()),
        }
    }
    if !unverified.is_empty() {
        // #281: verify runs AFTER the working tree was rewritten — return the
        // partial outcome so the backup blob SHAs reach the oplog.
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
        assert_eq!(discard_rel_path(wd, "./a/b.txt"), "a/b.txt");
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
        assert_eq!(
            discard_rel_path(wd, wd.join("a/b.txt").to_str().unwrap()),
            "a/b.txt"
        );
        // Absolute path to a file that no longer exists (an unstaged deletion):
        // canonicalize fails, the lexical fallback still strips the prefix.
        assert_eq!(
            discard_rel_path(wd, wd.join("src/gone.txt").to_str().unwrap()),
            "src/gone.txt"
        );
    }

    #[test]
    fn discard_normalized_inputs_use_git_separators() {
        let tmp = tempfile::tempdir().unwrap();
        let wd = tmp.path();
        assert_eq!(discard_rel_path(wd, "./a/b.txt"), "a/b.txt");
        assert_eq!(
            discard_rel_path(wd, wd.join("a/b.txt").to_str().unwrap()),
            "a/b.txt"
        );
        #[cfg(unix)]
        assert_eq!(discard_rel_path(wd, r"a\b.txt"), r"a\b.txt");
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
