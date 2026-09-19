//! Editing a pull request's fields - reviewers, assignees, labels - as the
//! diff `gh pr edit` is sent (ADR-0200 §11). Split out of `github.rs` when it
//! passed its LOC ceiling; re-exported from there so callers do not move.

/// The reviewer / assignee / label changes one `gh pr edit` call carries.
///
/// Add and remove lists, not target lists: `gh pr edit` speaks
/// `--add-reviewer` / `--remove-reviewer` (and the assignee and label pairs),
/// so a *diff* is what the CLI accepts. It is also the only safe shape — a
/// "set these labels" request would silently drop a label somebody else added
/// between the picker opening and the user confirming.
///
/// Pure data: [`PrFieldEdit::diff`] computes one field's two lists, and the
/// caller fills the three pairs it wants to change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrFieldEdit {
    pub add_reviewers: Vec<String>,
    pub remove_reviewers: Vec<String>,
    pub add_assignees: Vec<String>,
    pub remove_assignees: Vec<String>,
    pub add_labels: Vec<String>,
    pub remove_labels: Vec<String>,
}

impl PrFieldEdit {
    /// Nothing to send. `gh pr edit` with no field flags is a request that
    /// changes nothing, so the plan refuses it rather than spending a round
    /// trip (and an oplog receipt) on a no-op.
    pub fn is_empty(&self) -> bool {
        self.add_reviewers.is_empty()
            && self.remove_reviewers.is_empty()
            && self.add_assignees.is_empty()
            && self.remove_assignees.is_empty()
            && self.add_labels.is_empty()
            && self.remove_labels.is_empty()
    }

    /// Pure diff: `(add, remove)` to get from `current` to `next`.
    ///
    /// Set difference both ways, order-stable — adds follow `next`, removes
    /// follow `current` — so the argument vector is deterministic and a test
    /// can assert it. Duplicates on either side collapse: one flag per value,
    /// never two.
    ///
    /// Case-**sensitive** on purpose. GitHub logins differ only in case from
    /// nothing (they are unique case-insensitively), but labels genuinely can
    /// (`bug` and `Bug` are two labels), and folding case here would make
    /// "rename the label's case" look like no change at all.
    pub fn diff(current: &[String], next: &[String]) -> (Vec<String>, Vec<String>) {
        let pick = |from: &[String], other: &[String]| {
            let mut out: Vec<String> = Vec::new();
            for value in from {
                if !other.contains(value) && !out.contains(value) {
                    out.push(value.clone());
                }
            }
            out
        };
        (pick(next, current), pick(current, next))
    }
}

#[cfg(test)]
mod pr_field_edit_tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Nothing selected is not an edit: `gh pr edit` with no field flags is a
    /// round trip that changes nothing.
    #[test]
    fn an_edit_with_no_lists_is_empty() {
        assert!(PrFieldEdit::default().is_empty());
        for edit in [
            PrFieldEdit {
                add_reviewers: v(&["bob"]),
                ..Default::default()
            },
            PrFieldEdit {
                remove_labels: v(&["bug"]),
                ..Default::default()
            },
            PrFieldEdit {
                add_assignees: v(&["carol"]),
                ..Default::default()
            },
        ] {
            assert!(!edit.is_empty(), "{edit:?}");
        }
    }

    #[test]
    fn diff_is_a_two_way_set_difference_in_a_stable_order() {
        // No change: neither side has anything the other lacks.
        assert_eq!(
            PrFieldEdit::diff(&v(&["bob", "carol"]), &v(&["carol", "bob"])),
            (vec![], vec![])
        );
        // Pure add — adds follow `next`'s order.
        assert_eq!(
            PrFieldEdit::diff(&v(&["bob"]), &v(&["bob", "dave", "carol"])),
            (v(&["dave", "carol"]), vec![])
        );
        // Pure remove — removes follow `current`'s order.
        assert_eq!(
            PrFieldEdit::diff(&v(&["bob", "carol", "dave"]), &v(&["carol"])),
            (vec![], v(&["bob", "dave"]))
        );
        // A swap is both at once.
        assert_eq!(
            PrFieldEdit::diff(&v(&["bob"]), &v(&["carol"])),
            (v(&["carol"]), v(&["bob"]))
        );
    }

    /// A value listed twice must not produce two flags: `gh` would send the
    /// same request twice over, and the argv assertion would be unstable.
    #[test]
    fn duplicates_in_the_input_collapse_to_one_flag() {
        assert_eq!(
            PrFieldEdit::diff(&v(&["bob", "bob"]), &v(&["carol", "carol", "dave"])),
            (v(&["carol", "dave"]), v(&["bob"]))
        );
    }

    /// GitHub labels are case-sensitive (`bug` and `Bug` coexist), so a case
    /// change is a real edit, not a no-op.
    #[test]
    fn diff_is_case_sensitive() {
        assert_eq!(
            PrFieldEdit::diff(&v(&["bug"]), &v(&["Bug"])),
            (v(&["Bug"]), v(&["bug"]))
        );
    }
}
