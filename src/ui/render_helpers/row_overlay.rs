//! Per-row decoration a diff embedding adds to the shared diff list (#351):
//! a gutter marker beside some rows and, for an expanded row, one extra list
//! item right under it. Rows themselves are never split or renumbered; only
//! the list's item index shifts while something is expanded.

use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, Weak};

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
    /// The embedding's owned projection for this diff and expansion state.
    fn projection(&self) -> Rc<Projection>;
    /// What goes under a list row whose expanded rows are `rows`.
    fn expansion(&self, rows: &[usize], cx: &mut App) -> AnyElement;
}

/// The diff rows each base list row shows: one unified; a pair's two split.
fn layout(rows: &Arc<Vec<DiffRow>>, split: bool) -> Vec<[Option<usize>; 2]> {
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
fn items(layout: &[[Option<usize>; 2]], expanded: &dyn Fn(usize) -> bool) -> Vec<Item> {
    let mut items = Vec::with_capacity(layout.len());
    for (b, base) in layout.iter().enumerate() {
        items.push(Item::Row(b));
        if expanded_in(base, expanded).iter().any(Option::is_some) {
            items.push(Item::Expansion(b));
        }
    }
    items
}

/// A render snapshot owns only derived indices, never the source diff rows.
pub(crate) struct Projection {
    pub(crate) layout: Rc<Vec<[Option<usize>; 2]>>,
    pub(crate) items: Vec<Item>,
    expanded: BTreeSet<usize>,
}

impl Projection {
    pub(crate) fn expanded(&self, row: usize) -> bool {
        self.expanded.contains(&row)
    }

    pub(crate) fn expansion_rows(&self, base: usize) -> [Option<usize>; 2] {
        self.layout
            .get(base)
            .map(|base| expanded_in(base, &|row| self.expanded.contains(&row)))
            .unwrap_or([None, None])
    }
}

struct ProjectionEntry {
    source: Weak<Vec<DiffRow>>,
    path: String,
    split: bool,
    expanded_revision: u64,
    projection: Rc<Projection>,
}

/// The embedding retains its last source, not a second process-wide diff cache.
#[derive(Default)]
pub(crate) struct ProjectionCache {
    cached: Option<ProjectionEntry>,
}

impl ProjectionCache {
    pub(crate) fn clear(&mut self) {
        self.cached = None;
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
            entry.source.strong_count() != 0
                && entry.source.as_ptr() == Arc::as_ptr(rows)
                && entry.split == split
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
            items: items(&layout, &|row| expanded.contains(&row)),
            layout,
            expanded: expanded.clone(),
        });
        self.cached = Some(ProjectionEntry {
            // Weak identity prevents address reuse and detaches Arc::make_mut.
            source: Arc::downgrade(rows),
            path: path.to_string(),
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
    use kagi_git::DiffLineKind;

    fn line(kind: DiffLineKind) -> DiffRow {
        DiffRow::Line {
            kind,
            text: "line".into(),
            old_lineno: None,
            new_lineno: None,
            highlights: Vec::new(),
        }
    }

    fn paired_rows() -> Arc<Vec<DiffRow>> {
        Arc::new(vec![
            line(DiffLineKind::Removed),
            line(DiffLineKind::Added),
            line(DiffLineKind::Context),
        ])
    }

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

    #[test]
    fn mode_revision_and_path_changes_keep_expanded_source_rows_in_order() {
        let rows = paired_rows();
        let mut cache = ProjectionCache::default();
        let both = BTreeSet::from([0, 1]);
        let unified = cache.get("file.rs", &rows, false, 1, &both);
        assert_eq!(
            unified.items,
            [
                Item::Row(0),
                Item::Expansion(0),
                Item::Row(1),
                Item::Expansion(1),
                Item::Row(2),
            ]
        );
        let split = cache.get("file.rs", &rows, true, 1, &both);
        assert_eq!(
            split.items,
            [Item::Row(0), Item::Expansion(0), Item::Row(1)]
        );
        assert_eq!(split.expansion_rows(0), [Some(0), Some(1)]);

        let context = BTreeSet::from([2]);
        let changed = cache.get("file.rs", &rows, true, 2, &context);
        assert_eq!(
            changed.items,
            [Item::Row(0), Item::Row(1), Item::Expansion(1)]
        );
        assert_eq!(changed.expansion_rows(1), [Some(2), None]);
        let other_path = cache.get("other.rs", &rows, true, 2, &BTreeSet::new());
        assert_eq!(other_path.items, [Item::Row(0), Item::Row(1)]);
        let closed = cache.get("file.rs", &rows, true, 3, &BTreeSet::new());
        assert_eq!(closed.items, [Item::Row(0), Item::Row(1)]);
    }

    #[test]
    fn cached_expansion_changes_splice_without_losing_the_later_row_anchor() {
        let rows = paired_rows();
        let mut cache = ProjectionCache::default();
        let mut open = BTreeSet::new();
        let closed = cache.get("file.rs", &rows, true, 0, &open);
        let list = ListState::new(closed.items.len(), gpui::ListAlignment::Top, px(100.));
        list.scroll_to(gpui::ListOffset {
            item_ix: 1,
            offset_in_item: px(7.),
        });
        for (revision, row) in [(1, 0), (2, 1), (3, 0), (4, 1)] {
            let before = cache.get("file.rs", &rows, true, revision - 1, &open);
            let previous = open.clone();
            if !open.remove(&row) {
                open.insert(row);
            }
            splice_expansion(
                &list,
                &before.layout,
                row,
                &|row| previous.contains(&row),
                &|row| open.contains(&row),
            );
            let after = cache.get("file.rs", &rows, true, revision, &open);
            assert_eq!(list.item_count(), after.items.len());
            let top = list.logical_scroll_top();
            assert_eq!(after.items[top.item_ix], Item::Row(1));
            assert_eq!(top.offset_in_item, px(7.));
        }
    }

    #[test]
    fn render_snapshots_do_not_keep_source_rows_or_replaced_projections_alive() {
        let rows = paired_rows();
        let source = Arc::downgrade(&rows);
        let mut cache = ProjectionCache::default();
        let snapshot = cache.get("file.rs", &rows, true, 1, &BTreeSet::from([1]));
        let old_projection = Rc::downgrade(&snapshot);
        drop(rows);
        assert!(
            source.upgrade().is_none(),
            "the source buffer must be released"
        );

        let replacement = paired_rows();
        cache.get("file.rs", &replacement, true, 1, &BTreeSet::new());
        drop(snapshot);
        assert!(
            old_projection.upgrade().is_none(),
            "the owner must release the dead source's derived allocation"
        );
    }
}
