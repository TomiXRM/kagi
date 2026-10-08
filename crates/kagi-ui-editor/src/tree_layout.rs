//! Revision-keyed display mapping, shared by the virtual tree and keyboard selection.
use super::{visible_tree_indices, EditorWorkspaceView, TreeRowA11y};
use kagi_ui_core::{file_tree::TreeRow, tree_a11y};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct TreeLayoutCache {
    tree_revision: u64,
    collapse_revision: u64,
    layout_for: Option<(u64, u64)>,
    pub visible: Vec<usize>,
    pub a11y: Vec<TreeRowA11y>,
    pub file_rows: Vec<(usize, usize)>,
    pub file_base_indices: Vec<Option<usize>>,
    pub file_row_indices: Vec<Option<usize>>,
    #[cfg(any(test, feature = "gui-e2e"))]
    derivations: (u64, u64),
}

impl TreeLayoutCache {
    pub fn tree_changed(&mut self) {
        self.tree_revision = self.tree_revision.wrapping_add(1);
        self.layout_for = None;
    }

    pub fn collapse_changed(&mut self) {
        self.collapse_revision = self.collapse_revision.wrapping_add(1);
        self.layout_for = None;
    }

    pub fn refresh(&mut self, tree: &[TreeRow], collapsed: &HashSet<usize>) {
        let key = (self.tree_revision, self.collapse_revision);
        if self.layout_for == Some(key) {
            return;
        }
        #[cfg(any(test, feature = "gui-e2e"))]
        {
            self.derivations.0 += 1;
        }
        self.visible = visible_tree_indices(tree, collapsed);
        let levels: Vec<usize> = self
            .visible
            .iter()
            .map(|&index| match &tree[index] {
                TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. } => depth + 1,
            })
            .collect();
        #[cfg(any(test, feature = "gui-e2e"))]
        {
            self.derivations.1 += 1;
        }
        let positions = tree_a11y::sibling_positions(&levels);
        self.a11y = levels
            .into_iter()
            .zip(positions)
            .map(|(level, position)| TreeRowA11y { level, position })
            .collect();
        self.file_rows = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(visible_pos, &index)| match &tree[index] {
                TreeRow::File { file_index, .. } => Some((visible_pos, *file_index)),
                _ => None,
            })
            .collect();
        self.file_row_indices.clear();
        self.file_row_indices.resize(tree.len(), None);
        for (row_index, &(_, file_index)) in self.file_rows.iter().enumerate() {
            self.file_row_indices[file_index] = Some(row_index);
        }
        self.file_base_indices.clear();
        self.file_base_indices.resize(tree.len(), None);
        for (index, row) in tree.iter().enumerate() {
            if let TreeRow::File { file_index, .. } = row {
                self.file_base_indices[*file_index] = Some(index);
            }
        }
        self.layout_for = Some(key);
    }
}

impl EditorWorkspaceView {
    pub(super) fn refresh_tree_layout(&mut self) {
        self.tree_layout.refresh(&self.tree, &self.collapsed);
    }

    /// Actual visibility and accessibility derivations, not render counts.
    #[cfg(feature = "gui-e2e")]
    pub fn tree_layout_derivations(&self) -> (u64, u64) {
        self.tree_layout.derivations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{file, sample_tree};

    #[test]
    fn warm_layout_and_equal_size_replacement_remap_file_rows() {
        let mut cache = TreeLayoutCache::default();
        let tree = sample_tree();
        let collapsed = HashSet::new();
        cache.tree_changed();
        cache.refresh(&tree, &collapsed);
        assert_eq!(cache.visible, vec![0, 1, 2, 3, 4]);
        assert_eq!(cache.file_rows, vec![(1, 0), (3, 1), (4, 2)]);
        assert_eq!(cache.a11y[3].level, 3);
        assert_eq!(cache.a11y[3].position, (1, 1));
        for _ in 0..10 {
            cache.refresh(&tree, &collapsed);
        }
        assert_eq!(cache.derivations, (1, 1));

        let replacement = vec![
            file(0, "a", 4),
            file(0, "b", 3),
            file(0, "c", 2),
            file(0, "d", 1),
            file(0, "e", 0),
        ];
        cache.tree_changed();
        cache.refresh(&replacement, &collapsed);
        assert_eq!(cache.derivations, (2, 2));
        assert_eq!(
            cache.file_rows,
            vec![(0, 4), (1, 3), (2, 2), (3, 1), (4, 0)]
        );
        assert_eq!(
            cache.file_base_indices,
            vec![Some(4), Some(3), Some(2), Some(1), Some(0)]
        );
        assert!(cache.a11y.iter().all(|row| row.level == 1));
        assert_eq!(cache.a11y[3].position, (4, 5));
    }

    #[test]
    fn collapse_rederives_visible_siblings_and_keyboard_mapping() {
        let mut cache = TreeLayoutCache::default();
        let tree = sample_tree();
        let mut collapsed = HashSet::new();
        cache.refresh(&tree, &collapsed);
        collapsed.insert(2);
        cache.collapse_changed();
        cache.refresh(&tree, &collapsed);
        assert_eq!(cache.visible, vec![0, 1, 2, 4]);
        assert_eq!(cache.file_rows, vec![(1, 0), (3, 2)]);
        assert_eq!(cache.a11y[2].position, (2, 2));
        assert_eq!(cache.file_base_indices[1], Some(3));
        collapsed.insert(0);
        cache.collapse_changed();
        cache.refresh(&tree, &collapsed);
        assert_eq!(cache.visible, vec![0, 4]);
        assert_eq!(cache.file_rows, vec![(1, 2)]);
        assert_eq!(cache.a11y[1].position, (2, 2));
        collapsed.clear();
        cache.collapse_changed();
        cache.refresh(&tree, &collapsed);
        assert_eq!(cache.visible, vec![0, 1, 2, 3, 4]);
        assert_eq!(cache.file_rows, vec![(1, 0), (3, 1), (4, 2)]);
        assert_eq!(cache.derivations, (4, 4));
    }
}
