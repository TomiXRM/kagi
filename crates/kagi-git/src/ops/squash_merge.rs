//! Squash-merge detection by patch-id equivalence.
//!
//! Split out of `branch.rs` — used by `plan_delete_branch` so a branch that
//! was squash-merged (and therefore looks like a dead-end leaf in the graph)
//! can still be deleted.

use git2::Repository;

// ────────────────────────────────────────────────────────────
// Squash-merge detection
// ────────────────────────────────────────────────────────────

/// How many commits of `base..head` to examine when looking for the squash
/// commit. A squash lands at (or very near) the tip of the target branch, so
/// this is generous; the cap only stops a pathological walk on a branch that
/// forked years ago.
const SQUASH_SCAN_LIMIT: usize = 500;

/// Whether `tip` was **squash-merged** into `head`.
///
/// A squash merge replays the branch's whole diff as one new commit, so the
/// branch tip never becomes an ancestor of the target — `graph_descendant_of`
/// says "unmerged" and the commit sits in the graph as a dead-end leaf, which
/// is exactly what a user sees after `gh pr merge --squash` (user report).
///
/// The proof is git's own patch-id equivalence, the same idea as `git cherry`:
/// diff `merge_base(head, tip) → tip` is the change the branch introduces. If
/// some single-parent commit in `base..head` introduces a diff with an
/// identical patch-id, that change is already in `head` and deleting the
/// branch loses nothing.
///
/// Returns the squash commit's OID, or `None` — never an error — when anything
/// cannot be resolved: this only ever *unblocks* a delete, so an inconclusive
/// answer must stay "not merged".
///
// ponytail: squash only. A "rebase and merge" replays each commit separately,
// so the combined patch-id won't match; detecting that means matching every
// commit of `base..tip` individually (N×M diffs). Add it if anyone asks.
pub fn squash_merged_as(
    repo: &Repository,
    tip_oid: git2::Oid,
    head_oid: git2::Oid,
) -> Option<git2::Oid> {
    squash_merged_as_inner(repo, tip_oid, head_oid).unwrap_or(None)
}

fn squash_merged_as_inner(
    repo: &Repository,
    tip_oid: git2::Oid,
    head_oid: git2::Oid,
) -> Result<Option<git2::Oid>, git2::Error> {
    let base = repo.merge_base(head_oid, tip_oid)?;
    if base == tip_oid {
        // Already an ancestor — the plain reachability check covers this.
        return Ok(None);
    }
    let base_tree = repo.find_commit(base)?.tree()?;
    let tip_commit = repo.find_commit(tip_oid)?;
    let want = repo
        .diff_tree_to_tree(Some(&base_tree), Some(&tip_commit.tree()?), None)?
        .patchid(None)?;

    // The squash commit is created after the branch's last commit, so anything
    // older cannot be it. Cheap prune before the expensive per-commit diffs.
    let earliest = tip_commit.time().seconds();

    let mut walk = repo.revwalk()?;
    walk.push(head_oid)?;
    walk.hide(base)?;
    let mut scanned = 0usize;
    for oid in walk {
        let oid = oid?;
        if scanned >= SQUASH_SCAN_LIMIT {
            break;
        }
        let commit = repo.find_commit(oid)?;
        // Merge commits have no single "the change it introduces".
        if commit.parent_count() != 1 || commit.time().seconds() < earliest {
            continue;
        }
        scanned += 1;
        let parent_tree = commit.parent(0)?.tree()?;
        let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&commit.tree()?), None)?;
        if diff.patchid(None)? == want {
            return Ok(Some(oid));
        }
    }
    Ok(None)
}
