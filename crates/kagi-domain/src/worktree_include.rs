//! `.worktreeinclude` selection rules (issue #339) — the pure decision layer.
//!
//! The I/O (reading `.worktreeinclude`, walking the tree, asking git whether a
//! path is tracked/ignored) lives in `kagi-git`; this module only decides which
//! of the *already pattern-matched* candidates actually get copied. Keeping the
//! rules here makes them unit-testable without a repo.
//!
//! Locked decisions (issue §5):
//! - copy only if the candidate is gitignored **and not tracked** (git checks
//!   tracked files out itself — copying would double them);
//! - symlinks are skipped (never copied, never followed);
//! - a total-size cap is advisory: over-cap still copies but raises a warning.
//!
//! Destination-already-exists is NOT decided here — a freshly created worktree
//! has no such files, and the no-overwrite guard is a cheap `dst.exists()` check
//! at copy time in `kagi-git`.

/// Default advisory total-size cap (100 MiB). Over this, copy still proceeds but
/// the plan raises a warning — this is the `node_modules/` guard.
pub const WORKTREE_INCLUDE_CAP_BYTES: u64 = 100 * 1024 * 1024;

/// One file that already matched a `.worktreeinclude` pattern, annotated with
/// the git facts needed to decide whether to copy it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeIncludeCandidate {
    pub path: String,
    pub is_tracked: bool,
    pub is_ignored: bool,
    pub is_symlink: bool,
    pub size: u64,
}

/// The outcome of applying the selection rules to a candidate list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorktreeIncludeSelection {
    /// Relative paths to copy (sorted).
    pub copy: Vec<String>,
    /// Sum of the sizes of `copy`.
    pub total_bytes: u64,
    /// Matched symlinks that were skipped (sorted).
    pub skipped_symlinks: Vec<String>,
    /// `total_bytes` exceeded `cap_bytes`.
    pub over_cap: bool,
    /// The cap that was applied (echoed for the warning message).
    pub cap_bytes: u64,
}

/// Apply the locked selection rules to `candidates`.
pub fn select_worktree_include(
    candidates: &[WorktreeIncludeCandidate],
    cap_bytes: u64,
) -> WorktreeIncludeSelection {
    let mut sel = WorktreeIncludeSelection {
        cap_bytes,
        ..Default::default()
    };
    for c in candidates {
        // Never copy a tracked file (git checks it out), and only copy files
        // that git ignores. Dropping either half of this guard is a bug the
        // unit tests below catch.
        if c.is_tracked || !c.is_ignored {
            continue;
        }
        if c.is_symlink {
            sel.skipped_symlinks.push(c.path.clone());
            continue;
        }
        sel.copy.push(c.path.clone());
        sel.total_bytes += c.size;
    }
    sel.copy.sort();
    sel.skipped_symlinks.sort();
    sel.over_cap = sel.total_bytes > cap_bytes;
    sel
}

/// Human-readable byte size for plan warnings (e.g. `1.5 MiB`, `512 B`).
pub fn human_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if n >= GIB {
        format!("{:.1} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KiB", n as f64 / KIB as f64)
    } else {
        format!("{} B", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(
        path: &str,
        tracked: bool,
        ignored: bool,
        symlink: bool,
        size: u64,
    ) -> WorktreeIncludeCandidate {
        WorktreeIncludeCandidate {
            path: path.to_string(),
            is_tracked: tracked,
            is_ignored: ignored,
            is_symlink: symlink,
            size,
        }
    }

    #[test]
    fn copies_ignored_untracked_file() {
        let sel = select_worktree_include(
            &[cand(".env", false, true, false, 10)],
            WORKTREE_INCLUDE_CAP_BYTES,
        );
        assert_eq!(sel.copy, vec![".env".to_string()]);
        assert_eq!(sel.total_bytes, 10);
        assert!(!sel.over_cap);
    }

    // Mutation guard: dropping the `is_tracked` exclusion would copy this.
    #[test]
    fn never_copies_tracked_file_even_if_ignored() {
        let sel = select_worktree_include(
            &[cand("tracked.txt", true, true, false, 10)],
            WORKTREE_INCLUDE_CAP_BYTES,
        );
        assert!(sel.copy.is_empty(), "tracked files must never be copied");
    }

    // Mutation guard: dropping the `is_ignored` requirement would copy this.
    #[test]
    fn never_copies_non_ignored_file() {
        let sel = select_worktree_include(
            &[cand("plain.txt", false, false, false, 10)],
            WORKTREE_INCLUDE_CAP_BYTES,
        );
        assert!(
            sel.copy.is_empty(),
            "non-ignored files must never be copied"
        );
    }

    #[test]
    fn skips_symlinks() {
        let sel = select_worktree_include(
            &[cand("link", false, true, true, 10)],
            WORKTREE_INCLUDE_CAP_BYTES,
        );
        assert!(sel.copy.is_empty());
        assert_eq!(sel.skipped_symlinks, vec!["link".to_string()]);
        assert_eq!(sel.total_bytes, 0);
    }

    #[test]
    fn over_cap_flag_set_but_still_selected() {
        let sel = select_worktree_include(&[cand("big.bin", false, true, false, 200)], 100);
        assert_eq!(sel.copy, vec!["big.bin".to_string()]);
        assert!(
            sel.over_cap,
            "over-cap set must still be listed with the flag"
        );
    }

    #[test]
    fn at_cap_is_not_over() {
        let sel = select_worktree_include(&[cand("x", false, true, false, 100)], 100);
        assert!(!sel.over_cap);
    }

    #[test]
    fn results_are_sorted() {
        let sel = select_worktree_include(
            &[
                cand("b", false, true, false, 1),
                cand("a", false, true, false, 1),
            ],
            WORKTREE_INCLUDE_CAP_BYTES,
        );
        assert_eq!(sel.copy, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(100 * 1024 * 1024), "100.0 MiB");
    }
}
