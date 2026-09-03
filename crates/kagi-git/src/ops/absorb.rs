//! Absorb operation pipeline (issue #345, ADR-0151).
//!
//! `absorb` folds each uncommitted working-tree hunk into the **mutable**
//! ancestor commit that last touched those lines, leaving ambiguous hunks in
//! the working tree. It is the `git-absorb` / `jj absorb` idea, self-implemented
//! on git2's blame API (PM §5: no vendored code, no new dependency).
//!
//! # The triple
//!
//! - [`plan_absorb`] — blame every hunk's deleted lines, filter to mutable
//!   targets, build the distribution table ([`AbsorbPlan`]).
//! - [`preflight_absorb`] — refuse if HEAD moved, a target is no longer mutable,
//!   or a merge commit sits in the rebuild range.
//! - [`execute_absorb`] — rebuild the affected slice of history **in memory**
//!   (git2 `apply_to_tree` per commit), then move the branch ref last. The
//!   working tree is never touched, so kept hunks simply remain uncommitted.
//! - [`verify_absorb`] — confirm the branch tip is the rebuilt commit.
//!
//! # Safety
//!
//! No destructive command is used. Absorb only rewrites **unpushed** commits on
//! the current (non-protected) branch, exactly like amend (ADR-0143). The
//! rebuild is a chain of fresh commit objects; nothing is deleted (the old
//! commits stay reachable via the reflog). The branch ref is moved with a
//! reflog-logged `reference(...)`, the same ref-order rule amend/undo follow.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use super::*;
use git2::{ApplyOptions, DiffOptions, Oid, Patch};
use kagi_domain::absorb::{
    AbsorbBlocker, AbsorbOutcome, AbsorbPlan, AbsorbTarget, HunkAssignment, HunkDisposition,
    KeepReason,
};
use kagi_domain::status::WorkingTreeStatus;

/// Default size of the mutable window (how many commits back from HEAD count as
/// candidate targets). Configurable by the caller; PM §5 default is 10.
pub const DEFAULT_ABSORB_WINDOW: usize = 10;

/// Identifies one hunk during the rebuild: `(file, old_start, old_lines,
/// new_start)`. Matches a freshly recomputed diff hunk back to a plan row.
type HunkKey = (String, u32, u32, u32);

/// A candidate commit on HEAD's first-parent chain within the window.
struct Candidate {
    depth: usize,
    mutable: bool,
    target: AbsorbTarget,
}

/// Walk HEAD's first-parent chain up to `window` commits, classifying each as a
/// mutable absorb target or not. A commit is mutable when it is NOT pushed
/// (unreachable from the branch upstream, mirroring amend/undo's judgment,
/// ADR-0143) AND is not a merge commit. Returns the candidates keyed by oid.
fn collect_candidates(
    repo: &Repository,
    head_commit: &git2::Commit<'_>,
    branch: Option<&str>,
    window: usize,
) -> Result<HashMap<Oid, Candidate>, GitError> {
    // Upstream tip (if any) — commits reachable from it are "pushed".
    let upstream_oid = branch.and_then(|b| {
        repo.find_branch(b, BranchType::Local)
            .ok()
            .and_then(|br| br.upstream().ok())
            .and_then(|up| up.get().target())
    });

    let mut map = HashMap::new();
    let mut cur = Some(head_commit.clone());
    let mut depth = 0usize;
    while let Some(commit) = cur {
        if depth >= window {
            break;
        }
        let oid = commit.id();
        let is_merge = commit.parent_count() > 1;
        let pushed = match upstream_oid {
            Some(up) => up == oid || repo.graph_descendant_of(up, oid).unwrap_or(false),
            None => false, // no upstream → local-only → never pushed
        };
        let signed = repo.extract_signature(&oid, None).is_ok();
        let subject = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("(no message)")
            .chars()
            .take(72)
            .collect::<String>();
        map.insert(
            oid,
            Candidate {
                depth,
                mutable: !is_merge && !pushed,
                target: AbsorbTarget {
                    oid: oid.to_string(),
                    short: oid.to_string().chars().take(8).collect(),
                    subject,
                    signed,
                },
            },
        );
        // First-parent only (linear history assumption for v1).
        cur = commit.parent(0).ok();
        depth += 1;
    }
    Ok(map)
}

