//! Absorb distribution plan — pure data (issue #345, ADR-0151).
//!
//! `absorb` takes the uncommitted hunks in the working tree and folds each one
//! into the mutable ancestor commit that last touched those lines (the same idea
//! as `git-absorb` / `jj absorb` / `sl absorb`). Hunks that cannot be attributed
//! to a single mutable commit are **left in the working tree** — never forced.
//!
//! The blame computation and the in-memory history rebuild live in the git
//! backend (`kagi-git`, which is the only layer allowed to touch `git2`). These
//! types are the pure hand-off between that computation and the UI: the
//! distribution table the plan modal renders and the user confirms.

use crate::plan::StateSummary;

/// The commit a hunk will be folded into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsorbTarget {
    /// Full 40-char object id — used to match / rebuild during execute.
    pub oid: String,
    /// 8-char short id for display.
    pub short: String,
    /// First line of the target commit's message.
    pub subject: String,
    /// Whether this target carries a GPG signature (which the rewrite drops —
    /// surfaced as a warning, never a blocker; PM §5).
    pub signed: bool,
}

/// Why a hunk is kept in the working tree instead of being absorbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepReason {
    /// The hunk only adds lines (nothing to blame), so no ancestor owns it.
    PureAddition,
    /// The hunk's lines are owned by more than one commit — no single target.
    Ambiguous,
    /// The single owning commit is not mutable (pushed, outside the window,
    /// a merge, or not on the current branch).
    Immutable,
}

/// The disposition of a single hunk in the distribution table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkDisposition {
    /// Fold this hunk into `target`.
    Absorb(AbsorbTarget),
    /// Leave this hunk in the working tree.
    Keep(KeepReason),
}

/// One row of the distribution table: a hunk and where it goes.
///
/// `old_range` / `new_range` are the `(start, count)` coordinates from the
/// zero-context diff (HEAD → working tree). They uniquely identify the hunk so
/// the executor can re-match it against a freshly computed diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkAssignment {
    /// Repo-relative path of the file the hunk belongs to.
    pub file: String,
    /// `(start, count)` in the old (HEAD) file.
    pub old_range: (u32, u32),
    /// `(start, count)` in the new (working-tree) file.
    pub new_range: (u32, u32),
    /// Where the hunk goes.
    pub disposition: HunkDisposition,
}

impl HunkAssignment {
    /// The absorb target, if this hunk is being absorbed.
    pub fn target(&self) -> Option<&AbsorbTarget> {
        match &self.disposition {
            HunkDisposition::Absorb(t) => Some(t),
            HunkDisposition::Keep(_) => None,
        }
    }

    /// Whether this hunk is being absorbed (vs kept).
    pub fn is_absorbed(&self) -> bool {
        matches!(self.disposition, HunkDisposition::Absorb(_))
    }
}

/// A condition that refuses the whole absorb (rendered red in the modal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbsorbBlocker {
    /// HEAD is detached — no branch ref to rewrite.
    DetachedHead,
    /// HEAD is unborn — no history to absorb into.
    UnbornHead,
    /// The repository is mid-conflict.
    Conflicted { count: usize },
    /// The current branch is protected (`main` etc.); rewriting it is refused
    /// (same rule as amend, ADR-0143).
    ProtectedBranch { branch: String },
    /// There are staged changes; absorb operates on unstaged working-tree hunks
    /// only (v1 restriction — keeps the index out of the rewrite).
    StagedChanges { count: usize },
    /// A merge commit sits inside the range that would be rebuilt.
    MergeInRange { short: String },
    /// Nothing can be absorbed (no hunks, or every hunk was kept).
    NothingToAbsorb,
}

/// The absorb distribution plan: the table the user confirms before execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsorbPlan {
    /// Repository state before the operation.
    pub current: StateSummary,
    /// The branch that will be rewritten (`None` when detached/unborn).
    pub branch: Option<String>,
    /// Full object id of HEAD when the plan was built. `preflight_absorb`
    /// refuses to execute if HEAD has moved since (TOCTOU guard). Empty when
    /// HEAD is detached / unborn.
    pub head_at_plan: String,
    /// How many commits back from HEAD were considered mutable (config, PM §5).
    pub window: usize,
    /// Content fingerprint of the HEAD→working-tree diff at plan time (#417).
    /// The distribution table is reasoned against a specific set of hunks at
    /// specific line coordinates; `preflight_absorb` recomputes this and refuses
    /// to execute if it no longer matches, so an edit made after planning can
    /// never be silently (mis)absorbed and the outcome counts stay accurate.
    /// `0` when there was no diff / HEAD is detached-unborn (no absorb possible).
    pub worktree_digest: u64,
    /// One row per hunk, in file / position order.
    pub assignments: Vec<HunkAssignment>,
    /// Conditions preventing execution.
    pub blockers: Vec<AbsorbBlocker>,
}

