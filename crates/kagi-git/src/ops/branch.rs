use super::*;
use kagi_domain::plan_note::{BranchNote, BranchRecovery, BranchTitle, CommonNote};
use kagi_domain::remote::shell_quote;

// ────────────────────────────────────────────────────────────
// plan_create_branch
// ────────────────────────────────────────────────────────────

/// Compute the keyed branch-name validation errors for the **create-branch**
/// path (W29-I18N-WAVE2), in the same order the legacy code pushed them.
///
/// This is the single source of truth for the create-branch name reasons: the
/// plan builder maps each error through [`BranchNameError::Display`] into the
/// English-only `blockers` Vec (preserving the pinned wording), and the UI maps
/// the same errors to localized messages. The commit-existence blocker is *not*
/// keyed here (it stays English-only in the plan).
pub fn create_branch_name_errors(repo: &Repository, name: &str) -> Vec<BranchNameError> {
    let mut errs: Vec<BranchNameError> = Vec::new();

    if name.is_empty() {
        errs.push(BranchNameError::EmptyCreate);
    }

    // Invalid name (use git2 ref validation on the full ref path).
    if !name.is_empty() && !git2::Reference::is_valid_name(&format!("refs/heads/{}", name)) {
        errs.push(BranchNameError::CreateInvalidRef(name.to_string()));
    }

    // Leading `-` is rejected explicitly: although git2 considers it a valid ref
    // name, it is ambiguous on the command line (may be interpreted as a flag).
    if !name.is_empty() && is_flag_like(name) {
        errs.push(BranchNameError::CreateLeadingDash(name.to_string()));
    }

    // Already-exists check.
    if !name.is_empty() && repo.find_branch(name, BranchType::Local).is_ok() {
        errs.push(BranchNameError::CreateExists(name.to_string()));
    }

    errs
}

/// Analyse whether creating a new local branch at `at` is safe and return an
/// [`OperationPlan`].
///
/// This is a **Safe-class** operation (ADR-0004): it does not modify HEAD or the
/// working tree.  No warnings are produced; only blockers.
///
/// # Blocker conditions
///
/// - `name` is empty.
/// - `name` fails `git2::Reference::is_valid_name("refs/heads/<name>")` — e.g.
///   names containing `..`, a leading `-`, spaces, or other invalid characters.
/// - A local branch with `name` already exists.
/// - The commit `at` does not exist in the repository.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_create_branch(
    repo: &Repository,
    name: &str,
    at: &CommitId,
) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD ──────────────────────────────────────
    let head = resolve_head(repo)?;
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
        dirty: dirty_display.clone(),
    };

    // ── 3. Check blockers ────────────────────────────────────
    // The branch-name reasons are computed as keyed errors (W29-I18N-WAVE2) so
    // the UI can localize them (ADR-0129 appendix §E) as
    // `CommonNote::BranchNameErrorKeyed`, not a `BranchNote`.
    let mut blockers: Vec<PlanNote> = create_branch_name_errors(repo, name)
        .into_iter()
        .map(|e| PlanNote::Common(CommonNote::BranchNameErrorKeyed(e)))
        .collect();

    // Commit existence check.
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())));
    let commit_exists = match oid {
        Ok(oid) => repo.find_commit(oid).is_ok(),
        Err(_) => false,
    };
    if !commit_exists {
        blockers.push(PlanNote::Branch(BranchNote::CommitMissing {
            sha: at.short().to_string(),
        }));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD is unchanged; the new branch appears as an additional ref.
    let short_sha = at.short().to_string();
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display,
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let recovery = PlanRecovery {
        kind: RecoveryKind::Branch(BranchRecovery::CreateBranch {
            name: name.to_string(),
        }),
        commands: vec![format!("git branch -d {}", shell_quote(name))],
    };

    let mut plan = OperationPlan {
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Branch(BranchTitle::CreateBranch {
            name: name.to_string(),
            at: short_sha,
            checkout: false,
        }),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    };

    // GitHub ruleset pre-verification (#346, ADR-0150): if a ruleset for the
    // *new* branch name is cached, fold in branch_name / creation findings.
    // Cache-only (no network at plan time); a no-op when nothing is cached.
    crate::ruleset::augment_branch_create_plan(&mut plan, repo, name);

    Ok(plan)
}

