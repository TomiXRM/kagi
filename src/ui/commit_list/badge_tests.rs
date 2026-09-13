//! Ref-badge folding and graph copy targets.
//!
//! Split out of `commit_list.rs` beside the worktree-badge tests that
//! already live here (#704: the file was at its LOC ceiling).

mod remote_fold_tests {
    use super::super::*;
    use kagi_git::{Branch, Head, RemoteBranch, RepoSnapshot, UpstreamInfo};

    fn tip(c: char) -> CommitId {
        CommitId(c.to_string().repeat(40))
    }

    fn base_snap() -> RepoSnapshot {
        RepoSnapshot {
            head: Head::Attached {
                branch: "main".into(),
                target: "a".repeat(40),
            },
            commits: Vec::new(),
            branches: Vec::new(),
            remote_branches: Vec::new(),
            tags: Vec::new(),
            status: Default::default(),
            stashes: Vec::new(),
            worktrees: Vec::new(),
            cleanup_rows: Vec::new(),
            last_fetch_secs: None,
            operation: None,
        }
    }

    fn all_badges(snap: &RepoSnapshot) -> Vec<RefBadge> {
        let mut v: Vec<RefBadge> = build_badge_map(snap).into_values().flatten().collect();
        v.sort_by(|a, b| a.label.cmp(&b.label));
        v
    }

    /// `[main][origin/main]` at one commit becomes ONE chip: the local badge
    /// with the remote folded into `remotes` (rendered as ☁ + tooltip).
    #[test]
    fn same_name_remote_at_same_commit_is_folded_into_the_local_badge() {
        let mut s = base_snap();
        s.branches.push(Branch {
            name: "main".into(),
            target: tip('a'),
            upstream: None,
        });
        s.remote_branches.push(RemoteBranch {
            remote: "origin".into(),
            name: "main".into(),
            target: tip('a'),
        });
        let b = all_badges(&s);
        assert_eq!(b.len(), 1, "one chip, not two: {:?}", b);
        assert_eq!(b[0].label.as_ref(), "main ✓");
        assert_eq!(b[0].remotes, vec![SharedString::from("origin/main")]);
        assert_eq!(
            badge_display(&b[0]),
            (
                "main ✓".to_string(),
                BadgeWhere {
                    local: true,
                    remote: true
                }
            )
        );
        assert_eq!(badge_tooltip(&b[0]), "main ✓\norigin/main");
    }

    /// Different names but tracking (local `foo` → origin/bar), same commit:
    /// folded too — the LOCAL name stays the label, the remote is on hover.
    #[test]
    fn tracked_upstream_with_a_different_name_is_folded_too() {
        let mut s = base_snap();
        s.branches.push(Branch {
            name: "foo".into(),
            target: tip('b'),
            upstream: Some(UpstreamInfo {
                remote_branch: "origin/bar".into(),
                ahead: 0,
                behind: 0,
            }),
        });
        s.remote_branches.push(RemoteBranch {
            remote: "origin".into(),
            name: "bar".into(),
            target: tip('b'),
        });
        let b = all_badges(&s);
        assert_eq!(b.len(), 1, "{:?}", b);
        assert_eq!(b[0].label.as_ref(), "foo");
        assert_eq!(b[0].remotes, vec![SharedString::from("origin/bar")]);
    }

    /// Diverged (remote ahead): the remote is at ANOTHER commit → its own
    /// chip, shown as `☁ main` (prefix in the tooltip only).
    #[test]
    fn remote_at_a_different_commit_keeps_its_own_chip() {
        let mut s = base_snap();
        s.branches.push(Branch {
            name: "main".into(),
            target: tip('a'),
            upstream: None,
        });
        s.remote_branches.push(RemoteBranch {
            remote: "origin".into(),
            name: "main".into(),
            target: tip('c'),
        });
        let b = all_badges(&s);
        assert_eq!(b.len(), 2);
        let remote = b.iter().find(|x| x.kind == BadgeKind::Remote).unwrap();
        assert_eq!(
            remote.label.as_ref(),
            "origin/main",
            "label stays the full ref"
        );
        assert_eq!(
            badge_display(remote),
            (
                "main".to_string(),
                BadgeWhere {
                    local: false,
                    remote: true
                }
            )
        );
        // A non-origin remote keeps its prefix so two remotes can't collide.
        let up = RefBadge::new(BadgeKind::Remote, "upstream/main");
        assert_eq!(badge_display(&up).0, "upstream/main");
    }
}

#[cfg(test)]
mod graph_copy_tests {
    use super::super::*;

    const SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

    fn branch(name: &str) -> RefBadge {
        RefBadge::new(BadgeKind::Branch, name)
    }

    #[test]
    fn hash_target_copies_full_sha() {
        let badges = vec![branch("feature")];
        assert_eq!(graph_copy_value(&badges, SHA, CopyTarget::Hash), SHA);
    }

    #[test]
    fn branch_target_copies_local_branch() {
        let badges = vec![branch("feature")];
        assert_eq!(
            graph_copy_value(&badges, SHA, CopyTarget::Branch),
            "feature"
        );
    }

    #[test]
    fn branch_target_with_no_branch_falls_back_to_hash() {
        // Row carries only a remote + tag — no local branch → copy the SHA.
        let badges = vec![
            RefBadge::new(BadgeKind::Remote, "origin/feature"),
            RefBadge::new(BadgeKind::Tag, "v1.0"),
        ];
        assert_eq!(graph_copy_value(&badges, SHA, CopyTarget::Branch), SHA);
    }

    #[test]
    fn branch_target_prefers_local_branch_over_remote_and_tag() {
        let badges = vec![
            RefBadge::new(BadgeKind::Remote, "origin/feature"),
            branch("feature"),
            RefBadge::new(BadgeKind::Tag, "v1.0"),
        ];
        assert_eq!(
            graph_copy_value(&badges, SHA, CopyTarget::Branch),
            "feature"
        );
    }

    #[test]
    fn branch_target_strips_head_and_worktree_markers() {
        let head = [RefBadge::new(BadgeKind::HeadBranch, "main ✓")];
        assert_eq!(graph_copy_value(&head, SHA, CopyTarget::Branch), "main");
        let wt = [RefBadge::new(BadgeKind::Branch, "🌲 feat")];
        assert_eq!(graph_copy_value(&wt, SHA, CopyTarget::Branch), "feat");
    }
}
