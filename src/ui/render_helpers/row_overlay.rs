//! Per-row decoration a diff embedding adds to the shared diff list (#351):
//! a gutter marker beside some rows and, for an expanded row, one extra list
//! item right under it. Rows themselves are never split or renumbered; only
//! the list's item index shifts while something is expanded.

use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ListState};

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
    /// `row` is expanded: its expansion follows the list row showing it.
    fn expanded(&self, row: usize) -> bool;
    /// What goes under a list row whose expanded rows are `rows`.
    fn expansion(&self, rows: &[usize], cx: &mut App) -> AnyElement;
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

fn expanded_in(base: &[Option<usize>; 2], expanded: &dyn Fn(usize) -> bool) -> Vec<usize> {
    let mut rows: Vec<usize> = base
        .iter()
        .flatten()
        .copied()
        .filter(|r| expanded(*r))
        .collect();
    rows.dedup();
    rows
}

/// The list's items: every base row, each followed by its expansion if any
/// of its rows is expanded.
pub(crate) fn items(layout: &[[Option<usize>; 2]], expanded: &dyn Fn(usize) -> bool) -> Vec<Item> {
    let mut items = Vec::with_capacity(layout.len());
    for (b, base) in layout.iter().enumerate() {
        items.push(Item::Row(b));
        if !expanded_in(base, expanded).is_empty() {
            items.push(Item::Expansion(b));
        }
    }
    items
}

/// Keep `list` in step with `row`'s expansion changing from `before` to
/// `after`: splice the item under its base row in or out (or re-measure it),
/// so the scroll position survives instead of the count check resetting it.
pub(crate) fn splice_expansion(
    list: &ListState,
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
        .filter(|base| !expanded_in(base, before).is_empty())
        .count();
    let had = !expanded_in(&layout[b], before).is_empty();
    let has = !expanded_in(&layout[b], after).is_empty();
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

/// The overlay's expanded rows under base row `b`.
pub(crate) fn expansion_rows(
    layout: &[[Option<usize>; 2]],
    b: usize,
    overlay: &Rc<dyn RowOverlay>,
) -> Vec<usize> {
    layout
        .get(b)
        .map(|base| expanded_in(base, &|row| overlay.expanded(row)))
        .unwrap_or_default()
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
        let expanded = |row: usize| row == 3 || row == 1;
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
            items(&LAYOUT, &|_| false).len(),
            LAYOUT.len(),
            "collapsed: one item per row"
        );
    }

    #[test]
    fn both_sides_of_a_pair_share_one_expansion() {
        let expanded = |row: usize| row == 2 || row == 3;
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
        let list = ListState::new(LAYOUT.len(), gpui::ListAlignment::Top, px(100.));
        let steps: [(&dyn Fn(usize) -> bool, &dyn Fn(usize) -> bool, usize); 4] = [
            (&none, &three, 3),          // open: one item appears
            (&three, &two_and_three, 2), // same pair: re-measured, not added
            (&two_and_three, &three, 2), // still one row open in the pair
            (&three, &none, 3),          // closed: the item goes
        ];
        for (before, after, row) in steps {
            splice_expansion(&list, &LAYOUT, row, before, after);
            assert_eq!(list.item_count(), items(&LAYOUT, after).len(), "row {row}");
        }
    }
}