// ────────────────────────────────────────────────────────────
// execute_create_branch
// ────────────────────────────────────────────────────────────

/// Create a new local branch named `name` pointing at commit `at`.
///
/// Uses `repo.branch(name, &commit, false)` — the `force` argument is **always
/// `false`** (a literal constant) to prevent overwriting an existing branch.
///
/// **This function does not perform a checkout.**  HEAD remains unchanged.
///
/// # Errors
///
/// Returns [`GitError::Other`] if:
/// - `at` is not a valid or existing commit OID.
/// - A branch named `name` already exists (`force=false` is enforced by libgit2).
/// - Any other libgit2 failure.
pub(crate) fn execute_create_branch(
    repo: &Repository,
    name: &str,
    at: &CommitId,
) -> Result<(), GitError> {
    // Resolve the target commit.
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())))?;
    let commit = repo.find_commit(oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            at.short(),
            e.message()
        ))
    })?;

    // Create the branch.  force=false is a literal constant — never change this.
    repo.branch(name, &commit, false)
        .map_err(|e| GitError::Other(format!("branch creation failed: {}", e.message())))?;

    Ok(())
}

fn local_branch_names(repo: &Repository) -> Result<Vec<String>, GitError> {
    let mut names = Vec::new();
    let branches = repo
        .branches(Some(BranchType::Local))
        .map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
    for branch_result in branches {
        let (branch, _) = branch_result
            .map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
        if let Ok(Some(name)) = branch.name() {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Config names are section.subsection.variable; only the last dot separates
/// the variable. A prefix match would also select a branch named `foo.bar`.
fn branch_config_variable<'a>(key: &'a str, branch: &str) -> Option<&'a str> {
    let (section, tail) = key.split_once('.')?;
    let (subsection, variable) = tail.rsplit_once('.')?;
    (section == "branch" && subsection == branch).then_some(variable)
}

fn branch_config_keys(repo: &Repository, branch: &str) -> Result<Vec<String>, git2::Error> {
    let mut config = repo.config()?.open_level(git2::ConfigLevel::Local)?;
    let snapshot = config.snapshot()?;
    let mut entries = snapshot.entries(None)?;
    let mut result = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry?;
        let key = entry.name()?;
        if branch_config_variable(key, branch).is_some() && !result.iter().any(|k| k == key) {
            result.push(key.to_owned());
        }
    }
    Ok(result)
}

struct RenameConfig {
    /// Origin, full key, decoded value, in Git's original order.
    entries: Vec<(String, String, String)>,
    files: Vec<String>,
    keys: Vec<String>,
    digest: String,
    blockers: Vec<PlanNote>,
}

fn local_config_records(repo: &Repository, scope: &str) -> Result<String, GitError> {
    let output = run_git(
        repo.workdir().unwrap_or(repo.path()),
        &[
            "config",
            scope,
            "--includes",
            "--null",
            "--show-origin",
            "--list",
        ],
    )?;
    if output.status != 0 {
        return Err(GitError::Other(format!(
            "local branch config read failed: {}",
            output.stderr
        )));
    }
    Ok(output.stdout)
}

fn rename_config_header_is_safe(
    root: &Path,
    file: &str,
    old: &str,
    new: &str,
) -> Result<bool, GitError> {
    let scratch = tempfile::NamedTempFile::new()
        .map_err(|e| GitError::Other(format!("cannot reserve config dry-run file: {e}")))?;
    std::fs::copy(file, scratch.path())
        .map_err(|e| GitError::Other(format!("cannot copy config for dry-run: {e}")))?;
    let scratch_path = scratch
        .path()
        .to_str()
        .ok_or_else(|| GitError::Other("config dry-run path is not UTF-8".into()))?;
    let before = run_git(
        root,
        &[
            "config",
            "--file",
            scratch_path,
            "--no-includes",
            "--null",
            "--list",
        ],
    )?;
    if before.status != 0 {
        return Ok(false);
    }
    let expected: Vec<_> = before
        .stdout
        .split_terminator('\0')
        .map(|entry| {
            let key = entry.split_once('\n').map_or(entry, |(key, _)| key);
            match branch_config_variable(key, old) {
                Some(variable) => format!("branch.{new}.{variable}{}", &entry[key.len()..]),
                None => entry.to_owned(),
            }
        })
        .collect();
    let renamed = run_git(
        root,
        &[
            "config",
            "--file",
            scratch_path,
            "--rename-section",
            &format!("branch.{old}"),
            &format!("branch.{new}"),
        ],
    )?;
    if renamed.status != 0 {
        return Ok(false);
    }
    let after = run_git(
        root,
        &[
            "config",
            "--file",
            scratch_path,
            "--no-includes",
            "--null",
            "--list",
        ],
    )?;
    Ok(after.status == 0
        && after
            .stdout
            .split_terminator('\0')
            .eq(expected.iter().map(String::as_str)))
}

