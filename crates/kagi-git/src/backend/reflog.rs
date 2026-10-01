//! Ref reads for the Operation Log (#334, ADR-0214): reflog lines in a time
//! span (slice 1's estimate) and the ref snapshot `Backend::run` diffs around
//! an execution (slice 2a's record). Read-only: nothing here writes a ref or
//! touches the oplog.

use super::Backend;
use crate::GitError;
use git2::Repository;
use kagi_domain::oplog_reflog::ReflogLine;
use kagi_domain::ref_moves::RefSnapshot;

/// HEAD of the worktree `repo` is (its symbolic target and commit) and every
/// `refs/heads/*`. Other worktrees' HEADs are not read: a branch they hold
/// moving shows in `refs/heads/*`. `None` when the refs cannot be read — the
/// caller then records nothing rather than a wrong difference.
pub(super) fn ref_snapshot(repo: &Repository) -> Option<RefSnapshot> {
    let head = repo.find_reference("HEAD").ok()?;
    let head_symbolic = head.symbolic_target().ok().flatten().map(str::to_string);
    // An unborn branch has a symbolic target but nothing to resolve.
    let head_oid = head
        .resolve()
        .ok()
        .and_then(|r| r.target())
        .map(|o| o.to_string());
    let mut branches = std::collections::BTreeMap::new();
    for reference in repo.references_glob("refs/heads/*").ok()? {
        let reference = reference.ok()?;
        let (Ok(name), Some(oid)) = (reference.name(), reference.target()) else {
            continue;
        };
        branches.insert(name.to_string(), oid.to_string());
    }
    Some(RefSnapshot {
        head_oid,
        head_symbolic,
        branches,
    })
}

impl Backend {
    /// HEAD's and every local branch's reflog lines with a signature time in
    /// `(after, until]`, unsorted. This repository's HEAD is the worktree's
    /// own (`logs/HEAD` is per worktree); branch reflogs are shared.
    pub fn reflog_lines_between(
        &self,
        after: i64,
        until: i64,
    ) -> Result<Vec<ReflogLine>, GitError> {
        reflog_lines_between(&self.repo, after, until)
    }
}

fn reflog_lines_between(
    repo: &Repository,
    after: i64,
    until: i64,
) -> Result<Vec<ReflogLine>, GitError> {
    let mut refnames = vec!["HEAD".to_string()];
    let branches = repo
        .references_glob("refs/heads/*")
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    refnames.extend(
        branches
            .flatten()
            .filter_map(|r| r.name().ok().map(str::to_string)),
    );
    let mut lines = Vec::new();
    for refname in refnames {
        // A ref without a reflog has nothing to show.
        let Ok(reflog) = repo.reflog(&refname) else {
            continue;
        };
        for entry in reflog.iter() {
            let time = entry.committer().when().seconds();
            if time <= after || time > until {
                continue;
            }
            lines.push(ReflogLine {
                refname: refname.clone(),
                old: entry.id_old().to_string(),
                new: entry.id_new().to_string(),
                message: entry.message().ok().flatten().unwrap_or("").to_string(),
                time,
            });
        }
    }
    Ok(lines)
}