impl AbsorbPlan {
    /// Hunks that will be absorbed.
    pub fn absorbed(&self) -> impl Iterator<Item = &HunkAssignment> {
        self.assignments.iter().filter(|a| a.is_absorbed())
    }

    /// Hunks that will be kept in the working tree.
    pub fn kept(&self) -> impl Iterator<Item = &HunkAssignment> {
        self.assignments.iter().filter(|a| !a.is_absorbed())
    }

    /// Number of hunks that will be absorbed.
    pub fn absorb_count(&self) -> usize {
        self.absorbed().count()
    }

    /// Number of hunks that will be kept.
    pub fn keep_count(&self) -> usize {
        self.assignments.len() - self.absorb_count()
    }

    /// Number of distinct target commits that will be rewritten.
    pub fn targets_rewritten(&self) -> usize {
        let mut oids: Vec<&str> = self
            .absorbed()
            .filter_map(|a| a.target())
            .map(|t| t.oid.as_str())
            .collect();
        oids.sort_unstable();
        oids.dedup();
        oids.len()
    }

    /// Whether any absorbed target is signed (the rewrite drops the signature).
    pub fn signature_loss(&self) -> bool {
        self.absorbed().filter_map(|a| a.target()).any(|t| t.signed)
    }

    /// Whether execution is blocked.
    pub fn has_blockers(&self) -> bool {
        !self.blockers.is_empty()
    }

    /// A plan with no absorbable hunks is a no-op (nothing to execute).
    pub fn is_noop(&self) -> bool {
        self.absorb_count() == 0
    }
}

/// The result of executing an absorb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsorbOutcome {
    /// Full object id of the rewritten branch tip.
    pub new_head: String,
    /// How many hunks were folded into ancestors.
    pub absorbed_hunks: usize,
    /// How many hunks were left in the working tree.
    pub kept_hunks: usize,
    /// How many distinct commits were rewritten.
    pub targets_rewritten: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(oid: &str, signed: bool) -> AbsorbTarget {
        AbsorbTarget {
            oid: oid.to_string(),
            short: oid.chars().take(8).collect(),
            subject: "subject".to_string(),
            signed,
        }
    }

    fn absorb_row(file: &str, oid: &str, signed: bool) -> HunkAssignment {
        HunkAssignment {
            file: file.to_string(),
            old_range: (1, 1),
            new_range: (1, 1),
            disposition: HunkDisposition::Absorb(target(oid, signed)),
        }
    }

    fn keep_row(reason: KeepReason) -> HunkAssignment {
        HunkAssignment {
            file: "f".to_string(),
            old_range: (0, 0),
            new_range: (1, 1),
            disposition: HunkDisposition::Keep(reason),
        }
    }

    #[test]
    fn counts_targets_and_dispositions() {
        let plan = AbsorbPlan {
            current: StateSummary {
                head: "branch: main".into(),
                dirty: "3 modified".into(),
            },
            branch: Some("feature".into()),
            head_at_plan: "deadbeef".into(),
            window: 10,
            worktree_digest: 0,
            assignments: vec![
                absorb_row("a.rs", "aaaaaaaaaaaa", false),
                absorb_row("b.rs", "aaaaaaaaaaaa", false), // same target
                absorb_row("c.rs", "bbbbbbbbbbbb", true),
                keep_row(KeepReason::Ambiguous),
            ],
            blockers: vec![],
        };
        assert_eq!(plan.absorb_count(), 3);
        assert_eq!(plan.keep_count(), 1);
        assert_eq!(plan.targets_rewritten(), 2);
        assert!(plan.signature_loss());
        assert!(!plan.is_noop());
        assert!(!plan.has_blockers());
    }

    #[test]
    fn no_absorbable_hunks_is_noop() {
        let plan = AbsorbPlan {
            current: StateSummary {
                head: "branch: main".into(),
                dirty: "1 modified".into(),
            },
            branch: Some("main".into()),
            head_at_plan: "deadbeef".into(),
            window: 10,
            worktree_digest: 0,
            assignments: vec![keep_row(KeepReason::PureAddition)],
            blockers: vec![],
        };
        assert!(plan.is_noop());
        assert_eq!(plan.targets_rewritten(), 0);
        assert!(!plan.signature_loss());
    }
}
