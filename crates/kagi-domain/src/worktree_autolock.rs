//! Kagi-owned worktree lock tokens (#772 Phase 1, ADR-0208).
//!
//! `git worktree lock` records a free-text reason. Kagi's terminal auto-lock
//! writes a reason of the exact form `kagi:auto:<owner>` and treats **only**
//! such locks as its own: a manual lock, another process's lock, a prefix
//! without an owner, or an empty reason is never adopted, overwritten or
//! released automatically (contract B). This module is the pure judgement;
//! reading the lock and unlocking live in `kagi-git`.

use crate::remove::WorktreeId;

/// Reason prefix that marks a lock as placed by Kagi's terminal auto-lock.
pub const AUTO_LOCK_PREFIX: &str = "kagi:auto:";

/// A lock reason Kagi owns: `kagi:auto:<owner>`, where `owner` identifies the
/// terminal session that placed it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AutoLockToken {
    owner: String,
}

impl AutoLockToken {
    /// A token for `owner`. `owner` must be non-empty and free of whitespace,
    /// so the reason survives `git worktree list --porcelain` unchanged and
    /// can never be confused with a manual reason.
    pub fn new(owner: &str) -> Option<Self> {
        if owner.is_empty() || owner.chars().any(char::is_whitespace) {
            return None;
        }
        Some(Self {
            owner: owner.to_string(),
        })
    }

    /// The token recorded in `reason`, if `reason` is exactly a Kagi token.
    pub fn parse(reason: Option<&str>) -> Option<Self> {
        let reason = reason?.trim();
        let owner = reason.strip_prefix(AUTO_LOCK_PREFIX)?;
        Self::new(owner)
    }

    /// The reason string to record with `git worktree lock`.
    pub fn reason(&self) -> String {
        format!("{AUTO_LOCK_PREFIX}{}", self.owner)
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }
}

/// Why an automatic release must not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoUnlockRefusal {
    /// The worktree is not locked at all.
    NotLocked,
    /// The lock is a manual or foreign one (no Kagi token); its reason, if any.
    NotAutoLock { reason: Option<String> },
    /// The lock is Kagi's but belongs to another terminal session.
    TokenMismatch { found: AutoLockToken },
    /// The worktree the lock is on is not the one this session locked.
    IdentityMismatch,
}

/// What an automatic release expects to find: the token it placed and the
/// worktree it placed it on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoUnlockTarget {
    pub token: AutoLockToken,
    pub worktree: WorktreeId,
}

/// Name prefix of the file a release moves `locked` to while it checks the
/// content (#836, ADR-0212): `locked.kagi-<pid>-<nonce>`. One left behind
/// means a release was interrupted; Git then sees the worktree as unlocked,
/// and it is never removed automatically (contract D).
pub const LOCK_ASIDE_PREFIX: &str = "locked.kagi-";

/// Test seam for the compare-and-unlock (#836): another process's
/// `git worktree lock` / `unlock` landing at a fixed point of the release,
/// so the race is reproduced deterministically instead of by timing. A
/// finite set, like the remove / stash fault points; production passes none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoUnlockRace {
    /// After preflight, before the move-aside: the lock is removed.
    UnlockBeforeMove,
    /// After preflight, before the move-aside: the lock is replaced by one
    /// with this reason (the window read-then-unlock could not close).
    RelockBeforeMove(String),
    /// After the move-aside, before it is read: a new lock with this reason.
    RelockAfterMove(String),
    /// Both: the moved lock is someone else's, and a newer one appears before
    /// it can go back.
    RelockAroundMove { before: String, after: String },
}

/// Decide whether the lock currently on a worktree may be released by the
/// session that holds `target`.
///
/// `found_reason` is the recorded reason (`None` = unlocked or no reason),
/// `locked` whether a lock exists at all, and `found_worktree` the identity
/// of the worktree the lock was read from. Every refusal is a distinct
/// variant so a plan can say exactly why nothing will be touched.
pub fn classify_auto_unlock(
    locked: bool,
    found_reason: Option<&str>,
    found_worktree: &WorktreeId,
    target: &AutoUnlockTarget,
) -> Result<(), AutoUnlockRefusal> {
    if found_worktree != &target.worktree {
        return Err(AutoUnlockRefusal::IdentityMismatch);
    }
    if !locked {
        return Err(AutoUnlockRefusal::NotLocked);
    }
    match AutoLockToken::parse(found_reason) {
        None => Err(AutoUnlockRefusal::NotAutoLock {
            reason: found_reason
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_string),
        }),
        Some(found) if found == target.token => Ok(()),
        Some(found) => Err(AutoUnlockRefusal::TokenMismatch { found }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remove::RepoId;
    use std::path::PathBuf;

    fn wt(name: &str) -> WorktreeId {
        WorktreeId {
            repo: RepoId(PathBuf::from("/repo/.git")),
            git_dir: PathBuf::from(format!("/repo/.git/worktrees/{name}")),
        }
    }

    fn target(owner: &str, name: &str) -> AutoUnlockTarget {
        AutoUnlockTarget {
            token: AutoLockToken::new(owner).unwrap(),
            worktree: wt(name),
        }
    }

    #[test]
    fn token_round_trips_through_its_reason() {
        let token = AutoLockToken::new("s-1").unwrap();
        assert_eq!(token.reason(), "kagi:auto:s-1");
        assert_eq!(AutoLockToken::parse(Some("kagi:auto:s-1")), Some(token));
        assert_eq!(
            AutoLockToken::parse(Some("  kagi:auto:s-1\n"))
                .unwrap()
                .owner(),
            "s-1"
        );
    }

    #[test]
    fn manual_and_malformed_reasons_are_never_tokens() {
        for reason in [
            None,
            Some(""),
            Some("locked in kagi"),
            Some("agent still running"),
            Some("kagi:auto:"),
            Some("kagi:auto: "),
            Some("kagi:auto:a b"),
            Some("KAGI:AUTO:s-1"),
            Some("prefix kagi:auto:s-1"),
        ] {
            assert_eq!(AutoLockToken::parse(reason), None, "{reason:?}");
        }
        assert_eq!(AutoLockToken::new(""), None);
        assert_eq!(AutoLockToken::new("a\tb"), None);
    }

    #[test]
    fn release_only_for_the_same_token_on_the_same_worktree() {
        let t = target("s-1", "wt");
        assert_eq!(
            classify_auto_unlock(true, Some("kagi:auto:s-1"), &wt("wt"), &t),
            Ok(())
        );
        assert_eq!(
            classify_auto_unlock(false, None, &wt("wt"), &t),
            Err(AutoUnlockRefusal::NotLocked)
        );
        assert_eq!(
            classify_auto_unlock(true, Some("locked in kagi"), &wt("wt"), &t),
            Err(AutoUnlockRefusal::NotAutoLock {
                reason: Some("locked in kagi".into())
            })
        );
        assert_eq!(
            classify_auto_unlock(true, None, &wt("wt"), &t),
            Err(AutoUnlockRefusal::NotAutoLock { reason: None })
        );
        assert_eq!(
            classify_auto_unlock(true, Some("kagi:auto:s-2"), &wt("wt"), &t),
            Err(AutoUnlockRefusal::TokenMismatch {
                found: AutoLockToken::new("s-2").unwrap()
            })
        );
        // Identity wins over everything: even our own token on another
        // worktree is not ours to release.
        assert_eq!(
            classify_auto_unlock(true, Some("kagi:auto:s-1"), &wt("other"), &t),
            Err(AutoUnlockRefusal::IdentityMismatch)
        );
    }
}
