//! WorktreeNote — ADR-0129 appendix §B-8 (create-branch+checkout /
//! create-worktree / unlock-worktree).
//!
//! `plan_create_worktree_impl`'s `validate_worktree_path_keyed` blocker is
//! `PlanNote::Common(CommonNote::WorktreePathErrorKeyed(..))` for the two keyed
//! reasons (empty / already exists), or `CommonNote::GitErrorPassthrough` for
//! any other (`WorktreeValidationError::Other`) — not redefined here since
//! `CommonNote` already covers it (§E). Likewise the branch-missing blocker in
//! `plan_create_worktree_impl`'s existing-branch path is the cross-op
//! `PlanNote::Common(CommonNote::BranchMissing { in_repo: true, .. })`
//! (§A14) — not redefined here.
//!
//! `plan_create_branch_with_checkout`'s conflicted/dirty-working-tree
//! blockers are their own dedicated sentences (verified byte-for-byte against
//! the current `ops/worktree.rs` source, appendix §B-8 row 2): the dirty one
//! reads "…checkout after branch creation could lose work. Stash changes
//! before continuing." which is NOT the cross-op `CommonNote::DirtyBlocksOp`
//! sentence ("…stash or commit changes before {op}.") — so it stays a
//! dedicated `DirtyBlocksCheckoutAfterCreate` variant here rather than being
//! folded into `CommonNote`. Its conflicted-files blocker IS the cross-op
//! template and reuses `CommonNote::ConflictedFiles { before:
//! OpPhrase::CheckingOutTheNewBranch }`; its untracked-files warning reuses
//! `CommonNote::UntrackedRemain { ctx: UntrackedCtx::AfterSwitchingBranches }`.

/// Plan notes for the worktree op family (create-branch+checkout,
/// create-worktree, unlock-worktree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeNote {
    /// blocker (`plan_create_branch_with_checkout`) — the working tree has
    /// staged/modified changes that the post-create checkout could clobber.
    DirtyBlocksCheckoutAfterCreate { parts: super::common::DirtyParts },
    /// blocker (`plan_create_worktree_impl`) — the branch is already checked
    /// out in a different worktree.
    BranchInOtherWorktree { branch: String, path: String },
    /// warning (`plan_create_worktree_impl`) — describes the linked worktree
    /// that will be created.
    CreatesLinkedWorktree {
        path: String,
        branch: String,
        start: String,
    },
    /// warning (`plan_unlock_worktree`) — the worktree is locked; surfaces the
    /// recorded reason (or notes that none was recorded).
    LockedWithReason { reason: Option<String> },
    /// blocker (`plan_unlock_worktree`) — the worktree is already unlocked
    /// (no-op family).
    AlreadyUnlocked { name: String },
    /// blocker (`plan_unlock_worktree`) — the lock state could not be read.
    LockStateUnreadable { name: String, err: String },
    /// blocker (`plan_unlock_worktree`) — the named worktree does not exist.
    WorktreeMissing { name: String },
    /// warning (`plan_create_worktree_impl`) — files matched by
    /// `.worktreeinclude` (and gitignored) that will be copied into the new
    /// worktree. `sample` holds the first few names; `more` is how many names
    /// are not shown (issue #339).
    IncludeCopy {
        count: usize,
        total_bytes: u64,
        sample: Vec<String>,
        more: usize,
    },
    /// warning (`plan_create_worktree_impl`) — matched symlinks that are
    /// skipped (symlinks are never copied; issue #339).
    IncludeSkippedSymlinks { count: usize },
    /// warning (`plan_create_worktree_impl`) — the matched set exceeds the
    /// copy-size cap; copy still proceeds (issue #339).
    IncludeOverCap { total_bytes: u64, cap_bytes: u64 },

    // ── issue #340: worktree lifecycle (remove / lock / prune / repair) ──
    /// blocker (`plan_remove_worktree`) — the target is the main worktree,
    /// which is never removable.
    RemoveMainRefused,
    /// blocker (`plan_remove_worktree`) — the worktree has uncommitted changes;
    /// removal is refused (kagi never forces).
    RemoveDirty { path: String, summary: String },
    /// blocker (`plan_remove_worktree`) — the worktree is locked; unlock it
    /// before removing (kagi never forces past a lock).
    RemoveLocked {
        path: String,
        reason: Option<String>,
    },
    /// warning (`plan_remove_worktree`) — describes the removal and whether the
    /// branch is kept or also deleted.
    RemovesWorktree {
        path: String,
        branch: Option<String>,
        delete_branch: bool,
    },
    /// warning (`plan_lock_worktree`) — describes the lock about to be placed.
    LocksWorktree {
        path: String,
        reason: Option<String>,
    },
    /// blocker (`plan_lock_worktree`) — the worktree is already locked.
    AlreadyLocked {
        name: String,
        reason: Option<String>,
    },
    /// warning (`plan_prune_worktrees`) — dry-run preview of the prunable
    /// worktrees kagi will prune. `sample` holds the first few paths; `more` is
    /// how many are not shown.
    PrunePreview {
        count: usize,
        sample: Vec<String>,
        more: usize,
    },
    /// blocker (`plan_prune_worktrees`) — nothing is prunable (no-op).
    PruneNothing,
    /// warning (`plan_repair_worktrees`) — describes what `git worktree repair`
    /// fixes (moved main / moved linked / both).
    RepairsWorktrees,

    // ── issue #341: typed post-create / pre-remove steps + trust ──
    /// warning (`plan_create_worktree_impl`) — the `post_create` steps from
    /// `.kagi/worktree.toml`, enumerated by type. `trust_required` is set when
    /// a `command` step is present and the config is not yet trusted, so
    /// confirming the plan doubles as the trust prompt (issue #341 §5).
    PostCreateSteps {
        steps: Vec<crate::worktree_steps::WorktreeStep>,
        trust_required: bool,
    },
    /// warning (`plan_remove_worktree`) — the `pre_remove` steps from
    /// `.kagi/worktree.toml`, enumerated by type. A failed or untrusted
    /// `command` here **aborts the removal** (issue #341 §5).
    PreRemoveSteps {
        steps: Vec<crate::worktree_steps::WorktreeStep>,
        trust_required: bool,
    },
}

