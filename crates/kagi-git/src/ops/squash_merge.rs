//! Squash-merge detection by patch-id equivalence.
//!
//! Two entry points over the same idea:
//!
//! - [`squash_merged_as`] — one branch, on demand. `plan_delete_branch` uses it
//!   so a squash-merged branch (which looks like a dead-end leaf in the graph)
//!   can still be deleted.
//! - [`collect_squash_links`] — every branch at once, for the graph's ghost
//!   connectors. Running the single-branch scan per branch would be N×M tree
//!   diffs; this inverts the loop into one patch-id index over the candidate
//!   window plus one diff per branch, i.e. N+M.

use std::collections::HashMap;

use git2::Repository;

// ────────────────────────────────────────────────────────────
// Squash-merge detection
// ────────────────────────────────────────────────────────────

/// How many commits of `base..head` to examine when looking for the squash
/// commit. A squash lands at (or very near) the tip of the target branch, so
/// this is generous; the cap only stops a pathological walk on a branch that
/// forked years ago.
const SQUASH_SCAN_LIMIT: usize = 500;

/// The diff a whole branch introduces, as a patch-id: `merge_base(a, tip) → tip`.
fn combined_patch_id(
    repo: &Repository,
    base: git2::Oid,
    tip: git2::Oid,
) -> Result<git2::Oid, git2::Error> {
    let base_tree = repo.find_commit(base)?.tree()?;
    let tip_tree = repo.find_commit(tip)?.tree()?;
    repo.diff_tree_to_tree(Some(&base_tree), Some(&tip_tree), None)?
        .patchid(None)
}

/// The diff one single-parent commit introduces, as a patch-id.
fn commit_patch_id(repo: &Repository, commit: &git2::Commit) -> Result<git2::Oid, git2::Error> {
    let parent_tree = commit.parent(0)?.tree()?;
    repo.diff_tree_to_tree(Some(&parent_tree), Some(&commit.tree()?), None)?
        .patchid(None)
}

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
    let tip_commit = repo.find_commit(tip_oid)?;
    let want = combined_patch_id(repo, base, tip_oid)?;

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
        if commit_patch_id(repo, &commit)? == want {
            return Ok(Some(oid));
        }
    }
    Ok(None)
}

// ────────────────────────────────────────────────────────────
// Whole-repo scan — the graph's ghost connectors
// ────────────────────────────────────────────────────────────

/// One proven squash merge: the branch whose change was replayed, its tip, and
/// the commit on the target that replayed it. Hex OIDs, so the pure UI layer
/// can match them against `CommitId` without touching git2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SquashLink {
    pub branch: String,
    /// The branch tip — the dead-end leaf in the graph.
    pub tip: String,
    /// The commit that already carries the identical change.
    pub squash: String,
}

/// Every local branch that is squash-merged into HEAD, paired with the commit
/// that carries its change.
///
/// The loop is inverted relative to [`squash_merged_as`] on purpose. Asking it
/// per branch costs `branches × SQUASH_SCAN_LIMIT` tree diffs — measured at
/// ~15s on a 1100-commit repo with 24 stale branches, far too slow to sit on
/// any refresh path. Indexing the window's patch-ids once and then doing one
/// diff per branch costs `SQUASH_SCAN_LIMIT + branches`, ~0.8s for the same
/// repo. Still a background job (the caller runs it off the UI thread), but a
/// cacheable one.
///
/// Errors are swallowed per branch: a link that cannot be proven is simply not
/// drawn.
pub fn collect_squash_links(repo: &Repository) -> Result<Vec<SquashLink>, git2::Error> {
    let Ok(head_oid) = repo.head().and_then(|h| h.peel_to_commit()).map(|c| c.id()) else {
        return Ok(Vec::new());
    };

    // Candidates: local branches whose tip is NOT reachable from HEAD. A
    // reachable tip is plainly merged and the graph already connects it.
    let mut candidates: Vec<(String, git2::Oid)> = Vec::new();
    for br in repo.branches(Some(git2::BranchType::Local))? {
        let (br, _) = br?;
        let (Some(name), Some(tip)) = (br.name()?.map(str::to_string), br.get().target()) else {
            continue;
        };
        if tip == head_oid || repo.graph_descendant_of(head_oid, tip).unwrap_or(false) {
            continue;
        }
        candidates.push((name, tip));
    }
    if candidates.is_empty() {
        // Nothing to look for — skip building the index entirely.
        return Ok(Vec::new());
    }

    // The oldest candidate bounds the walk: a squash commit is always created
    // after the work it replays, so nothing older than that can be one.
    let oldest = candidates
        .iter()
        .filter_map(|(_, tip)| repo.find_commit(*tip).ok().map(|c| c.time().seconds()))
        .min()
        .unwrap_or(i64::MIN);

    // patch-id → (commit, time). Newest wins: the revwalk yields newest first,
    // and a re-applied change should point at the most recent copy.
    let mut index: HashMap<git2::Oid, (git2::Oid, i64)> = HashMap::new();
    let mut walk = repo.revwalk()?;
    walk.push(head_oid)?;
    let mut scanned = 0usize;
    for oid in walk {
        if scanned >= SQUASH_SCAN_LIMIT {
            break;
        }
        let Ok(commit) = repo.find_commit(oid?) else {
            continue;
        };
        if commit.parent_count() != 1 || commit.time().seconds() < oldest {
            continue;
        }
        scanned += 1;
        if let Ok(pid) = commit_patch_id(repo, &commit) {
            index
                .entry(pid)
                .or_insert((commit.id(), commit.time().seconds()));
        }
    }

    let mut links = Vec::new();
    for (branch, tip) in candidates {
        let Ok(base) = repo.merge_base(head_oid, tip) else {
            continue;
        };
        let Ok(want) = combined_patch_id(repo, base, tip) else {
            continue;
        };
        let tip_time = repo
            .find_commit(tip)
            .map(|c| c.time().seconds())
            .unwrap_or(0);
        if let Some((squash, time)) = index.get(&want) {
            if *time >= tip_time {
                links.push(SquashLink {
                    branch,
                    tip: tip.to_string(),
                    squash: squash.to_string(),
                });
            }
        }
    }
    Ok(links)
}
