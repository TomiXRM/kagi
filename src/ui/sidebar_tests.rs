use super::*;

/// Compact view of a GroupRow for assertions: ("G", prefix, count) for a
/// group header, ("L", leaf_label, item) for a grouped leaf, and
/// ("T", item, item) for a top-level item.
fn summarize(rows: &[GroupRow<String>]) -> Vec<(&'static str, String, String)> {
    rows.iter()
        .map(|r| match r {
            GroupRow::Group { prefix, count } => ("G", prefix.clone(), count.to_string()),
            GroupRow::GroupedLeaf {
                leaf_label, item, ..
            } => ("L", leaf_label.clone(), item.clone()),
            GroupRow::TopLevel { item } => ("T", item.clone(), item.clone()),
        })
        .collect()
}

fn group(names: &[&str]) -> Vec<GroupRow<String>> {
    let owned: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    group_by_prefix(&owned, |s| s.as_str())
}

#[test]
fn split_basic() {
    assert_eq!(
        split_first_segment("feat/a"),
        Some(("feat".into(), "a".into()))
    );
    assert_eq!(
        split_first_segment("feat/ui/x"),
        Some(("feat".into(), "ui/x".into()))
    );
    assert_eq!(split_first_segment("main"), None);
    // Empty halves stay top-level.
    assert_eq!(split_first_segment("/x"), None);
    assert_eq!(split_first_segment("feat/"), None);
}

#[test]
fn split_non_ascii() {
    // chars()-based: multibyte prefixes must not panic or mis-split.
    assert_eq!(
        split_first_segment("機能/あ"),
        Some(("機能".into(), "あ".into()))
    );
}

#[test]
fn groups_and_top_level() {
    // feat/a, feat/b → group feat(2); fix/c → group fix(1); main → top.
    let rows = group(&["feat/a", "feat/b", "fix/c", "main"]);
    assert_eq!(
        summarize(&rows),
        vec![
            ("G", "feat".into(), "2".into()),
            ("L", "a".into(), "feat/a".into()),
            ("L", "b".into(), "feat/b".into()),
            ("G", "fix".into(), "1".into()),
            ("L", "c".into(), "fix/c".into()),
            ("T", "main".into(), "main".into()),
        ]
    );
}

#[test]
fn multi_segment_leaf_keeps_remainder() {
    // Single first-level split: feat/ui/x → group feat, leaf "ui/x".
    let rows = group(&["feat/ui/x"]);
    assert_eq!(
        summarize(&rows),
        vec![
            ("G", "feat".into(), "1".into()),
            ("L", "ui/x".into(), "feat/ui/x".into()),
        ]
    );
}

#[test]
fn remote_grouped_by_remote_name() {
    // origin/feat/x → group origin, leaf "feat/x".
    let rows = group(&["origin/main", "origin/feat/x", "upstream/dev"]);
    assert_eq!(
        summarize(&rows),
        vec![
            ("G", "origin".into(), "2".into()),
            ("L", "main".into(), "origin/main".into()),
            ("L", "feat/x".into(), "origin/feat/x".into()),
            ("G", "upstream".into(), "1".into()),
            ("L", "dev".into(), "upstream/dev".into()),
        ]
    );
}

#[test]
fn no_groups_all_top_level() {
    let rows = group(&["main", "dev", "trunk"]);
    assert!(rows.iter().all(|r| matches!(r, GroupRow::TopLevel { .. })));
    assert_eq!(rows.len(), 3);
}

// ── W19-REMOTE-TREE: two-level remote grouping ──────────────────

/// Compact view of a RemoteRow: ("R", remote, count), ("S", prefix, count),
/// ("RL", remote, leaf), ("SL", prefix, leaf).
fn summarize_remote(rows: &[RemoteRow<(String, String)>]) -> Vec<(&'static str, String, String)> {
    rows.iter()
        .map(|r| match r {
            RemoteRow::Remote { remote, count } => ("R", remote.clone(), count.to_string()),
            RemoteRow::SubGroup { prefix, count, .. } => ("S", prefix.clone(), count.to_string()),
            RemoteRow::RemoteLeaf { leaf_label, .. } => ("RL", leaf_label.clone(), String::new()),
            RemoteRow::SubGroupedLeaf {
                prefix, leaf_label, ..
            } => ("SL", prefix.clone(), leaf_label.clone()),
        })
        .collect()
}

/// Build remote rows from (remote, name) pairs.
fn group_rem(pairs: &[(&str, &str)]) -> Vec<RemoteRow<(String, String)>> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(r, n)| (r.to_string(), n.to_string()))
        .collect();
    group_remotes(&owned, |(r, _)| r.as_str(), |(_, n)| n.as_str())
}

#[test]
fn remote_two_levels_basic() {
    // origin/main → origin ▸ main (direct leaf)
    // origin/feat/x → origin ▸ feat ▸ x (sub-grouped leaf)
    let rows = group_rem(&[("origin", "main"), ("origin", "feat/x")]);
    assert_eq!(
        summarize_remote(&rows),
        vec![
            ("R", "origin".into(), "2".into()),
            ("RL", "main".into(), String::new()),
            ("S", "feat".into(), "1".into()),
            ("SL", "feat".into(), "x".into()),
        ]
    );
}

