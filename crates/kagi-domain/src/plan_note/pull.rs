//! PullNote — ADR-0129 Phase 2 category (appendix §B-4).
//!
//! Covers the three pull plan producers in `crates/kagi-git/src/ops/pull.rs`:
//! `plan_pull` (current-branch pull), `plan_pull_branch_ff` (ref-only
//! fast-forward pull for a non-current branch), and `plan_pull_remote` (SSH
//! snapshot-only pull plan). Cross-op notes (HEAD state, conflicted files,
//! untracked-remain) are NOT duplicated here — they map to the existing
//! `CommonNote` variants (appendix §A) from the ops file directly.

use super::DirtyParts;

/// Plan notes for the pull op family (ADR-0129 appendix §B-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullNote {
    /// warning (`plan_pull`): dirty working tree may block the pull if the
    /// fetched update touches the same paths.
    DirtyPullGuard { parts: DirtyParts },
    /// blocker (`plan_pull`): no upstream configured for the current branch,
    /// with the `git branch --set-upstream-to=…` hint.
    NoUpstreamWithHint { branch: String, err: String },
    /// warning (`plan_pull`, via the `predict_merge_conflict` helper):
    /// plan-time in-memory merge predicts a conflict with the upstream tip.
    MergePrediction,
    /// warning (`plan_pull_branch_ff`): conflicted files exist; this
    /// ref-only pull will not touch the working tree regardless.
    ConflictedRefOnly { count: usize },
    /// warning (`plan_pull_branch_ff`): working tree is dirty; this
    /// ref-only pull will not touch the working tree regardless.
    DirtyRefOnly,
    /// blocker (`plan_pull_branch_ff`): no upstream configured for the
    /// target branch (no set-upstream hint — distinct wording from
    /// [`PullNote::NoUpstreamWithHint`]).
    NoUpstream { branch: String, err: String },
    /// blocker (`plan_pull_branch_ff`): branch is already up to date with
    /// its upstream.
    AlreadyUpToDate { branch: String },
    /// blocker (`plan_pull_branch_ff`): branch cannot be fast-forwarded to
    /// its upstream.
    CannotFastForward { branch: String },
    /// warning (`plan_pull_remote`, SSH): the branch has diverged from its
    /// upstream — the pull will create a merge commit on the remote host.
    RemoteDiverged {
        branch: String,
        ahead: usize,
        behind: usize,
    },
    /// warning (`plan_pull_remote`, SSH): the remote working tree has
    /// uncommitted changes.
    RemoteDirty,
}

