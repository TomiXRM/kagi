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
    /// warning (`KagiApp` local pull orchestration): dirty work will be saved
    /// before pull and restored afterwards.
    AutoStash { parts: DirtyParts, untracked: usize },
    /// blocker (`plan_pull`): no upstream configured for the current branch,
    /// with the `git branch --set-upstream-to=…` hint.
    NoUpstreamWithHint { branch: String, err: String },
    /// warning (`plan_pull`, via the `predict_merge_conflict` helper):
    /// plan-time in-memory merge predicts a conflict with the upstream tip.
    MergePrediction,
    /// warning (`plan_pull`): paths that are modified in the working tree
    /// *and* changed by the incoming update, named before the user confirms
    /// (#625).
    ///
    /// A fast-forward pull cannot conflict commit-to-commit, so
    /// [`PullNote::MergePrediction`] stays silent for it — what conflicts is
    /// the working-tree content against the incoming content, which only shows
    /// up when the auto-stash is restored *after* the pull has run. Listing the
    /// paths at plan time is what keeps "a confirmed operation does not
    /// surprise you" true for a dirty pull.
    RestoreConflict { paths: Vec<String> },
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

/// How many colliding paths a [`PullNote::RestoreConflict`] spells out before
/// it counts the rest.
///
/// A plan note is a confirmation aid, not a file listing: an unbounded list
/// would push the modal's own buttons out of reach. Twelve is enough to name
/// the realistic overlap while keeping the note a paragraph.
pub const RESTORE_CONFLICT_PATH_LIMIT: usize = 12;

/// `(the paths to spell out, how many are left over)`.
///
/// Shared by the EN and JA renderings so they can never disagree about which
/// paths are shown or how many are hidden.
pub fn restore_conflict_paths(paths: &[String]) -> (&[String], usize) {
    if paths.len() <= RESTORE_CONFLICT_PATH_LIMIT {
        (paths, 0)
    } else {
        (
            &paths[..RESTORE_CONFLICT_PATH_LIMIT],
            paths.len() - RESTORE_CONFLICT_PATH_LIMIT,
        )
    }
}

impl PullNote {
    /// Byte-identical to the legacy `ops/pull.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            PullNote::DirtyPullGuard { parts } => format!(
                "Working tree has {}. Pull will proceed only if fetched changes do not touch those paths.",
                parts.parts_en()
            ),
            PullNote::AutoStash { parts, untracked } => {
                let mut changes = Vec::new();
                let tracked = parts.parts_en();
                if !tracked.is_empty() {
                    changes.push(tracked);
                }
                if *untracked > 0 {
                    changes.push(format!("{untracked} untracked"));
                }
                format!(
                    "Working tree has {}. Kagi will stash these changes, pull, then restore them. If restoration conflicts, the stash is kept.",
                    changes.join(", ")
                )
            }
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
            PullNote::RestoreConflict { paths } => {
                let (shown, extra) = restore_conflict_paths(paths);
                let mut out = String::from(
                    "Restoring the stash after this pull will conflict. These paths are modified \
                     here and also changed by the incoming update:",
                );
                for path in shown {
                    out.push_str("\n  - ");
                    out.push_str(path);
                }
                if extra > 0 {
                    out.push_str(&format!("\n  - … and {extra} more"));
                }
                out.push_str(
                    "\nCommit or stash those paths yourself first, or resolve the conflict after \
                     the pull — the stash is kept either way.",
                );
                out
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
    /// Local current-branch pull with UI-orchestrated stash + restore.
    PullAutoStash,
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
                 To undo a merge commit after execution without rewriting history:\n  git revert -m 1 HEAD\n\
                 The reflog records every HEAD movement:\n  git reflog"
                    .to_string()
            }
            PullRecovery::PullAutoStash => {
                "Kagi will stash staged, unstaged, and untracked changes before pulling, then pop that temporary stash.\n\
                 If the pull fails, Kagi first tries to restore the stash.\n\
                 If restoration conflicts, the stash is kept so no saved work is discarded."
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
    fn auto_stash_describes_the_complete_sequence() {
        assert_eq!(
            PullNote::AutoStash {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                },
                untracked: 3,
            }
            .message_en(),
            "Working tree has 2 staged, 1 modified, 3 untracked. Kagi will stash these changes, pull, then restore them. If restoration conflicts, the stash is kept."
        );
    }

    /// #625: the note exists to name paths, so the paths must be *in the text*
    /// — a count would leave the user exactly as surprised as before.
    #[test]
    fn restore_conflict_names_every_colliding_path() {
        let text = PullNote::RestoreConflict {
            paths: vec!["shared.txt".into(), "src/lib.rs".into()],
        }
        .message_en();
        assert!(text.contains("\n  - shared.txt"), "{text}");
        assert!(text.contains("\n  - src/lib.rs"), "{text}");
        assert!(text.contains("the stash is kept"), "{text}");
        assert!(!text.contains("more"), "nothing was truncated: {text}");
    }

    /// A working tree can collide on hundreds of paths; the note stays a
    /// paragraph and accounts for the remainder instead of listing them all.
    #[test]
    fn restore_conflict_truncates_past_the_limit_and_counts_the_rest() {
        let paths: Vec<String> = (0..RESTORE_CONFLICT_PATH_LIMIT + 3)
            .map(|i| format!("file{i}.txt"))
            .collect();
        let text = PullNote::RestoreConflict {
            paths: paths.clone(),
        }
        .message_en();
        assert!(text.contains("\n  - file0.txt"), "{text}");
        assert!(
            text.contains(&format!(
                "\n  - file{}.txt",
                RESTORE_CONFLICT_PATH_LIMIT - 1
            )),
            "{text}"
        );
        assert!(
            !text.contains(&format!("- file{RESTORE_CONFLICT_PATH_LIMIT}.txt")),
            "{text}"
        );
        assert!(text.contains("… and 3 more"), "{text}");
        // The split is shared with the JA rendering, so lock it directly too.
        let (shown, extra) = restore_conflict_paths(&paths);
        assert_eq!(shown.len(), RESTORE_CONFLICT_PATH_LIMIT);
        assert_eq!(extra, 3);
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
             To undo a merge commit after execution without rewriting history:\n  git revert -m 1 HEAD\n\
             The reflog records every HEAD movement:\n  git reflog"
        );
        assert!(!PullRecovery::Pull.message_en().contains("reset --hard"));
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
