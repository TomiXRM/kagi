//! CleanupNote — ADR-0129 appendix §B-10 (merged-branch cleanup, ADR-0128).
//!
//! Every template in `plan_delete_merged_branches`
//! (`crates/kagi-git/src/ops/branch_cleanup.rs`) is op-specific — none of it
//! reuses `CommonNote` (verified against source; no shared shapes here).

/// Plan notes for the branch-cleanup op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupNote {
    /// blocker — no branches were selected for deletion.
    NoSelection,
    /// blocker — the target branch dropped out of the fresh collect (gone, or
    /// reclassified to a non-deletable class) since the list was built.
    NoLongerCandidate { name: String },
    /// blocker — the target branch is present but not safely deletable (it
    /// may have grown new commits since merge).
    NotSafelyDeletable { name: String },
    /// blocker — the target's local or remote tip moved since the list was
    /// built.
    TipMoved { name: String },
    /// warning — the selection includes a `[gone]`-heuristic (squash-merge
    /// likely) branch with no local proof of the merge.
    SquashHeuristicOnly,
    /// warning — the selection includes a remote half; deletion writes to
    /// `origin` over the network.
    RemoteDeleteNetwork,
}

impl CleanupNote {
    /// Sole English renderer (byte-identical to the legacy `branch_cleanup.rs`
    /// producer strings).
    pub fn message_en(&self) -> String {
        match self {
            CleanupNote::NoSelection => "No branches selected for deletion.".to_string(),
            CleanupNote::NoLongerCandidate { name } => format!(
                "Branch '{}' is no longer a cleanup candidate. Refresh the list.",
                name
            ),
            CleanupNote::NotSafelyDeletable { name } => format!(
                "Branch '{}' is not safely deletable (it may have grown new commits since merge). Refresh the list.",
                name
            ),
            CleanupNote::TipMoved { name } => format!(
                "Branch '{}' moved since the list was built. Refresh the list.",
                name
            ),
            CleanupNote::SquashHeuristicOnly => {
                "Some branches are only *likely* squash-merged (upstream gone); there is no local proof of the merge."
                    .to_string()
            }
            CleanupNote::RemoteDeleteNetwork => {
                "Remote branches on 'origin' will be deleted (network write).".to_string()
            }
        }
    }
}

/// Plan titles for the branch-cleanup op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupTitle {
    /// `plan_delete_merged_branches` — `Delete <count> merged branch(es)`.
    CleanupDelete { count: usize },
}

impl CleanupTitle {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            CleanupTitle::CleanupDelete { count } => {
                format!("Delete {} merged branch(es)", count)
            }
        }
    }
}

/// Recovery kinds for the branch-cleanup op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupRecovery {
    /// `plan_delete_merged_branches` — every deleted tip OID is recorded in
    /// the oplog; the two restore commands are TEMPLATES (`<name>`/`<oid>`
    /// placeholders), not filled in per-branch.
    CleanupDelete,
}

impl CleanupRecovery {
    /// Sole English renderer (byte-identical to the legacy string).
    pub fn message_en(&self) -> String {
        match self {
            CleanupRecovery::CleanupDelete => {
                "Every deleted tip OID is recorded in the oplog. To restore:\n  \
                 git branch <name> <oid>          (local)\n  \
                 git push origin <oid>:refs/heads/<name>   (remote)"
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): dynamic values and quotes
    //    must render byte-identically to the legacy producer strings. ──

    #[test]
    fn no_selection() {
        assert_eq!(
            CleanupNote::NoSelection.message_en(),
            "No branches selected for deletion."
        );
    }

    #[test]
    fn no_longer_candidate() {
        assert_eq!(
            CleanupNote::NoLongerCandidate {
                name: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' is no longer a cleanup candidate. Refresh the list."
        );
    }

    #[test]
    fn not_safely_deletable() {
        assert_eq!(
            CleanupNote::NotSafelyDeletable {
                name: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' is not safely deletable (it may have grown new commits since merge). Refresh the list."
        );
    }

    #[test]
    fn tip_moved() {
        assert_eq!(
            CleanupNote::TipMoved {
                name: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' moved since the list was built. Refresh the list."
        );
    }

    #[test]
    fn squash_heuristic_only() {
        assert_eq!(
            CleanupNote::SquashHeuristicOnly.message_en(),
            "Some branches are only *likely* squash-merged (upstream gone); there is no local proof of the merge."
        );
    }

    #[test]
    fn remote_delete_network() {
        assert_eq!(
            CleanupNote::RemoteDeleteNetwork.message_en(),
            "Remote branches on 'origin' will be deleted (network write)."
        );
    }

    #[test]
    fn cleanup_delete_title_singular_and_plural() {
        assert_eq!(
            CleanupTitle::CleanupDelete { count: 1 }.message_en(),
            "Delete 1 merged branch(es)"
        );
        assert_eq!(
            CleanupTitle::CleanupDelete { count: 3 }.message_en(),
            "Delete 3 merged branch(es)"
        );
    }

    #[test]
    fn cleanup_delete_recovery() {
        assert_eq!(
            CleanupRecovery::CleanupDelete.message_en(),
            "Every deleted tip OID is recorded in the oplog. To restore:\n  git branch <name> <oid>          (local)\n  git push origin <oid>:refs/heads/<name>   (remote)"
        );
    }
}
