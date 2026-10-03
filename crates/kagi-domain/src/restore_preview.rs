//! Pure reachability projection over the tab's already laid-out commit rows.
//! Ref movements change badges and reachability, not the original graph rails.

use crate::commit::CommitId;
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

/// What still points at commits after a branch-only restore, besides local
/// branches: remote-tracking branches, tags, detached worktree HEADs, stash
/// bases. Tag changes are not graph-projected: their raw ref OIDs can point
/// at tag objects rather than commits.
#[derive(Debug, Clone, Default)]
pub struct FixedRoots(pub Vec<CommitId>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRow {
    pub id: CommitId,
    /// True when the restore takes a formerly reachable commit off every ref.
    pub off_branch: bool,
    /// Local branches (short names) at this commit after the restore that the
    /// restore did not move.
    pub branches: Vec<String>,
    /// Branches the restore puts at this commit.
    pub moved_here: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestorePreview {
    /// A branch would go back to a commit the tab has not loaded.
    NotLoaded { refname: String, oid: String },
    /// An annotated tag can point at a tag object, not a graph commit.
    TagChange,
    Graph {
        rows: Vec<PreviewRow>,
        /// Loaded commits losing all refs, including those outside the window.
        removed: usize,
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

/// Project ref changes onto the current commit order without re-layout.
/// Rows that leave every branch remain visible as context ghosts.
pub fn preview(
    commits: &[LoadedCommit],
    branches: &BTreeMap<String, CommitId>,
    fixed: &FixedRoots,
    restores: &[RefRestore],
) -> RestorePreview {
    if restores.iter().any(|r| r.refname.starts_with("refs/tags/")) {
        return RestorePreview::TagChange;
    }
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
    // Never claim that a commit unreachable even before the restore was lost.
    let off_branch: Vec<bool> = (0..commits.len())
        .map(|i| before[i] && !kept_reach[i])
        .collect();
    let removed = off_branch.iter().filter(|lost| **lost).count();
    let targets: Vec<usize> = restores
        .iter()
        .filter_map(|r| r.restore_to.as_ref())
        .filter_map(|to| index.get(&CommitId(to.clone())).copied())
        .collect();
    let mut focus = targets.clone();
    focus.extend(
        off_branch
            .iter()
            .enumerate()
            .filter_map(|(i, lost)| lost.then_some(i)),
    );
    let (lo, hi) = match (focus.iter().min(), focus.iter().max()) {
        (Some(lo), Some(hi)) => (*lo, *hi),
        _ => (0, 0),
    };
    // When a long off-branch stretch exceeds the window, keep the restored
    // branch target in view rather than showing forty ghosts and no destination.
    let anchor = if hi - lo >= PREVIEW_MAX_ROWS {
        targets.iter().copied().min().unwrap_or(lo)
    } else {
        lo
    };
    let start = anchor.saturating_sub(PREVIEW_CONTEXT);
    let end = (hi + PREVIEW_CONTEXT + 1)
        .min(commits.len())
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
    let rows = commits[start..end]
        .iter()
        .enumerate()
        .map(|(offset, commit)| {
            let (branches, moved_here) = at.get(&commit.id).cloned().unwrap_or_default();
            PreviewRow {
                id: commit.id.clone(),
                off_branch: off_branch[start + offset],
                branches,
                moved_here,
            }
        })
        .collect();
    RestorePreview::Graph {
        rows,
        removed,
        hidden_above: start,
        hidden_below: commits.len() - end,
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
    fn deleting_a_branch_ghosts_only_its_own_commits() {
        let (rows, removed, ..) = graph(preview(
            &fork(),
            &branches(&[("main", "m"), ("feat", "f2")]),
            &FixedRoots::default(),
            &[restore("feat", None, Some("f2"))],
        ));
        assert_eq!(removed, 2);
        let ghosts: Vec<&str> = rows
            .iter()
            .filter(|r| r.off_branch)
            .map(|r| r.id.0.as_str())
            .collect();
        assert_eq!(ghosts, ["f2", "f1"]);
        assert!(rows
            .iter()
            .find(|r| r.id.0 == "m")
            .is_some_and(|r| !r.off_branch));
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
        assert!(rows.iter().find(|r| r.id.0 == "f2").unwrap().off_branch);
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
    fn annotated_tag_object_is_not_mistaken_for_unloaded_commit_or_fixed_root() {
        let tag = RefRestore {
            refname: "refs/tags/release".into(),
            restore_to: Some("raw-annotated-tag-object".into()),
            expect: None,
        };
        assert_eq!(
            preview(
                &fork(),
                &branches(&[("main", "m"), ("feat", "f2")]),
                &FixedRoots(vec![id("f2")]),
                &[restore("feat", None, Some("f2")), tag],
            ),
            RestorePreview::TagChange,
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
        assert_eq!(above + rows.len() + below, loaded.len());
        assert!(rows.iter().any(|r| r.id.0 == "m50"), "where x hung");
        assert!(rows.iter().find(|r| r.id.0 == "x").unwrap().off_branch);

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

    #[test]
    fn distant_restored_tip_stays_in_window_when_ghost_span_is_long() {
        let loaded: Vec<LoadedCommit> = (0..80)
            .map(|i| LoadedCommit {
                id: id(&format!("c{i}")),
                parents: (i < 79)
                    .then(|| id(&format!("c{}", i + 1)))
                    .into_iter()
                    .collect(),
            })
            .collect();
        let (rows, removed, ..) = graph(preview(
            &loaded,
            &branches(&[("main", "c0")]),
            &FixedRoots::default(),
            &[restore("main", Some("c70"), Some("c0"))],
        ));
        assert_eq!(removed, 70);
        assert!(rows.len() <= PREVIEW_MAX_ROWS);
        assert!(rows
            .iter()
            .any(|row| row.id.0 == "c70" && row.moved_here == ["main"]));
    }
}
