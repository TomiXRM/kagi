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

/// Which local remote **is** `base_repo` (`<host>/<owner>/<repo>`) — by
/// identity, never by name (#701 final review 4).
///
/// Exclude candidates with a different owner/repo before resolving SSH hosts.
/// Among the remaining candidates, a literal URL and an alias may name the same
/// repository, and a literal hostname can itself be remapped by `HostName`.
///
/// An alias that could not be resolved is a **refusal**, not a miss. An
/// unexamined candidate excludes nothing, so over it neither "no remote
/// matches" nor a unique winner may be claimed — and least of all the alias
/// token read as though it were a hostname.
///
/// Only an alias can be unexamined, though. Where git reaches remotes through
/// a replacement for `ssh`, a candidate whose URL spells its host out is
/// matched from the URL exactly as it was before any of this existed, so an
/// unrelated `GIT_SSH_COMMAND` or user-level `core.sshCommand` cannot stop a
/// PR fetch that had always found its remote (#706 review 2).
fn remote_for_repo(repo: &Repository, base_repo: &str) -> Result<String, GitError> {
    let config = repo
        .config()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let mut remotes = Vec::new();
    if let Ok(names) = repo.remotes() {
        for name in names.iter().flatten().flatten() {
            if let Ok(url) = config.get_string(&format!("remote.{name}.url")) {
                remotes.push((name.to_string(), url));
            }
        }
    }
    let (mut matched, mut unidentified) = (Vec::new(), Vec::new());
    let base_path = base_repo.split_once('/').map(|(_, path)| path);
    for (name, url) in &remotes {
        if crate::backend::remote_ref::repo_identity(url)
            .is_some_and(|identity| identity.split_once('/').map(|(_, path)| path) != base_path)
        {
            continue;
        }
        match crate::backend::remote_identity::resolve_repo_identity(url, &config) {
            Ok(identity) if identity.as_deref() == Some(base_repo) => matched.push(name.clone()),
            Ok(_) => {}
            Err(error) => unidentified.push(format!("{name} ({error})")),
        }
    }
    if !unidentified.is_empty() {
        return Err(GitError::Other(format!(
            "cannot tell which local remote is {base_repo}: {}",
            unidentified.join(", ")
        )));
    }
    match matched.len() {
        1 => Ok(matched.remove(0)),
        0 => Err(GitError::Other(format!(
            "no local remote matches {base_repo}"
        ))),
        _ => Err(ambiguous(base_repo, &matched)),
    }
}

