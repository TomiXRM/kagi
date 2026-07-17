//! Branch solo — hide every commit outside one branch's history (ADR-0121
//! Phase A: behaviour-preserving relocation out of `mod.rs`).
//!
//! `toggle_branch_solo` filters the graph to the ancestry closure of a branch
//! tip and re-runs the lane layout on the sub-DAG; toggling again (or soloing
//! another branch) restores the saved full row set. The saved state lives in
//! [`BranchSolo`] on the active [`super::TabViewState`].

use std::collections::{HashMap, HashSet};

use gpui::{Context, SharedString};

use kagi_git::CommitId;

use super::{BranchSolo, FooterStatus, KagiApp, ToastKind};

pub(super) fn collect_history_commits(
    target: &CommitId,
    parents_by_id: &HashMap<CommitId, Vec<CommitId>>,
) -> HashSet<CommitId> {
    let mut visible = HashSet::new();
    let mut stack = vec![target.clone()];

    while let Some(id) = stack.pop() {
        if !visible.insert(id.clone()) {
            continue;
        }
        if let Some(parents) = parents_by_id.get(&id) {
            stack.extend(parents.iter().cloned());
        }
    }

    visible
}

impl KagiApp {
    fn branch_history_commits(&self, target: &CommitId) -> HashSet<CommitId> {
        let parents_by_id: HashMap<CommitId, Vec<CommitId>> = self
            .active_view
            .rows
            .iter()
            .map(|row| (row.id.clone(), row.parents.clone()))
            .collect();
        collect_history_commits(target, &parents_by_id)
    }

    pub fn toggle_branch_solo(&mut self, name: String, target: CommitId, cx: &mut Context<Self>) {
        let already_soloed = self
            .active_view
            .branch_solo
            .as_ref()
            .is_some_and(|solo| solo.name == name && solo.target == target);

        // Remember the currently selected commit so it can be remapped into
        // the new row indexing (or dropped if hidden).
        let selected_id: Option<CommitId> = self
            .selected
            .and_then(|idx| self.active_view.rows.get(idx))
            .map(|row| row.id.clone());

        if already_soloed {
            // Restore the full row set saved at solo-on.
            if let Some(solo) = self.active_view.branch_solo.take() {
                self.active_view.rows = solo.saved_rows;
                self.active_view.details = solo.saved_details;
                self.active_view.commit_row_index = solo.saved_row_index;
            }
            self.selected =
                selected_id.and_then(|id| self.active_view.commit_row_index.get(&id).copied());
            self.status_footer = FooterStatus::Idle(SharedString::from("Solo off"));
            self.push_toast(ToastKind::Info, "Solo off", cx);
            return;
        }

        // Toggling from one solo to another: restore the full set first so the
        // filter below always starts from the complete graph.
        if let Some(prev) = self.active_view.branch_solo.take() {
            self.active_view.rows = prev.saved_rows;
            self.active_view.details = prev.saved_details;
            self.active_view.commit_row_index = prev.saved_row_index;
        }

        let visible_commits = self.branch_history_commits(&target);

        // Filter rows to the branch history (order preserved) and re-run the
        // lane layout on the sub-DAG so lanes/edges stay consistent — simply
        // dropping rows would leave other branches' pass-through lane lines
        // floating without nodes.
        let saved_rows = std::mem::take(&mut self.active_view.rows);
        let saved_details = std::mem::take(&mut self.active_view.details);
        let saved_row_index = std::mem::take(&mut self.active_view.commit_row_index);

        let keep: Vec<usize> = saved_rows
            .iter()
            .enumerate()
            .filter(|(_, r)| visible_commits.contains(&r.id))
            .map(|(i, _)| i)
            .collect();
        // Minimal domain commits for the layout (it reads only id/parents).
        let empty_sig = || kagi_domain::commit::Signature {
            name: String::new(),
            email: String::new(),
            time: 0,
        };
        let sub_commits: Vec<kagi_domain::commit::Commit> = keep
            .iter()
            .map(|&i| kagi_domain::commit::Commit {
                id: saved_rows[i].id.clone(),
                parents: saved_rows[i].parents.clone(),
                author: empty_sig(),
                committer: empty_sig(),
                summary: String::new(),
                message: String::new(),
            })
            .collect();
        let graph = kagi_domain::graph::layout_with(
            &sub_commits,
            kagi_domain::graph::GraphLayoutMode::Compact,
        );

        let mut rows = Vec::with_capacity(keep.len());
        let mut details = Vec::with_capacity(keep.len());
        let mut commit_row_index = HashMap::with_capacity(keep.len());
        for (new_ix, &old_ix) in keep.iter().enumerate() {
            let mut row = saved_rows[old_ix].clone();
            let g = &graph.rows[new_ix];
            row.lane = g.lane;
            row.node_color = g.color;
            row.edges = g.edges.clone();
            row.lane_count = graph.lane_count;
            commit_row_index.insert(row.id.clone(), new_ix);
            rows.push(row);
            details.push(saved_details[old_ix].clone());
        }
        self.active_view.rows = rows;
        self.active_view.details = details;
        self.active_view.commit_row_index = commit_row_index;
        self.selected =
            selected_id.and_then(|id| self.active_view.commit_row_index.get(&id).copied());

        klog!(
            "solo: {} rows={} (of {})",
            name,
            keep.len(),
            saved_rows.len()
        );
        self.active_view.branch_solo = Some(BranchSolo {
            name: name.clone(),
            target,
            visible_commits,
            saved_rows,
            saved_details,
            saved_row_index,
        });
        self.status_footer = FooterStatus::Idle(SharedString::from(format!("Solo: {}", name)));
        self.push_toast(ToastKind::Info, format!("Solo: {}", name), cx);
    }
}

#[cfg(test)]
mod solo_history_tests {
    use super::collect_history_commits;
    use kagi_git::CommitId;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn history_collection_follows_all_merge_parents() {
        let tip = CommitId("m".to_string());
        let a = CommitId("a".to_string());
        let b = CommitId("b".to_string());
        let root = CommitId("root".to_string());
        let mut parents = HashMap::new();
        parents.insert(tip.clone(), vec![a.clone(), b.clone()]);
        parents.insert(a.clone(), vec![root.clone()]);
        parents.insert(b.clone(), vec![root.clone()]);
        parents.insert(root.clone(), vec![]);

        let actual = collect_history_commits(&tip, &parents);
        let expected = HashSet::from([tip, a, b, root]);
        assert_eq!(actual, expected);
    }
}
