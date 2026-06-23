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
pub fn fetch_remote(repo: &Repository, repo_path: &Path) -> Result<FetchOutcome, GitError> {
    // Resolve the upstream remote for the current branch, falling back to
    // fetching every remote when no single upstream can be determined.
    let remote = resolve_fetch_remote(repo);

    let args: Vec<&str> = match remote.as_deref() {
        Some(name) => vec!["fetch", name],
        None => vec!["fetch", "--all"],
    };

    // Snapshot remote-tracking refs before the fetch so we can tell whether it
    // actually moved anything — a no-op fetch must NOT trigger a graph reload
    // (which closes/re-mines HEAD-versioned overlays and re-walks the graph).
    let before = remote_ref_oids(repo);

    let out =
        run_git(repo_path, &args).map_err(|e| GitError::Other(format!("fetch failed: {}", e)))?;

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
