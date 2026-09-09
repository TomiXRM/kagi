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

/// What the stash stack looked like before the CLI ran, so the entry this call
/// creates can be identified afterwards.
///
/// Reading the tip again afterwards is not identification: `refs/stash` is a
/// stack whose top is whatever pushed *last*, and an external `git stash push`
/// (a terminal, a hook, another tool — the app-layer write lease does not bind
/// them) between kagi's push and the read hands back **someone else's** OID.
/// The returned OID resolves the pop target for auto-stash pull (#618) and is
/// the recovery handle in the oplog (#500), so a wrong one pops another
/// person's work.
struct StashStackBefore {
    /// `refs/stash`, or `None` when no stash exists yet.
    tip: Option<git2::Oid>,
    /// Stack depth = `refs/stash` reflog entries. The stack is the reflog, not
    /// a parent chain: a stash commit's first parent is HEAD, its second the
    /// index tree and its third (with `-u`) the untracked tree.
    depth: usize,
    /// HEAD when kagi planned. Every stash kagi's push creates has it as first
    /// parent; a stash pushed from a different HEAD cannot be ours.
    head: git2::Oid,
    /// The ref label git writes into the stash message — the branch shorthand,
    /// or `(no branch)` while detached.
    label: String,
}

/// Reflog depth of `refs/stash`; a missing ref is depth 0, not an error.
fn stash_depth(repo: &Repository) -> Result<usize, GitError> {
    match repo.reflog("refs/stash") {
        Ok(reflog) => Ok(reflog.len()),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(0),
        Err(e) => Err(GitError::Other(format!(
            "stash push: cannot read the refs/stash reflog: {}",
            e.message()
        ))),
    }
}

/// Is `stored` the message git writes for *this* push?
///
/// Verified against git 2.50.1: `git stash push -m X` stores `On <label>: X`
/// verbatim — multi-line bodies, trailing spaces *and* trailing newlines are
/// all preserved — and `<label>` is `(no branch)` while detached. Trailing
/// newlines are therefore normalised on **both** sides: comparing a trimmed
/// stored message against an untrimmed expected one would make a message that
/// ends in a newline unable to identify its own stash.
///
/// Without `-m`, git generates `WIP on <label>: <abbrev> <subject>`, whose
/// abbreviation length follows `core.abbrev`, so only the generated prefix can
/// be matched — the first-parent test and the single-candidate rule still pin
/// the entry.
fn stash_message_matches(stored: &str, message: Option<&str>, label: &str) -> bool {
    match message {
        Some(passed) => {
            stored.trim_end_matches('\n') == format!("On {label}: {passed}").trim_end_matches('\n')
        }
        None => stored.starts_with(&format!("WIP on {label}: ")),
    }
}

/// The OID of the stash *this* call created, or an error that never guesses.
///
/// Only entries appended to the `refs/stash` reflog since [`StashStackBefore`]
/// are considered, and of those only ones whose commit has kagi's planned HEAD
/// as first parent and carries exactly the message kagi passed. Exactly one
/// survivor is ours; zero or several means an external push cannot be told
/// apart from kagi's, which is reported as unknown — the stash exists, so the
/// user's work is safe, but no OID is invented for it.
fn identify_created_stash(
    repo: &Repository,
    before: &StashStackBefore,
    message: Option<&str>,
) -> Result<git2::Oid, GitError> {
    let unknown = |detail: String| GitError::StashIdentityUnverified(detail);
    let reflog = repo.reflog("refs/stash").map_err(|e| {
        unknown(format!(
            "the refs/stash reflog is unreadable: {}",
            e.message()
        ))
    })?;
    let Some(added) = reflog.len().checked_sub(before.depth) else {
        // The stack got shorter: an external drop rewrote the reflog while
        // kagi's push ran, so "entries added since" no longer bounds anything.
        return Err(unknown(format!(
            "the refs/stash reflog shrank from {} to {} entries during the push",
            before.depth,
            reflog.len()
        )));
    };
    if added == 0 {
        let tip = match repo.refname_to_id("refs/stash") {
            Ok(oid) => Some(oid),
            Err(e) if e.code() == git2::ErrorCode::NotFound => None,
            Err(e) => return Err(unknown(format!("refs/stash unreadable: {}", e.message()))),
        };
        if tip == before.tip {
            return Err(GitError::Other(
                "stash push did not create a new stash".into(),
            ));
        }
        return Err(unknown(
            "refs/stash moved without a new reflog entry".to_string(),
        ));
    }
    let mut mine: Option<git2::Oid> = None;
    let mut matched = 0usize;
    // Reflog index 0 is the newest entry, so this is exactly the window of
    // pushes that landed after kagi's snapshot — kagi's own among them.
    for entry in reflog.iter().take(added) {
        let oid = entry.id_new();
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        if commit.parent_id(0).ok() != Some(before.head) {
            continue;
        }
        if !stash_message_matches(commit.message().unwrap_or_default(), message, &before.label) {
            continue;
        }
        matched += 1;
        mine = Some(oid);
    }
    match (matched, mine) {
        (1, Some(oid)) => Ok(oid),
        (0, _) => Err(unknown(format!(
            "none of the {added} stash entries added during the push has HEAD {} as first parent \
             with kagi's message",
            before.head
        ))),
        (n, _) => Err(unknown(format!(
            "{n} of the {added} stash entries added during the push are indistinguishable from \
             kagi's (same HEAD and message)"
        ))),
    }
}