/// Zero-context is wrong for later re-application, so we use default context and
/// blame only the DELETED lines of each hunk (precise attribution without the
/// surrounding context muddying the blame). Returns `None` for a pure-addition
/// hunk (no deleted lines to blame).
fn deleted_linenos(patch: &Patch<'_>, hunk_idx: usize) -> Result<Vec<u32>, GitError> {
    let n = patch.num_lines_in_hunk(hunk_idx).map_err(|e| {
        GitError::Other(format!(
            "num_lines_in_hunk({hunk_idx}) failed: {}",
            e.message()
        ))
    })?;
    let mut out = Vec::new();
    for l in 0..n {
        let line = patch.line_in_hunk(hunk_idx, l).map_err(|e| {
            GitError::Other(format!(
                "line_in_hunk({hunk_idx},{l}) failed: {}",
                e.message()
            ))
        })?;
        if line.origin() == '-' {
            if let Some(ln) = line.old_lineno() {
                out.push(ln);
            }
        }
    }
    Ok(out)
}

/// Build the HEAD-tree → working-tree diff that absorb reasons about. Default
/// context (so hunks relocate cleanly when re-applied to older trees), tracked
/// files only (an untracked file has no ancestor to absorb into).
fn absorb_diff<'a>(
    repo: &'a Repository,
    head_tree: &git2::Tree<'_>,
) -> Result<git2::Diff<'a>, GitError> {
    let mut opts = DiffOptions::new();
    opts.include_untracked(false);
    repo.diff_tree_to_workdir(Some(head_tree), Some(&mut opts))
        .map_err(|e| GitError::Other(format!("diff_tree_to_workdir failed: {}", e.message())))
}

/// Are there staged changes (index differs from HEAD)? Absorb operates on
/// unstaged hunks only in v1, so staged content is a blocker.
fn staged_count(repo: &Repository, head_tree: &git2::Tree<'_>) -> usize {
    repo.diff_tree_to_index(Some(head_tree), None, None)
        .map(|d| d.deltas().len())
        .unwrap_or(0)
}

