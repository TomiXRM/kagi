//! Merged-branch cleanup classification — ADR-0128
//!
//! Pure decision logic for the Branch Cleanup table: given per-branch facts
//! computed by the git backend (reachability, merge-commit walk results,
//! upstream state), classify each branch into a merged class and decide
//! which delete affordances it gets. No `git2`, no I/O — the backend reduces
//! the repository to [`BranchCleanupInput`] and everything after that is
//! unit-tested here.
//!
//! The three merged classes and their treatment (ADR-0128):
//!
//! | Class | Meaning | Delete |
//! |---|---|---|
//! | `FullyMerged` | tip is an ancestor of main | bulk + individual |
//! | `SquashMergedLikely` | tip not an ancestor, but upstream is `[gone]` | individual only |
//! | `MergedThenGrown` | a merged ancestor exists but the tip grew past it | none (WARN) |
//!
//! Staleness (no commits for [`STALE_THRESHOLD_SECS`]) is orthogonal to the
//! merged class: a stale branch is *shown* even when `NotMerged`, but being
//! stale never enables deletion.

use crate::commit::CommitId;

/// 90 days — a branch whose tip commit is older than this is shown as stale.
/// Fixed in v1 (ADR-0128 non-goal: no settings UI for the threshold yet).
pub const STALE_THRESHOLD_SECS: i64 = 90 * 24 * 60 * 60;

/// Per-branch facts computed by the git backend (`kagi-git`), reduced to plain
/// data so classification stays pure.
///
/// A logical branch is the union of a local branch and its same-named remote
/// counterpart; at least one of `local_tip` / `remote_tip` is `Some`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCleanupInput {
    /// Short branch name, e.g. `"feature/x"` (no `origin/` prefix).
    pub name: String,
    /// Local branch tip, if a local branch of this name exists.
    pub local_tip: Option<CommitId>,
    /// Remote-tracking tip (`origin/<name>`), if it exists.
    pub remote_tip: Option<CommitId>,
    /// True when HEAD is attached to this branch — always excluded.
    pub is_current: bool,
    /// True for the default branch (main) — always excluded.
    pub is_default: bool,
    /// True when the branch tip is an ancestor of (reachable from) main.
    pub tip_is_ancestor_of_main: bool,
    /// Merge-commit timestamp (Unix seconds) from walking main's first-parent
    /// history, when a merge commit whose second parent equals the tip was
    /// found. `None` for fast-forward merges (caller may fall back to the tip
    /// commit date for display) and for squash merges.
    pub merged_at: Option<i64>,
    /// `Some(n)` when a merge commit in main merged an *ancestor* of the tip,
    /// but the tip itself is not an ancestor of main — i.e. the branch was
    /// merged and then grew `n` commits (the develop pattern, ADR-0128 WARN).
    pub grown_ahead: Option<usize>,
    /// True when the local branch has an upstream configured whose remote
    /// branch no longer exists (`[gone]`) — the squash-merge heuristic.
    pub upstream_gone: bool,
    /// Commit time (Unix seconds) of the branch tip, for staleness.
    pub tip_committed_at: i64,
}

/// Merged classification of one branch (ADR-0128 decision table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergedBranchStatus {
    /// Tip is an ancestor of main — deleting loses nothing.
    FullyMerged,
    /// Upstream is `[gone]`, so a PR was probably squash-merged. No local
    /// proof, so: individual delete only, never bulk.
    SquashMergedLikely,
    /// Merged at some point, but `ahead` commits grew on top since. Shown
    /// with a WARN badge; no delete affordance at all.
    MergedThenGrown { ahead: usize },
    /// No evidence of a merge. Only listed when stale.
    NotMerged,
}

