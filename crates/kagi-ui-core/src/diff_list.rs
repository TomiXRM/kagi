//! Source/layout invalidation metadata for owner-retained diff geometry.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::variable_list::VariableListState;
use gpui::{px, rems, Pixels, TextStyle};

/// Identity of one row source, independent of highlight-only Arc changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffListSource(u64);

impl Default for DiffListSource {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl DiffListSource {
    /// Opaque monotonic identity; never derived from a path or row count.
    pub fn token(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    source: DiffListSource,
    split: bool,
    gutter: bool,
}

/// One memo per retained list; measured heights and scroll live in its owned geometry.
#[derive(Default)]
pub struct DiffListLayout {
    selection_owner: DiffListSource,
    key: Cell<Option<LayoutKey>>,
    estimate: Cell<Option<(f32, Pixels)>>,
}

impl DiffListLayout {
    /// Stable identity of this retained list, independent of its content.
    pub fn selection_owner(&self) -> DiffListSource {
        self.selection_owner
    }

    pub fn sync(
        &self,
        list: &VariableListState,
        source: DiffListSource,
        split: bool,
        gutter: bool,
        count: usize,
        mut remap: impl FnMut(bool, bool, usize) -> Option<usize>,
    ) {
        let key = LayoutKey {
            source,
            split,
            gutter,
        };
        let rem_size = crate::theme::rem_size_px();
        let height = match self.estimate.get() {
            Some((previous, height)) if previous == rem_size => height,
            _ => {
                let height = estimated_row_height(px(rem_size));
                self.estimate.set(Some((rem_size, height)));
                height
            }
        };
        match self.key.get() {
            Some(previous) if previous.source == source && previous.split == split => {
                list.set_estimate(height);
                if list.item_count() != count {
                    list.reset(count);
                } else if previous.gutter != gutter {
                    list.remeasure();
                }
            }
            Some(previous) if previous.source == source => {
                list.remap_layout(count, height, |index| {
                    remap(previous.split, previous.gutter, index)
                });
            }
            _ => list.reset_with_estimate(count, height),
        }
        self.key.set(Some(key));
    }

    pub fn clear(&self, list: &VariableListState) {
        if self.key.take().is_some() || list.item_count() != 0 {
            list.reset(0);
        }
    }
}

fn estimated_row_height(rem_size: Pixels) -> Pixels {
    // text_sm plus py_px in both the unified line and the split cell. Wrapped
    // lines, binary rows, hunk actions and thread expansions resolve on layout.
    let text = TextStyle {
        font_size: rems(0.875).into(),
        ..TextStyle::default()
    };
    text.line_height_in_pixels(rem_size) + rem_size / 8.
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::ListOffset;

    #[test]
    fn same_count_sources_reset_but_repaints_and_gutters_keep_anchor() {
        let list = VariableListState::new(0, px(0.));
        let layout = DiffListLayout::default();
        let source = DiffListSource::default();
        layout.sync(&list, source, false, false, 100, |_, _, _| None);
        list.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(3.),
        });
        layout.sync(&list, source, false, false, 100, |_, _, _| None);
        assert_eq!(list.logical_scroll_top().item_ix, 12);
        layout.sync(&list, source, false, true, 100, |_, _, _| None);
        assert_eq!(list.logical_scroll_top().item_ix, 12);
        layout.sync(
            &list,
            DiffListSource::default(),
            false,
            true,
            100,
            |_, _, _| None,
        );
        assert_eq!(list.logical_scroll_top().item_ix, 0);
        list.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(0.),
        });
        layout.sync(&list, source, true, true, 100, |_, _, _| None);
        assert_eq!(list.logical_scroll_top().item_ix, 0);
        layout.clear(&list);
        assert_eq!(list.item_count(), 0);
    }

    #[test]
    fn expansion_splice_does_not_reset_the_existing_anchor() {
        let list = VariableListState::new(0, px(0.));
        let layout = DiffListLayout::default();
        let source = DiffListSource::default();
        layout.sync(&list, source, false, true, 100, |_, _, _| None);
        list.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(3.),
        });
        list.splice(4..4, 1);
        layout.sync(&list, source, false, true, 101, |_, _, _| None);
        assert_eq!(list.logical_scroll_top().item_ix, 13);
        assert_eq!(list.logical_scroll_top().offset_in_item, px(3.));
    }
}
