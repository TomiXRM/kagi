//! CheckoutNote — ADR-0129 appendix §B-2 (the checkout half; `switch.rs`
//! covers the tracking-checkout / switch-to-latest op family separately).
//!
//! `CheckoutOverlap` is produced by the single shared helper
//! `predict_checkout_conflict` (`crates/kagi-git/src/ops/checkout.rs`) and
//! pushed at two call sites (`plan_checkout` and `plan_checkout_commit`) —
//! appendix §0 counts it once here, as one variant.
//!
//! ## §G-1 sanctioned byte-identity exception
//!
//! The legacy `plan_checkout_commit` unconditionally pushed two **Japanese**
//! warning strings (`detached HEAD になります。…` / `Create branch here を
//! 先に使うことを推奨します。`) — an existing exception to the "EN source of
//! truth" contract the ADR flags for Phase 2 checkout to key properly
//! (ADR-0129 appendix §G-1). [`CheckoutNote::WillDetachHead`] and
//! [`CheckoutNote::RecommendCreateBranchHereFirst`] give these NEW English
//! `message_en()` text (not byte-identical to the legacy strings — sanctioned
//! by the ADR appendix) while [`kagi-ui-core`]'s JA rendering keeps the
//! CURRENT Japanese wording byte-for-byte.

use super::common::DirtyParts;

/// Plan notes for the checkout op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutNote {
    /// blocker (`plan_checkout`) — target branch is already the current HEAD
    /// branch (no-op family, ADR-0129 appendix §F).
    AlreadyCurrent { branch: String },
    /// blocker (`plan_checkout_commit`) — target commit is already HEAD
    /// (no-op family).
    CommitAlreadyHead,
    /// blocker (helper `predict_checkout_conflict`, shared by `plan_checkout`
    /// and `plan_checkout_commit`) — locally modified tracked paths overlap
    /// the target tree; a safe-mode checkout would be refused.
    CheckoutOverlap { count: usize, files: String },
    /// warning (`plan_checkout`) — non-overlapping staged/modified changes
    /// carry over to the target branch.
    DirtyCarriedOver { parts: DirtyParts, branch: String },
    /// warning (`plan_checkout_commit`) — dirty working tree may make a safe
    /// checkout fail.
    DirtyMayFail { display: String },
    /// warning (`plan_checkout_commit`, unconditional) — checking out a
    /// commit detaches HEAD. §G-1 exception: see module docs.
    WillDetachHead,
    /// warning (`plan_checkout_commit`, unconditional) — recommend using
    /// "Create branch here" first instead. §G-1 exception: see module docs.
    RecommendCreateBranchHereFirst,
}

impl CheckoutNote {
    /// Sole English renderer. Byte-identical to the legacy producer strings
    /// **except** [`CheckoutNote::WillDetachHead`] and
    /// [`CheckoutNote::RecommendCreateBranchHereFirst`] — see the §G-1
    /// exception documented on the module.
    pub fn message_en(&self) -> String {
        match self {
            CheckoutNote::AlreadyCurrent { branch } => {
                format!("Branch '{}' is already the current HEAD branch.", branch)
            }
            CheckoutNote::CommitAlreadyHead => "Commit is already HEAD.".to_string(),
            CheckoutNote::CheckoutOverlap { count, files } => format!(
                "Working tree has local changes to {} file(s) that the target also \
                 modifies: {}. Safe checkout would be refused (the conflict prevents checkout). \
                 Stash or commit these changes first.",
                count, files
            ),
            CheckoutNote::DirtyCarriedOver { parts, branch } => {
                format!("{} will be carried over to '{}'.", parts.parts_en(), branch)
            }
            CheckoutNote::DirtyMayFail { display } => format!(
                "Working tree is dirty ({}). Safe checkout may fail; stash or commit first.",
                display
            ),
            CheckoutNote::WillDetachHead => "This will leave you in a detached HEAD state. \
                 Create a branch first if you want to keep new work."
                .to_string(),
            CheckoutNote::RecommendCreateBranchHereFirst => {
                "Using 'Create branch here' first is recommended.".to_string()
            }
        }
    }
}

/// Plan titles for the checkout op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutTitle {
    /// `plan_checkout` — `Checkout branch '<branch>'`.
    Checkout { branch: String },
    /// `plan_checkout_commit` — `Checkout commit <sha> '<summary>' (detached HEAD)`.
    CheckoutCommit { sha: String, summary: String },
}

impl CheckoutTitle {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            CheckoutTitle::Checkout { branch } => format!("Checkout branch '{}'", branch),
            CheckoutTitle::CheckoutCommit { sha, summary } => {
                format!("Checkout commit {} '{}' (detached HEAD)", sha, summary)
            }
        }
    }
}

/// Recovery kinds for the checkout op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutRecovery {
    /// `plan_checkout` — return to the previous branch.
    Checkout { previous: String },
    /// `plan_checkout_commit` — return from the detached HEAD.
    CheckoutCommit { previous: String },
}

