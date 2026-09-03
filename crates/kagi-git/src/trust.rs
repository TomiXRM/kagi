//! git2 (libgit2) owner-trust gate — ADR-0160, continuing ADR-0146.
//!
//! ADR-0146 hardened the **CLI** (`run_git`) path against a hostile repository;
//! git 2.35.2+ does its own owner check on every CLI invocation, so `run_git`
//! inherits `safe.directory` for free. The **git2 path does not** — libgit2
//! never enforces `safe.directory`. This module closes that hole at
//! [`crate::backend::Backend::open`] time.
//!
//! Rule (all three must hold to be [`RepoTrust::Untrusted`]):
//! 1. the repo workdir is owned by a **different uid** than this process, AND
//! 2. the path is **not** in git's own `safe.directory` config, AND
//! 3. the path is **not** in our own [`trusted_repos`] store.
//!
//! Untrusted repos are still **readable** (so the user can inspect before
//! trusting); only [`Backend::run`](crate::backend::Backend::run) — every
//! mutating operation — refuses until [`trust_repo`] records a grant. This is a
//! deliberate soft gate: hard-failing the open would brick a legitimate shared
//! or root-owned repo with no opt-in path.

use std::path::{Path, PathBuf};

use crate::GitError;

/// Whether a repository opened through the git2 path may be **written** to.
/// Reads are always permitted; only `Backend::run` consults this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoTrust {
    /// Owned by us, or explicitly trusted (store / `safe.directory`).
    Trusted,
    /// Foreign-owned and not yet trusted. Inspection allowed; writes refused
    /// until [`trust_repo`].
    Untrusted,
}

impl RepoTrust {
    pub fn is_trusted(self) -> bool {
        matches!(self, RepoTrust::Trusted)
    }
}

/// Canonicalize `path`, falling back to the lexical path when the file is gone
/// (matches how the rest of the crate keys paths). The trust store is keyed by
/// this canonical form.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Full trust decision for a repo workdir: canonicalizes, probes ownership, and
/// applies [`evaluate_trust`]. This is what `Backend::open` calls.
pub fn evaluate(workdir: &Path) -> RepoTrust {
    let c = canonical(workdir);
    evaluate_trust(&c, owner_is_foreign(&c))
}

/// The trust decision, with the uid comparison lifted out as `foreign_uid` so
/// it is unit-testable without a `chown` (which test sandboxes disallow).
/// Consults the trust store and `safe.directory` for the foreign case.
pub fn evaluate_trust(canonical_path: &Path, foreign_uid: bool) -> RepoTrust {
    decide(
        foreign_uid,
        is_trusted(canonical_path),
        is_covered_by_safe_directory(canonical_path),
    )
}

/// The pure three-input decision (no I/O): a repo is `Untrusted` only when it is
/// foreign-owned AND neither in our trust store NOR covered by `safe.directory`.
/// Separated so the exact boolean logic is testable without env or git config.
fn decide(foreign_uid: bool, in_store: bool, in_safe_directory: bool) -> RepoTrust {
    if foreign_uid && !in_store && !in_safe_directory {
        RepoTrust::Untrusted
    } else {
        RepoTrust::Trusted
    }
}

// ── Ownership probe ─────────────────────────────────────────────────────────

/// True when `path` is owned by a uid other than this process's effective uid.
/// Non-unix has no uid ownership model, so it always returns `false` (allow).
#[cfg(unix)]
pub fn owner_is_foreign(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false; // can't stat → don't gate
    };
    match process_euid() {
        Some(me) => meta.uid() != me,
        None => false, // can't determine our uid → don't gate
    }
}

#[cfg(not(unix))]
pub fn owner_is_foreign(_path: &Path) -> bool {
    false
}

/// This process's effective uid, learned once by stat-ing a freshly created
/// temp file (which the kernel stamps with our euid) — std exposes no
/// `geteuid`, and we avoid pulling in `libc` for one call. Cached for the
/// process lifetime.
#[cfg(unix)]
fn process_euid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    use std::sync::OnceLock;
    static EUID: OnceLock<Option<u32>> = OnceLock::new();
    *EUID.get_or_init(|| {
        let f = tempfile::NamedTempFile::new().ok()?;
        std::fs::metadata(f.path()).ok().map(|m| m.uid())
    })
}

// ── git safe.directory ──────────────────────────────────────────────────────

/// Whether git's own **global/system** `safe.directory` config already marks
/// `canonical_path` as safe. We read `Config::open_default()` (user + system
/// only — NOT the repo-local config, which git ignores for `safe.directory` by
/// design, and which an attacker controls).
///
/// ponytail: handles the two forms kagi will actually meet — `*` (trust all)
/// and an exact absolute path. `%(prefix)`-relative and trailing-`/*` subtree
/// forms are not expanded; a user relying on those still gets the trust prompt
/// and can confirm once. Upgrade path: match git's `is_path_safe` if it matters.
pub fn is_covered_by_safe_directory(canonical_path: &Path) -> bool {
    let Ok(cfg) = git2::Config::open_default() else {
        return false;
    };
    let Ok(entries) = cfg.entries(Some("safe.directory")) else {
        return false;
    };
    let mut values = Vec::new();
    let mut entries = entries;
    while let Some(Ok(entry)) = entries.next() {
        if let Ok(v) = entry.value() {
            values.push(v.to_string());
        }
    }
    safe_directory_matches(values.iter().map(String::as_str), canonical_path)
}

