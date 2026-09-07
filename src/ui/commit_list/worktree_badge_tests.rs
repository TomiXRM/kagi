use super::*;
use kagi_git::{Branch, Head, RepoSnapshot, Worktree};

fn snap(head_branch: &str) -> RepoSnapshot {
    let tip = CommitId("a".repeat(40));
    let branch = |name: &str| Branch {
        name: name.into(),
        target: tip.clone(),
        upstream: None,
    };
    let wt = |branch: &str, is_main: bool, is_current: bool| Worktree {
        name: if is_main { "main" } else { branch }.into(),
        path: std::path::PathBuf::from(format!("/r/{branch}")),
        branch: Some(branch.into()),
        is_current,
        is_main,
        wip: None,
        head: None,
        locked: false,
        lock_reason: None,
    };
    RepoSnapshot {
        head: Head::Attached {
            branch: head_branch.into(),
            target: "a".repeat(40),
        },
        commits: Vec::new(),
        branches: vec![branch("master"), branch("feat"), branch("other")],
        remote_branches: Vec::new(),
        tags: Vec::new(),
        status: Default::default(),
        stashes: Vec::new(),
        worktrees: vec![
            wt("master", true, head_branch == "master"),
            wt("feat", false, head_branch == "feat"),
        ],
        cleanup_rows: Vec::new(),
        last_fetch_secs: None,
    }
}

fn labels(head: &str) -> Vec<String> {
    let map = build_badge_map(&snap(head));
    let mut labels: Vec<String> = map
        .values()
        .flatten()
        .map(|badge| badge.label.to_string())
        .collect();
    labels.sort();
    labels
}

/// 🌲 consistently means "checked out in another worktree", including the
/// main worktree when the linked worktree is current (#591).
#[test]
fn every_other_worktree_branch_gets_the_tree_glyph() {
    assert_eq!(labels("master"), vec!["master ✓", "other", "🌲 feat"]);
    assert_eq!(labels("feat"), vec!["feat ✓", "other", "🌲 master"]);

    let map = build_badge_map(&snap("feat"));
    let main = map
        .values()
        .flatten()
        .find(|badge| badge.label == "🌲 master")
        .expect("main worktree branch badge");
    assert_eq!(
        main.worktree
            .as_ref()
            .map(|worktree| worktree.path.as_path()),
        Some(std::path::Path::new("/r/master"))
    );
    assert!(main.worktree.as_ref().unwrap().is_main);

    let main_view = build_badge_map(&snap("master"));
    let feature = main_view
        .values()
        .flatten()
        .find(|badge| badge.label == "🌲 feat")
        .expect("linked worktree branch badge");
    assert_eq!(
        feature
            .worktree
            .as_ref()
            .map(|worktree| worktree.path.as_path()),
        Some(std::path::Path::new("/r/feat"))
    );
    let current = main_view
        .values()
        .flatten()
        .find(|badge| badge.label == "master ✓")
        .expect("current branch badge");
    assert!(
        current.worktree.is_none(),
        "current worktree must not self-link"
    );
}

#[test]
fn clean_detached_worktree_gets_an_actionable_head_badge() {
    let mut snap = snap("master");
    let detached_head = CommitId("b".repeat(40));
    snap.worktrees.push(Worktree {
        name: "detached-wt".into(),
        path: "/r/detached".into(),
        branch: None,
        is_current: false,
        is_main: false,
        wip: None,
        head: Some(detached_head.clone()),
        locked: true,
        lock_reason: Some("reason".into()),
    });

    let badge = build_badge_map(&snap)
        .remove(&detached_head)
        .expect("detached HEAD row")
        .into_iter()
        .find(|badge| badge.kind == BadgeKind::Worktree)
        .expect("detached worktree badge");
    assert_eq!(badge.label, "🌲 detached bbbbbbbb");
    let worktree = badge.worktree.expect("actionable worktree metadata");
    assert_eq!(worktree.name, "detached-wt");
    assert_eq!(worktree.path, std::path::Path::new("/r/detached"));
    assert!(worktree.locked);
}
