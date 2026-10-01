//! Reflog lines in a time span — the Operation Log panel's "what moved during
//! this operation" (#334 slice 1, ADR-0214). Read-only: opening a reflog never
//! writes, and nothing here touches the oplog.

use super::Backend;
use crate::GitError;
use git2::Repository;
use kagi_domain::oplog_reflog::ReflogLine;

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
