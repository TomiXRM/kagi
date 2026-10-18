//! Per-row decoration a diff embedding adds to the shared diff list (#351):
//! a gutter marker beside some rows and, for an expanded row, one extra list
//! item right under it. Rows themselves are never split or renumbered; only
//! the list's item index shifts while something is expanded.

use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, Window};
use kagi_ui_core::variable_list::VariableListState;

use crate::ui::diff_split::SplitDiffRow;
use crate::ui::diff_view::DiffRow;
use crate::ui::theme;

/// The gutter column's width (a badge with a count fits).
pub(crate) const GUTTER_W: f32 = 22.;

/// Which gutter a marker belongs to in the side-by-side view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GutterSide {
    Left,
    Right,
}

/// What an embedding decorates. `row` is always an index into the diff rows.
pub(crate) trait RowOverlay {
    /// The marker for `row`: on `side` in the split view, `None` in the
    /// unified view (one gutter for both sides).
    fn marker(&self, row: usize, side: Option<GutterSide>, cx: &mut App) -> Option<AnyElement>;
    /// The embedding's retained projection for this diff and expansion state.
    fn projection(&self) -> Rc<Projection>;
    /// What goes under a list row whose expanded rows are `rows`.
    fn expansion(&self, rows: &[usize], window: &mut Window, cx: &mut App) -> AnyElement;
}

/// The diff rows each base list row shows: one unified; a pair's two split.
pub(crate) fn layout(rows: &Arc<Vec<DiffRow>>, split: bool) -> Vec<[Option<usize>; 2]> {
    if !split {
        return (0..rows.len()).map(|ix| [Some(ix), None]).collect();
    }
    crate::ui::diff_split::split_projection(rows)
        .rows
        .iter()
        .map(|row| match *row {
            SplitDiffRow::Full(ix) => [Some(ix), None],
            SplitDiffRow::Pair { left, right } => [left, right.filter(|r| Some(*r) != left)],
        })
        .collect()
}

/// One list item: a base row, or the expansion under base row `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Item {
    Row(usize),
    Expansion(usize),
}

fn expanded_in(base: &[Option<usize>; 2], expanded: &dyn Fn(usize) -> bool) -> [Option<usize>; 2] {
    let left = base[0].filter(|&row| expanded(row));
    let right = base[1].filter(|&row| Some(row) != left && expanded(row));
    [left, right]
}

/// The list's items: every base row, each followed by its expansion if any
/// of its rows is expanded.
fn items(layout: &[[Option<usize>; 2]], expanded: &BTreeSet<usize>) -> Vec<Item> {
    let mut items = Vec::with_capacity(layout.len() + expanded.len());
    for (b, base) in layout.iter().enumerate() {
        items.push(Item::Row(b));
        if base.iter().flatten().any(|row| expanded.contains(row)) {
            items.push(Item::Expansion(b));
        }
    }
    items
}

/// A render snapshot owns derived indices, never the source diff rows.
pub(crate) struct Projection {
    pub(crate) layout: Rc<Vec<[Option<usize>; 2]>>,
    pub(crate) items: Rc<Vec<Item>>,
    expanded: BTreeSet<usize>,
}

impl Projection {
    pub(crate) fn expansion_rows(&self, base: usize) -> ([usize; 2], usize) {
        let expanded = self
            .layout
            .get(base)
            .map(|base| expanded_in(base, &|row| self.expanded.contains(&row)))
            .unwrap_or([None, None]);
        let mut rows = [0; 2];
        let mut len = 0;
        for row in expanded.into_iter().flatten() {
            rows[len] = row;
            len += 1;
        }
        (rows, len)
    }

    /// A list item represented by its source row and expansion/code identity.
    pub(crate) fn source_anchor(&self, item: usize) -> Option<(usize, bool)> {
        let (base, expansion) = match *self.items.get(item)? {
            Item::Row(base) => (base, false),
            Item::Expansion(base) => (base, true),
        };
        self.layout
            .get(base)?
            .iter()
            .flatten()
            .copied()
            .find(|row| !expansion || self.expanded.contains(row))
            .map(|row| (row, expansion))
    }

    pub(crate) fn item_for_source(&self, source: usize, expansion: bool) -> Option<usize> {
        let base = self
            .layout
            .iter()
            .position(|base| base.contains(&Some(source)))?;
        let item = if expansion && self.expanded.contains(&source) {
            Item::Expansion(base)
        } else {
            Item::Row(base)
        };
        self.items.iter().position(|candidate| *candidate == item)
    }
}

struct ProjectionEntry {
    source: Weak<Vec<DiffRow>>,
    path: String,
    split: bool,
    expanded_revision: u64,
    projection: Rc<Projection>,
}

/// One projection per embedding; no second process-wide diff cache.
#[derive(Default)]
pub(crate) struct ProjectionCache {
    cached: Option<ProjectionEntry>,
}

impl ProjectionCache {
    /// Keep the retiring mode's existing snapshot alive through synchronization.
    /// The list owner separately gates reuse by its content-source identity.
    pub(crate) fn previous_mode(&self, path: &str, split: bool) -> Option<Rc<Projection>> {
        self.cached
            .as_ref()
            .filter(|entry| entry.split != split && entry.path == path)
            .map(|entry| entry.projection.clone())
    }