/// One row of the Branch Cleanup table, ready for the UI to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCleanupRow {
    /// Short branch name.
    pub name: String,
    /// Local branch tip, if present (drives the "local" chip and local delete).
    pub local_tip: Option<CommitId>,
    /// Remote tip, if present (drives the "origin" chip and remote delete).
    pub remote_tip: Option<CommitId>,
    /// Merged classification.
    pub status: MergedBranchStatus,
    /// Merge timestamp for display, when known (see [`BranchCleanupInput::merged_at`]).
    pub merged_at: Option<i64>,
    /// True when the tip commit is older than [`STALE_THRESHOLD_SECS`].
    pub stale: bool,
    /// True when the row gets an enabled per-row delete button.
    pub deletable: bool,
    /// True when the row is included in the bulk "delete N merged branches"
    /// action (strictly `FullyMerged` rows).
    pub bulk_deletable: bool,
}

/// One branch selected for deletion, with the tip OIDs captured at plan time.
/// The execute step re-verifies these OIDs right before deleting (local via
/// git2, remote via `ls-remote`) so a branch that moved after planning is
/// refused instead of silently deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupDeleteTarget {
    /// Short branch name.
    pub name: String,
    /// Local tip at plan time; `Some` means "delete the local branch".
    pub local_tip: Option<CommitId>,
    /// `origin/<name>` tip at plan time; `Some` means "delete the remote branch".
    pub remote_tip: Option<CommitId>,
    /// Class at plan time (`FullyMerged` targets get an ancestor re-check at
    /// execute; `SquashMergedLikely` targets rely on the OID checks alone).
    pub status: MergedBranchStatus,
}

/// One successfully deleted branch. The tip OIDs are what the oplog records
/// for recovery: `git branch <name> <local_tip>` / push `<remote_tip>` back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupDeleted {
    /// Short branch name.
    pub name: String,
    /// Local tip at deletion time, when a local branch was deleted.
    pub local_tip: Option<CommitId>,
    /// Remote tip at deletion time, when the remote branch was deleted.
    pub remote_tip: Option<CommitId>,
}

/// Aggregate outcome of one execute run. Each branch's deletion is
/// independent, so per-branch failures do not abort the run — they are
/// collected in `failed` and surfaced via oplog + modal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CleanupOutcome {
    /// Branches that were deleted (locally, remotely, or both).
    pub deleted: Vec<CleanupDeleted>,
    /// `(branch, reason)` for branches that were refused or failed.
    pub failed: Vec<(String, String)>,
}

impl BranchCleanupRow {
    /// The delete target for this row, or `None` for rows without a delete
    /// affordance (`MergedThenGrown` / stale-only) — so a caller physically
    /// cannot build a deletion out of a WARN row.
    pub fn delete_target(&self) -> Option<CleanupDeleteTarget> {
        if !self.deletable {
            return None;
        }
        Some(CleanupDeleteTarget {
            name: self.name.clone(),
            local_tip: self.local_tip.clone(),
            remote_tip: self.remote_tip.clone(),
            status: self.status.clone(),
        })
    }
}

/// Classify one branch per the ADR-0128 decision table.
///
/// Order matters: a branch can be both `[gone]` and grown (squash-merged PR,
/// remote deleted, then new local commits) — `MergedThenGrown` must win so the
/// WARN protects the new work.
pub fn classify(input: &BranchCleanupInput) -> MergedBranchStatus {
    if input.tip_is_ancestor_of_main {
        MergedBranchStatus::FullyMerged
    } else if let Some(ahead) = input.grown_ahead {
        MergedBranchStatus::MergedThenGrown { ahead }
    } else if input.upstream_gone {
        MergedBranchStatus::SquashMergedLikely
    } else {
        MergedBranchStatus::NotMerged
    }
}

/// True when a tip committed at `tip_committed_at` counts as stale at `now`
/// (both Unix seconds).
pub fn is_stale(tip_committed_at: i64, now: i64) -> bool {
    now.saturating_sub(tip_committed_at) > STALE_THRESHOLD_SECS
}

