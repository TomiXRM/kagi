//! The refs one recorded operation moved, by OID (#334 slice 2a, ADR-0214 §4).
//!
//! `Backend::run` reads [`RefSnapshot`] before and after executing and stores
//! [`diff`] on the oplog entry. Unlike the reflog time window (§3, an
//! estimate), this is the exact record a restore or revert may build on.

use std::collections::BTreeMap;

/// One ref's change. `None` = the ref did not exist (created / deleted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefMove {
    /// `HEAD` (this worktree's) or a branch ref (`refs/heads/main`).
    pub refname: String,
    pub old: Option<String>,
    pub new: Option<String>,
    /// `HEAD` only: the branch it pointed at; `None` = detached (or a branch).
    pub old_symbolic: Option<String>,
    pub new_symbolic: Option<String>,
}

/// HEAD and every local branch at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefSnapshot {
    /// The commit HEAD resolves to; `None` on an unborn branch.
    pub head_oid: Option<String>,
    /// The branch ref HEAD points at; `None` when detached.
    pub head_symbolic: Option<String>,
    /// `refs/heads/*` → OID.
    pub branches: BTreeMap<String, String>,
}

/// Every ref that differs between `before` and `after`: HEAD first (when its
/// target or commit changed), then branches in name order.
pub fn diff(before: &RefSnapshot, after: &RefSnapshot) -> Vec<RefMove> {
    let mut moves = Vec::new();
    if (&before.head_oid, &before.head_symbolic) != (&after.head_oid, &after.head_symbolic) {
        moves.push(RefMove {
            refname: "HEAD".into(),
            old: before.head_oid.clone(),
            new: after.head_oid.clone(),
            old_symbolic: before.head_symbolic.clone(),
            new_symbolic: after.head_symbolic.clone(),
        });
    }
    let names: std::collections::BTreeSet<&String> = before
        .branches
        .keys()
        .chain(after.branches.keys())
        .collect();
    for name in names {
        let (old, new) = (before.branches.get(name), after.branches.get(name));
        if old != new {
            moves.push(RefMove {
                refname: name.clone(),
                old: old.cloned(),
                new: new.cloned(),
                old_symbolic: None,
                new_symbolic: None,
            });
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(head: (&str, Option<&str>), branches: &[(&str, &str)]) -> RefSnapshot {
        RefSnapshot {
            head_oid: Some(head.0.into()),
            head_symbolic: head.1.map(Into::into),
            branches: branches
                .iter()
                .map(|(n, o)| (n.to_string(), o.to_string()))
                .collect(),
        }
    }

    #[test]
    fn nothing_moved_is_empty() {
        let s = snap(("a", Some("refs/heads/main")), &[("refs/heads/main", "a")]);
        assert!(diff(&s, &s.clone()).is_empty());
    }

    #[test]
    fn a_checkout_moves_head_target_without_moving_any_branch() {
        let branches = [("refs/heads/main", "a"), ("refs/heads/f", "a")];
        let before = snap(("a", Some("refs/heads/main")), &branches);
        let after = snap(("a", Some("refs/heads/f")), &branches);
        assert_eq!(
            diff(&before, &after),
            vec![RefMove {
                refname: "HEAD".into(),
                old: Some("a".into()),
                new: Some("a".into()),
                old_symbolic: Some("refs/heads/main".into()),
                new_symbolic: Some("refs/heads/f".into()),
            }]
        );
    }

    #[test]
    fn creation_and_deletion_have_an_absent_side() {
        let before = snap(
            ("a", Some("refs/heads/main")),
            &[("refs/heads/main", "a"), ("refs/heads/gone", "b")],
        );
        let after = snap(
            ("a", Some("refs/heads/main")),
            &[("refs/heads/main", "a"), ("refs/heads/new", "a")],
        );
        let moves = diff(&before, &after);
        let by_name: Vec<(&str, Option<&str>, Option<&str>)> = moves
            .iter()
            .map(|m| (m.refname.as_str(), m.old.as_deref(), m.new.as_deref()))
            .collect();
        assert_eq!(
            by_name,
            vec![
                ("refs/heads/gone", Some("b"), None),
                ("refs/heads/new", None, Some("a")),
            ]
        );
    }

    #[test]
    fn a_commit_moves_the_branch_and_heads_commit() {
        let before = snap(("a", Some("refs/heads/main")), &[("refs/heads/main", "a")]);
        let after = snap(("b", Some("refs/heads/main")), &[("refs/heads/main", "b")]);
        let names: Vec<String> = diff(&before, &after)
            .into_iter()
            .map(|m| m.refname)
            .collect();
        assert_eq!(names, vec!["HEAD", "refs/heads/main"]);
    }
}