fn rename_config(repo: &Repository, old: &str, new: &str) -> Result<RenameConfig, GitError> {
    use sha2::{Digest, Sha256};
    // Git enumerates all ordered values with origins, including repo-local
    // includes, without consulting global/system config. Unlike libgit2's
    // entry API, --show-origin identifies which included file must be edited.
    let root = repo.workdir().unwrap_or(repo.path());
    let mut output = local_config_records(repo, "--local")?;
    if repo
        .config()
        .ok()
        .and_then(|config| config.get_bool("extensions.worktreeConfig").ok())
        == Some(true)
        && repo.path().join("config.worktree").exists()
    {
        output.push_str(&local_config_records(repo, "--worktree")?);
    }
    if output.contains('\u{fffd}') {
        return Err(GitError::Other(
            "local branch config is not representable as UTF-8".into(),
        ));
    }
    let mut records = output.split_terminator('\0');
    let mut entries = Vec::new();
    let mut files = Vec::new();
    let mut keys = Vec::new();
    let mut hash = Sha256::new();
    let mut blockers = Vec::new();
    let gitdir = std::fs::canonicalize(repo.path())
        .map_err(|e| GitError::Other(format!("cannot resolve Git directory: {e}")))?;
    let commondir = std::fs::canonicalize(repo.commondir())
        .map_err(|e| GitError::Other(format!("cannot resolve common Git directory: {e}")))?;
    while let Some(origin) = records.next() {
        let record = records
            .next()
            .ok_or_else(|| GitError::Other("incomplete local config record".into()))?;
        let (key, value) = record.split_once('\n').unwrap_or((record, ""));
        if let Some(condition) = key
            .strip_prefix("includeif.")
            .and_then(|tail| tail.strip_suffix(".path"))
            .and_then(|condition| condition.strip_prefix("onbranch:"))
        {
            let matches = if condition.ends_with('/') {
                old.starts_with(condition)
            } else if condition.contains(['*', '?', '[']) {
                git2::Pathspec::new([condition])
                    .map(|pattern| {
                        pattern.matches_path(Path::new(old), git2::PathspecFlags::USE_CASE)
                    })
                    .unwrap_or(false)
            } else {
                condition == old
            };
            if matches {
                blockers.push(PlanNote::Branch(BranchNote::RenameConfigConditional {
                    condition: condition.to_owned(),
                }));
            }
        }
        let source = branch_config_variable(key, old).is_some();
        if !source && branch_config_variable(key, new).is_none() {
            continue;
        }
        let file = origin
            .strip_prefix("file:")
            .ok_or_else(|| GitError::Other("local config has no file origin".into()))?;
        let path = std::fs::canonicalize(root.join(file)).map_err(|e| {
            GitError::Other(format!("cannot resolve branch config origin '{file}': {e}"))
        })?;
        let file = path
            .to_str()
            .ok_or_else(|| GitError::Other("branch config origin is not UTF-8".into()))?;
        if source && !path.starts_with(&gitdir) && !path.starts_with(&commondir) {
            blockers.push(PlanNote::Branch(BranchNote::RenameConfigExternal {
                path: file.to_owned(),
            }));
        }
        for field in [file, record] {
            hash.update((field.len() as u64).to_le_bytes());
            hash.update(field.as_bytes());
        }
        if source {
            if !files.iter().any(|existing| existing == file) {
                files.push(file.to_owned());
            }
            if !keys.iter().any(|existing| existing == key) {
                keys.push(key.to_owned());
            }
        }
        entries.push((file.to_owned(), key.to_owned(), value.to_owned()));
    }
    if blockers.is_empty()
        && old != new
        && git2::Reference::is_valid_name(&format!("refs/heads/{new}"))
    {
        for file in &files {
            if !rename_config_header_is_safe(root, file, old, new)? {
                blockers.push(PlanNote::Branch(BranchNote::RenameConfigHeader {
                    path: file.clone(),
                }));
            }
        }
    }
    Ok(RenameConfig {
        entries,
        files,
        keys,
        digest: format!("{:x}", hash.finalize()),
        blockers,
    })
}