/// Content fingerprint of the absorb diff (#417): file paths + each hunk's
/// old/new coordinates + every line's origin and bytes, in the diff's
/// deterministic enumeration order. The distribution table is reasoned against
/// exactly these hunks at exactly these coordinates, so any post-plan edit
/// (even one that only shifts line numbers) changes this value. `preflight`
/// compares it to `plan.worktree_digest` and refuses on mismatch.
fn diff_content_digest(diff: &git2::Diff<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    for delta_idx in 0..diff.deltas().len() {
        let patch = match Patch::from_diff(diff, delta_idx) {
            Ok(Some(p)) => p,
            _ => continue,
        };
        let delta = patch.delta();
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        path.hash(&mut h);
        for hi in 0..patch.num_hunks() {
            if let Ok((hunk, _)) = patch.hunk(hi) {
                hunk.old_start().hash(&mut h);
                hunk.old_lines().hash(&mut h);
                hunk.new_start().hash(&mut h);
                hunk.new_lines().hash(&mut h);
            }
            let nl = patch.num_lines_in_hunk(hi).unwrap_or(0);
            for l in 0..nl {
                if let Ok(line) = patch.line_in_hunk(hi, l) {
                    (line.origin() as u8).hash(&mut h);
                    line.content().hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// Analyse the working tree and build the absorb distribution table.
pub fn plan_absorb(repo: &Repository, window: usize) -> Result<AbsorbPlan, GitError> {
    let head = resolve_head(repo)?;
    let status: WorkingTreeStatus = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };

    let mut blockers: Vec<AbsorbBlocker> = Vec::new();

    // Structural blockers.
    let (head_commit, branch) = match &head {
        Head::Attached { target, branch } => {
            let oid = Oid::from_str(target)
                .map_err(|e| GitError::Other(format!("HEAD oid parse failed: {}", e.message())))?;
            let commit = repo.find_commit(oid).map_err(|e| {
                GitError::Other(format!("HEAD commit lookup failed: {}", e.message()))
            })?;
            (Some(commit), Some(branch.clone()))
        }
        Head::Detached { .. } => {
            blockers.push(AbsorbBlocker::DetachedHead);
            (None, None)
        }
        Head::Unborn { .. } => {
            blockers.push(AbsorbBlocker::UnbornHead);
            (None, None)
        }
    };

    if !status.conflicted.is_empty() {
        blockers.push(AbsorbBlocker::Conflicted {
            count: status.conflicted.len(),
        });
    }
    if let Some(b) = &branch {
        if kagi_domain::refs::is_protected_branch(b) {
            blockers.push(AbsorbBlocker::ProtectedBranch { branch: b.clone() });
        }
    }

    let head_commit = match head_commit {
        Some(c) => c,
        None => {
            return Ok(AbsorbPlan {
                current,
                branch,
                head_at_plan: String::new(),
                window,
                worktree_digest: 0,
                assignments: Vec::new(),
                blockers,
            })
        }
    };

    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;

    let staged = staged_count(repo, &head_tree);
    if staged > 0 {
        blockers.push(AbsorbBlocker::StagedChanges { count: staged });
    }

    // Candidate ancestors + blame per hunk.
    let candidates = collect_candidates(repo, &head_commit, branch.as_deref(), window)?;
    let diff = absorb_diff(repo, &head_tree)?;

    // Blame cache: file path → Blame, blamed at HEAD.
    let head_oid = head_commit.id();
    let mut blame_cache: HashMap<String, Option<git2::Blame<'_>>> = HashMap::new();

    let mut assignments: Vec<HunkAssignment> = Vec::new();
    for delta_idx in 0..diff.deltas().len() {
        let patch = match Patch::from_diff(&diff, delta_idx) {
            Ok(Some(p)) => p,
            _ => continue, // binary / no textual patch
        };
        let delta = patch.delta();
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(PathBuf::from)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        for h in 0..patch.num_hunks() {
            let (hunk, _) = patch
                .hunk(h)
                .map_err(|e| GitError::Other(format!("patch.hunk failed: {}", e.message())))?;
            let old_range = (hunk.old_start(), hunk.old_lines());
            let new_range = (hunk.new_start(), hunk.new_lines());

            let disposition = classify_hunk(
                repo,
                &path,
                &patch,
                h,
                head_oid,
                &candidates,
                &mut blame_cache,
            )?;
            assignments.push(HunkAssignment {
                file: path.clone(),
                old_range,
                new_range,
                disposition,
            });
        }
    }

    if !assignments.iter().any(|a| a.is_absorbed()) && blockers.is_empty() {
        blockers.push(AbsorbBlocker::NothingToAbsorb);
    }

    // #417: fingerprint the exact hunks this plan was built from so preflight can
    // refuse if the working tree changes before execute.
    let worktree_digest = diff_content_digest(&diff);

    Ok(AbsorbPlan {
        current,
        branch,
        head_at_plan: head_oid.to_string(),
        window,
        worktree_digest,
        assignments,
        blockers,
    })
}

/// Decide where one hunk goes: blame its deleted lines, require a single owner
/// that is a mutable candidate.
fn classify_hunk<'r>(
    repo: &'r Repository,
    path: &str,
    patch: &Patch<'_>,
    hunk_idx: usize,
    head_oid: Oid,
    candidates: &HashMap<Oid, Candidate>,
    blame_cache: &mut HashMap<String, Option<git2::Blame<'r>>>,
) -> Result<HunkDisposition, GitError> {
    let deleted = deleted_linenos(patch, hunk_idx)?;
    if deleted.is_empty() {
        return Ok(HunkDisposition::Keep(KeepReason::PureAddition));
    }

    // Blame the file at HEAD (cached).
    let blame = blame_cache.entry(path.to_string()).or_insert_with(|| {
        let mut opts = git2::BlameOptions::new();
        opts.newest_commit(head_oid);
        repo.blame_file(std::path::Path::new(path), Some(&mut opts))
            .ok()
    });
    let blame = match blame {
        Some(b) => b,
        None => return Ok(HunkDisposition::Keep(KeepReason::Ambiguous)),
    };

    let mut owners: Vec<Oid> = Vec::new();
    for ln in deleted {
        if let Some(bh) = blame.get_line(ln as usize) {
            let owner = bh.final_commit_id();
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        }
    }

    if owners.len() != 1 {
        // 0 (blame miss) or >1 (split across commits) → ambiguous.
        return Ok(HunkDisposition::Keep(KeepReason::Ambiguous));
    }
    let owner = owners[0];
    match candidates.get(&owner) {
        Some(c) if c.mutable => Ok(HunkDisposition::Absorb(c.target.clone())),
        // Owned by a real commit, but pushed / merge / outside window.
        _ => Ok(HunkDisposition::Keep(KeepReason::Immutable)),
    }
}

/// Re-validate the plan against the live repo just before execution.
pub fn preflight_absorb(repo: &Repository, plan: &AbsorbPlan) -> Result<(), GitError> {
    if plan.has_blockers() {
        return Err(GitError::Other(
            "absorb plan has blockers; refusing to execute".to_string(),
        ));
    }
    if plan.is_noop() {
        return Err(GitError::Other(
            "absorb plan has nothing to absorb".to_string(),
        ));
    }

    let head = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head.is_branch() {
        return Err(GitError::Other("HEAD is not on a branch".to_string()));
    }
    let head_oid = head
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target".to_string()))?;
    if head_oid.to_string() != plan.head_at_plan {
        return Err(GitError::Other(
            "HEAD moved since the plan was built (stale plan)".to_string(),
        ));
    }

    // Re-derive depths and re-check every target is still a mutable candidate,
    // and that no merge commit sits in the rebuild range.
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let branch = head.shorthand().ok().map(|s| s.to_string());
    let candidates = collect_candidates(repo, &head_commit, branch.as_deref(), plan.window)?;

    let mut max_depth = 0usize;
    for a in plan.absorbed() {
        let target = a.target().unwrap();
        let oid = Oid::from_str(&target.oid)
            .map_err(|e| GitError::Other(format!("target oid parse failed: {}", e.message())))?;
        match candidates.get(&oid) {
            Some(c) if c.mutable => max_depth = max_depth.max(c.depth),
            Some(_) => {
                return Err(GitError::Other(format!(
                    "target {} is no longer mutable (pushed/merge)",
                    target.short
                )))
            }
            None => {
                return Err(GitError::Other(format!(
                    "target {} is no longer within the mutable window",
                    target.short
                )))
            }
        }
    }

    // #417: pin the working tree. The plan's distribution table (and its
    // reported counts) were reasoned against a specific set of hunks at specific
    // line coordinates. If the tree changed since — an edit shifts line numbers,
    // a hunk is added/removed, or content was staged — refuse rather than move
    // the branch ref while silently mis-absorbing hunks whose coords no longer
    // match. The caller must re-plan.
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;
    if staged_count(repo, &head_tree) > 0 {
        return Err(GitError::Other(
            "staged changes appeared since the plan was built; re-plan absorb".to_string(),
        ));
    }
    let diff = absorb_diff(repo, &head_tree)?;
    if diff_content_digest(&diff) != plan.worktree_digest {
        return Err(GitError::Other(
            "working tree changed since the plan was built (stale absorb plan); re-plan"
                .to_string(),
        ));
    }

    // No merge commit may sit in [HEAD .. oldest_target].
    let mut cur = Some(head_commit);
    let mut depth = 0usize;
    while let Some(c) = cur {
        if depth > max_depth {
            break;
        }
        if c.parent_count() > 1 {
            return Err(GitError::Other(format!(
                "merge commit {} in absorb range; refusing",
                &c.id().to_string()[..8]
            )));
        }
        cur = c.parent(0).ok();
        depth += 1;
    }
    Ok(())
}

/// Rebuild the affected slice of history in memory, folding each absorbed hunk
/// into its target commit, then move the branch ref. Returns the outcome.
pub fn execute_absorb(repo: &Repository, plan: &AbsorbPlan) -> Result<AbsorbOutcome, GitError> {
    preflight_absorb(repo, plan)?;

    let head = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let branch_refname = head
        .name()
        .map_err(|e| GitError::Other(format!("HEAD ref name missing: {}", e.message())))?
        .to_string();
    let head_oid = head
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;

    // Chain HEAD..window with depths.
    let mut chain: Vec<git2::Commit<'_>> = Vec::new();
    {
        let mut cur = Some(head_commit.clone());
        let mut depth = 0usize;
        while let Some(c) = cur {
            if depth >= plan.window {
                break;
            }
            chain.push(c.clone());
            cur = c.parent(0).ok();
            depth += 1;
        }
    }
    let depth_of: HashMap<String, usize> = chain
        .iter()
        .enumerate()
        .map(|(d, c)| (c.id().to_string(), d))
        .collect();

    // hunk key (old_start, old_lines, new_start) + file → target depth.
    let mut hunk_depth: HashMap<HunkKey, usize> = HashMap::new();
    let mut max_depth = 0usize;
    for a in plan.absorbed() {
        let t = a.target().unwrap();
        let d = *depth_of
            .get(&t.oid)
            .ok_or_else(|| GitError::Other(format!("target {} not on HEAD chain", t.short)))?;
        max_depth = max_depth.max(d);
        hunk_depth.insert(
            (a.file.clone(), a.old_range.0, a.old_range.1, a.new_range.0),
            d,
        );
    }
    let hunk_depth = Rc::new(hunk_depth);
    // #417: count what is ACTUALLY absorbed, not what the plan predicted. Each
    // hunk-callback that returns `true` records its key here; the set dedups the
    // repeated applications a hunk gets as its change propagates forward through
    // the rebuilt chain, so `len()` is the number of distinct hunks folded in.
    let applied_keys: Rc<std::cell::RefCell<HashSet<HunkKey>>> =
        Rc::new(std::cell::RefCell::new(HashSet::new()));

    // base = parent of the oldest target — `None` when the oldest target is the
    // root commit (it stays a root in the rebuilt history).
    let oldest = &chain[max_depth];
    let base: Option<git2::Commit<'_>> = oldest.parent(0).ok();

    // Recompute the diff (same inputs → same hunk coords as the plan).
    let diff = absorb_diff(repo, &head_tree)?;

    let committer = build_signature(repo)?;
    let mut new_parent: Option<git2::Commit<'_>> = base;

    // Rebuild oldest → newest. commit at depth d gets its own tree plus every
    // absorbed hunk whose target depth ≥ d (ancestor changes propagate forward).
    for d in (0..=max_depth).rev() {
        let ci = &chain[d];
        let ci_tree = ci
            .tree()
            .map_err(|e| GitError::Other(format!("tree lookup failed: {}", e.message())))?;

        let current_path: Rc<std::cell::RefCell<String>> =
            Rc::new(std::cell::RefCell::new(String::new()));
        let cp_delta = current_path.clone();
        let cp_hunk = current_path.clone();
        let hd = hunk_depth.clone();
        let applied = applied_keys.clone();

        let mut apply_opts = ApplyOptions::new();
        apply_opts.delta_callback(move |delta| {
            let p = delta
                .and_then(|d| d.new_file().path().or_else(|| d.old_file().path()))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            *cp_delta.borrow_mut() = p;
            true
        });
        apply_opts.hunk_callback(move |hunk| {
            let hunk = match hunk {
                Some(h) => h,
                None => return false,
            };
            let key = (
                cp_hunk.borrow().clone(),
                hunk.old_start(),
                hunk.old_lines(),
                hunk.new_start(),
            );
            let take = matches!(hd.get(&key), Some(&td) if td >= d);
            if take {
                applied.borrow_mut().insert(key);
            }
            take
        });

        let new_index = repo
            .apply_to_tree(&ci_tree, &diff, Some(&mut apply_opts))
            .map_err(|e| {
                GitError::Other(format!(
                    "apply_to_tree failed at depth {d}: {} (absorb aborted; no ref moved)",
                    e.message()
                ))
            })?;
        let mut new_index = new_index;
        let new_tree_oid = new_index
            .write_tree_to(repo)
            .map_err(|e| GitError::Other(format!("write_tree_to failed: {}", e.message())))?;
        let new_tree = repo
            .find_tree(new_tree_oid)
            .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

        let author = ci.author();
        let message = ci.message_raw().unwrap_or("(no message)");
        let parents: Vec<&git2::Commit<'_>> = new_parent.iter().collect();
        let new_oid = repo
            .commit(None, &author, &committer, message, &new_tree, &parents)
            .map_err(|e| GitError::Other(format!("commit rebuild failed: {}", e.message())))?;
        new_parent = Some(
            repo.find_commit(new_oid)
                .map_err(|e| GitError::Other(format!("find_commit failed: {}", e.message())))?,
        );
    }

    let new_head = new_parent
        .ok_or_else(|| GitError::Other("absorb produced no commit".to_string()))?
        .id();
    let log_msg = format!(
        "absorb: {} {} -> {}",
        branch_refname,
        &head_oid.to_string()[..8],
        &new_head.to_string()[..8],
    );
    repo.reference(&branch_refname, new_head, true, &log_msg)
        .map_err(|e| {
            GitError::Other(format!(
                "branch ref update (absorb) failed: {}",
                e.message()
            ))
        })?;

    // Sync the index to the rewritten HEAD **without touching the working tree**
    // (a `reset --mixed`, never `--hard`): the absorbed hunks now live in HEAD so
    // they must leave the index, while the kept hunks stay as unstaged working-
    // tree changes. Reading the tree into the index does exactly that.
    let new_head_tree = repo
        .find_commit(new_head)
        .and_then(|c| c.tree())
        .map_err(|e| GitError::Other(format!("new HEAD tree lookup failed: {}", e.message())))?;
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    index
        .read_tree(&new_head_tree)
        .map_err(|e| GitError::Other(format!("index.read_tree failed: {}", e.message())))?;
    index
        .write()
        .map_err(|e| GitError::Other(format!("index.write failed: {}", e.message())))?;

    // #417: build the outcome from what was actually applied, so the reported
    // counts can never disagree with reality. (With the preflight digest guard
    // in place these equal the plan's predictions, but deriving them from the
    // applied set makes that a fact, not an assumption.)
    let applied = applied_keys.borrow();
    let absorbed_hunks = applied.len();
    let kept_hunks = plan.assignments.len() - absorbed_hunks;
    let mut depths: Vec<usize> = applied
        .iter()
        .filter_map(|k| hunk_depth.get(k).copied())
        .collect();
    depths.sort_unstable();
    depths.dedup();

    Ok(AbsorbOutcome {
        new_head: new_head.to_string(),
        absorbed_hunks,
        kept_hunks,
        targets_rewritten: depths.len(),
    })
}

/// Confirm the branch tip now points at the rebuilt commit.
pub fn verify_absorb(repo: &Repository, outcome: &AbsorbOutcome) -> Result<(), GitError> {
    let head_oid = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .ok_or_else(|| GitError::Other("HEAD has no target after absorb".to_string()))?;
    if head_oid.to_string() != outcome.new_head {
        return Err(GitError::Other(
            "HEAD does not match rebuilt commit after absorb".to_string(),
        ));
    }
    Ok(())
}