/// Execute a stash push: save local modifications to a new stash entry.
///
/// Uses the hardened Git CLI to avoid libgit2's full tracked-content scan when
/// building the untracked tree (#622). Without `include_untracked`, new files
/// remain in the working tree.
///
/// The signature is read from the repository config (`user.name` / `user.email`);
/// if either is absent, falls back to `"kagi <kagi@local>"`.
///
/// External filters are disabled by the shared [`run_git`] config hardening,
/// matching libgit2's built-in-only filters.
///
/// Returns the created stash commit OID as a hex string, identified by
/// [`identify_created_stash`] rather than by re-reading the `refs/stash` tip
/// (#623).
///
/// # Errors
///
/// Returns [`GitError::TerminationUnknown`] if subprocess completion is
/// uncertain, or if the created stash cannot be told apart from a concurrent
/// external push ([`GitError::StashIdentityUnverified`]), and
/// [`GitError::Other`] if execution or the resulting ref/index read fails.
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
    let mut args = vec![
        "-c".to_owned(),
        format!("user.name={}", sig.name().unwrap_or("kagi")),
        "-c".to_owned(),
        format!("user.email={}", sig.email().unwrap_or("kagi@local")),
    ];
    args.extend(["stash".to_owned(), "push".to_owned()]);
    if include_untracked {
        args.push("--include-untracked".to_owned());
    }
    if let Some(message) = message {
        args.extend(["--message".to_owned(), message.to_owned()]);
    }
    // Snapshotted last, so the window an external push can slip into before
    // kagi's own is only the hardened child startup.
    let before = StashStackBefore {
        tip: match repo.refname_to_id("refs/stash") {
            Ok(oid) => Some(oid),
            Err(e) if e.code() == git2::ErrorCode::NotFound => None,
            Err(e) => return Err(error(e)),
        },
        depth: stash_depth(repo)?,
        head: repo
            .head()
            .and_then(|head| head.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|e| {
                GitError::Other(format!(
                    "stash push requires a HEAD commit: {}",
                    e.message()
                ))
            })?,
        label: stash_ref_label(repo)?,
    };
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
    Ok(identify_created_stash(repo, &before, message)?.to_string())
}

/// The label git puts in a stash message: the branch shorthand, `(no branch)`
/// when HEAD is detached.
fn stash_ref_label(repo: &Repository) -> Result<String, GitError> {
    let detached = repo
        .head_detached()
        .map_err(|e| GitError::Other(format!("stash push: cannot read HEAD: {}", e.message())))?;
    if detached {
        return Ok("(no branch)".to_string());
    }
    let head = repo
        .head()
        .map_err(|e| GitError::Other(format!("stash push: cannot read HEAD: {}", e.message())))?;
    Ok(head.shorthand().unwrap_or("(no branch)").to_string())
}

#[cfg(test)]
mod tests {
    use super::stash_message_matches;

    /// The shapes git actually stores (verified against git 2.50.1), including
    /// the trailing-newline case a trimmed-vs-untrimmed comparison broke: a
    /// message ending in `\n` could not identify its own stash, so every such
    /// push reported an unverified identity.
    #[test]
    fn explicit_message_matches_what_git_stores() {
        assert!(stash_message_matches("On main: work", Some("work"), "main"));
        assert!(stash_message_matches(
            "On main: work\n",
            Some("work"),
            "main"
        ));
        assert!(stash_message_matches(
            "On main: work\n",
            Some("work\n"),
            "main"
        ));
        assert!(stash_message_matches(
            "On main: first\nsecond\n",
            Some("first\nsecond\n"),
            "main"
        ));
        // Trailing spaces are content, not framing: git keeps them.
        assert!(stash_message_matches(
            "On main: work  ",
            Some("work  "),
            "main"
        ));
        // Detached HEAD writes the same shape with git's own label.
        assert!(stash_message_matches(
            "On (no branch): work",
            Some("work"),
            "(no branch)"
        ));
    }

    #[test]
    fn another_pushs_message_or_branch_does_not_match() {
        assert!(!stash_message_matches(
            "On main: theirs",
            Some("work"),
            "main"
        ));
        assert!(!stash_message_matches(
            "On other: work",
            Some("work"),
            "main"
        ));
        // A generated message is not an explicit one, and vice versa.
        assert!(!stash_message_matches(
            "WIP on main: 0123abc base",
            Some("work"),
            "main"
        ));
        assert!(!stash_message_matches("On main: work", None, "main"));
        assert!(!stash_message_matches("", Some("work"), "main"));
    }

    /// Without `-m` the abbreviation length follows `core.abbrev`, so only the
    /// generated prefix is matched — the first-parent and single-candidate
    /// rules carry the rest.
    #[test]
    fn generated_message_matches_on_its_prefix_only() {
        assert!(stash_message_matches(
            "WIP on main: 0123abc base commit",
            None,
            "main"
        ));
        assert!(stash_message_matches(
            "WIP on (no branch): 0123abcdef1 base",
            None,
            "(no branch)"
        ));
        assert!(!stash_message_matches(
            "WIP on feature: 0123abc base",
            None,
            "main"
        ));
    }
}