pub fn preflight_rename_branch(
    repo: &Repository,
    plan: &OperationPlan,
    old: &str,
    new: &str,
) -> Result<(), GitError> {
    let current = rename_config(repo, old, new)?;
    if let Some(blocker) = current.blockers.into_iter().next() {
        return Err(GitError::Blocked(Box::new(blocker)));
    }
    check_rename_config(plan, &current.digest)
}

fn check_rename_config(plan: &OperationPlan, current_digest: &str) -> Result<(), GitError> {
    let approved = plan.warnings.iter().find_map(|note| match note {
        PlanNote::Branch(BranchNote::RenameConfig { digest, .. }) => Some(digest.as_str()),
        _ => None,
    });
    if approved != Some(current_digest) {
        return Err(GitError::Blocked(Box::new(PlanNote::Branch(
            BranchNote::RenameConfigChanged,
        ))));
    }
    Ok(())
}

/// Tolerantly wipe the `branch.<name>.*` config section before a ref delete.
///
/// gh CLI is known to write duplicated `branch.<name>.*` keys (e.g.
/// github-pr-owner-number, one copy per `gh pr` invocation). libgit2's
/// `Branch::delete()` wipes the section key-by-key and aborts on the
/// duplicates ("could not find key … to delete") BEFORE deleting the ref —
/// so the first attempt fails and a retry succeeds. Removing the entries
/// tolerantly first means the ref deletion cannot be blocked by config
/// garbage. Best-effort: a key that is already gone (or lives in a read-only
/// level) is not an error.
pub(crate) fn pre_clean_branch_config(repo: &Repository, name: &str) {
    if let (Ok(keys), Ok(mut config)) = (
        branch_config_keys(repo, name),
        repo.config()
            .and_then(|config| config.open_level(git2::ConfigLevel::Local)),
    ) {
        for key in keys {
            if config.remove_multivar(&key, ".*").is_err() {
                let _ = config.remove(&key);
            }
        }
    }
}

pub fn plan_rename_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if repo.find_branch(old_name, BranchType::Local).is_err() {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: old_name.to_string(),
            in_repo: false,
        }));
    }
    let existing = local_branch_names(repo)?;
    if let BranchRenameValidation::Invalid(reason) =
        validate_branch_rename(old_name, new_name, &existing)
    {
        // ADR-0129 appendix §E: also a keyed `BranchNameError`, not a
        // `BranchNote`.
        blockers.push(PlanNote::Common(CommonNote::BranchNameErrorKeyed(reason)));
    }
    if status.is_dirty() {
        warnings.push(PlanNote::Branch(BranchNote::RenameRefOnlyDirty));
    }
    warnings.push(PlanNote::Branch(BranchNote::RenameRemoteNotRenamed));
    let config = rename_config(repo, old_name, new_name)?;
    blockers.extend(config.blockers);
    warnings.push(PlanNote::Branch(BranchNote::RenameConfig {
        keys: config.keys,
        digest: config.digest,
    }));

    Ok(OperationPlan {
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Branch(BranchTitle::RenameBranch {
            old: old_name.to_string(),
            new: new_name.to_string(),
        }),
        current,
        predicted: StateSummary {
            head: match &head {
                Head::Attached { branch, .. } if branch == old_name => {
                    format!("branch: {}", new_name)
                }
                _ => head.display(),
            },
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Branch(BranchRecovery::RenameBranch {
                old: old_name.to_string(),
                new: new_name.to_string(),
            }),
            commands: vec![format!(
                "git branch -m {} {}",
                shell_quote(new_name),
                shell_quote(old_name)
            )],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    })
}