    pub(crate) fn get(
        &mut self,
        path: &str,
        rows: &Arc<Vec<DiffRow>>,
        split: bool,
        expanded_revision: u64,
        expanded: &BTreeSet<usize>,
    ) -> Rc<Projection> {
        let same_layout = self.cached.as_ref().filter(|entry| {
            std::ptr::eq(entry.source.as_ptr(), Arc::as_ptr(rows)) && entry.split == split
        });
        if let Some(entry) = same_layout {
            if entry.path == path && entry.expanded_revision == expanded_revision {
                return entry.projection.clone();
            }
        }
        let layout = same_layout
            .map(|entry| entry.projection.layout.clone())
            .unwrap_or_else(|| Rc::new(layout(rows, split)));
        let projection = Rc::new(Projection {
            items: Rc::new(items(&layout, expanded)),
            layout,
            expanded: expanded.clone(),
        });
        self.cached = Some(ProjectionEntry {
            // Weak identity prevents address reuse and detaches Arc::make_mut.
            source: Arc::downgrade(rows),
            path: path.to_owned(),
            split,
            expanded_revision,
            projection: projection.clone(),
        });
        projection
    }
}

/// Keep `list` in step with `row`'s expansion changing from `before` to
/// `after`: splice the item under its base row in or out (or re-measure it),
/// so the scroll position survives instead of the count check resetting it.
pub(crate) fn splice_expansion(
    list: &VariableListState,
    layout: &[[Option<usize>; 2]],
    row: usize,
    before: &dyn Fn(usize) -> bool,
    after: &dyn Fn(usize) -> bool,
) {
    let Some(b) = layout.iter().position(|base| base.contains(&Some(row))) else {
        return;
    };
    let item = b + layout[..b]
        .iter()
        .filter(|base| expanded_in(base, before).iter().any(Option::is_some))
        .count();
    let had = expanded_in(&layout[b], before).iter().any(Option::is_some);
    let has = expanded_in(&layout[b], after).iter().any(Option::is_some);
    if had || has {
        list.splice(item + 1..item + 1 + usize::from(had), usize::from(has));
    }
}

/// A fixed-width gutter cell holding `marker` (or nothing, to keep columns
/// aligned).
pub(crate) fn gutter(marker: Option<AnyElement>) -> AnyElement {
    div()
        .flex_shrink_0()
        .w(theme::scaled_px(GUTTER_W))
        .flex()
        .justify_center()
        .children(marker)
        .into_any_element()
}

/// `row` with a gutter column in front of it.
pub(crate) fn with_gutter(gutter: AnyElement, row: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .child(gutter)
        .child(div().flex_1().min_w(px(0.)).child(row))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: [[Option<usize>; 2]; 4] = [
        [Some(0), None],
        [Some(1), Some(1)],
        [Some(2), Some(3)],
        [None, Some(4)],
    ];

    #[test]
    fn an_expansion_follows_its_base_row_and_shifts_only_later_items() {
        let expanded = BTreeSet::from([1, 3]);
        assert_eq!(
            items(&LAYOUT, &expanded),
            vec![
                Item::Row(0),
                Item::Row(1),
                Item::Expansion(1),
                Item::Row(2),
                Item::Expansion(2),
                Item::Row(3),
            ]
        );
        assert_eq!(
            items(&LAYOUT, &BTreeSet::new()).len(),
            LAYOUT.len(),
            "collapsed: one item per row"
        );
    }

    #[test]
    fn both_sides_of_a_pair_share_one_expansion() {
        let expanded = BTreeSet::from([2, 3]);
        assert_eq!(
            items(&LAYOUT, &expanded)
                .iter()
                .filter(|i| matches!(i, Item::Expansion(_)))
                .count(),
            1
        );
    }

    /// The list must already hold the new item count when the diff next
    /// renders; otherwise its count check resets it (and the scroll) to the top.
    #[test]
    fn splicing_leaves_the_list_at_the_new_item_count() {
        let none = |_: usize| false;
        let three = |row: usize| row == 3;
        let two_and_three = |row: usize| row == 2 || row == 3;
        let list = VariableListState::new(LAYOUT.len(), px(100.));
        let steps: [(&dyn Fn(usize) -> bool, &dyn Fn(usize) -> bool, usize); 4] = [
            (&none, &three, 3),          // open: one item appears
            (&three, &two_and_three, 2), // same pair: re-measured, not added
            (&two_and_three, &three, 2), // still one row open in the pair
            (&three, &none, 3),          // closed: the item goes
        ];
        for (before, after, row) in steps {
            splice_expansion(&list, &LAYOUT, row, before, after);
            let expanded = LAYOUT
                .iter()
                .flatten()
                .flatten()
                .copied()
                .filter(|&row| after(row))
                .collect();
            assert_eq!(
                list.item_count(),
                items(&LAYOUT, &expanded).len(),
                "row {row}"
            );
        }
    }
}
