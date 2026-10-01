//! Op-revert / restore-to-point planning (#334 slice 2b, ADR-0214 §5): which
//! branches to put back, from the **recorded** ref moves of Operation Log
//! entries alone. Pure — the backend supplies the entries and current refs,
//! applies the result as one `git update-ref --stdin` transaction.

use crate::plan_note::OplogRestoreNote;
use crate::ref_moves::{RefMove, RefSnapshot};
use std::collections::BTreeMap;

/// One Operation Log entry of the repository, as the planner needs it.
#[derive(Debug, Clone)]
pub struct RecordedEntry {
    pub id: u64,
    pub op: String,
    pub ref_moves: Option<Vec<RefMove>>,
}

/// Put `refname` back: it must be at `expect` now and goes to `restore_to`
/// (`None` = absent: a ref the range created is deleted, one it deleted is
/// recreated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefRestore {
    pub refname: String,
    pub expect: Option<String>,
    pub restore_to: Option<String>,
}

/// What an executed restore did to one ref, and where its tip was retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredRef {
    pub refname: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// `refs/kagi/backups/...` holding `from` (none when the ref was absent).
    pub backup: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    /// Undo exactly one entry.
    Revert,
    /// Undo every entry newer than the target (state right after it).
    RestoreTo,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestorePlan {
    pub restores: Vec<RefRestore>,
    pub blockers: Vec<OplogRestoreNote>,
}

/// HEAD that changed its branch or was / became detached. A HEAD that only
/// followed its branch (a commit) is that branch's move, not a checkout.
fn switches_head(m: &RefMove) -> bool {
    m.refname == "HEAD" && !(m.old_symbolic.is_some() && m.old_symbolic == m.new_symbolic)
}

fn branch_moves(moves: &[RefMove]) -> impl Iterator<Item = &RefMove> {
    moves.iter().filter(|m| m.refname != "HEAD")
}

/// `entries` oldest first (by id). `current` = the refs now.
pub fn plan(
    entries: &[RecordedEntry],
    target: u64,
    mode: RestoreMode,
    current: &RefSnapshot,
) -> RestorePlan {
    let Some(pos) = entries.iter().position(|e| e.id == target) else {
        return RestorePlan {
            restores: Vec::new(),
            blockers: vec![OplogRestoreNote::EntryNotLoaded { id: target }],
        };
    };
    let range = match mode {
        RestoreMode::Revert => &entries[pos..=pos],
        RestoreMode::RestoreTo => &entries[pos + 1..],
    };
    let mut blockers = Vec::new();
    for e in range {
        match &e.ref_moves {
            None => blockers.push(OplogRestoreNote::NotRecorded {
                id: e.id,
                op: e.op.clone(),
            }),
            Some(moves) if moves.iter().any(switches_head) => {
                blockers.push(OplogRestoreNote::HeadMoved {
                    id: e.id,
                    op: e.op.clone(),
                })
            }
            Some(_) => {}
        }
    }
    if mode == RestoreMode::Revert {
        for m in entries[pos]
            .ref_moves
            .iter()
            .flatten()
            .filter(|m| m.refname != "HEAD")
        {
            let later = entries[pos + 1..].iter().find(|later| {
                later
                    .ref_moves
                    .iter()
                    .flatten()
                    .any(|x| x.refname == m.refname)
            });
            if let Some(later) = later {
                blockers.push(OplogRestoreNote::LaterEntryMoved {
                    refname: m.refname.clone(),
                    id: later.id,
                    op: later.op.clone(),
                });
            }
        }
    }
    // refname → (restore_to = oldest `old` in the range, expect = newest `new`).
    let mut table: BTreeMap<&str, (Option<String>, Option<String>)> = BTreeMap::new();
    for e in range {
        for m in branch_moves(e.ref_moves.as_deref().unwrap_or_default()) {
            table
                .entry(m.refname.as_str())
                .and_modify(|(_, expect)| *expect = m.new.clone())
                .or_insert((m.old.clone(), m.new.clone()));
        }
    }
    let restores: Vec<RefRestore> = table
        .into_iter()
        .filter(|(_, (to, expect))| to != expect)
        .map(|(refname, (restore_to, expect))| RefRestore {
            refname: refname.to_string(),
            expect,
            restore_to,
        })
        .collect();
    if blockers.is_empty() && restores.is_empty() {
        blockers.push(OplogRestoreNote::NothingToRestore);
    }
    for r in &restores {
        let now = current.branches.get(&r.refname).cloned();
        if now != r.expect {
            blockers.push(OplogRestoreNote::RefMovedSince {
                refname: r.refname.clone(),
                expected: r.expect.clone(),
                current: now,
            });
        }
    }
    RestorePlan { restores, blockers }
}