pub(crate) fn execute_rename_branch(
    repo: &Repository,
    plan: &OperationPlan,
    old_name: &str,
    new_name: &str,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;
    let existing = local_branch_names(repo)?;
    if let BranchRenameValidation::Invalid(reason) =
        validate_branch_rename(old_name, new_name, &existing)
    {
        return Err(GitError::Other(reason.to_string()));
    }

    let saved_config = rename_config(repo, old_name, new_name)?;
    if let Some(blocker) = saved_config.blockers.first() {
        return Err(GitError::Blocked(Box::new(blocker.clone())));
    }
    check_rename_config(plan, &saved_config.digest)?;
    // Branch::rename also rewrites config with set_str, losing repeated values
    // and escapes. Reference::rename is ref-only and retargets all worktree HEADs.
    let mut reference = repo
        .find_reference(&format!("refs/heads/{old_name}"))
        .map_err(|e| GitError::Other(format!("branch lookup failed: {}", e.message())))?;
    reference
        .rename(
            &format!("refs/heads/{new_name}"),
            false,
            &format!("branch: renamed {old_name} to {new_name}"),
        )
        .map_err(|e| GitError::Other(format!("branch rename failed: {}", e.message())))?;
    for file in &saved_config.files {
        // Git's parser matches an exact subsection and changes only its header.
        // Every value is written verbatim before the old section name disappears;
        // includes are edited at their own local origin, never global/system.
        let output = run_git(
            repo.workdir().unwrap_or(repo.path()),
            &[
                "config",
                "--file",
                file,
                "--rename-section",
                &format!("branch.{old_name}"),
                &format!("branch.{new_name}"),
            ],
        )?;
        if output.status != 0 {
            return Err(GitError::Other(format!(
                "config carry-over failed: {}",
                output.stderr
            )));
        }
    }
    let expected: Vec<_> = saved_config
        .entries
        .into_iter()
        .map(|(file, key, value)| {
            let key = match branch_config_variable(&key, old_name) {
                Some(variable) => format!("branch.{new_name}.{variable}"),
                None => key,
            };
            (file, key, value)
        })
        .collect();
    if rename_config(repo, old_name, new_name)?.entries != expected {
        return Err(GitError::Other("branch config differs after rename".into()));
    }

    if repo.find_branch(new_name, BranchType::Local).is_err() {
        return Err(GitError::Other(format!(
            "branch '{}' was not found after rename",
            new_name
        )));
    }
    if repo.find_branch(old_name, BranchType::Local).is_ok() {
        return Err(GitError::Other(format!(
            "branch '{}' still exists after rename",
            old_name
        )));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
// UndoOutcome  (T-HT-009)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// plan_undo_commit  (T-HT-009)
// ────────────────────────────────────────────────────────────

/// A worktree (other than the main one) that has `branch` checked out.
pub struct WorktreeCheckout {
    /// Worktree admin name (`git worktree list` identifier, used for prune).
    pub name: String,
    /// Working-directory path shown to the user.
    pub path: std::path::PathBuf,
    /// Uncommitted changes present (staged/unstaged/untracked/conflicted).
    pub dirty: bool,
    /// Worktree is locked (`git worktree lock`) — never auto-removed.
    pub locked: bool,
}

/// Find the linked worktree that has local branch `name` checked out, if any.
///
/// Git refuses to delete a branch while any worktree has it checked out; the
/// raw libgit2 error is user-hostile (user report: agent-created worktrees
/// linger and pin their branch). Detect it at PLAN time instead.
pub fn worktree_checkout_of(repo: &Repository, name: &str) -> Option<WorktreeCheckout> {
    let full_ref = format!("refs/heads/{name}");
    let wt_names = repo.worktrees().ok()?;
    for i in 0..wt_names.len() {
        let Ok(Some(wt_name)) = wt_names.get(i) else {
            continue;
        };
        let Ok(wt) = repo.find_worktree(wt_name) else {
            continue;
        };
        let Ok(wt_repo) = Repository::open_from_worktree(&wt) else {
            continue;
        };
        let head_matches = wt_repo
            .head()
            .ok()
            .and_then(|h| h.name().ok().map(|n| n == full_ref))
            .unwrap_or(false);
        if !head_matches {
            continue;
        }
        let dirty = working_tree_status(&wt_repo)
            .map(|st| st.is_dirty())
            .unwrap_or(true); // unreadable status: err on the safe side
        let locked = matches!(wt.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_)));
        return Some(WorktreeCheckout {
            name: wt_name.to_string(),
            path: wt.path().to_path_buf(),
            dirty,
            locked,
        });
    }
    None
}