/// Pure matcher: does any `safe.directory` value cover `canonical_path`?
/// Separated from libgit2 so the trust logic is testable without touching the
/// process-global git config (which libgit2 caches, defeating env-based tests).
fn safe_directory_matches<'a>(
    values: impl Iterator<Item = &'a str>,
    canonical_path: &Path,
) -> bool {
    let key = canonical_path.to_string_lossy();
    let key = key.trim_end_matches('/');
    for v in values {
        if v == "*" || v.trim_end_matches('/') == key {
            return true;
        }
    }
    false
}

// ── Trust store (`trusted_repos.json`) ──────────────────────────────────────

/// Path to the trust store, resolved like the oplog: `$KAGI_LOG_DIR` first
/// (tests/CI point this at a tempdir), then `$HOME/.kagi/`.
fn trust_store_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("trusted_repos.json"));
        }
    }
    crate::oplog::dirs_home().map(|h| h.join(".kagi").join("trusted_repos.json"))
}

fn load_trusted_at(store: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(store) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Whether `canonical_path` is listed in the trust store `store`.
fn is_trusted_at(store: &Path, canonical_path: &Path) -> bool {
    let key = canonical_path.to_string_lossy();
    load_trusted_at(store).iter().any(|s| s.as_str() == key)
}

/// Add `path` (canonicalized) to the trust store `store`. Idempotent.
fn trust_repo_at(store: &Path, path: &Path) -> Result<(), GitError> {
    let key = canonical(path).to_string_lossy().to_string();
    let mut list = load_trusted_at(store);
    if list.iter().any(|s| s == &key) {
        return Ok(()); // already trusted
    }
    list.push(key);
    if let Some(parent) = store.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitError::Other(format!("trust store mkdir failed: {e}")))?;
    }
    let json = serde_json::to_string_pretty(&list)
        .map_err(|e| GitError::Other(format!("trust store serialize failed: {e}")))?;
    std::fs::write(store, format!("{json}\n"))
        .map_err(|e| GitError::Other(format!("trust store write failed: {e}")))
}

/// Whether `canonical_path` is in our trust store (production path resolution).
pub fn is_trusted(canonical_path: &Path) -> bool {
    match trust_store_path() {
        Some(store) => is_trusted_at(&store, canonical_path),
        None => false,
    }
}

/// Record a user's decision to trust the repo at `path` for writes. Idempotent;
/// persists the whole list. Keyed by canonical path (`trusted_repos`).
pub fn trust_repo(path: &Path) -> Result<(), GitError> {
    let store = trust_store_path().ok_or_else(|| {
        GitError::Other("could not determine trust-store path (no HOME or KAGI_LOG_DIR)".into())
    })?;
    trust_repo_at(&store, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The trust-store round-trip is tested through the `_at` helpers with an
    // explicit temp file — no process-global env var, so these run in parallel
    // with every other test (KAGI_LOG_DIR is racy across test modules).

    #[test]
    fn decide_only_untrusts_foreign_and_ungranted() {
        // The one Untrusted case: foreign-owned, not in store, not in safe.dir.
        assert_eq!(decide(true, false, false), RepoTrust::Untrusted);
        // Any grant, or our own ownership, is Trusted.
        assert_eq!(decide(false, false, false), RepoTrust::Trusted); // ours
        assert_eq!(decide(true, true, false), RepoTrust::Trusted); // trust store
        assert_eq!(decide(true, false, true), RepoTrust::Trusted); // safe.directory
    }

    #[test]
    fn store_round_trip_grants_trust() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("trusted_repos.json");
        let p = Path::new("/srv/shared/repo");

        assert!(!is_trusted_at(&store, p), "not trusted before grant");
        trust_repo_at(&store, p).unwrap();
        assert!(is_trusted_at(&store, p), "trusted after grant");
    }

    #[test]
    fn safe_directory_matcher_covers_exact_and_wildcard() {
        let repo = Path::new("/srv/shared/repo");
        // Exact match (with and without a trailing slash).
        assert!(safe_directory_matches(
            ["/srv/shared/repo"].into_iter(),
            repo
        ));
        assert!(safe_directory_matches(
            ["/srv/shared/repo/"].into_iter(),
            repo
        ));
        // Wildcard trusts everything.
        assert!(safe_directory_matches(["*"].into_iter(), repo));
        // A different path is not covered.
        assert!(!safe_directory_matches(
            ["/srv/shared/other"].into_iter(),
            repo
        ));
        assert!(!safe_directory_matches(std::iter::empty(), repo));
    }

    #[test]
    fn trust_repo_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("trusted_repos.json");
        let p = Path::new("/srv/shared/repo");
        trust_repo_at(&store, p).unwrap();
        trust_repo_at(&store, p).unwrap();
        assert_eq!(
            load_trusted_at(&store)
                .iter()
                .filter(|s| s.contains("repo"))
                .count(),
            1
        );
    }
}
