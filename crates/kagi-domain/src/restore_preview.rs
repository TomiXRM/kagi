//! The graph after an op-revert / restore-to-point, before it runs (#334
//! slice 2c, ADR-0214 §6). Pure: computed from the commits already loaded for
//! the tab and the plan's restores, laid out by the one [`crate::graph::layout`].
//! Display only — nothing here is part of the plan or its checks.

use crate::commit::{Commit, CommitId, Signature};
use crate::graph::{layout, GraphRow};
use crate::ref_restore::RefRestore;
use std::collections::{BTreeMap, HashMap, HashSet};

/// At most this many rows are drawn.
pub const PREVIEW_MAX_ROWS: usize = 40;
/// Rows kept above and below the rows that change.
pub const PREVIEW_CONTEXT: usize = 4;

/// One loaded commit: its id and parents, in the view's (topological) order.
#[derive(Debug, Clone)]
pub struct LoadedCommit {
    pub id: CommitId,
    pub parents: Vec<CommitId>,
}

/// What still points at commits after the restore, besides local branches:
/// remote-tracking branches, tags, detached worktree HEADs, stash bases.
/// These never move, so whatever they reach stays.
#[derive(Debug, Clone, Default)]
pub struct FixedRoots(pub Vec<CommitId>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRow {
    pub id: CommitId,
    /// Lane, colour and edges from the after-restore layout.
    pub graph: GraphRow,
    /// Local branches (short names) at this commit after the restore that the
    /// restore did not move.
    pub branches: Vec<String>,
    /// Branches the restore puts at this commit.
    pub moved_here: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestorePreview {
    /// A branch would go back to a commit the tab has not loaded (a deleted
    /// branch's history, or older than the loaded page): no guess is drawn.
    NotLoaded { refname: String, oid: String },
    Graph {
        rows: Vec<PreviewRow>,
        lane_count: usize,
        /// Loaded commits no ref reaches after the restore (all of them, not
        /// only the drawn window).
        removed: usize,
        /// Rows of the after-restore graph outside the drawn window.
        hidden_above: usize,
        hidden_below: usize,
    },
}

fn short(refname: &str) -> String {
    refname.trim_start_matches("refs/heads/").to_string()
}

/// Every loaded commit reachable from `roots` (parents outside the loaded set
/// are not followed: the page ends there).
fn reachable(
    index: &HashMap<&CommitId, usize>,
    commits: &[LoadedCommit],
    roots: &[CommitId],
) -> Vec<bool> {
    let mut seen = vec![false; commits.len()];
    let mut stack: Vec<usize> = roots.iter().filter_map(|r| index.get(r).copied()).collect();
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        stack.extend(
            commits[i]
                .parents
                .iter()
                .filter_map(|p| index.get(p).copied()),
        );
    }
    seen
}

/// The after-restore graph around the rows that change.
///
/// `branches`: every local branch now (`refs/heads/x` → commit). A loaded
/// commit disappears when the moved branches were the only refs reaching it;
/// a commit no ref reached before stays where it is (nothing changes for it).
pub fn preview(
    commits: &[LoadedCommit],
    branches: &BTreeMap<String, CommitId>,
    fixed: &FixedRoots,
    restores: &[RefRestore],
) -> RestorePreview {
    let index: HashMap<&CommitId, usize> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| (&c.id, i))
        .collect();
    for r in restores {
        if let Some(to) = &r.restore_to {
            if !index.contains_key(&CommitId(to.clone())) {
                return RestorePreview::NotLoaded {
                    refname: r.refname.clone(),
                    oid: to.clone(),
                };
            }
        }
    }
    let mut after = branches.clone();
    for r in restores {
        match &r.restore_to {
            Some(to) => after.insert(r.refname.clone(), CommitId(to.clone())),
            None => after.remove(&r.refname),
        };
    }
    let roots = |map: &BTreeMap<String, CommitId>| -> Vec<CommitId> {
        map.values()
            .cloned()
            .chain(fixed.0.iter().cloned())
            .collect()
    };
    let before = reachable(&index, commits, &roots(branches));
    let kept_reach = reachable(&index, commits, &roots(&after));
    // Kept: still reached, or never reached (untouched by any ref move).
    let kept: Vec<bool> = (0..commits.len())
        .map(|i| kept_reach[i] || !before[i])
        .collect();
    let removed = kept.iter().filter(|k| !**k).count();

    let kept_commits: Vec<Commit> = commits
        .iter()
        .zip(&kept)
        .filter(|(_, k)| **k)
        .map(|(c, _)| Commit {
            id: c.id.clone(),
            parents: c.parents.clone(),
            author: Signature {
                name: String::new(),
                email: String::new(),
                time: 0,
            },
            committer: Signature {
                name: String::new(),
                email: String::new(),
                time: 0,
            },
            summary: String::new(),
            message: String::new(),
        })
        .collect();
    let graph = layout(&kept_commits);

    // Where the change shows: each restored branch's new commit, and the kept
    // row right below every run of removed rows (where they hung).
    let mut kept_pos = vec![None; commits.len()];
    let mut n = 0;
    for (i, k) in kept.iter().enumerate() {
        if *k {
            kept_pos[i] = Some(n);
            n += 1;
        }
    }
    let mut focus: Vec<usize> = restores
        .iter()
        .filter_map(|r| r.restore_to.as_ref())
        .filter_map(|to| index.get(&CommitId(to.clone())).and_then(|i| kept_pos[*i]))
        .collect();
    let mut next_kept = None;
    for i in (0..commits.len()).rev() {
        match kept_pos[i] {
            Some(p) => next_kept = Some(p),
            None => focus.extend(next_kept.or(n.checked_sub(1))),
        }
    }
    let (lo, hi) = match (focus.iter().min(), focus.iter().max()) {
        (Some(lo), Some(hi)) => (*lo, *hi),
        _ => (0, 0),
    };
    let start = lo.saturating_sub(PREVIEW_CONTEXT);
    let end = (hi + PREVIEW_CONTEXT + 1)
        .min(n)
        .min(start + PREVIEW_MAX_ROWS);

    let moved: HashSet<&str> = restores.iter().map(|r| r.refname.as_str()).collect();
    let mut at: HashMap<&CommitId, (Vec<String>, Vec<String>)> = HashMap::new();
    for (name, oid) in &after {
        let slot = at.entry(oid).or_default();
        if moved.contains(name.as_str()) {
            slot.1.push(short(name));
        } else {
            slot.0.push(short(name));
        }
    }
    let rows = graph.rows[start..end]
        .iter()
        .map(|g| {
            let (branches, moved_here) = at.get(&g.commit).cloned().unwrap_or_default();
            PreviewRow {
                id: g.commit.clone(),
                graph: g.clone(),
                branches,
                moved_here,
            }
        })
        .collect();
    RestorePreview::Graph {
        rows,
        lane_count: graph.lane_count,
        removed,
        hidden_above: start,
        hidden_below: n - end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> CommitId {
        CommitId(s.into())
    }

    /// `(id, parents)`, newest first.
    fn commits(spec: &[(&str, &[&str])]) -> Vec<LoadedCommit> {
        spec.iter()
            .map(|(c, ps)| LoadedCommit {
                id: id(c),
                parents: ps.iter().map(|p| id(p)).collect(),
            })
            .collect()
    }

    fn branches(spec: &[(&str, &str)]) -> BTreeMap<String, CommitId> {
        spec.iter()
            .map(|(n, c)| (format!("refs/heads/{n}"), id(c)))
            .collect()
    }

    fn restore(name: &str, to: Option<&str>, expect: Option<&str>) -> RefRestore {
        RefRestore {
            refname: format!("refs/heads/{name}"),
            restore_to: to.map(Into::into),
            expect: expect.map(Into::into),
        }
    }

    fn graph(p: RestorePreview) -> (Vec<PreviewRow>, usize, usize, usize) {
        match p {
            RestorePreview::Graph {
                rows,
                removed,
                hidden_above,
                hidden_below,
                ..
            } => (rows, removed, hidden_above, hidden_below),
            other => panic!("{other:?}"),
        }
    }

    // f2 → f1 → b ; main → b
    fn fork() -> Vec<LoadedCommit> {
        commits(&[("f2", &["f1"]), ("f1", &["b"]), ("m", &["b"]), ("b", &[])])
    }

    #[test]
    fn deleting_a_branch_removes_only_its_own_commits() {
        let (rows, removed, ..) = graph(preview(
            &fork(),
            &branches(&[("main", "m"), ("feat", "f2")]),
            &FixedRoots::default(),
            &[restore("feat", None, Some("f2"))],
        ));
        assert_eq!(removed, 2);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.0.as_str()).collect();
        assert_eq!(ids, vec!["m", "b"]);
    }

    #[test]
    fn a_tag_or_remote_keeps_what_it_reaches() {
        let (_, removed, ..) = graph(preview(
            &fork(),
            &branches(&[("main", "m"), ("feat", "f2")]),
            &FixedRoots(vec![id("f1")]),
            &[restore("feat", None, Some("f2"))],
        ));
        assert_eq!(removed, 1, "only f2: f1 is held by the tag");
    }

    #[test]
    fn a_moved_branch_is_labelled_at_its_target() {
        let (rows, removed, ..) = graph(preview(
            &fork(),
            &branches(&[("main", "m"), ("feat", "f2")]),
            &FixedRoots::default(),
            &[restore("feat", Some("f1"), Some("f2"))],
        ));
        assert_eq!(removed, 1);
        let f1 = rows.iter().find(|r| r.id.0 == "f1").unwrap();
        assert_eq!(f1.moved_here, vec!["feat"]);
        let m = rows.iter().find(|r| r.id.0 == "m").unwrap();
        assert_eq!(
            (m.branches.clone(), m.moved_here.is_empty()),
            (vec!["main".to_string()], true)
        );
    }

    #[test]
    fn a_target_outside_the_loaded_commits_is_not_guessed() {
        assert_eq!(
            preview(
                &fork(),
                &branches(&[("main", "m")]),
                &FixedRoots::default(),
                &[restore("gone", Some("zz"), None)],
            ),
            RestorePreview::NotLoaded {
                refname: "refs/heads/gone".into(),
                oid: "zz".into()
            }
        );
    }

    #[test]
    fn the_window_is_the_change_plus_context_and_at_most_the_cap() {
        // A long main line with a feature tip deep inside it.
        let mut spec: Vec<(String, Vec<String>)> = Vec::new();
        for i in 0..100 {
            spec.push((
                format!("m{i}"),
                if i < 99 {
                    vec![format!("m{}", i + 1)]
                } else {
                    vec![]
                },
            ));
        }
        spec.insert(50, ("x".into(), vec!["m50".into()]));
        let loaded: Vec<LoadedCommit> = spec
            .iter()
            .map(|(c, ps)| LoadedCommit {
                id: id(c),
                parents: ps.iter().map(|p| id(p)).collect(),
            })
            .collect();
        let (rows, removed, above, below) = graph(preview(
            &loaded,
            &branches(&[("main", "m0"), ("x", "x")]),
            &FixedRoots::default(),
            &[restore("x", None, Some("x"))],
        ));
        assert_eq!(removed, 1);
        assert_eq!(
            rows.len(),
            2 * PREVIEW_CONTEXT + 1,
            "the hang point ± context"
        );
        assert_eq!(above + rows.len() + below, 100);
        assert!(rows.iter().any(|r| r.id.0 == "m50"), "where x hung");

        // Two changes far apart are capped.
        let (rows, ..) = graph(preview(
            &loaded,
            &branches(&[("main", "m0"), ("x", "x")]),
            &FixedRoots::default(),
            &[
                restore("x", None, Some("x")),
                restore("main", Some("m90"), Some("m0")),
            ],
        ));
        assert!(rows.len() <= PREVIEW_MAX_ROWS, "{}", rows.len());
    }
}
