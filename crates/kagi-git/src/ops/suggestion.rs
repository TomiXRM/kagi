//! This module owns apply-suggestion's `plan_*`, `preflight_*`, and `execute_*`.
//! Keep them aligned here: drift can splice a suggestion into content different
//! from the lines the user confirmed.
//!
use super::*;
use kagi_domain::plan::SuggestionOutcome;
use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};
use kagi_domain::suggestion::{line_range, Suggestion};

// ────────────────────────────────────────────────────────────
// apply-suggestion (#351, ADR-0172 / ADR-0210) — local apply of a GitHub PR
// review "suggested change" to the WORKING TREE (never a commit).
//
// Safety: the suggestion's line numbers are the PR head's, so it is applied
// only while the working-tree file is exactly the blob the PR head holds at
// that path (`blob_ids_at`). The anchored lines are also captured at plan time
// (`expected`); if the working-tree file at that range no longer matches at
// execute time, the apply is REFUSED — a suggestion must never be spliced onto
// the wrong lines (TOCTOU, same class as #393 / #405). The pre-apply file
// content is backed up under a `refs/kagi/backups/` ref first (#523), so the
// edit is recoverable from the oplog receipt even after a gc.
// ────────────────────────────────────────────────────────────

/// Read the working-tree file for `path` as a string, or `None` when it is
/// missing / unreadable / not valid UTF-8 (a binary target can't carry a
/// text suggestion anyway).
fn read_wt_file(repo: &Repository, path: &str) -> Option<String> {
    let workdir = repo.workdir()?;
    std::fs::read_to_string(workdir.join(path)).ok()
}

/// Capture the anchored `[start_line, end_line]` lines of the suggestion's
/// working-tree file — the content the confirm modal is reasoning about and the
/// value the execute-time stale guard compares against. Call this at plan time
/// (when the user opens the suggestion) and thread the result into
/// [`Operation::ApplySuggestion`]'s `expected_original`.
pub fn capture_suggestion_context(
    repo: &Repository,
    s: &Suggestion,
) -> Result<Vec<String>, GitError> {
    let content = read_wt_file(repo, &s.path).ok_or_else(|| {
        GitError::Other(format!(
            "apply-suggestion: '{}' is missing or not a text file",
            s.path
        ))
    })?;
    line_range(&content, s.start_line, s.end_line).ok_or_else(|| {
        GitError::Other(format!(
            "apply-suggestion: lines {}-{} are out of bounds in '{}'",
            s.start_line, s.end_line, s.path
        ))
    })
}

