//! Accessibility for the sidebar navigator (#354 slice 3, sidebar).
//!
//! The navigator is one virtualized list whose rows are section headers
//! (collapsible), group headers (collapsible) and leaves (branches, remote
//! branches, tags, worktrees, stashes, pull requests). That is a tree, so it
//! is exposed as `Role::Tree` with `TreeItem` rows carrying their level,
//! expanded state (headers) and position among their siblings. The sidebar
//! has no selection of its own; "current branch / worktree" is part of the
//! row's name.

use kagi_ui_core::i18n::Msg;

use super::sidebar::SidebarRow;

/// What one row tells assistive technology.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeItemSpec {
    pub label: String,
    /// 1 = section header.
    pub level: usize,
    /// `Some` for collapsible headers.
    pub expanded: Option<bool>,
}

fn fill(msg: Msg, args: &[&str]) -> String {
    args.iter()
        .fold(msg.t().to_string(), |s, a| s.replacen("{}", a, 1))
}

/// Level, expanded state and name for one navigator row.
pub fn tree_item(row: &SidebarRow) -> TreeItemSpec {
    let (label, level, expanded) = match row {
        SidebarRow::SectionHeader {
            title,
            count,
            collapsed,
            ..
        } => (
            fill(Msg::A11ySidebarGroup, &[title, &count.to_string()]),
            1,
            Some(!collapsed),
        ),
        SidebarRow::LocalGroupHeader {
            prefix,
            count,
            collapsed,
            ..
        }
        | SidebarRow::RemoteHeader {
            remote: prefix,
            count,
            collapsed,
            ..
        } => (
            fill(Msg::A11ySidebarGroup, &[prefix, &count.to_string()]),
            2,
            Some(!collapsed),
        ),
        SidebarRow::RemoteSubGroup {
            prefix,
            count,
            collapsed,
            ..
        } => (
            fill(Msg::A11ySidebarGroup, &[prefix, &count.to_string()]),
            3,
            Some(!collapsed),
        ),
        SidebarRow::PrGroupHeader {
            title,
            count,
            collapsed,
            ..
        } => (
            fill(Msg::A11ySidebarGroup, &[title, &count.to_string()]),
            2,
            Some(!collapsed),
        ),
        SidebarRow::LocalBranchLeaf {
            name,
            is_head,
            indented,
            ..
        } => (
            if *is_head {
                fill(Msg::A11ySidebarCurrentBranch, &[name])
            } else {
                fill(Msg::A11ySidebarBranch, &[name])
            },
            if *indented { 3 } else { 2 },
            None,
        ),
        SidebarRow::RemoteLeaf { display, depth, .. } => (
            fill(Msg::A11ySidebarRemoteBranch, &[display]),
            2 + *depth as usize,
            None,
        ),
        SidebarRow::Tag { name, .. } => (fill(Msg::A11ySidebarTag, &[name]), 2, None),
        SidebarRow::Worktree {
            name,
            path_label,
            is_current,
            locked,
            ..
        } => {
            let mut s = fill(Msg::A11ySidebarWorktree, &[name, path_label]);
            if *is_current {
                s.push_str(Msg::A11ySidebarCurrentSuffix.t());
            }
            if *locked {
                s.push_str(Msg::A11ySidebarLockedSuffix.t());
            }
            (s, 2, None)
        }
        SidebarRow::Stash { message, .. } => (fill(Msg::A11yStashRow, &[message]), 2, None),
        SidebarRow::PullRequest { pr, .. } => (
            fill(Msg::A11ySidebarPr, &[&pr.number.to_string(), &pr.title]),
            3,
            None,
        ),
    };
    TreeItemSpec {
        label,
        level,
        expanded,
    }
}

/// 1-based position among siblings and sibling count for each row of a
/// flattened tree given each row's level. Siblings are the consecutive rows
/// at the same level since the last row at a lower level (their parent).
pub fn sibling_positions(levels: &[usize]) -> Vec<(usize, usize)> {
    // Linear: one open sibling run per level on a stack; a run closes (and
    // its members learn the size) when a shallower row or the end arrives.
    // Runs every render batch over all navigator rows, which can number in
    // the thousands, so this must not be quadratic.
    let mut out = vec![(0, 0); levels.len()];
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    let close = |run: (usize, Vec<usize>), out: &mut Vec<(usize, usize)>| {
        let size = run.1.len();
        for ix in run.1 {
            out[ix].1 = size;
        }
    };
    for (i, &level) in levels.iter().enumerate() {
        while stack.last().is_some_and(|(l, _)| *l > level) {
            let run = stack.pop().expect("checked");
            close(run, &mut out);
        }
        match stack.last_mut() {
            Some((l, members)) if *l == level => members.push(i),
            _ => stack.push((level, vec![i])),
        }
        out[i].0 = stack.last().expect("pushed").1.len();
    }
    while let Some(run) = stack.pop() {
        close(run, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings_restart_under_each_parent() {
        // LOCAL (1) > feat/ (2) > a, b (3) ; main (2) ; TAGS (1) > v1 (2)
        let levels = [1, 2, 3, 3, 2, 1, 2];
        assert_eq!(
            sibling_positions(&levels),
            vec![(1, 2), (1, 2), (1, 2), (2, 2), (2, 2), (2, 2), (1, 1)]
        );
        assert_eq!(sibling_positions(&[]), vec![]);
    }

    #[test]
    fn headers_are_expandable_and_leaves_carry_state_in_the_name() {
        let section = tree_item(&SidebarRow::SectionHeader {
            section: "local",
            title: "LOCAL BRANCHES",
            count: 3,
            collapsed: true,
        });
        assert_eq!((section.level, section.expanded), (1, Some(false)));
        assert!(section.label.contains("LOCAL BRANCHES") && section.label.contains('3'));

        let head = tree_item(&SidebarRow::LocalBranchLeaf {
            name: "main".into(),
            display_label: "main".into(),
            is_head: true,
            indented: false,
        });
        let other = tree_item(&SidebarRow::LocalBranchLeaf {
            name: "feat/x".into(),
            display_label: "x".into(),
            is_head: false,
            indented: true,
        });
        assert_eq!((head.level, head.expanded), (2, None));
        assert_eq!(other.level, 3);
        assert!(head.label.contains("main"));
        assert!(
            other.label.contains("feat/x"),
            "full name, not the stripped label"
        );
        assert_ne!(
            head.label.replace("main", ""),
            other.label.replace("feat/x", ""),
            "the current branch is named as current"
        );
    }
}