impl PullNote {
    /// Byte-identical to the legacy `ops/pull.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            PullNote::DirtyPullGuard { parts } => format!(
                "Working tree has {}. Pull will proceed only if fetched changes do not touch those paths.",
                parts.parts_en()
            ),
            PullNote::NoUpstreamWithHint { branch, err } => format!(
                "No upstream configured for branch '{}': {}. Set one with `git branch --set-upstream-to=<remote>/<branch>`.",
                branch, err
            ),
            PullNote::MergePrediction => {
                "Plan-time merge prediction: the current upstream tip would conflict with HEAD. \
                 Execute is NOT blocked (fetch may change things), but be aware that if the \
                 upstream has not changed, execute will fail safely leaving the repo untouched."
                    .to_string()
            }
            PullNote::ConflictedRefOnly { count } => format!(
                "Repository has {} conflicted file(s); this ref-only pull will not touch the working tree.",
                count
            ),
            PullNote::DirtyRefOnly => {
                "Working tree is dirty; this ref-only pull will not touch the working tree.".to_string()
            }
            PullNote::NoUpstream { branch, err } => {
                format!("No upstream configured for branch '{}': {}.", branch, err)
            }
            PullNote::AlreadyUpToDate { branch } => format!(
                "Branch '{}' is already up to date with its upstream.",
                branch
            ),
            PullNote::CannotFastForward { branch } => format!(
                "Branch '{}' cannot be fast-forwarded to its upstream; pull it while checked out to merge.",
                branch
            ),
            PullNote::RemoteDiverged {
                branch,
                ahead,
                behind,
            } => format!(
                "{branch} has diverged ({ahead} ahead, {behind} behind); \
                 the pull will create a merge commit on the remote."
            ),
            PullNote::RemoteDirty => {
                "The remote working tree has uncommitted changes; the pull may fail \
                 or produce conflicts that must be resolved on the host."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the pull op family (ADR-0129 appendix §C).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullTitle {
    /// `plan_pull_remote` (SSH). `behind == 0` renders the "up to date
    /// (local knowledge)" form; otherwise the "N commit(s) behind" form.
    PullRemote {
        branch: String,
        upstream: String,
        behind: usize,
    },
    /// `plan_pull` (current-branch pull). NOTE the two spaces between the
    /// closing quote and the opening paren — byte-exact in the legacy
    /// producer. `behind == 0` renders the "up to date (local knowledge;
    /// fetch may reveal more)" sub-form.
    Pull {
        branch: String,
        remote: String,
        behind: usize,
    },
    /// `plan_pull_branch_ff` (ref-only fast-forward pull).
    PullBranchFf {
        branch: String,
        remote: String,
        behind: usize,
    },
}

impl PullTitle {
    /// Byte-identical to the legacy `ops/pull.rs` title strings.
    pub fn message_en(&self) -> String {
        match self {
            PullTitle::PullRemote {
                branch,
                upstream,
                behind,
            } => {
                if *behind == 0 {
                    format!("Pull {branch} — up to date (local knowledge)")
                } else {
                    format!("Pull {branch} from {upstream} — {behind} commit(s) behind")
                }
            }
            PullTitle::Pull {
                branch,
                remote,
                behind,
            } => {
                let behind_label = if *behind == 0 {
                    "up to date (local knowledge; fetch may reveal more)".to_string()
                } else {
                    format!(
                        "{} behind upstream (local knowledge; fetch may reveal more)",
                        behind
                    )
                };
                format!("Pull '{}' from '{}'  ({})", branch, remote, behind_label)
            }
            PullTitle::PullBranchFf {
                branch,
                remote,
                behind,
            } => format!(
                "Pull '{}' from '{}' (ff-only, ref-only, {} behind)",
                branch, remote, behind
            ),
        }
    }
}

/// Recovery kinds for the pull op family (ADR-0129 appendix §D).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRecovery {
    /// `plan_pull`.
    Pull,
    /// `plan_pull_remote` (SSH).
    PullRemote,
    /// `plan_pull_branch_ff`.
    PullBranchFf { branch: String },
}

