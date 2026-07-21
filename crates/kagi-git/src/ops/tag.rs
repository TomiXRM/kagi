use super::*;
use kagi_domain::plan_note::tag::TagNameError;
use kagi_domain::plan_note::{TagNote, TagRecovery, TagTitle};

// ────────────────────────────────────────────────────────────
// plan_create_tag
// ────────────────────────────────────────────────────────────

/// Compute the keyed tag-name validation errors for the **create-tag** path,
/// mirroring [`super::create_branch_name_errors`] but scoped to `refs/tags/`.
pub fn create_tag_name_errors(repo: &Repository, name: &str) -> Vec<TagNameError> {
    let mut errs: Vec<TagNameError> = Vec::new();

    if name.is_empty() {
        errs.push(TagNameError::Empty);
    }

    if !name.is_empty() && !git2::Reference::is_valid_name(&format!("refs/tags/{}", name)) {
        errs.push(TagNameError::InvalidRef(name.to_string()));
    }

    if !name.is_empty() && name.starts_with('-') {
        errs.push(TagNameError::LeadingDash(name.to_string()));
    }

    if !name.is_empty() && repo.find_reference(&format!("refs/tags/{}", name)).is_ok() {
        errs.push(TagNameError::Exists(name.to_string()));
    }

    errs
}

/// Analyse whether creating a new lightweight tag at `at` is safe and return
/// an [`OperationPlan`].
///
/// This is a **Safe-class** operation (ADR-0004): it does not modify HEAD or
/// the working tree — a tag is a ref, nothing else.
///
/// # Blocker conditions
///
/// - `name` is empty, invalid (`git2::Reference::is_valid_name("refs/tags/<name>")`),
///   starts with `-`, or a tag with that name already exists.
/// - The commit `at` does not exist in the repository.
pub fn plan_create_tag(
    repo: &Repository,
    name: &str,
    at: &CommitId,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
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

    let mut blockers: Vec<PlanNote> = create_tag_name_errors(repo, name)
        .into_iter()
        .map(|e| PlanNote::Tag(TagNote::NameError(e)))
        .collect();

    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())));
    let commit_exists = match oid {
        Ok(oid) => repo.find_commit(oid).is_ok(),
        Err(_) => false,
    };
    if !commit_exists {
        blockers.push(PlanNote::Tag(TagNote::CommitMissing {
            sha: at.short().to_string(),
        }));
    }

    let short_sha = at.short().to_string();
    let predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let recovery = PlanRecovery {
        kind: RecoveryKind::Tag(TagRecovery::CreateTag {
            name: name.to_string(),
        }),
        commands: vec![format!("git tag -d {}", name)],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Tag(TagTitle::CreateTag {
            name: name.to_string(),
            at: short_sha,
        }),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_create_tag
// ────────────────────────────────────────────────────────────

/// Create a new lightweight tag named `name` pointing at commit `at`.
///
/// Uses `repo.tag_lightweight(name, &object, false)` — the `force` argument is
/// **always `false`** (a literal constant) to prevent overwriting an existing
/// tag, mirroring [`super::execute_create_branch`].
///
/// **This function does not perform a checkout.** HEAD remains unchanged.
pub fn execute_create_tag(repo: &Repository, name: &str, at: &CommitId) -> Result<(), GitError> {
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())))?;
    let object = repo
        .find_object(oid, None)
        .map_err(|e| GitError::Other(format!("commit lookup failed: {}", e.message())))?;
    repo.tag_lightweight(name, &object, false)
        .map_err(|e| GitError::Other(format!("tag creation failed: {}", e.message())))?;
    Ok(())
}
