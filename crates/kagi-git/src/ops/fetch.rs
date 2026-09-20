//! Fetch operation (W5-MENU) — download remote objects, never merge.
//!
//! Split out of the monolithic `ops/pull_push.rs` (Wave 3, ADR-0116 /
//! T-SPLIT-PULLPUSH-001). Behaviour-preserving move only.

use super::remote_common::resolve_upstream_info;
use super::*;

/// Run `git fetch` for the repository at `repo_path`.
///
/// This is **fetch-only**: it downloads remote objects and updates the
/// remote-tracking refs, but it **never merges, fast-forwards, or moves the
/// current branch**.  It is the safe sibling of [`execute_pull`](super::execute_pull) and is wired
/// to the Repository → Fetch menu command (W5-MENU / ADR-0029).
///
/// The remote is resolved from the current branch's upstream when possible;
/// otherwise `git fetch --all` is used so a detached / no-upstream repo still
/// gets its remote-tracking refs updated.  The CLI wrapper ([`run_git`]) is
/// reused (60 s timeout, `GIT_TERMINAL_PROMPT=0`).
///
/// # Errors
///
/// Returns [`GitError::Other`] when the git CLI fails to start or exits
/// non-zero.
pub(crate) fn fetch_remote(repo: &Repository, repo_path: &Path) -> Result<FetchOutcome, GitError> {
    // Resolve the upstream remote for the current branch, falling back to
    // fetching every remote when no single upstream can be determined.
    let remote = resolve_fetch_remote(repo);

    // `--prune` (ADR-0128): remote-tracking refs whose upstream branch is gone
    // are dropped. Without it, branches deleted on the hoster (e.g. after a PR
    // merge) linger locally forever as ghost `origin/*` refs — which the
    // Branch Cleanup table then reports as remote branches that don't exist.
    // Prune only removes tracking-ref cache entries: local branches and the
    // object store are untouched, and a pruned upstream is exactly what turns
    // a local branch `[gone]` (the squash-merge heuristic input).
    // `--` before the remote name, and a leading-dash reject on the name itself
    // (#291): the remote name comes straight out of the repo's own config, and
    // a remote named `--upload-pack=<cmd>` is otherwise executed as a flag.
    let args: Vec<&str> = match remote.as_deref() {
        Some(name) => {
            check_operand("remote", name)?;
            vec!["fetch", "--prune", "--", name]
        }
        None => vec!["fetch", "--all", "--prune"],
    };

    // Snapshot remote-tracking refs before the fetch so we can tell whether it
    // actually moved anything — a no-op fetch must NOT trigger a graph reload
    // (which closes/re-mines HEAD-versioned overlays and re-walks the graph).
    let before = remote_ref_oids(repo);

    let out = run_git(repo_path, &args).map_err(|e| crate::cli::context("fetch failed", e))?;

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    let after = remote_ref_oids(repo);

    Ok(FetchOutcome {
        remote: remote.unwrap_or_else(|| "--all".to_string()),
        changed: before != after,
    })
}

/// Snapshot every remote-tracking ref (`refs/remotes/**`) as `(name, oid hex)`,
/// sorted, so two snapshots can be compared for equality. Symbolic refs (e.g.
/// `origin/HEAD`) have no direct target and are skipped.
fn remote_ref_oids(repo: &Repository) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(refs) = repo.references_glob("refs/remotes/**") {
        for r in refs.flatten() {
            if let (Ok(name), Some(oid)) = (r.name(), r.target()) {
                out.push((name.to_string(), oid.to_string()));
            }
        }
    }
    out.sort();
    out
}