const ABSENT: &str = "-";

/// The plan carries its restores as lines (`restore <ref> <to|-> <expect|->`)
/// so preflight re-plans and compares exactly what was confirmed.
pub fn to_lines(restores: &[RefRestore]) -> Vec<String> {
    restores
        .iter()
        .map(|r| {
            format!(
                "restore {} {} {}",
                r.refname,
                r.restore_to.as_deref().unwrap_or(ABSENT),
                r.expect.as_deref().unwrap_or(ABSENT)
            )
        })
        .collect()
}

pub fn from_lines(lines: &[String]) -> Result<Vec<RefRestore>, String> {
    let side = |s: &str| (s != ABSENT).then(|| s.to_string());
    lines
        .iter()
        .map(
            |line| match line.split(' ').collect::<Vec<_>>().as_slice() {
                ["restore", refname, to, expect] if refname.starts_with("refs/heads/") => {
                    Ok(RefRestore {
                        refname: refname.to_string(),
                        expect: side(expect),
                        restore_to: side(to),
                    })
                }
                _ => Err(format!("not a restore line: {line}")),
            },
        )
        .collect()
}

/// The `git update-ref --stdin` script: one transaction, every old value
/// re-checked by git (`create` requires absence).
pub fn transaction(restores: &[RefRestore]) -> String {
    let mut script = String::new();
    for r in restores {
        let line = match (&r.restore_to, &r.expect) {
            (Some(to), Some(expect)) => format!("update {} {to} {expect}\n", r.refname),
            (Some(to), None) => format!("create {} {to}\n", r.refname),
            (None, Some(expect)) => format!("delete {} {expect}\n", r.refname),
            (None, None) => continue,
        };
        script.push_str(&line);
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(refname: &str, old: Option<&str>, new: Option<&str>) -> RefMove {
        RefMove {
            refname: refname.into(),
            old: old.map(Into::into),
            new: new.map(Into::into),
            old_symbolic: None,
            new_symbolic: None,
        }
    }

    fn head(old: &str, new: &str, from: &str, to: &str) -> RefMove {
        RefMove {
            refname: "HEAD".into(),
            old: Some(old.into()),
            new: Some(new.into()),
            old_symbolic: Some(from.into()),
            new_symbolic: Some(to.into()),
        }
    }

    fn entry(id: u64, moves: Option<Vec<RefMove>>) -> RecordedEntry {
        RecordedEntry {
            id,
            op: format!("op{id}"),
            ref_moves: moves,
        }
    }

    fn refs(branches: &[(&str, &str)]) -> RefSnapshot {
        RefSnapshot {
            head_oid: None,
            head_symbolic: None,
            branches: branches
                .iter()
                .map(|(n, o)| (n.to_string(), o.to_string()))
                .collect(),
        }
    }

    const A: &str = "refs/heads/a";
    const B: &str = "refs/heads/b";

    #[test]
    fn restore_combines_the_range_oldest_old_to_newest_new() {
        let entries = [
            entry(1, Some(vec![mv(A, None, Some("a1"))])),
            entry(2, Some(vec![mv(A, Some("a1"), Some("a2"))])),
            entry(3, Some(vec![mv(B, None, Some("b1"))])),
            entry(4, Some(vec![mv(A, Some("a2"), Some("a3"))])),
        ];
        let now = refs(&[(A, "a3"), (B, "b1")]);
        let p = plan(&entries, 1, RestoreMode::RestoreTo, &now);
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert_eq!(
            p.restores,
            vec![
                RefRestore {
                    refname: A.into(),
                    expect: Some("a3".into()),
                    restore_to: Some("a1".into())
                },
                RefRestore {
                    refname: B.into(),
                    expect: Some("b1".into()),
                    restore_to: None
                },
            ]
        );
    }

    #[test]
    fn a_ref_moved_and_moved_back_is_left_alone() {
        let entries = [
            entry(1, Some(vec![])),
            entry(2, Some(vec![mv(A, Some("x"), Some("y"))])),
            entry(3, Some(vec![mv(A, Some("y"), Some("x"))])),
        ];
        let p = plan(&entries, 1, RestoreMode::RestoreTo, &refs(&[(A, "x")]));
        assert_eq!(p.restores, vec![]);
        assert_eq!(p.blockers, vec![OplogRestoreNote::NothingToRestore]);
    }

    #[test]
    fn revert_is_blocked_only_by_a_later_move_of_the_same_ref() {
        let entries = [
            entry(1, Some(vec![mv(A, Some("a0"), Some("a1"))])),
            entry(2, Some(vec![mv(B, Some("b0"), Some("b1"))])),
        ];
        let now = refs(&[(A, "a1"), (B, "b1")]);
        let p = plan(&entries, 1, RestoreMode::Revert, &now);
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert_eq!(p.restores.len(), 1);

        let entries = [
            entries[0].clone(),
            entry(2, Some(vec![mv(A, Some("a1"), Some("a2"))])),
        ];
        let p = plan(&entries, 1, RestoreMode::Revert, &refs(&[(A, "a2")]));
        assert!(p.blockers.contains(&OplogRestoreNote::LaterEntryMoved {
            refname: A.into(),
            id: 2,
            op: "op2".into()
        }));
    }

    #[test]
    fn an_unrecorded_entry_in_the_range_blocks_never_guessed() {
        let entries = [
            entry(1, Some(vec![])),
            entry(2, None),
            entry(3, Some(vec![mv(A, Some("a0"), Some("a1"))])),
        ];
        let p = plan(&entries, 1, RestoreMode::RestoreTo, &refs(&[(A, "a1")]));
        assert!(p.blockers.contains(&OplogRestoreNote::NotRecorded {
            id: 2,
            op: "op2".into()
        }));
    }

    #[test]
    fn a_checkout_blocks_but_a_head_following_its_branch_does_not() {
        let commit = entry(
            2,
            Some(vec![
                head("c0", "c1", "refs/heads/a", "refs/heads/a"),
                mv(A, Some("c0"), Some("c1")),
            ]),
        );
        let entries = [entry(1, Some(vec![])), commit.clone()];
        let p = plan(&entries, 1, RestoreMode::RestoreTo, &refs(&[(A, "c1")]));
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);

        let checkout = entry(
            3,
            Some(vec![head("c1", "c1", "refs/heads/a", "refs/heads/b")]),
        );
        let entries = [entry(1, Some(vec![])), commit, checkout];
        let p = plan(&entries, 1, RestoreMode::RestoreTo, &refs(&[(A, "c1")]));
        assert!(p.blockers.contains(&OplogRestoreNote::HeadMoved {
            id: 3,
            op: "op3".into()
        }));
    }

    #[test]
    fn a_ref_moved_outside_the_record_blocks() {
        let entries = [
            entry(1, Some(vec![])),
            entry(2, Some(vec![mv(A, Some("a0"), Some("a1"))])),
        ];
        let p = plan(&entries, 1, RestoreMode::RestoreTo, &refs(&[(A, "zz")]));
        assert_eq!(
            p.blockers,
            vec![OplogRestoreNote::RefMovedSince {
                refname: A.into(),
                expected: Some("a1".into()),
                current: Some("zz".into()),
            }]
        );
    }

    #[test]
    fn lines_round_trip_and_become_one_checked_transaction() {
        let restores = vec![
            RefRestore {
                refname: A.into(),
                expect: Some("a3".into()),
                restore_to: Some("a1".into()),
            },
            RefRestore {
                refname: B.into(),
                expect: Some("b1".into()),
                restore_to: None,
            },
            RefRestore {
                refname: "refs/heads/c".into(),
                expect: None,
                restore_to: Some("c1".into()),
            },
        ];
        assert_eq!(from_lines(&to_lines(&restores)).unwrap(), restores);
        assert_eq!(
            transaction(&restores),
            "update refs/heads/a a1 a3\ndelete refs/heads/b b1\ncreate refs/heads/c c1\n"
        );
        assert!(from_lines(&["restore HEAD x y".into()]).is_err());
    }
}