/// Analyse whether deleting local branch `name` is safe and return an
/// [`OperationPlan`].
///
/// # Design (ADR-0014)
///
/// Delete-branch is a **ref-only** operation: `Branch::delete()` removes the
/// local ref and does NOT touch the working tree or index.  **Force delete is
/// intentionally absent.**
///
/// The merged-or-not check uses `repo.graph_descendant_of(head_oid, tip_oid)`:
/// this returns `true` when `head_oid` is a descendant of `tip_oid`, meaning
/// `tip_oid` is reachable from HEAD (i.e. already merged into HEAD).
///
/// # Blocker conditions
///
/// - The named branch does not exist.
/// - The named branch is the currently checked-out branch (HEAD is attached to it).
/// - HEAD is detached and the branch tip is HEAD (prevents deleting the only
///   ref pointing at the current commit).
/// - Any main or linked worktree has the branch checked out.
///
/// # Warning conditions
/// - Unmerged branches require two confirmations and a retained tip (#584).
///
/// - The branch has an upstream configured: the remote branch is NOT deleted
///   by this operation.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_delete_branch(repo: &Repository, name: &str) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD ──────────────────────────────────────
    let head = resolve_head(repo)?;
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
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    // Branch existence check.
    let branch_result = repo.find_branch(name, BranchType::Local);
    let branch = match branch_result {
        Ok(b) => b,
        Err(_) => {
            blockers.push(PlanNote::Common(CommonNote::BranchMissing {
                name: name.to_string(),
                in_repo: true,
            }));
            // Build minimal plan with blocker and return early.
            let predicted = StateSummary {
                head: head_display.clone(),
                dirty: current.dirty.clone(),
            };
            return Ok(OperationPlan {
                tag_push_identity: None,
                approved_index_digest: None,
                disposition: PlanDisposition::for_blockers(&blockers),
                title: PlanTitle::Branch(BranchTitle::DeleteBranch {
                    name: name.to_string(),
                    tip: None,
                }),
                current,
                predicted,
                warnings,
                blockers,
                recovery: Some(PlanRecovery {
                    kind: RecoveryKind::Branch(BranchRecovery::DeleteBranch {
                        name: name.to_string(),
                        tip: None,
                    }),
                    commands: Vec::new(),
                }),
                head_at_plan: head,
                stash_count_at_plan: 0,
                stash_identity: None,
                pull_identity: None,
                worktree_digest: None,
                preview_files: Vec::new(),
                preview_commits: Vec::new(),
                destructive: false,
                equivalent_command: None,
            });
        }
    };

    // Resolve the branch tip OID (needed for merged check and recovery string).
    let tip_oid = branch
        .get()
        .target()
        .ok_or_else(|| GitError::Other(format!("branch '{}' has no target OID", name)))?;

    let tip_short = {
        let s = tip_oid.to_string();
        s.get(..8).unwrap_or(&s).to_string()
    };

    // Current-branch check (HEAD attached to this branch).
    if let Head::Attached {
        branch: ref head_branch,
        ..
    } = head
    {
        if head_branch == name {
            blockers.push(PlanNote::Branch(BranchNote::DeleteCurrentBranch {
                name: name.to_string(),
            }));
        }
    }

    // Never remove a checked-out branch, even from a clean worktree.
    let repositories = super::branch_delete_safety::repositories(repo)?;
    if let Some(path) = super::branch_delete_safety::checked_out_at(&repositories, name)? {
        blockers.push(PlanNote::Branch(BranchNote::DeleteBranchCheckedOut {
            name: name.to_string(),
            path: path.display().to_string(),
        }));
    }
    // Keep the existing dirty/locked diagnostic hints as additional blockers.
    if let Some(wt) = worktree_checkout_of(repo, name) {
        if wt.locked {
            blockers.push(PlanNote::Branch(BranchNote::DeleteBranchInLockedWorktree {
                name: name.to_string(),
                path: wt.path.display().to_string(),
            }));
        } else if wt.dirty {
            blockers.push(PlanNote::Branch(BranchNote::DeleteBranchInDirtyWorktree {
                name: name.to_string(),
                path: wt.path.display().to_string(),
            }));
        }
    }

    // Detached HEAD + tip == HEAD check.
    if let Head::Detached { ref target } = head {
        let head_oid_res = git2::Oid::from_str(target);
        if let Ok(head_oid) = head_oid_res {
            if head_oid == tip_oid {
                blockers.push(PlanNote::Branch(BranchNote::DeleteDetachedAtTip {
                    name: name.to_string(),
                }));
            }
        }
    }

    // HEAD's commit, if it has one. An unborn HEAD can have merged nothing.
    let head_oid = match &head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            git2::Oid::from_str(target).ok()
        }
        Head::Unborn { .. } => None,
    };

    // Merged check: the branch tip must be reachable from HEAD.
    // graph_descendant_of(a, b) is true when a descends from b, i.e. b is
    // reachable FROM a — here: HEAD can reach tip.
    let is_merged = head_oid
        .is_some_and(|h| h == tip_oid || repo.graph_descendant_of(h, tip_oid).unwrap_or(false));

    // A squash merge replays the branch as one new commit, so the tip is never
    // an ancestor and the check above says "unmerged" forever — the branch sits
    // in the graph as a dead-end leaf and could not be deleted at all (user
    // report). Prove the change is already in HEAD by patch-id instead.
    let squash_merge = (!is_merged)
        .then(|| head_oid.and_then(|h| squash_merged_as(repo, tip_oid, h)))
        .flatten();

    if let Some(squash) = squash_merge {
        warnings.push(PlanNote::Branch(BranchNote::DeleteSquashMerged {
            name: name.to_string(),
            squash: short_oid(squash),
        }));
    } else if !is_merged {
        warnings.push(PlanNote::Branch(BranchNote::DeleteUnmerged {
            name: name.to_string(),
            tip: tip_short.clone(),
            commits: delete_unreachable_commits(repo, name, tip_oid)?,
        }));
    }

    // Upstream warning: remote branch is NOT deleted.
    let has_upstream = branch.upstream().is_ok();
    if has_upstream {
        warnings.push(PlanNote::Branch(BranchNote::DeleteKeepsRemote {
            name: name.to_string(),
        }));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD is unchanged; the deleted branch disappears from the ref list.
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: current.dirty.clone(),
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    // ADR-0129 F-4: the restore command is structured data (`commands`) — the
    // UI reads `recovery.commands.first()` instead of parsing the display
    // text's second line.
    let recovery = PlanRecovery {
        kind: RecoveryKind::Branch(BranchRecovery::DeleteBranch {
            name: name.to_string(),
            tip: Some(tip_oid.to_string()),
        }),
        commands: vec![format!(
            "git branch {} {}",
            shell_quote(name),
            shell_quote(&tip_oid.to_string())
        )],
    };

    Ok(OperationPlan {
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Branch(BranchTitle::DeleteBranch {
            name: name.to_string(),
            tip: Some(tip_short),
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
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        // The unmerged route additionally requires mandatory recovery retention.
        equivalent_command: if is_merged && !cfg!(windows) {
            Some(format!("git branch -d {}", shell_quote(name)))
        } else {
            None
        },
    })
}

/// Commits whose only ref reachability is the branch being removed.
fn delete_unreachable_commits(
    repo: &Repository,
    name: &str,
    tip: git2::Oid,
) -> Result<usize, GitError> {
    let mut walk = repo.revwalk().map_err(|e| GitError::Other(e.to_string()))?;
    walk.push(tip).map_err(|e| GitError::Other(e.to_string()))?;
    let deleting = format!("refs/heads/{name}");
    for reference in repo
        .references()
        .map_err(|e| GitError::Other(e.to_string()))?
    {
        let reference = reference.map_err(|e| GitError::Other(e.to_string()))?;
        if super::branch_delete_safety::depends_on(repo, reference.clone(), &deleting)? {
            continue;
        }
        if let Ok(commit) = reference.peel_to_commit() {
            walk.hide(commit.id())
                .map_err(|e| GitError::Other(e.to_string()))?;
        }
    }
    if let Ok(head) = repo.head().and_then(|r| r.peel_to_commit()) {
        walk.hide(head.id())
            .map_err(|e| GitError::Other(e.to_string()))?;
    }
    walk.try_fold(0, |count, oid| {
        oid.map(|_| count + 1)
            .map_err(|e| GitError::Other(e.to_string()))
    })
}

// ────────────────────────────────────────────────────────────
// execute_delete_branch  (W2-DELETE, ADR-0014)
// ────────────────────────────────────────────────────────────

/// Delete the local branch named `name`.
///
/// # Design (ADR-0014)
///
/// Locks and removes only the approved local ref after retaining its commit.
/// HEAD/index/remotes and every worktree remain unchanged.
///
/// Steps:
/// 1. [`preflight_check`] — verify HEAD has not moved since planning.
/// 2. Lock the branch, compare its full tip, and create the recovery ref.
/// 3. Refuse checked-out branches, clean the reflog, and commit ref deletion.
/// 4. Verify the branch is gone (`find_branch` now returns `Err`).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any failure, including:
/// - HEAD has moved since planning (preflight mismatch).
/// - Branch no longer exists at execute time (already deleted externally).
/// - Backup creation or the ref transaction fails.
/// - Post-delete verify finds the branch still present.
pub(crate) fn execute_delete_branch(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    backup_refs: &mut Vec<String>,
    partial_after: &mut Option<StateSummary>,
) -> Result<crate::OperationOutcome, GitError> {
    // Lock every existing HEAD, not just the caller's: a concurrent checkout
    // in the main or another linked worktree must fail while deleting the ref.
    let repositories = super::branch_delete_safety::repositories(repo)?;
    let mut head_locks = Vec::new();
    for worktree in &repositories {
        let mut lock = worktree
            .transaction()
            .map_err(|e| GitError::Other(e.to_string()))?;
        lock.lock_ref("HEAD")
            .map_err(|e| GitError::Other(e.to_string()))?;
        head_locks.push(lock);
    }
    let branch_ref = format!("refs/heads/{name}");
    let mut transaction = repo
        .transaction()
        .map_err(|e| GitError::Other(e.to_string()))?;
    transaction
        .lock_ref(&branch_ref)
        .map_err(|e| GitError::Other(e.to_string()))?;
    preflight_check(repo, plan)?;
    let branch = repo
        .find_branch(name, BranchType::Local)
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", name, e.message())))?;

    let tip = branch
        .get()
        .target()
        .ok_or_else(|| GitError::Other("branch has no tip".into()))?;
    let expected = plan.recovery.as_ref().and_then(|r| match &r.kind {
        RecoveryKind::Branch(BranchRecovery::DeleteBranch { name: planned, tip })
            if planned == name =>
        {
            tip.as_deref()
        }
        _ => None,
    });
    if expected != Some(tip.to_string().as_str()) {
        return Err(GitError::Other(
            "branch tip changed after planning; please re-plan".into(),
        ));
    }
    if let Some(path) = super::branch_delete_safety::checked_out_at(&repositories, name)? {
        return Err(GitError::Other(format!(
            "branch '{name}' is checked out in worktree '{}'",
            path.display()
        )));
    }
    let latest = super::branch_delete_safety::repositories(repo)?;
    if !repositories
        .iter()
        .map(|r| r.path())
        .eq(latest.iter().map(|r| r.path()))
    {
        return Err(GitError::Other(
            "worktree registrations changed; please re-plan".into(),
        ));
    }
    let reference = super::backup::retain_object(repo, &super::backup::operation_id(), 0, tip)?;
    backup_refs.push(reference.clone());

    // libgit2 transaction.remove bypasses Branch::delete's reflog cleanup.
    // Do this while the branch is still locked so a recreated ref cannot lose
    // its new reflog. On failure keep the original branch and recovery root.
    let progress = super::branch_delete_safety::remove_reflog(repo, &branch_ref)?;
    if progress.reflog_removed {
        // Retain observed progress before any later error can leave this arm.
        // The ref may still exist: do not claim deletion without verification.
        *partial_after = Some(StateSummary {
            head: plan.current.head.clone(),
            dirty: format!(
                "branch '{name}' reflog removed; deletion not verified (tip {tip}); restore: git branch {name} {reference}"
            ),
        });
    }

    // ── 2.5 Pre-clean the branch's config section ─────────────
    pre_clean_branch_config(repo, name);

    // ── 3. Delete the branch ref (ref-only, no WT change) ─────
    // Keep the ref locked from the full-tip comparison through removal.
    transaction
        .remove(&branch_ref)
        .map_err(|e| GitError::Other(e.to_string()))?;
    super::branch_delete_safety::commit(transaction)?;

    // ── 4. Verify the branch is gone ─────────────────────────
    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(GitError::Other(format!(
            "branch '{}' still exists after delete — unexpected state",
            name
        )));
    }

    Ok(crate::OperationOutcome::DeleteBranch {
        name: name.into(),
        tip: tip.to_string(),
        reference,
    })
}