/// Fetch a single remote branch's refspec (branch-menu "Fetch remote
/// branch"), updating only its remote-tracking ref.
///
/// Unlike [`fetch_remote`], this never falls back to `--all` — `remote` and
/// `branch` are already known from the menu item (e.g. `"origin"` /
/// `"feature/x"` split from `"origin/feature/x"`). Same fetch-only guarantee:
/// never merges, fast-forwards, or moves the current branch.
///
/// # Errors
///
/// Returns [`GitError::Other`] when the git CLI fails to start or exits
/// non-zero.
pub(crate) fn fetch_remote_branch(
    repo: &Repository,
    repo_path: &Path,
    remote: &str,
    branch: &str,
) -> Result<FetchOutcome, GitError> {
    let before = remote_ref_oids(repo);

    check_operand("remote", remote)?;
    check_operand("branch", branch)?;

    let out = run_git(repo_path, &["fetch", "--prune", "--", remote, branch])
        .map_err(|e| crate::cli::context("fetch failed", e))?;

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    let after = remote_ref_oids(repo);

    Ok(FetchOutcome {
        remote: remote.to_string(),
        changed: before != after,
    })
}

/// Fetch the base and synthetic GitHub head refs needed to inspect one PR.
/// `refs/pull/<number>/head` exists in the base repository even when the PR
/// comes from a fork, so this never guesses which remote owns `headRefName`.
pub(crate) fn fetch_pr_refs(
    repo: &Repository,
    repo_path: &Path,
    base_repo: &str,
    number: u64,
    base: &str,
    expected_head: &str,
) -> Result<(FetchOutcome, CommitId, CommitId), GitError> {
    check_operand("base branch", base)?;
    let remote = remote_for_repo(repo, base_repo)?;
    check_operand("remote", &remote)?;
    let base_source = format!("refs/heads/{base}");
    let base_destination = format!("refs/remotes/{remote}/{base}");
    let pr_source = format!("refs/pull/{number}/head");
    let pr_destination = format!("refs/kagi/pr/{remote}/{number}/head");
    for name in [&base_source, &base_destination, &pr_source, &pr_destination] {
        if !git2::Reference::is_valid_name(name) {
            return Err(GitError::Other(format!("invalid fetch ref: {name}")));
        }
    }
    let base_refspec = format!("+{base_source}:{base_destination}");
    let pr_refspec = format!("+{pr_source}:{pr_destination}");
    let before = (
        reference_oid(repo, &base_destination),
        reference_oid(repo, &pr_destination),
    );
    let out = run_git(
        repo_path,
        &[
            "fetch",
            "--prune",
            "--",
            &remote,
            &base_refspec,
            &pr_refspec,
        ],
    )
    .map_err(|e| crate::cli::context("fetch PR refs failed", e))?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch PR refs failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }
    let base_tip = reference_commit(repo, &format!("refs/remotes/{remote}/{base}"))?;
    let head = reference_commit(repo, &format!("refs/kagi/pr/{remote}/{number}/head"))?;
    if !expected_head.is_empty() && head.0 != expected_head {
        return Err(GitError::Other(format!(
            "PR #{number} moved while loading (expected {expected_head}, fetched {})",
            head.0
        )));
    }
    Ok((
        FetchOutcome {
            remote,
            changed: before
                != (
                    reference_oid(repo, &base_destination),
                    reference_oid(repo, &pr_destination),
                ),
        },
        base_tip,
        head,
    ))
}

fn reference_oid(repo: &Repository, name: &str) -> Option<git2::Oid> {
    repo.find_reference(name).ok()?.target()
}

fn reference_commit(repo: &Repository, name: &str) -> Result<CommitId, GitError> {
    repo.find_reference(name)
        .ok()
        .and_then(|reference| reference.target())
        .map(|oid| CommitId(oid.to_string()))
        .ok_or_else(|| GitError::Other(format!("fetched ref has no commit: {name}")))
}

fn remote_for_repo(repo: &Repository, base_repo: &str) -> Result<String, GitError> {
    let config = repo
        .config()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let mut matches = Vec::new();
    if let Ok(remotes) = repo.remotes() {
        for name in remotes.iter().flatten().flatten() {
            let key = format!("remote.{name}.url");
            if config
                .get_string(&key)
                .ok()
                .and_then(|url| crate::backend::remote_ref::repo_identity(&url))
                .as_deref()
                == Some(base_repo)
            {
                matches.push(name.to_string());
            }
        }
    }
    match matches.as_slice() {
        [remote] => Ok(remote.clone()),
        [] => Err(GitError::Other(format!(
            "no local remote matches {base_repo}"
        ))),
        _ => Err(GitError::Other(format!(
            "multiple local remotes match {base_repo}: {}",
            matches.join(", ")
        ))),
    }
}