#[test]
fn remote_multiple_remotes_independent() {
    // origin and upstream group independently, in first-seen order.
    let rows = group_rem(&[
        ("origin", "feat/a"),
        ("origin", "feat/b"),
        ("upstream", "feat/c"),
        ("upstream", "dev"),
    ]);
    assert_eq!(
        summarize_remote(&rows),
        vec![
            ("R", "origin".into(), "2".into()),
            ("S", "feat".into(), "2".into()),
            ("SL", "feat".into(), "a".into()),
            ("SL", "feat".into(), "b".into()),
            ("R", "upstream".into(), "2".into()),
            ("S", "feat".into(), "1".into()),
            ("SL", "feat".into(), "c".into()),
            ("RL", "dev".into(), String::new()),
        ]
    );
}

#[test]
fn remote_deep_name_keeps_remainder() {
    // origin/feat/ui/x → origin ▸ feat ▸ ui/x (single sub-level split).
    let rows = group_rem(&[("origin", "feat/ui/x")]);
    assert_eq!(
        summarize_remote(&rows),
        vec![
            ("R", "origin".into(), "1".into()),
            ("S", "feat".into(), "1".into()),
            ("SL", "feat".into(), "ui/x".into()),
        ]
    );
}

#[test]
fn remote_collapse_keys_unique_and_no_collision() {
    // Level-1 remote header vs level-2 sub-group vs local must all differ.
    assert_eq!(remote_key("origin"), "remote:origin");
    assert_eq!(remote_group_key("origin", "feat"), "remote:origin:feat");
    assert_eq!(remote_group_key("upstream", "feat"), "remote:upstream:feat");
    // Two remotes with the same sub-group prefix get distinct keys.
    assert_ne!(
        remote_group_key("origin", "feat"),
        remote_group_key("upstream", "feat")
    );
    // The remote header key (2 segments) never equals any sub-group key
    // (3 segments), and never matches a local key.
    assert_ne!(remote_key("origin"), remote_group_key("origin", "feat"));
    assert_ne!(
        remote_group_key("origin", "feat"),
        group_key(SECTION_LOCAL, "feat")
    );
}

#[test]
fn remote_non_ascii_subgroup() {
    let rows = group_rem(&[("origin", "機能/あ")]);
    assert_eq!(
        summarize_remote(&rows),
        vec![
            ("R", "origin".into(), "1".into()),
            ("S", "機能".into(), "1".into()),
            ("SL", "機能".into(), "あ".into()),
        ]
    );
}

/// Rebuild `rows` the way `render` does (rows and fingerprint together),
/// then let the list processor's cache catch up.
fn rebuild(state: &mut SidebarState, fingerprint: u64, groups: &HashSet<String>, filter: &str) {
    let branches = [
        ("main".to_string(), true),
        ("feat/a".to_string(), false),
        ("feat/b".to_string(), false),
    ];
    let rows = build_sidebar_rows(
        &branches,
        &[],
        &[],
        &[],
        &[],
        Default::default(),
        &state.collapsed,
        groups,
        filter,
    );
    state.rows = rows;
    state.rows_fingerprint = fingerprint;
    state.refresh_tree_positions();
}

fn branch_position(state: &SidebarState, branch: &str) -> Option<(usize, usize)> {
    let i = state
        .rows
        .iter()
        .position(|r| matches!(r, SidebarRow::LocalBranchLeaf { name, .. } if name == branch))?;
    Some(state.tree_positions[i])
}

#[test]
fn tree_positions_follow_each_rows_rebuild() {
    let mut state = SidebarState::new();
    let no_groups = HashSet::new();
    rebuild(&mut state, 1, &no_groups, "");
    assert_eq!(branch_position(&state, "feat/b"), Some((2, 2)));

    // Filter edit: only `feat/b` is left under its parent.
    rebuild(&mut state, 2, &no_groups, "b");
    assert_eq!(state.tree_positions.len(), state.rows.len());
    assert_eq!(branch_position(&state, "feat/b"), Some((1, 1)));

    // Group collapse: its leaves leave the list, the cache follows.
    let collapsed: HashSet<String> = [group_key(SECTION_LOCAL, "feat")].into();
    rebuild(&mut state, 3, &collapsed, "");
    assert_eq!(state.tree_positions.len(), state.rows.len());
    assert_eq!(branch_position(&state, "feat/b"), None);

    // Section collapse leaves only the header.
    state.collapsed.insert(SECTION_LOCAL);
    rebuild(&mut state, 4, &no_groups, "");
    assert_eq!(state.tree_positions.len(), state.rows.len());
    assert_eq!(branch_position(&state, "main"), None);

    // Expanding again restores the positions.
    state.collapsed.remove(SECTION_LOCAL);
    rebuild(&mut state, 5, &no_groups, "");
    assert_eq!(branch_position(&state, "feat/b"), Some((2, 2)));
}