impl PullRecovery {
    /// Byte-identical to the legacy `ops/pull.rs` recovery strings.
    pub fn message_en(&self) -> String {
        match self {
            PullRecovery::Pull => {
                "Pull is non-destructive: fast-forward and clean merges do not lose work.\n\
                 Dirty working-tree paths are checked against the fetched update before checkout.\n\
                 If the merge would conflict or overwrite dirty paths, execute is blocked and the repo remains untouched.\n\
                 To undo a merge commit after execution:\n  git reset --hard HEAD~1\n\
                 The reflog records every HEAD movement:\n  git reflog"
                    .to_string()
            }
            PullRecovery::PullRemote => {
                "Runs `git pull` on the host using its own credentials. \
                 Conflicts are left for resolution on the host."
                    .to_string()
            }
            PullRecovery::PullBranchFf { branch } => format!(
                "This updates only refs/heads/{} after verifying a fast-forward. \
                 The working tree is not changed. If needed, restore the old tip with git branch -f {} <old-sha>.",
                branch, branch
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PullNote golden tests ──────────────────────────────────────────

    #[test]
    fn dirty_pull_guard_staged_and_modified() {
        assert_eq!(
            PullNote::DirtyPullGuard {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                }
            }
            .message_en(),
            "Working tree has 2 staged, 1 modified. Pull will proceed only if fetched changes do not touch those paths."
        );
        assert_eq!(
            PullNote::DirtyPullGuard {
                parts: DirtyParts {
                    staged: 0,
                    modified: 3
                }
            }
            .message_en(),
            "Working tree has 3 modified. Pull will proceed only if fetched changes do not touch those paths."
        );
    }

    #[test]
    fn no_upstream_with_hint() {
        assert_eq!(
            PullNote::NoUpstreamWithHint {
                branch: "feat/x".into(),
                err: "no upstream branch".into()
            }
            .message_en(),
            "No upstream configured for branch 'feat/x': no upstream branch. Set one with `git branch --set-upstream-to=<remote>/<branch>`."
        );
    }

    #[test]
    fn merge_prediction() {
        assert_eq!(
            PullNote::MergePrediction.message_en(),
            "Plan-time merge prediction: the current upstream tip would conflict with HEAD. \
             Execute is NOT blocked (fetch may change things), but be aware that if the \
             upstream has not changed, execute will fail safely leaving the repo untouched."
        );
    }

    #[test]
    fn conflicted_ref_only() {
        assert_eq!(
            PullNote::ConflictedRefOnly { count: 2 }.message_en(),
            "Repository has 2 conflicted file(s); this ref-only pull will not touch the working tree."
        );
    }

    #[test]
    fn dirty_ref_only() {
        assert_eq!(
            PullNote::DirtyRefOnly.message_en(),
            "Working tree is dirty; this ref-only pull will not touch the working tree."
        );
    }

    #[test]
    fn no_upstream_no_hint() {
        assert_eq!(
            PullNote::NoUpstream {
                branch: "feat/x".into(),
                err: "no upstream branch".into()
            }
            .message_en(),
            "No upstream configured for branch 'feat/x': no upstream branch."
        );
    }

    #[test]
    fn already_up_to_date() {
        assert_eq!(
            PullNote::AlreadyUpToDate {
                branch: "main".into()
            }
            .message_en(),
            "Branch 'main' is already up to date with its upstream."
        );
    }

    #[test]
    fn cannot_fast_forward() {
        assert_eq!(
            PullNote::CannotFastForward {
                branch: "main".into()
            }
            .message_en(),
            "Branch 'main' cannot be fast-forwarded to its upstream; pull it while checked out to merge."
        );
    }

    #[test]
    fn remote_diverged() {
        assert_eq!(
            PullNote::RemoteDiverged {
                branch: "main".into(),
                ahead: 1,
                behind: 2
            }
            .message_en(),
            "main has diverged (1 ahead, 2 behind); the pull will create a merge commit on the remote."
        );
    }

    #[test]
    fn remote_dirty() {
        assert_eq!(
            PullNote::RemoteDirty.message_en(),
            "The remote working tree has uncommitted changes; the pull may fail or produce conflicts that must be resolved on the host."
        );
    }

    // ── PullTitle golden tests ──────────────────────────────────────────

    #[test]
    fn pull_remote_title_up_to_date() {
        assert_eq!(
            PullTitle::PullRemote {
                branch: "main".into(),
                upstream: "origin/main".into(),
                behind: 0
            }
            .message_en(),
            "Pull main — up to date (local knowledge)"
        );
    }

    #[test]
    fn pull_remote_title_behind() {
        assert_eq!(
            PullTitle::PullRemote {
                branch: "main".into(),
                upstream: "origin/main".into(),
                behind: 3
            }
            .message_en(),
            "Pull main from origin/main — 3 commit(s) behind"
        );
    }

    #[test]
    fn pull_title_two_spaces_before_paren() {
        // Byte-exact: TWO spaces between the closing quote and '('.
        assert_eq!(
            PullTitle::Pull {
                branch: "main".into(),
                remote: "origin".into(),
                behind: 0
            }
            .message_en(),
            "Pull 'main' from 'origin'  (up to date (local knowledge; fetch may reveal more))"
        );
        assert_eq!(
            PullTitle::Pull {
                branch: "main".into(),
                remote: "origin".into(),
                behind: 4
            }
            .message_en(),
            "Pull 'main' from 'origin'  (4 behind upstream (local knowledge; fetch may reveal more))"
        );
    }

    #[test]
    fn pull_branch_ff_title() {
        assert_eq!(
            PullTitle::PullBranchFf {
                branch: "feat/x".into(),
                remote: "origin".into(),
                behind: 5
            }
            .message_en(),
            "Pull 'feat/x' from 'origin' (ff-only, ref-only, 5 behind)"
        );
    }

    // ── PullRecovery golden tests ─────────────────────────────────────

    #[test]
    fn pull_recovery_text() {
        assert_eq!(
            PullRecovery::Pull.message_en(),
            "Pull is non-destructive: fast-forward and clean merges do not lose work.\n\
             Dirty working-tree paths are checked against the fetched update before checkout.\n\
             If the merge would conflict or overwrite dirty paths, execute is blocked and the repo remains untouched.\n\
             To undo a merge commit after execution:\n  git reset --hard HEAD~1\n\
             The reflog records every HEAD movement:\n  git reflog"
        );
    }

    #[test]
    fn pull_remote_recovery_text() {
        assert_eq!(
            PullRecovery::PullRemote.message_en(),
            "Runs `git pull` on the host using its own credentials. Conflicts are left for resolution on the host."
        );
    }

    #[test]
    fn pull_branch_ff_recovery_text() {
        assert_eq!(
            PullRecovery::PullBranchFf {
                branch: "feat/x".into()
            }
            .message_en(),
            "This updates only refs/heads/feat/x after verifying a fast-forward. The working tree is not changed. If needed, restore the old tip with git branch -f feat/x <old-sha>."
        );
    }
}
