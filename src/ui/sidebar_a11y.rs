//! Accessibility for the sidebar navigator (#354 slice 3, sidebar).
//!
//! Each navigator pane is a virtualized list whose rows are its section header
//! (collapsible), group headers (collapsible) and leaves (branches, remote
//! branches, tags, worktrees, stashes). That is a tree, so each pane is
//! exposed as `Role::Tree` with `TreeItem` rows carrying their level,
//! expanded state (headers) and position among their siblings. The sidebar
//! has no selection of its own; "current branch / worktree" is part of the
//! row's name.

use kagi_domain::text_safety::append_sanitized_control_bytes;
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

/// Substitute each `{}` of `msg` with the next argument, neutralizing control
/// bytes exactly as the visible rows do (#356: branch / remote / tag / worktree
/// / stash text is remote-origin). Placeholders are taken from the
/// template only, so a `{}` inside a branch name or path is not filled by the
/// following argument.
fn fill(msg: Msg, args: &[&str]) -> String {
    let mut rest = msg.t();
    let args_len: usize = args.iter().map(|a| a.len()).sum();
    let mut out = String::with_capacity(rest.len() + args_len);
    for arg in args {
        let Some(at) = rest.find("{}") else { break };
        out.push_str(&rest[..at]);
        append_sanitized_control_bytes(&mut out, arg);
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// Tree level of one navigator row (1 = section header). Allocation-free, so
/// sibling positions can be derived from every row without building names.
pub fn level(row: &SidebarRow) -> usize {
    match row {
        SidebarRow::SectionHeader { .. } => 1,
        SidebarRow::LocalGroupHeader { .. }
        | SidebarRow::RemoteHeader { .. }
        | SidebarRow::Tag { .. }
        | SidebarRow::Worktree { .. }
        | SidebarRow::Stash { .. } => 2,
        SidebarRow::RemoteSubGroup { .. } => 3,
        SidebarRow::LocalBranchLeaf { indented, .. } => {
            if *indented {
                3
            } else {
                2
            }
        }
        SidebarRow::RemoteLeaf { depth, .. } => 2 + *depth as usize,
    }
}

/// Level, expanded state and name for one navigator row.
pub fn tree_item(row: &SidebarRow) -> TreeItemSpec {
    let (label, expanded) = match row {
        SidebarRow::SectionHeader {
            title,
            count,
            collapsed,
            ..
        } => (
            fill(Msg::A11ySidebarGroup, &[title, &count.to_string()]),
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
            Some(!collapsed),
        ),
        SidebarRow::RemoteSubGroup {
            prefix,
            count,
            collapsed,
            ..
        } => (
            fill(Msg::A11ySidebarGroup, &[prefix, &count.to_string()]),
            Some(!collapsed),
        ),
        SidebarRow::LocalBranchLeaf { name, is_head, .. } => (
            if *is_head {
                fill(Msg::A11ySidebarCurrentBranch, &[name])
            } else {
                fill(Msg::A11ySidebarBranch, &[name])
            },
            None,
        ),
        SidebarRow::RemoteLeaf { display, .. } => {
            (fill(Msg::A11ySidebarRemoteBranch, &[display]), None)
        }
        SidebarRow::Tag { name, .. } => (fill(Msg::A11ySidebarTag, &[name]), None),
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
            (s, None)
        }
        SidebarRow::Stash { message, .. } => (fill(Msg::A11yStashRow, &[message]), None),
    };
    TreeItemSpec {
        label,
        level: level(row),
        expanded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::text_safety::sanitize_control_bytes;

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

    #[test]
    fn names_neutralize_control_bytes_like_the_visible_rows() {
        let evil = "origin/\u{1b}[31mx\u{9b}y\r";
        let safe = sanitize_control_bytes(evil);
        let rows = [
            SidebarRow::RemoteLeaf {
                display: evil.into(),
                display_label: "x".into(),
                target: kagi_git::CommitId("a".repeat(40)),
                depth: 1,
            },
            SidebarRow::LocalBranchLeaf {
                name: evil.into(),
                display_label: evil.into(),
                is_head: true,
                indented: false,
            },
            SidebarRow::RemoteHeader {
                key: "remote:x".into(),
                remote: evil.into(),
                count: 1,
                collapsed: false,
            },
            SidebarRow::LocalGroupHeader {
                key: "local:x".into(),
                prefix: evil.into(),
                count: 1,
                collapsed: false,
            },
            SidebarRow::RemoteSubGroup {
                key: "remote:x:y".into(),
                prefix: evil.into(),
                count: 1,
                collapsed: false,
            },
            SidebarRow::Stash {
                index: 0,
                message: evil.into(),
            },
        ];
        for row in &rows {
            let label = tree_item(row).label;
            assert!(label.contains(&safe), "{row:?} -> {label:?}");
            assert!(
                !label.chars().any(|c| c.is_control()),
                "{row:?} leaked a control byte: {label:?}"
            );
        }

        let worktree = tree_item(&SidebarRow::Worktree {
            name: "wt\u{1b}]0;".into(),
            path: "/tmp/wt".into(),
            path_label: "/tmp/\u{7f}wt".into(),
            is_current: false,
            is_main: false,
            locked: false,
            port: None,
        })
        .label;
        assert!(worktree.contains(&sanitize_control_bytes("wt\u{1b}]0;")));
        assert!(worktree.contains(&sanitize_control_bytes("/tmp/\u{7f}wt")));
        assert!(!worktree.chars().any(|c| c.is_control()));
    }

    #[test]
    fn placeholders_inside_names_are_not_filled() {
        let label = tree_item(&SidebarRow::Worktree {
            name: "a{}b".into(),
            path: "/p/{}".into(),
            path_label: "/p/{}".into(),
            is_current: false,
            is_main: false,
            locked: false,
            port: None,
        })
        .label;
        assert!(label.contains("a{}b"), "{label:?}");
        assert!(label.contains("/p/{}"), "{label:?}");
    }
}
