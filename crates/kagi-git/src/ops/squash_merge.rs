//! Squash-merge detection by change equivalence.
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
//!
//! # Why patch-id is a filter and not the answer
//!
//! `git patch-id` **strips whitespace**. Two diffs that differ only in
//! indentation hash the same — verified: indenting a line by four spaces and
//! by two tabs both produce patch-id `62d419e8…`. In Python that is a
//! behaviour change, and `git branch -d` correctly refuses to delete such a
//! branch. A tool whose reason to exist is being safer than git must not be
//! looser than git on an irreversible delete that has no `-D` escape hatch.
//!
//! So patch-id is used only as a cheap index, and every hit is then confirmed
//! byte-exactly by [`exact_change_key`]. The confirmation runs once per hit,
//! not once per candidate, so it costs nothing measurable.

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

/// Hard bound on the revwalk itself. `SQUASH_SCAN_LIMIT` only counts the
/// commits that reach the diff, so a filter that rejects most of them would
/// otherwise let the traversal run to the root of the history for free.
const SQUASH_WALK_LIMIT: usize = 20_000;

/// The tree-to-tree diff between two commits.
fn tree_diff<'r>(
    repo: &'r Repository,
    from: git2::Oid,
    to: git2::Oid,
) -> Result<git2::Diff<'r>, git2::Error> {
    let from_tree = repo.find_commit(from)?.tree()?;
    let to_tree = repo.find_commit(to)?.tree()?;
    repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)
}

/// Patch-id of a diff, refusing an **empty** one.
///
/// An empty diff hashes to a fixed value, so without this guard every net-zero
/// branch (added a file then removed it; experimented then reverted) matches
/// every `--allow-empty` CI-retrigger commit — and each other. Two unrelated
/// branches were observed being declared squash-merged by the same unrelated
/// commit. Nothing is lost by deleting such a branch, but the claim is false
/// and the named squash commit is nonsense, so refuse to make it.
fn patch_id_of(diff: &git2::Diff) -> Result<git2::Oid, git2::Error> {
    if diff.deltas().len() == 0 {
        return Err(git2::Error::from_str(
            "empty diff has no meaningful patch-id",
        ));
    }
    diff.patchid(None)
}

/// A byte-exact fingerprint of the change a diff makes.
///
/// Deliberately *not* the raw patch text: that carries `index abc..def` blob
/// OIDs and hunk line numbers, which differ whenever the target branch moved
/// between the fork point and the squash — the normal case. This keeps what
/// patch-id keeps (paths, and every `+`/`-`/context line) and drops what it
/// drops, minus the one thing that makes patch-id unsafe here: it does not
/// strip whitespace.
fn exact_change_key(diff: &git2::Diff) -> Result<Vec<u8>, git2::Error> {
    let mut key: Vec<u8> = Vec::new();
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            if let Some(p) = file.path() {
                key.extend_from_slice(p.to_string_lossy().as_bytes());
            }
            key.push(0);
        }
    }
    let mut lines: Vec<u8> = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        if matches!(line.origin(), '+' | '-' | ' ') {
            lines.push(line.origin() as u8);
            lines.extend_from_slice(line.content());
        }
        true
    })?;
    key.extend_from_slice(&lines);
    Ok(key)
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
    let branch_diff = tree_diff(repo, base, tip_oid)?;
    let want = patch_id_of(&branch_diff)?;
    let want_exact = exact_change_key(&branch_diff)?;

    // `hide(base)` already restricts the walk to commits the branch does not
    // contain, so no commit from before the fork point can be reached — which
    // is why this needs no timestamp prune. It used to have one, and it made
    // the whole feature stop working after a `git commit --amend` or a rebase
    // pushed the tip's committer time past the squash commit's.
    let mut walk = repo.revwalk()?;
    walk.push(head_oid)?;
    walk.hide(base)?;
    let mut scanned = 0usize;
    let mut visited = 0usize;
    for oid in walk {
        let oid = oid?;
        // Two bounds: `scanned` caps the expensive diffs, `visited` caps the
        // walk itself. Without the second, a filter that rejects nearly
        // everything lets the traversal run to the root of a million-commit
        // history for free.
        if scanned >= SQUASH_SCAN_LIMIT || visited >= SQUASH_WALK_LIMIT {
            break;
        }
        visited += 1;
        let commit = repo.find_commit(oid)?;
        // Merge commits have no single "the change it introduces".
        if commit.parent_count() != 1 {
            continue;
        }
        scanned += 1;
        let candidate = tree_diff(repo, commit.parent(0)?.id(), oid)?;
        // patch-id first (cheap, whitespace-blind), then the byte-exact
        // confirmation that makes this safe to gate a delete on.
        if patch_id_of(&candidate)? == want && exact_change_key(&candidate)? == want_exact {
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

    // patch-id → squash commit. Newest wins: the revwalk yields newest first,
    // and a change applied more than once should point at the latest copy.
    //
    // No timestamp prune here either — see `squash_merged_as_inner`. This walk
    // cannot `hide(base)` (each candidate has its own base), so the ancestry
    // check at accept time below is what keeps a pre-fork commit from
    // matching.
    let mut index: HashMap<git2::Oid, git2::Oid> = HashMap::new();
    let mut walk = repo.revwalk()?;
    walk.push(head_oid)?;
    let mut scanned = 0usize;
    let mut visited = 0usize;
    for oid in walk {
        if scanned >= SQUASH_SCAN_LIMIT || visited >= SQUASH_WALK_LIMIT {
            break;
        }
        visited += 1;
        let Ok(commit) = repo.find_commit(oid?) else {
            continue;
        };
        if commit.parent_count() != 1 {
            continue;
        }
        scanned += 1;
        let Ok(parent) = commit.parent(0) else {
            continue;
        };
        let Ok(diff) = tree_diff(repo, parent.id(), commit.id()) else {
            continue;
        };
        if let Ok(pid) = patch_id_of(&diff) {
            index.entry(pid).or_insert(commit.id());
        }
    }

    let mut links = Vec::new();
    for (branch, tip) in candidates {
        let Ok(base) = repo.merge_base(head_oid, tip) else {
            continue;
        };
        let Ok(branch_diff) = tree_diff(repo, base, tip) else {
            continue;
        };
        let (Ok(want), Ok(want_exact)) =
            (patch_id_of(&branch_diff), exact_change_key(&branch_diff))
        else {
            continue;
        };
        let Some(&squash) = index.get(&want) else {
            continue;
        };
        // The squash must be *newer* than the fork point. Reachability says so
        // exactly; a timestamp comparison only approximates it, and gets the
        // answer wrong after an amend, a rebase, or clock skew between
        // machines.
        if repo.graph_descendant_of(base, squash).unwrap_or(true) {
            continue;
        }
        // Confirm byte-exactly before claiming these are the same change.
        let Ok(parent) = repo.find_commit(squash).and_then(|c| c.parent(0)) else {
            continue;
        };
        let matches_exactly = tree_diff(repo, parent.id(), squash)
            .and_then(|d| exact_change_key(&d))
            .is_ok_and(|k| k == want_exact);
        if matches_exactly {
            links.push(SquashLink {
                branch,
                tip: tip.to_string(),
                squash: squash.to_string(),
            });
        }
    }
    Ok(links)
}
