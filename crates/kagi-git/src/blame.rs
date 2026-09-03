//! Line-level blame backend (issue #350, ADR-0162).
//!
//! Attributes each line of a file to the commit that last touched it, using
//! git2's in-process [`Repository::blame_file`] — matching Kagi's
//! single-git2-backend design (no shell-out; `file_history.rs` shells out for
//! the *commit-level* log, this is the *line-level* companion).
//!
//! # `.git-blame-ignore-revs`
//!
//! git2's blame API has no native ignore-revs support, so we detect a
//! repository-root `.git-blame-ignore-revs`, parse it with the pure
//! [`kagi_domain::blame::parse_blame_ignore_revs`], and apply the set here by
//! **marking** every line whose attributed commit is listed. This directly
//! answers the "a bulk reformat makes every line blame to one commit" problem:
//! those lines are flagged and counted rather than silently trusted.
//!
//! v1 marks + counts ignored lines; **full re-attribution** past an ignored
//! commit (walking to the commit before it) is a documented follow-up, not v1.
//!
//! # Algorithm note
//!
//! We use git2's default diff algorithm. `git blame --diff-algorithm=histogram`
//! (git 2.53+) reduces mis-attribution of moved code, but it is a CLI flag with
//! no git2 equivalent; adopting it is future work shared with ownership
//! (ADR-0119).

use std::collections::HashSet;
use std::path::Path;

use git2::{BlameOptions, Repository};

pub use kagi_domain::blame::{
    parse_blame_ignore_revs, BlameLine, BlameResult, IGNORED_MARK, UNBLAMABLE_MARK,
};

use super::GitError;

/// The conventional file name, at the repository root, that lists revisions to
/// ignore when blaming (git's `blame.ignoreRevsFile` default location).
pub const IGNORE_REVS_FILE: &str = ".git-blame-ignore-revs";

/// Blame `repo_relative_path` against HEAD, auto-detecting and respecting a
/// repository-root `.git-blame-ignore-revs`.
///
/// Each returned [`BlameLine`] carries the commit / author / time / summary of
/// the commit that last touched it. Lines attributed to a commit listed in the
/// ignore-revs file are marked (`ignored = true`); [`BlameResult::ignored_revs`]
/// reports how many distinct ignored commits actually took effect in this file.
///
/// # Errors
///
/// [`GitError::Other`] on any libgit2 failure (e.g. the path is untracked or
/// binary). An absent ignore-revs file is **not** an error.
pub fn blame_file(repo: &Repository, repo_relative_path: &Path) -> Result<BlameResult, GitError> {
    let ignore_set = load_ignore_revs(repo);

    let mut opts = BlameOptions::new();
    let blame = repo
        .blame_file(repo_relative_path, Some(&mut opts))
        .map_err(|e| GitError::Other(format!("blame failed: {}", e.message())))?;

    let mut lines: Vec<BlameLine> = Vec::new();
    let mut ignored_commits: HashSet<String> = HashSet::new();

    for hunk in blame.iter() {
        let commit_oid = hunk.final_commit_id();
        let unblamable = commit_oid.is_zero();
        let full = commit_oid.to_string();
        let ignored = !unblamable && is_ignored(&full, &ignore_set);
        if ignored {
            ignored_commits.insert(full.clone());
        }

        // Resolve author + summary once per hunk, then fan out over its lines.
        let (author, time) = match hunk.final_signature() {
            Some(sig) => (
                String::from_utf8_lossy(sig.name_bytes()).into_owned(),
                sig.when().seconds(),
            ),
            None => (String::new(), 0),
        };
        let summary = hunk
            .summary()
            .ok()
            .flatten()
            .map(str::to_owned)
            .unwrap_or_default();
        let short_commit = if unblamable {
            String::new()
        } else {
            full.chars().take(7).collect()
        };

        let start = hunk.final_start_line(); // 1-based
        for offset in 0..hunk.lines_in_hunk() {
            lines.push(BlameLine {
                line_no: start + offset,
                commit: if unblamable {
                    String::new()
                } else {
                    full.clone()
                },
                short_commit: short_commit.clone(),
                author: author.clone(),
                time,
                summary: summary.clone(),
                ignored,
                unblamable,
            });
        }
    }

    // git2 yields hunks in file order already, but sort defensively so line N
    // is always at index N-1 for the UI's visible-range slicing.
    lines.sort_by_key(|l| l.line_no);

    Ok(BlameResult {
        lines,
        ignored_revs: ignored_commits.len(),
    })
}

/// Read + parse the repository-root `.git-blame-ignore-revs`, returning the set
/// of revisions to ignore (lowercased). Empty when the file is absent/unreadable.
fn load_ignore_revs(repo: &Repository) -> Vec<String> {
    let Some(workdir) = repo.workdir() else {
        return Vec::new();
    };
    match std::fs::read_to_string(workdir.join(IGNORE_REVS_FILE)) {
        Ok(text) => parse_blame_ignore_revs(&text),
        Err(_) => Vec::new(),
    }
}

/// A full commit id is ignored when any ignore-revs entry is a prefix of it —
/// git allows abbreviated ids in the file, so we prefix-match rather than
/// requiring equality.
///
/// ponytail: O(lines × entries) prefix scan. Ignore files hold a handful of
/// entries, so this is fine; if one ever grows huge, resolve entries to full
/// oids up front and use a HashSet.
fn is_ignored(full_id: &str, ignore_set: &[String]) -> bool {
    ignore_set.iter().any(|rev| full_id.starts_with(rev))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_match_handles_abbrev() {
        let set = vec!["deadbeef".to_string()];
        assert!(is_ignored("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", &set));
        assert!(!is_ignored(
            "0000beefdeadbeefdeadbeefdeadbeefdeadbeef",
            &set
        ));
    }
}
