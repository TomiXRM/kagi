use super::*;
use kagi_domain::plan_note::tag::TagNameError;
use kagi_domain::plan_note::{TagNote, TagRecovery, TagTitle};
use kagi_domain::remote::shell_quote;

/// The remote a tag push targets. `origin` when it exists, else the first
/// configured remote — the same rule `plan_push` uses, so the two agree.
fn tag_push_remote(repo: &Repository) -> Option<String> {
    super::push::choose_push_remote(repo).ok()
}

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

    if !name.is_empty() && is_flag_like(name) {
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
        commands: vec![format!("git tag -d {}", shell_quote(name))],
    };

    Ok(OperationPlan {
        tag_push_identity: None,
        approved_index_digest: None,
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
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
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
pub(crate) fn execute_create_tag(
    repo: &Repository,
    name: &str,
    at: &CommitId,
) -> Result<(), GitError> {
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())))?;
    let object = repo
        .find_object(oid, None)
        .map_err(|e| GitError::Other(format!("commit lookup failed: {}", e.message())))?;
    repo.tag_lightweight(name, &object, false)
        .map_err(|e| GitError::Other(format!("tag creation failed: {}", e.message())))?;
    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_push_tag / preflight_push_tag / execute_push_tag
// ────────────────────────────────────────────────────────────

/// Analyse publishing the local tag `name` to a remote.
///
/// **Guarded-class**: it does not touch HEAD, the index or the working tree,
/// but it is the one tag action with an effect outside this machine, so the
/// plan says so and the UI confirms it like any other write.
///
/// Not destructive, and deliberately not force: an explicit approved-OID
/// refspec is refused by the remote when the tag already exists
/// there pointing elsewhere. That refusal is the safety property — kagi has no
/// way to know what a moved tag would break for everyone who already fetched
/// it, so the answer is to let the remote say no.
///
/// # Blocker conditions
///
/// - No local tag named `name`.
/// - The repository has no remote configured.
pub fn plan_push_tag(repo: &Repository, name: &str) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let head_display = head.display();
    let dirty_display = if status.is_dirty() {
        "dirty".to_string()
    } else {
        "clean".to_string()
    };
    let current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display.clone(),
    };
    let predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if repo.find_reference(&format!("refs/tags/{}", name)).is_err() {
        blockers.push(PlanNote::Tag(TagNote::NotFound {
            name: name.to_string(),
        }));
    }

    let remote = tag_push_remote(repo);
    if remote.is_none() {
        blockers.push(PlanNote::Tag(TagNote::NoRemote));
    }
    let remote_name = remote.clone().unwrap_or_default();
    let tag_push_identity = if blockers.is_empty() {
        Some(resolve_push_tag_identity(repo, &remote_name, name)?)
    } else {
        None
    };

    if blockers.is_empty() {
        warnings.push(PlanNote::Tag(TagNote::PushRemoteSideEffect {
            remote: remote_name.clone(),
            name: name.to_string(),
        }));
        warnings.push(PlanNote::Tag(TagNote::PushRejectedIfMoved {
            name: name.to_string(),
        }));
    }

    let recovery = remote.as_ref().map(|r| PlanRecovery {
        kind: RecoveryKind::Tag(TagRecovery::PushTag {
            name: name.to_string(),
            remote: r.clone(),
        }),
        commands: vec![format!(
            "git push --delete -- {} {}",
            shell_quote(r),
            shell_quote(name)
        )],
    });

    Ok(OperationPlan {
        tag_push_identity,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Tag(TagTitle::PushTag {
            name: name.to_string(),
            remote: remote_name,
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
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

fn resolve_push_tag_identity(
    repo: &Repository,
    remote: &str,
    name: &str,
) -> Result<kagi_domain::plan::TagPushIdentity, GitError> {
    let error = |e: git2::Error| GitError::Other(e.to_string());
    check_operand("remote", remote)?;
    let transport = run_git(
        repo.workdir().unwrap_or(repo.path()),
        &["remote", "get-url", "--push", "--", remote],
    )
    .map_err(|e| crate::cli::context("resolve tag push destination", e))?;
    if transport.status != 0 {
        return Err(GitError::Other(transport.stderr.trim().to_string()));
    }
    let push_url = transport.stdout.trim_end_matches('\n');
    let reference = repo
        .find_reference(&format!("refs/tags/{name}"))
        .map_err(error)?
        .resolve()
        .map_err(error)?;
    let object_oid = reference
        .target()
        .ok_or_else(|| GitError::Other("tag has no object OID".into()))?;
    let peeled_oid = reference.peel(git2::ObjectType::Any).map_err(error)?.id();
    Ok(kagi_domain::plan::TagPushIdentity {
        name: name.into(),
        remote: remote.into(),
        push_url: push_url.into(),
        object_oid: object_oid.to_string(),
        peeled_oid: peeled_oid.to_string(),
    })
}

pub fn preflight_push_tag(
    repo: &Repository,
    plan: &OperationPlan,
    remote: &str,
    name: &str,
) -> Result<(), GitError> {
    let valid = plan.tag_push_identity.as_ref().is_some_and(|approved| {
        approved.remote == remote
            && approved.name == name
            && resolve_push_tag_identity(repo, remote, name).ok().as_ref() == Some(approved)
    });
    if !valid {
        return Err(GitError::Blocked(Box::new(PlanNote::Tag(
            TagNote::PushIdentityChanged,
        ))));
    }
    Ok(())
}

/// Send only the approved object, without force or re-reading a mutable ref.
pub(crate) fn execute_push_tag(repo_path: &Path, plan: &OperationPlan) -> Result<(), GitError> {
    let approved = plan
        .tag_push_identity
        .as_ref()
        .ok_or_else(|| GitError::Blocked(Box::new(PlanNote::Tag(TagNote::PushIdentityChanged))))?;
    check_operand("remote", &approved.push_url)?;
    check_operand("tag", &approved.name)?;
    let refspec = format!("{}:refs/tags/{}", approved.object_oid, approved.name);
    let out = run_git(repo_path, &["push", "--", &approved.push_url, &refspec])
        .map_err(|e| crate::cli::context("push tag failed", e))?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "push tag failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }
    Ok(())
}

/// The remote `plan_push_tag` would target, for the UI's menu label.
pub fn push_tag_remote(repo: &Repository) -> Option<String> {
    tag_push_remote(repo)
}