/// Render a step list ("`  • copy: … → …`" per line) plus an optional trust
/// line. Shared by EN/JA so the enumeration format stays in one place.
pub fn worktree_steps_lines(
    steps: &[crate::worktree_steps::WorktreeStep],
    trust_required: bool,
    trust_line: &str,
) -> String {
    let mut out = String::new();
    for s in steps {
        out.push_str("\n  • ");
        out.push_str(&s.describe());
    }
    if trust_required {
        out.push('\n');
        out.push_str(trust_line);
    }
    out
}

impl WorktreeNote {
    /// Byte-identical to the legacy `ops/worktree.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            WorktreeNote::DirtyBlocksCheckoutAfterCreate { parts } => format!(
                "Working tree has {} — checkout after branch creation could lose work. Stash changes before continuing.",
                parts.parts_en()
            ),
            WorktreeNote::BranchInOtherWorktree { branch, path } => format!(
                "Branch '{}' is already checked out in another worktree: {}",
                branch, path
            ),
            WorktreeNote::CreatesLinkedWorktree {
                path,
                branch,
                start,
            } => format!(
                "Creates a linked worktree at '{}' with branch '{}' (start point {}).",
                path, branch, start
            ),
            WorktreeNote::LockedWithReason { reason } => {
                let reason_display = match reason {
                    Some(r) => format!("\"{}\"", r),
                    None => "(no reason recorded)".to_string(),
                };
                format!(
                    "Locked with reason: {} — a lock is deliberate protection someone \
                     placed on this worktree. Make sure it is no longer needed.",
                    reason_display
                )
            }
            WorktreeNote::AlreadyUnlocked { name } => {
                format!("Worktree '{}' is already unlocked.", name)
            }
            WorktreeNote::LockStateUnreadable { name, err } => format!(
                "Could not read the lock state of worktree '{}': {}",
                name, err
            ),
            WorktreeNote::WorktreeMissing { name } => {
                format!("Worktree '{}' does not exist.", name)
            }
            WorktreeNote::IncludeCopy {
                count,
                total_bytes,
                sample,
                more,
            } => {
                let mut names = sample.join(", ");
                if *more > 0 {
                    names = format!("{} (+{} more)", names, more);
                }
                format!(
                    "Copies {} .worktreeinclude file(s) ({}) into the new worktree: {}.",
                    count,
                    crate::worktree_include::human_bytes(*total_bytes),
                    names
                )
            }
            WorktreeNote::IncludeSkippedSymlinks { count } => format!(
                "Skips {} matched symlink(s) — symlinks are not copied.",
                count
            ),
            WorktreeNote::IncludeOverCap {
                total_bytes,
                cap_bytes,
            } => format!(
                ".worktreeinclude matches {}, over the {} copy cap — copy still proceeds but may be large (e.g. a node_modules match).",
                crate::worktree_include::human_bytes(*total_bytes),
                crate::worktree_include::human_bytes(*cap_bytes)
            ),
            WorktreeNote::RemoveMainRefused => {
                "This is the main worktree — it cannot be removed.".to_string()
            }
            WorktreeNote::RemoveDirty { path, summary } => format!(
                "Worktree '{}' has uncommitted changes ({}) — commit or stash them first (removal never forces).",
                path, summary
            ),
            WorktreeNote::RemoveLocked { path, reason } => {
                let reason_display = match reason {
                    Some(r) => format!("\"{}\"", r),
                    None => "(no reason recorded)".to_string(),
                };
                format!(
                    "Worktree '{}' is locked ({}) — unlock it before removing (kagi never forces).",
                    path, reason_display
                )
            }
            WorktreeNote::RemovesWorktree {
                path,
                branch,
                delete_branch,
            } => {
                let branch_display = branch.as_deref().unwrap_or("(detached HEAD)");
                if *delete_branch {
                    format!(
                        "Removes the linked worktree at '{}' and also deletes its branch '{}'.",
                        path, branch_display
                    )
                } else {
                    format!(
                        "Removes the linked worktree at '{}' — branch '{}' is kept.",
                        path, branch_display
                    )
                }
            }
            WorktreeNote::LocksWorktree { path, reason } => {
                let reason_display = match reason {
                    Some(r) => format!("\"{}\"", r),
                    None => "(no reason)".to_string(),
                };
                format!(
                    "Locks the worktree at '{}' with reason: {}.",
                    path, reason_display
                )
            }
            WorktreeNote::AlreadyLocked { name, reason } => {
                let reason_display = match reason {
                    Some(r) => format!("\"{}\"", r),
                    None => "(no reason recorded)".to_string(),
                };
                format!(
                    "Worktree '{}' is already locked ({}).",
                    name, reason_display
                )
            }
            WorktreeNote::PrunePreview {
                count,
                sample,
                more,
            } => {
                let mut names = sample.join(", ");
                if *more > 0 {
                    names = format!("{} (+{} more)", names, more);
                }
                format!(
                    "Prunes {} stale worktree admin entry(ies) whose working directory is gone: {}.",
                    count, names
                )
            }
            WorktreeNote::PruneNothing => {
                "No prunable worktrees — nothing to prune.".to_string()
            }
            WorktreeNote::RepairsWorktrees => {
                "Repairs worktree administrative links: fixes a moved main worktree, a moved \
                 linked worktree, or both. This never touches your files — only the .git links."
                    .to_string()
            }
            WorktreeNote::PostCreateSteps {
                steps,
                trust_required,
            } => format!(
                "Runs {} post-create step(s) from .kagi/worktree.toml:{}",
                steps.len(),
                worktree_steps_lines(
                    steps,
                    *trust_required,
                    "  ⚠ Confirming TRUSTS this config to run the command step(s) above \
                     (committed config is untrusted by default)."
                )
            ),
            WorktreeNote::PreRemoveSteps {
                steps,
                trust_required,
            } => format!(
                "Runs {} pre-remove step(s) from .kagi/worktree.toml (a failed or untrusted \
                 command aborts the removal):{}",
                steps.len(),
                worktree_steps_lines(
                    steps,
                    *trust_required,
                    "  ⚠ Confirming TRUSTS this config to run the command step(s) above \
                     (committed config is untrusted by default)."
                )
            ),
        }
    }
}

