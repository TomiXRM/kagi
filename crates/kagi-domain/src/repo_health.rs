//! Repository-health findings for Analyze (#358, ADR-0205).
//!
//! Large repositories get faster through optimizations git already ships —
//! a commit-graph for history walks and the built-in filesystem monitor for
//! `status` — so Kagi *detects* when they are off and *offers* to turn them
//! on through the normal plan → confirm pipeline. This module is the pure
//! half: it turns facts the Git backend read into findings. Nothing here
//! reads or writes a repository.

/// What the backend read about one repository. Plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthFacts {
    /// A bare repository has no working tree to monitor and is not shown in
    /// Analyze; nothing is suggested for it.
    pub bare: bool,
    /// Committer time (Unix seconds) of HEAD's commit; `None` for an unborn
    /// HEAD, where there is no history for a commit-graph to describe.
    pub head_commit_time: Option<i64>,
    /// Modification time (Unix seconds) of the newest commit-graph file —
    /// `objects/info/commit-graph` or the split chain — or `None` if absent.
    pub commit_graph_mtime: Option<i64>,
    /// The effective `core.fsmonitor` value, from any config level; `None`
    /// when it is not set anywhere. An explicit `false` is the user's choice.
    pub fsmonitor: Option<String>,
    /// Whether git's built-in fsmonitor daemon exists on this platform
    /// (macOS and Windows). Elsewhere `core.fsmonitor=true` only warns.
    pub fsmonitor_supported: bool,
}

/// A detected state git could handle faster, and the fix Kagi can plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthFinding {
    /// No commit-graph file: history walks parse every commit object.
    CommitGraphMissing,
    /// A commit-graph older than HEAD's commit: newer commits are not in it,
    /// so walks from HEAD fall back to parsing objects until they reach it.
    CommitGraphStale,
    /// `core.fsmonitor` is not set: every `status` scans the whole tree.
    FsmonitorUnset,
}

/// The write that addresses a finding. Each is a planned `Operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthFix {
    /// `git commit-graph write --reachable`.
    WriteCommitGraph,
    /// `git config core.fsmonitor true` (repository-local config).
    EnableFsmonitor,
}

impl HealthFinding {
    /// The fix Kagi offers for this finding.
    pub fn fix(self) -> HealthFix {
        match self {
            HealthFinding::CommitGraphMissing | HealthFinding::CommitGraphStale => {
                HealthFix::WriteCommitGraph
            }
            HealthFinding::FsmonitorUnset => HealthFix::EnableFsmonitor,
        }
    }
}

/// Everything worth suggesting for `facts`, commit-graph first.
///
/// Staleness compares the graph file's mtime with HEAD's committer time: a
/// commit made after the graph was written cannot be in it. The converse is
/// not guaranteed (a commit with an old committer time can arrive later by
/// fetch), so a graph that looks current may still miss commits — the check
/// never suggests a needless rewrite, at the price of missing some.
pub fn assess(facts: &HealthFacts) -> Vec<HealthFinding> {
    let mut findings = Vec::new();
    if facts.bare {
        return findings;
    }
    if let Some(head) = facts.head_commit_time {
        match facts.commit_graph_mtime {
            None => findings.push(HealthFinding::CommitGraphMissing),
            Some(written) if written < head => findings.push(HealthFinding::CommitGraphStale),
            Some(_) => {}
        }
    }
    if facts.fsmonitor_supported && facts.fsmonitor.is_none() {
        findings.push(HealthFinding::FsmonitorUnset);
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> HealthFacts {
        HealthFacts {
            bare: false,
            head_commit_time: Some(1_000),
            commit_graph_mtime: Some(2_000),
            fsmonitor: Some("true".into()),
            fsmonitor_supported: true,
        }
    }

    #[test]
    fn a_tuned_repository_has_no_findings() {
        assert!(assess(&healthy()).is_empty());
    }

    #[test]
    fn a_missing_commit_graph_is_suggested() {
        let facts = HealthFacts {
            commit_graph_mtime: None,
            ..healthy()
        };
        assert_eq!(assess(&facts), vec![HealthFinding::CommitGraphMissing]);
    }

    #[test]
    fn a_graph_older_than_head_is_stale_and_one_as_new_is_not() {
        let stale = HealthFacts {
            commit_graph_mtime: Some(999),
            ..healthy()
        };
        assert_eq!(assess(&stale), vec![HealthFinding::CommitGraphStale]);
        let same_second = HealthFacts {
            commit_graph_mtime: Some(1_000),
            ..healthy()
        };
        assert!(assess(&same_second).is_empty());
    }

    #[test]
    fn an_unborn_head_needs_no_commit_graph() {
        let facts = HealthFacts {
            head_commit_time: None,
            commit_graph_mtime: None,
            ..healthy()
        };
        assert!(assess(&facts).is_empty());
    }

    #[test]
    fn fsmonitor_is_suggested_only_when_unset_and_supported() {
        let unset = HealthFacts {
            fsmonitor: None,
            ..healthy()
        };
        assert_eq!(assess(&unset), vec![HealthFinding::FsmonitorUnset]);
        let declined = HealthFacts {
            fsmonitor: Some("false".into()),
            ..healthy()
        };
        assert!(
            assess(&declined).is_empty(),
            "an explicit false is a choice"
        );
        let unsupported = HealthFacts {
            fsmonitor: None,
            fsmonitor_supported: false,
            ..healthy()
        };
        assert!(assess(&unsupported).is_empty());
    }

    #[test]
    fn a_bare_repository_gets_nothing() {
        let facts = HealthFacts {
            bare: true,
            commit_graph_mtime: None,
            fsmonitor: None,
            ..healthy()
        };
        assert!(assess(&facts).is_empty());
    }

    #[test]
    fn findings_are_ordered_and_map_to_their_fix() {
        let facts = HealthFacts {
            commit_graph_mtime: None,
            fsmonitor: None,
            ..healthy()
        };
        let findings = assess(&facts);
        assert_eq!(
            findings,
            vec![
                HealthFinding::CommitGraphMissing,
                HealthFinding::FsmonitorUnset
            ]
        );
        assert_eq!(findings[0].fix(), HealthFix::WriteCommitGraph);
        assert_eq!(
            HealthFinding::CommitGraphStale.fix(),
            HealthFix::WriteCommitGraph
        );
        assert_eq!(findings[1].fix(), HealthFix::EnableFsmonitor);
    }
}