/// Best-effort resolution of the remote to fetch: the current branch's
/// configured upstream remote, else the sole configured remote, else `None`
/// (caller fetches `--all`).
fn resolve_fetch_remote(repo: &Repository) -> Option<String> {
    // Prefer the current branch's upstream remote.
    if let Ok(head_ref) = repo.head() {
        if let Ok(branch_name) = head_ref.shorthand() {
            if let Ok((_, remote_name, _)) = resolve_upstream_info(repo, branch_name) {
                return Some(remote_name);
            }
        }
    }
    // Otherwise, if exactly one remote is configured, use it.
    if let Ok(remotes) = repo.remotes() {
        if remotes.len() == 1 {
            if let Some(Ok(Some(name))) = remotes.iter().next() {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(
        repo: &Repository,
        reference: &str,
        message: &str,
        parent: Option<git2::Oid>,
    ) -> git2::Oid {
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Kagi Test", "kagi@example.com").unwrap();
        let parents = parent
            .and_then(|oid| repo.find_commit(oid).ok())
            .into_iter()
            .collect::<Vec<_>>();
        repo.commit(
            Some(reference),
            &signature,
            &signature,
            message,
            &tree,
            &parents.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn pr_ref_fetch_uses_base_repository_and_synthetic_fork_ref() {
        let root = tempfile::tempdir().unwrap();
        let remote_path = root.path().join("remote.git");
        let remote = Repository::init_bare(&remote_path).unwrap();
        let base = commit(&remote, "refs/heads/main", "base", None);
        let head = commit(&remote, "refs/pull/7/head", "fork head", Some(base));

        let local_path = root.path().join("local");
        let mut local = Repository::init(&local_path).unwrap();
        let mut config = local.config().unwrap();
        config
            .set_str("remote.origin.url", "https://github.com/acme/widgets.git")
            .unwrap();
        let file_url = format!("file://{}", remote_path.display());
        config
            .set_str(
                &format!("url.{file_url}.insteadOf"),
                "https://github.com/acme/widgets.git",
            )
            .unwrap();

        let (outcome, fetched_base, fetched_head) = fetch_pr_refs(
            &local,
            &local_path,
            "github.com/acme/widgets",
            7,
            "main",
            &head.to_string(),
        )
        .unwrap();

        assert!(outcome.changed, "the private PR ref must trigger a reload");
        assert_eq!(fetched_base.0, base.to_string());
        assert_eq!(fetched_head.0, head.to_string());
        assert_eq!(
            local
                .find_reference("refs/kagi/pr/origin/7/head")
                .unwrap()
                .target(),
            Some(head)
        );
        assert!(local
            .branches(Some(git2::BranchType::Remote))
            .unwrap()
            .flatten()
            .all(|(branch, _)| branch.name().ok().flatten() != Some("origin/pull/7")));
        let snapshot = crate::snapshot::snapshot(&mut local, 1).unwrap();
        assert!(
            snapshot
                .commits
                .iter()
                .any(|commit| commit.id.0 == head.to_string()),
            "the graph input must pin the synthetic PR head for the swimlane"
        );
    }

    #[test]
    fn pr_ref_fetch_refuses_an_ambiguous_repository_remote() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path()).unwrap();
        let mut config = repo.config().unwrap();
        for remote in ["origin", "upstream"] {
            config
                .set_str(
                    &format!("remote.{remote}.url"),
                    "https://github.com/acme/widgets.git",
                )
                .unwrap();
        }
        let error = remote_for_repo(&repo, "github.com/acme/widgets").unwrap_err();
        assert!(error.to_string().contains("multiple local remotes"));
    }
}