/// Plan titles for the worktree op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeTitle {
    /// `plan_create_branch_with_checkout` — `Create branch '<name>' @ <at> and
    /// checkout` (overrides the plain `BranchTitle::CreateBranch` title that
    /// `plan_create_branch` set).
    CreateBranchCheckout { name: String, at: String },
    /// `plan_create_worktree_impl` — `Create worktree '<branch>' @ <start>`.
    CreateWorktree { branch: String, start: String },
    /// `plan_unlock_worktree` — `Unlock worktree '<name>'`.
    UnlockWorktree { name: String },
    /// `plan_remove_worktree` — `Remove worktree '<name>'`.
    RemoveWorktree { name: String },
    /// `plan_lock_worktree` — `Lock worktree '<name>'`.
    LockWorktree { name: String },
    /// `plan_prune_worktrees` — `Prune stale worktrees`.
    PruneWorktrees,
    /// `plan_repair_worktrees` — `Repair worktree links`.
    RepairWorktrees,
}

impl WorktreeTitle {
    /// Byte-identical to the legacy strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            WorktreeTitle::CreateBranchCheckout { name, at } => {
                format!("Create branch '{}' @ {} and checkout", name, at)
            }
            WorktreeTitle::CreateWorktree { branch, start } => {
                format!("Create worktree '{}' @ {}", branch, start)
            }
            WorktreeTitle::UnlockWorktree { name } => format!("Unlock worktree '{}'", name),
            WorktreeTitle::RemoveWorktree { name } => format!("Remove worktree '{}'", name),
            WorktreeTitle::LockWorktree { name } => format!("Lock worktree '{}'", name),
            WorktreeTitle::PruneWorktrees => "Prune stale worktrees".to_string(),
            WorktreeTitle::RepairWorktrees => "Repair worktree links".to_string(),
        }
    }
}