/// Build the cleanup table: classify, filter, and sort.
///
/// - The current branch and the default branch never appear.
/// - A row appears when its class is not `NotMerged`, **or** it is stale
///   (stale-only rows are display-only).
/// - Sort: class group (`FullyMerged` → `SquashMergedLikely` →
///   `MergedThenGrown` → stale-only), then newest `merged_at` first within a
///   group, then name for a stable order.
pub fn build_rows(inputs: &[BranchCleanupInput], now: i64) -> Vec<BranchCleanupRow> {
    let mut rows: Vec<BranchCleanupRow> = inputs
        .iter()
        .filter(|i| !i.is_current && !i.is_default)
        .filter_map(|i| {
            let status = classify(i);
            let stale = is_stale(i.tip_committed_at, now);
            if status == MergedBranchStatus::NotMerged && !stale {
                return None;
            }
            let deletable = matches!(
                status,
                MergedBranchStatus::FullyMerged | MergedBranchStatus::SquashMergedLikely
            );
            let bulk_deletable = status == MergedBranchStatus::FullyMerged;
            Some(BranchCleanupRow {
                name: i.name.clone(),
                local_tip: i.local_tip.clone(),
                remote_tip: i.remote_tip.clone(),
                status,
                merged_at: i.merged_at,
                stale,
                deletable,
                bulk_deletable,
            })
        })
        .collect();

    rows.sort_by(|a, b| {
        group_rank(a)
            .cmp(&group_rank(b))
            .then_with(|| b.merged_at.cmp(&a.merged_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

fn group_rank(row: &BranchCleanupRow) -> u8 {
    match row.status {
        MergedBranchStatus::FullyMerged => 0,
        MergedBranchStatus::SquashMergedLikely => 1,
        MergedBranchStatus::MergedThenGrown { .. } => 2,
        MergedBranchStatus::NotMerged => 3,
    }
}

/// Newline-joined branch names for the "copy all names" button (ADR-0128).
pub fn copy_all_text(rows: &[BranchCleanupRow]) -> String {
    rows.iter()
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const FRESH: i64 = NOW - 24 * 60 * 60; // 1 day old
    const OLD: i64 = NOW - 100 * 24 * 60 * 60; // 100 days old

    fn input(name: &str) -> BranchCleanupInput {
        BranchCleanupInput {
            name: name.to_string(),
            local_tip: Some(CommitId("a".repeat(40))),
            remote_tip: None,
            is_current: false,
            is_default: false,
            tip_is_ancestor_of_main: false,
            merged_at: None,
            grown_ahead: None,
            upstream_gone: false,
            tip_committed_at: FRESH,
        }
    }

    #[test]
    fn ancestor_tip_is_fully_merged() {
        let mut i = input("f");
        i.tip_is_ancestor_of_main = true;
        assert_eq!(classify(&i), MergedBranchStatus::FullyMerged);
    }

    #[test]
    fn gone_upstream_is_squash_merged_likely() {
        let mut i = input("f");
        i.upstream_gone = true;
        assert_eq!(classify(&i), MergedBranchStatus::SquashMergedLikely);
    }

    #[test]
    fn grown_ahead_is_merged_then_grown() {
        let mut i = input("develop");
        i.grown_ahead = Some(3);
        assert_eq!(
            classify(&i),
            MergedBranchStatus::MergedThenGrown { ahead: 3 }
        );
    }

    #[test]
    fn grown_wins_over_gone() {
        // Squash-merged PR, remote branch deleted, then new local commits:
        // the WARN class must win so the new work is protected.
        let mut i = input("develop");
        i.upstream_gone = true;
        i.grown_ahead = Some(2);
        assert_eq!(
            classify(&i),
            MergedBranchStatus::MergedThenGrown { ahead: 2 }
        );
    }

    #[test]
    fn ancestor_wins_over_everything() {
        let mut i = input("f");
        i.tip_is_ancestor_of_main = true;
        i.upstream_gone = true;
        i.grown_ahead = Some(1); // backend wouldn't produce this, but the table is total
        assert_eq!(classify(&i), MergedBranchStatus::FullyMerged);
    }

    #[test]
    fn no_evidence_is_not_merged() {
        assert_eq!(classify(&input("f")), MergedBranchStatus::NotMerged);
    }

    #[test]
    fn staleness_threshold() {
        assert!(is_stale(OLD, NOW));
        assert!(!is_stale(FRESH, NOW));
        // Exactly at the threshold is not yet stale (strict >).
        assert!(!is_stale(NOW - STALE_THRESHOLD_SECS, NOW));
    }

    #[test]
    fn current_and_default_are_always_excluded() {
        let mut current = input("feature");
        current.tip_is_ancestor_of_main = true;
        current.is_current = true;
        let mut default = input("main");
        default.tip_is_ancestor_of_main = true;
        default.is_default = true;
        assert!(build_rows(&[current, default], NOW).is_empty());
    }

    #[test]
    fn not_merged_fresh_branch_is_hidden() {
        assert!(build_rows(&[input("wip")], NOW).is_empty());
    }

    #[test]
    fn not_merged_stale_branch_is_shown_but_not_deletable() {
        let mut i = input("dead");
        i.tip_committed_at = OLD;
        let rows = build_rows(&[i], NOW);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, MergedBranchStatus::NotMerged);
        assert!(rows[0].stale);
        assert!(!rows[0].deletable);
        assert!(!rows[0].bulk_deletable);
    }

    #[test]
    fn delete_affordances_per_class() {
        let mut full = input("full");
        full.tip_is_ancestor_of_main = true;
        let mut squash = input("squash");
        squash.upstream_gone = true;
        let mut grown = input("grown");
        grown.grown_ahead = Some(5);

        let rows = build_rows(&[full, squash, grown], NOW);
        let by_name = |n: &str| rows.iter().find(|r| r.name == n).unwrap();

        assert!(by_name("full").deletable);
        assert!(by_name("full").bulk_deletable);
        assert!(by_name("squash").deletable);
        assert!(!by_name("squash").bulk_deletable);
        assert!(!by_name("grown").deletable);
        assert!(!by_name("grown").bulk_deletable);
    }

    #[test]
    fn rows_sort_by_class_then_merged_at_then_name() {
        let mut old_merge = input("old-merge");
        old_merge.tip_is_ancestor_of_main = true;
        old_merge.merged_at = Some(NOW - 1000);
        let mut new_merge = input("new-merge");
        new_merge.tip_is_ancestor_of_main = true;
        new_merge.merged_at = Some(NOW - 10);
        let mut squash = input("a-squash");
        squash.upstream_gone = true;
        let mut grown = input("develop");
        grown.grown_ahead = Some(1);
        let mut dead = input("dead");
        dead.tip_committed_at = OLD;

        let rows = build_rows(&[dead, squash, old_merge, grown, new_merge], NOW);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["new-merge", "old-merge", "a-squash", "develop", "dead"]
        );
    }

    #[test]
    fn warn_rows_cannot_produce_a_delete_target() {
        let mut grown = input("develop");
        grown.grown_ahead = Some(4);
        let mut dead = input("dead");
        dead.tip_committed_at = OLD;
        let mut full = input("full");
        full.tip_is_ancestor_of_main = true;

        let rows = build_rows(&[grown, dead, full], NOW);
        let targets: Vec<_> = rows.iter().filter_map(|r| r.delete_target()).collect();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "full");
        assert_eq!(targets[0].status, MergedBranchStatus::FullyMerged);
    }

    #[test]
    fn copy_all_joins_names_with_newlines() {
        let mut a = input("a");
        a.tip_is_ancestor_of_main = true;
        let mut b = input("b");
        b.tip_is_ancestor_of_main = true;
        let rows = build_rows(&[a, b], NOW);
        assert_eq!(copy_all_text(&rows), "a\nb");
    }
}
