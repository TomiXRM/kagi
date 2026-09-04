use super::*;
use kagi_domain::plan::SuggestionOutcome;
use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};
use kagi_domain::suggestion::{line_range, Suggestion};

// ────────────────────────────────────────────────────────────
// apply-suggestion (#351, ADR-0172) — local apply of a GitHub PR
// review "suggested change" to the WORKING TREE (never a commit).
//
// Safety: the anchored lines are captured at plan time (`expected`). If the
// working-tree file at that range no longer matches at execute time, the apply
// is REFUSED — a suggestion must never be spliced onto the wrong lines (TOCTOU,
// same class as #393 / #405). The pre-apply file content is backed up to the
// ODB first, so the edit is recoverable by blob SHA via the oplog.
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
/// captured at plan time (see [`capture_suggestion_context`]).
///
/// # Blocker conditions
/// - the target file is gone / not a text file, or the anchored range is out of
///   bounds ([`GithubNote::SuggestionRangeGone`]).
/// - the working-tree range no longer matches `expected`
///   ([`GithubNote::SuggestionStale`]) — the reviewed lines have since changed.
pub fn plan_apply_suggestion(
    repo: &Repository,
    s: &Suggestion,
    expected: &[String],
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty = status_summary_display(&status);
    let current = StateSummary {
        head: head.display(),
        dirty: dirty.clone(),
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    match read_wt_file(repo, &s.path).and_then(|c| line_range(&c, s.start_line, s.end_line)) {
        Some(cur) if cur == expected => {} // fresh — applyable
        Some(_) => blockers.push(PlanNote::Github(GithubNote::SuggestionStale {
            path: s.path.clone(),
        })),
        None => blockers.push(PlanNote::Github(GithubNote::SuggestionRangeGone {
            path: s.path.clone(),
        })),
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
            commands: vec!["git cat-file -p <blob-sha>".to_string()],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        // The fine-grained stale guard lives in execute (line-range compare);
        // only HEAD needs the generic preflight digest here.
        worktree_digest: None,
        preview_files: vec![FileStatus {
            path: std::path::PathBuf::from(&s.path),
            change: ChangeKind::Modified,
        }],
        preview_commits: Vec::new(),
        // Edits only the working tree (like typing in the file); not a
        // history rewrite, so no two-stage confirm / auto-snapshot.
        destructive: false,
        equivalent_command: None,
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
pub fn execute_apply_suggestion(
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

    // ── BACKUP the pre-apply content to the ODB before touching the file. ──
    let backup_blob = repo
        .blob(content.as_bytes())
        .map_err(|e| {
            GitError::Other(format!(
                "apply-suggestion aborted: blob backup failed for '{}': {}",
                s.path,
                e.message()
            ))
        })?
        .to_string();

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
        backup_blob,
    })
}
