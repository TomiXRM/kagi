//! Stash push planning and execution; shared stash preflight lives in `stash`.
use super::*;
use kagi_domain::plan_note::{OpPhrase, StashNote, StashRecovery, StashTitle};

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
    let identity = stash_identity(repo, None)?;
    let stash_count = identity.oids.len();

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
        stash_identity: Some(identity),
        worktree_digest: Some(status.digest()),
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
/// Uses the hardened Git CLI to avoid libgit2's full tracked-content scan when
/// building the untracked tree (#622). Without `include_untracked`, new files
/// remain in the working tree.
///
/// The signature is read from the repository config (`user.name` / `user.email`);
/// if either is absent, falls back to `"kagi <kagi@local>"`.
///
/// External filters stay disabled, matching libgit2's built-in-only filters.
///
/// Returns the created stash commit OID as a hex string.
///
/// # Errors
///
/// Returns [`GitError::TerminationUnknown`] if subprocess completion is uncertain,
/// or [`GitError::Other`] if execution or the resulting ref/index read fails.
pub(crate) fn execute_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<String, GitError> {
    let error = |e: git2::Error| GitError::Other(format!("stash push failed: {}", e.message()));
    let sig = build_signature(repo)?;
    let root = repo
        .workdir()
        .ok_or_else(|| GitError::Other("stash push requires a worktree".into()))?;
    let before = match repo.refname_to_id("refs/stash") {
        Ok(oid) => Some(oid),
        Err(e) if e.code() == git2::ErrorCode::NotFound => None,
        Err(e) => return Err(error(e)),
    };
    let mut args = vec![
        "-c".to_owned(),
        format!("user.name={}", sig.name().unwrap_or("kagi")),
        "-c".to_owned(),
        format!("user.email={}", sig.email().unwrap_or("kagi@local")),
    ];
    // Git CLI supports executable filters that libgit2 never runs. Do not
    // introduce repository-config code execution by changing the engine.
    let config = repo.config().map_err(error)?;
    let mut entries = config.entries(None).map_err(error)?;
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(error)?;
        let key = entry.name().map_err(error)?;
        if key.starts_with("filter.") {
            if key.ends_with(".clean") || key.ends_with(".smudge") || key.ends_with(".process") {
                args.extend(["-c".to_owned(), format!("{key}=")]);
            } else if key.ends_with(".required") {
                args.extend(["-c".to_owned(), format!("{key}=false")]);
            }
        }
    }
    args.extend(["stash".to_owned(), "push".to_owned()]);
    if include_untracked {
        args.push("--include-untracked".to_owned());
    }
    if let Some(message) = message {
        args.extend(["--message".to_owned(), message.to_owned()]);
    }
    let output = run_git(root, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    if output.status != 0 {
        return Err(GitError::Other(format!(
            "stash push failed: {}",
            output.stderr.trim()
        )));
    }
    // The subprocess replaced the index; later libgit2 verification must not
    // see the pre-execution index cached by plan/preflight.
    repo.index().map_err(error)?.read(true).map_err(error)?;
    let oid = repo.refname_to_id("refs/stash").map_err(error)?;
    if Some(oid) == before {
        return Err(GitError::Other(
            "stash push did not create a new stash".into(),
        ));
    }
    Ok(oid.to_string())
}