/// Build the [`OperationPlan`] for applying `s`. `expected` is the range content
/// captured at plan time (see [`capture_suggestion_context`]); `head_commit`
/// is the PR head commit the review's line numbers belong to.
///
/// # Blocker conditions
/// - the PR head commit is not in the local object store
///   ([`GithubNote::SuggestionHeadUnavailable`]).
/// - the working-tree file is not the head's blob at `s.path`
///   ([`GithubNote::SuggestionNotPrHead`]).
/// - the target file is gone / not a text file, or the anchored range is out of
///   bounds ([`GithubNote::SuggestionRangeGone`]).
/// - the working-tree range no longer matches `expected`
///   ([`GithubNote::SuggestionStale`]) — the reviewed lines have since changed.
pub fn plan_apply_suggestion(
    repo: &Repository,
    s: &Suggestion,
    expected: &[String],
    head_commit: &CommitId,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty = status_summary_display(&status);
    let current = StateSummary {
        head: head.display(),
        dirty: dirty.clone(),
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    match pr_head_note(repo, s, head_commit) {
        Some(note) => blockers.push(PlanNote::Github(note)),
        None => match read_wt_file(repo, &s.path)
            .and_then(|c| line_range(&c, s.start_line, s.end_line))
        {
            Some(cur) if cur == expected => {} // fresh — applyable
            Some(_) => blockers.push(PlanNote::Github(GithubNote::SuggestionStale {
                path: s.path.clone(),
            })),
            None => blockers.push(PlanNote::Github(GithubNote::SuggestionRangeGone {
                path: s.path.clone(),
            })),
        },
    }

    let warnings = vec![PlanNote::Github(GithubNote::SuggestionWorkingTreeOnly)];

    let predicted = StateSummary {
        head: head.display(),
        dirty: if blockers.is_empty() {
            format!("suggestion applied to '{}'", s.path)
        } else {
            dirty
        },
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Github(GithubTitle::ApplySuggestion {
            path: s.path.clone(),
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Github(GithubRecovery::ApplySuggestion),
            commands: vec!["git cat-file blob <backup-ref>".to_string()],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        // The fine-grained stale guard lives in execute (line-range compare);
        // only HEAD needs the generic preflight digest here.
        worktree_digest: None,
        preview_files: vec![FileStatus {
            path: std::path::PathBuf::from(&s.path),
            change: ChangeKind::Modified,
        }],
        preview_commits: Vec::new(),
        // A working-tree rewrite: the pre-apply content survives only in the
        // backup ref (and the auto-snapshot a destructive plan triggers), not
        // in any commit (ADR-0210).
        destructive: true,
        equivalent_command: None,
    })
}

/// Why the working-tree file cannot take the head's line numbers, if it
/// cannot: the head commit is missing locally, or the file is not the head's
/// blob at that path. A missing or unreadable working-tree file is left to the
/// range check ([`GithubNote::SuggestionRangeGone`]).
fn pr_head_note(repo: &Repository, s: &Suggestion, head: &CommitId) -> Option<GithubNote> {
    let path = std::path::PathBuf::from(&s.path);
    let Ok(head_blobs) = crate::diff::blob_ids_at(repo, head, std::slice::from_ref(&path)) else {
        return Some(GithubNote::SuggestionHeadUnavailable);
    };
    let abs = repo.workdir()?.join(&path);
    let local = git2::Oid::hash_file(git2::ObjectType::Blob, &abs).ok()?;
    (head_blobs.first() != Some(&local.to_string())).then(|| GithubNote::SuggestionNotPrHead {
        path: s.path.clone(),
    })
}

/// HEAD-unchanged preflight (mirrors the other ops' `preflight_check`). The
/// range-level stale guard is enforced in [`execute_apply_suggestion`].
pub fn preflight_apply_suggestion(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    preflight_check(repo, plan)
}

/// Apply the suggestion to the working-tree file after re-verifying the
/// anchored range still matches `expected` (TOCTOU stale guard). Backs up the
/// pre-apply file content to the ODB and returns the blob SHA as the recovery
/// handle. Never stages or commits.
pub(crate) fn execute_apply_suggestion(
    repo: &Repository,
    plan: &OperationPlan,
    s: &Suggestion,
    expected: &[String],
) -> Result<SuggestionOutcome, GitError> {
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(format!(
            "apply-suggestion refused: plan has {} blocker(s)",
            plan.blockers.len()
        )));
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?
        .to_path_buf();
    let abs = workdir.join(&s.path);

    let content = std::fs::read_to_string(&abs).map_err(|e| {
        GitError::Other(format!(
            "apply-suggestion refused: cannot read '{}': {}",
            s.path, e
        ))
    })?;

    // ── STALE GUARD (critical): the anchored lines must still be exactly what
    // the suggestion was reviewed against. If they changed since plan time,
    // refuse — applying now would splice onto the wrong lines.
    let cur = line_range(&content, s.start_line, s.end_line);
    if cur.as_deref() != Some(expected) {
        return Err(GitError::Other(format!(
            "apply-suggestion refused: '{}' changed since the suggestion was reviewed \
             (stale range) — refusing to edit the wrong lines",
            s.path
        )));
    }

    // ── BACKUP the pre-apply content under a ref before touching the file. ──
    let backup = super::backup::write_blob(
        repo,
        &super::backup::operation_id(),
        0,
        s.path.clone(),
        content.as_bytes(),
    )
    .map_err(|e| {
        GitError::Other(format!(
            "apply-suggestion aborted: backup failed for '{}': {}",
            s.path, e
        ))
    })?;

    // ── APPLY (working tree only). ──
    let new_content = s.apply_to(&content).ok_or_else(|| {
        GitError::Other(format!(
            "apply-suggestion: range {}-{} out of bounds in '{}'",
            s.start_line, s.end_line, s.path
        ))
    })?;
    std::fs::write(&abs, &new_content).map_err(|e| {
        GitError::Other(format!(
            "apply-suggestion: failed to write '{}': {}",
            s.path, e
        ))
    })?;

    // ── VERIFY the write landed as computed. ──
    let after = std::fs::read_to_string(&abs).map_err(|e| {
        GitError::Other(format!(
            "apply-suggestion verify: cannot re-read '{}': {}",
            s.path, e
        ))
    })?;
    if after != new_content {
        return Err(GitError::Other(format!(
            "apply-suggestion verify failed: '{}' does not match the applied content",
            s.path
        )));
    }

    Ok(SuggestionOutcome {
        path: s.path.clone(),
        start_line: s.start_line,
        end_line: s.end_line,
        backup_blob: backup.blob,
        reference: backup.reference,
    })
}