/// Recovery kinds for the worktree op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeRecovery {
    /// `plan_create_branch_with_checkout` — remove the branch / switch back.
    CreateBranchCheckout { name: String, prev: String },
    /// `plan_create_worktree_impl` — remove the worktree / branch.
    CreateWorktree { path: String, branch: String },
    /// `plan_unlock_worktree` — re-lock if needed.
    Unlock { name: String },
    /// `plan_remove_worktree` — re-create the worktree if needed.
    RemoveWorktree {
        path: String,
        branch: Option<String>,
    },
    /// `plan_lock_worktree` — unlock if needed.
    LockWorktree { name: String },
    /// `plan_prune_worktrees` — prune only drops admin entries; re-add if needed.
    Prune,
    /// `plan_repair_worktrees` — repair is idempotent; re-run if needed.
    Repair,
}

impl WorktreeRecovery {
    /// Byte-identical to the legacy strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            WorktreeRecovery::CreateBranchCheckout { name, prev } => format!(
                "This creates branch '{}' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d {}\nTo return after checkout:\n  git checkout {}",
                name, name, prev
            ),
            WorktreeRecovery::CreateWorktree { path, branch } => format!(
                "Remove the linked worktree if needed:\n  git worktree remove {}\nThe branch can then be removed with:\n  git branch -d {}",
                path, branch
            ),
            WorktreeRecovery::Unlock { name } => format!(
                "Re-lock the worktree if needed:\n  git worktree lock --reason \"<why>\" <path-of-{}>",
                name
            ),
            WorktreeRecovery::RemoveWorktree { path, branch } => match branch {
                Some(b) => format!(
                    "Re-create the worktree if needed:\n  git worktree add {} {}",
                    path, b
                ),
                None => format!(
                    "Re-create the worktree if needed:\n  git worktree add {} <branch-or-commit>",
                    path
                ),
            },
            WorktreeRecovery::LockWorktree { name } => format!(
                "Unlock the worktree if needed:\n  git worktree unlock <path-of-{}>",
                name
            ),
            WorktreeRecovery::Prune => {
                "Prune only removes stale admin entries whose working directory is already gone. \
                 Re-create any worktree you still need with:\n  git worktree add <path> <branch>"
                    .to_string()
            }
            WorktreeRecovery::Repair => {
                "Repair is idempotent and only fixes .git links. If a link is still wrong, run it \
                 again from the main worktree:\n  git worktree repair [<path>...]"
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_note::common::DirtyParts;

    // ── message_en golden tests (ADR-0129 §3): dynamic values, quotes, and
    //    paths must render byte-identically to the legacy producer strings. ──

    #[test]
    fn dirty_blocks_checkout_after_create_staged_and_modified() {
        assert_eq!(
            WorktreeNote::DirtyBlocksCheckoutAfterCreate {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                }
            }
            .message_en(),
            "Working tree has 2 staged, 1 modified — checkout after branch creation could lose work. Stash changes before continuing."
        );
        assert_eq!(
            WorktreeNote::DirtyBlocksCheckoutAfterCreate {
                parts: DirtyParts {
                    staged: 0,
                    modified: 3
                }
            }
            .message_en(),
            "Working tree has 3 modified — checkout after branch creation could lose work. Stash changes before continuing."
        );
    }

    #[test]
    fn branch_in_other_worktree() {
        assert_eq!(
            WorktreeNote::BranchInOtherWorktree {
                branch: "feat/x".into(),
                path: "/repo/../wt one".into()
            }
            .message_en(),
            "Branch 'feat/x' is already checked out in another worktree: /repo/../wt one"
        );
    }

    #[test]
    fn creates_linked_worktree() {
        assert_eq!(
            WorktreeNote::CreatesLinkedWorktree {
                path: "/repo/../wt".into(),
                branch: "feat/x".into(),
                start: "a1b2c3d4".into()
            }
            .message_en(),
            "Creates a linked worktree at '/repo/../wt' with branch 'feat/x' (start point a1b2c3d4)."
        );
    }

    #[test]
    fn locked_with_reason_some_and_none() {
        assert_eq!(
            WorktreeNote::LockedWithReason {
                reason: Some("agent still running".into())
            }
            .message_en(),
            "Locked with reason: \"agent still running\" — a lock is deliberate protection someone placed on this worktree. Make sure it is no longer needed."
        );
        assert_eq!(
            WorktreeNote::LockedWithReason { reason: None }.message_en(),
            "Locked with reason: (no reason recorded) — a lock is deliberate protection someone placed on this worktree. Make sure it is no longer needed."
        );
    }

    #[test]
    fn already_unlocked() {
        assert_eq!(
            WorktreeNote::AlreadyUnlocked {
                name: "wt-free".into()
            }
            .message_en(),
            "Worktree 'wt-free' is already unlocked."
        );
    }

    #[test]
    fn lock_state_unreadable() {
        assert_eq!(
            WorktreeNote::LockStateUnreadable {
                name: "wt-x".into(),
                err: "corrupt lock file".into()
            }
            .message_en(),
            "Could not read the lock state of worktree 'wt-x': corrupt lock file"
        );
    }

    #[test]
    fn worktree_missing() {
        assert_eq!(
            WorktreeNote::WorktreeMissing {
                name: "no-such".into()
            }
            .message_en(),
            "Worktree 'no-such' does not exist."
        );
    }

    #[test]
    fn include_copy_with_and_without_more() {
        assert_eq!(
            WorktreeNote::IncludeCopy {
                count: 2,
                total_bytes: 1536,
                sample: vec![".env".into(), "config.local".into()],
                more: 0,
            }
            .message_en(),
            "Copies 2 .worktreeinclude file(s) (1.5 KiB) into the new worktree: .env, config.local."
        );
        assert_eq!(
            WorktreeNote::IncludeCopy {
                count: 5,
                total_bytes: 512,
                sample: vec![".env".into()],
                more: 4,
            }
            .message_en(),
            "Copies 5 .worktreeinclude file(s) (512 B) into the new worktree: .env (+4 more)."
        );
    }

    #[test]
    fn include_skipped_symlinks() {
        assert_eq!(
            WorktreeNote::IncludeSkippedSymlinks { count: 2 }.message_en(),
            "Skips 2 matched symlink(s) — symlinks are not copied."
        );
    }

    #[test]
    fn include_over_cap() {
        assert_eq!(
            WorktreeNote::IncludeOverCap {
                total_bytes: 200 * 1024 * 1024,
                cap_bytes: 100 * 1024 * 1024,
            }
            .message_en(),
            ".worktreeinclude matches 200.0 MiB, over the 100.0 MiB copy cap — copy still proceeds but may be large (e.g. a node_modules match)."
        );
    }

    #[test]
    fn create_branch_checkout_title() {
        assert_eq!(
            WorktreeTitle::CreateBranchCheckout {
                name: "feat/x".into(),
                at: "a1b2c3d4".into()
            }
            .message_en(),
            "Create branch 'feat/x' @ a1b2c3d4 and checkout"
        );
    }

    #[test]
    fn create_worktree_title() {
        assert_eq!(
            WorktreeTitle::CreateWorktree {
                branch: "feat/x".into(),
                start: "a1b2c3d4".into()
            }
            .message_en(),
            "Create worktree 'feat/x' @ a1b2c3d4"
        );
    }

    #[test]
    fn unlock_worktree_title() {
        assert_eq!(
            WorktreeTitle::UnlockWorktree {
                name: "wt-x".into()
            }
            .message_en(),
            "Unlock worktree 'wt-x'"
        );
    }

    #[test]
    fn create_branch_checkout_recovery() {
        assert_eq!(
            WorktreeRecovery::CreateBranchCheckout {
                name: "feat/x".into(),
                prev: "main".into()
            }
            .message_en(),
            "This creates branch 'feat/x' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d feat/x\nTo return after checkout:\n  git checkout main"
        );
    }

    #[test]
    fn create_worktree_recovery() {
        assert_eq!(
            WorktreeRecovery::CreateWorktree {
                path: "/repo/../wt".into(),
                branch: "feat/x".into()
            }
            .message_en(),
            "Remove the linked worktree if needed:\n  git worktree remove /repo/../wt\nThe branch can then be removed with:\n  git branch -d feat/x"
        );
    }

    // ── issue #340 lifecycle variants ──
    #[test]
    fn remove_lifecycle_messages() {
        assert_eq!(
            WorktreeNote::RemoveMainRefused.message_en(),
            "This is the main worktree — it cannot be removed."
        );
        assert_eq!(
            WorktreeNote::RemoveDirty {
                path: "/wt/x".into(),
                summary: "2 modified".into()
            }
            .message_en(),
            "Worktree '/wt/x' has uncommitted changes (2 modified) — commit or stash them first (removal never forces)."
        );
        assert_eq!(
            WorktreeNote::RemovesWorktree {
                path: "/wt/x".into(),
                branch: Some("feat/x".into()),
                delete_branch: false
            }
            .message_en(),
            "Removes the linked worktree at '/wt/x' — branch 'feat/x' is kept."
        );
        assert_eq!(
            WorktreeNote::RemovesWorktree {
                path: "/wt/x".into(),
                branch: Some("feat/x".into()),
                delete_branch: true
            }
            .message_en(),
            "Removes the linked worktree at '/wt/x' and also deletes its branch 'feat/x'."
        );
    }

    #[test]
    fn lock_prune_repair_messages() {
        assert_eq!(
            WorktreeNote::LocksWorktree {
                path: "/wt/x".into(),
                reason: Some("agent running".into())
            }
            .message_en(),
            "Locks the worktree at '/wt/x' with reason: \"agent running\"."
        );
        assert_eq!(
            WorktreeNote::AlreadyLocked {
                name: "wt-x".into(),
                reason: None
            }
            .message_en(),
            "Worktree 'wt-x' is already locked ((no reason recorded))."
        );
        assert_eq!(
            WorktreeNote::PrunePreview {
                count: 2,
                sample: vec!["/wt/a".into(), "/wt/b".into()],
                more: 0
            }
            .message_en(),
            "Prunes 2 stale worktree admin entry(ies) whose working directory is gone: /wt/a, /wt/b."
        );
        assert_eq!(
            WorktreeNote::PruneNothing.message_en(),
            "No prunable worktrees — nothing to prune."
        );
    }

    #[test]
    fn unlock_recovery() {
        assert_eq!(
            WorktreeRecovery::Unlock {
                name: "wt-x".into()
            }
            .message_en(),
            "Re-lock the worktree if needed:\n  git worktree lock --reason \"<why>\" <path-of-wt-x>"
        );
    }
}