impl CheckoutRecovery {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            CheckoutRecovery::Checkout { previous } => format!(
                "If anything goes wrong you can return to '{}' with:\n  git checkout {}\n\
                 The reflog records every HEAD movement:\n  git reflog",
                previous, previous
            ),
            CheckoutRecovery::CheckoutCommit { previous } => format!(
                "If this was accidental, return with:\n  git checkout {}\n\
                 To keep new work from the detached state, create a branch:\n  git switch -c <name>\n\
                 The reflog records every HEAD movement:\n  git reflog",
                previous
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): dynamic values, quotes, and
    //    paths must render byte-identically to the legacy producer strings
    //    (§G-1 exceptions noted where applicable). ──

    #[test]
    fn already_current() {
        assert_eq!(
            CheckoutNote::AlreadyCurrent {
                branch: "main".into()
            }
            .message_en(),
            "Branch 'main' is already the current HEAD branch."
        );
    }

    #[test]
    fn commit_already_head() {
        assert_eq!(
            CheckoutNote::CommitAlreadyHead.message_en(),
            "Commit is already HEAD."
        );
    }

    #[test]
    fn checkout_overlap_single_and_multi_file() {
        assert_eq!(
            CheckoutNote::CheckoutOverlap {
                count: 1,
                files: "README.md".into()
            }
            .message_en(),
            "Working tree has local changes to 1 file(s) that the target also modifies: \
             README.md. Safe checkout would be refused (the conflict prevents checkout). \
             Stash or commit these changes first."
        );
        assert_eq!(
            CheckoutNote::CheckoutOverlap {
                count: 2,
                files: "a b.rs, src/c.rs".into()
            }
            .message_en(),
            "Working tree has local changes to 2 file(s) that the target also modifies: \
             a b.rs, src/c.rs. Safe checkout would be refused (the conflict prevents checkout). \
             Stash or commit these changes first."
        );
    }

    #[test]
    fn dirty_carried_over_both_parts_and_single_part() {
        assert_eq!(
            CheckoutNote::DirtyCarriedOver {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                },
                branch: "feature/x".into()
            }
            .message_en(),
            "2 staged, 1 modified will be carried over to 'feature/x'."
        );
        assert_eq!(
            CheckoutNote::DirtyCarriedOver {
                parts: DirtyParts {
                    staged: 1,
                    modified: 0
                },
                branch: "main".into()
            }
            .message_en(),
            "1 staged will be carried over to 'main'."
        );
    }

    #[test]
    fn dirty_may_fail() {
        assert_eq!(
            CheckoutNote::DirtyMayFail {
                display: "2 staged, 1 untracked".into()
            }
            .message_en(),
            "Working tree is dirty (2 staged, 1 untracked). Safe checkout may fail; \
             stash or commit first."
        );
    }

    #[test]
    fn will_detach_head_and_recommend_create_branch_here_first() {
        // §G-1 exception: NEW English text (not byte-identical to the legacy
        // Japanese strings) — sanctioned by ADR-0129 appendix §G-1. The JA
        // rendering (kagi-ui-core) keeps the original Japanese wording.
        assert_eq!(
            CheckoutNote::WillDetachHead.message_en(),
            "This will leave you in a detached HEAD state. Create a branch first if you \
             want to keep new work."
        );
        assert_eq!(
            CheckoutNote::RecommendCreateBranchHereFirst.message_en(),
            "Using 'Create branch here' first is recommended."
        );
    }

    #[test]
    fn checkout_title() {
        assert_eq!(
            CheckoutTitle::Checkout {
                branch: "feature/one".into()
            }
            .message_en(),
            "Checkout branch 'feature/one'"
        );
    }

    #[test]
    fn checkout_commit_title() {
        assert_eq!(
            CheckoutTitle::CheckoutCommit {
                sha: "a1b2c3d4".into(),
                summary: "fix: quote \"weird\" summary".into()
            }
            .message_en(),
            "Checkout commit a1b2c3d4 'fix: quote \"weird\" summary' (detached HEAD)"
        );
    }

    #[test]
    fn checkout_recovery() {
        assert_eq!(
            CheckoutRecovery::Checkout {
                previous: "main".into()
            }
            .message_en(),
            "If anything goes wrong you can return to 'main' with:\n  git checkout main\n\
             The reflog records every HEAD movement:\n  git reflog"
        );
    }

    #[test]
    fn checkout_commit_recovery() {
        assert_eq!(
            CheckoutRecovery::CheckoutCommit {
                previous: "a1b2c3d4".into()
            }
            .message_en(),
            "If this was accidental, return with:\n  git checkout a1b2c3d4\n\
             To keep new work from the detached state, create a branch:\n  git switch -c <name>\n\
             The reflog records every HEAD movement:\n  git reflog"
        );
    }
}