fn ambiguous(base_repo: &str, matches: &[String]) -> GitError {
    GitError::Other(format!(
        "multiple local remotes match {base_repo}: {}",
        matches.join(", ")
    ))
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

    /// A stand-in `ssh` answering `-G` from a fixed script: no host is
    /// contacted, and the user's own `ssh_config` is never read.
    #[cfg(unix)]
    fn fake_ssh(dir: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join("ssh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        crate::backend::remote_identity::set_ssh_program_for_test(&path);
    }

    /// An `ssh_config` alias is a real address, and the PR fetch follows it to
    /// exactly one remote — while an alias for a *different* host stays a
    /// different repository (#706).
    #[cfg(unix)]
    #[test]
    fn pr_ref_fetch_follows_an_ssh_alias_to_the_base_repository() {
        let root = tempfile::tempdir().unwrap();
        let remote_path = root.path().join("remote.git");
        let remote = Repository::init_bare(&remote_path).unwrap();
        let base = commit(&remote, "refs/heads/main", "base", None);
        let head = commit(&remote, "refs/pull/7/head", "fork head", Some(base));

        let local_path = root.path().join("local");
        let local = Repository::init(&local_path).unwrap();
        let mut config = local.config().unwrap();
        let alias_url = "git@kagi-alias:acme/widgets.git";
        config.set_str("remote.origin.url", alias_url).unwrap();
        config
            .set_str(
                "remote.enterprise.url",
                "git@kagi-enterprise:acme/widgets.git",
            )
            .unwrap();
        // Repository-level, so a `core.sshCommand` in the developer's own
        // global config cannot decide what this fixture is about.
        config.set_str("core.sshCommand", "").unwrap();
        // The identity comes from `ssh -G`; the transport stays local, so the
        // fetch itself never leaves the fixture.
        let file_url = format!("file://{}", remote_path.display());
        config
            .set_str(&format!("url.{file_url}.insteadOf"), alias_url)
            .unwrap();
        fake_ssh(
            root.path(),
            "case \"$*\" in *kagi-enterprise*) host=ghe.example ;; *) host=github.com ;; esac\n\
             printf 'user git\\nhostname %s\\nport 22\\nidentityfile /dev/null\\n' \"$host\"",
        );

        let (outcome, fetched_base, fetched_head) = fetch_pr_refs(
            &local,
            &local_path,
            "github.com/acme/widgets",
            7,
            "main",
            &head.to_string(),
        )
        .unwrap();

        assert_eq!(
            outcome.remote, "origin",
            "the alias resolves to github.com; the enterprise alias is another repository"
        );
        assert_eq!(fetched_base.0, base.to_string());
        assert_eq!(fetched_head.0, head.to_string());
        assert_eq!(
            local
                .find_reference("refs/kagi/pr/origin/7/head")
                .unwrap()
                .target(),
            Some(head)
        );
        config
            .set_str(
                "remote.duplicate.url",
                "https://github.com/acme/widgets.git",
            )
            .unwrap();
        assert!(
            remote_for_repo(&local, "github.com/acme/widgets")
                .unwrap_err()
                .to_string()
                .contains("multiple local remotes"),
            "a literal URL cannot hide an alias naming the same repository"
        );
    }

    /// `ssh -G` failing is not "this remote is not the repository": the alias
    /// token must never be read as a hostname to fill the gap (#706).
    #[cfg(unix)]
    #[test]
    fn an_unreadable_ssh_config_refuses_rather_than_guessing_the_host() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path().join("local")).unwrap();
        let mut config = repo.config().unwrap();
        config
            .set_str("remote.origin.url", "git@kagi-alias:acme/widgets.git")
            .unwrap();
        config.set_str("core.sshCommand", "").unwrap();
        fake_ssh(
            root.path(),
            "echo 'Bad configuration option: kagi' >&2\nexit 255",
        );

        let error = remote_for_repo(&repo, "github.com/acme/widgets")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("kagi-alias") && error.contains("255"),
            "the refusal must name the target it could not map: {error}"
        );
        assert!(
            !error.contains("no local remote matches"),
            "an unexamined candidate is not an absent one: {error}"
        );
        config
            .set_str("remote.literal.url", "https://github.com/acme/widgets.git")
            .unwrap();
        assert!(
            remote_for_repo(&repo, "github.com/acme/widgets").is_err(),
            "an unidentified alias prevents proving the literal match is unique"
        );
    }

    #[test]
    fn unrelated_alias_paths_do_not_block_selection_with_an_ssh_override() {
        let root = tempfile::tempdir().unwrap();
        let mut repo = Repository::init(root.path().join("local")).unwrap();
        let global = root.path().join("global-config");
        std::fs::write(&global, "[core]\nsshCommand = kagi-unused-ssh\n").unwrap();
        let mut config = repo.config().unwrap();
        config
            .add_file(&global, git2::ConfigLevel::Global, true)
            .unwrap();
        config
            .set_str("remote.origin.url", "git@github.com:acme/widgets.git")
            .unwrap();
        repo.set_config(&config).unwrap();
        crate::backend::remote_identity::set_ssh_program_for_test(Path::new(
            "/nonexistent/kagi-must-not-run-ssh",
        ));
        config
            .set_str("remote.fork.url", "git@gh-personal:me/widgets.git")
            .unwrap();
        assert_eq!(
            remote_for_repo(&repo, "github.com/acme/widgets").unwrap(),
            "origin"
        );
        config
            .set_str("remote.fork.url", "git@gh-personal:acme/other.git")
            .unwrap();
        assert_eq!(
            remote_for_repo(&repo, "github.com/acme/widgets").unwrap(),
            "origin"
        );
        config
            .set_str("remote.fork.url", "git@gh-personal:acme/widgets.git")
            .unwrap();
        assert!(
            remote_for_repo(&repo, "github.com/acme/widgets").is_err(),
            "a same-path alias must still prevent claiming a unique match"
        );
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
