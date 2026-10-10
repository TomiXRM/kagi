//! Source/layout invalidation metadata for owner-retained native diff lists.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{px, rems, ListState, Pixels, TextStyle};

/// Identity of one row source, independent of highlight-only Arc changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffListSource(u64);

impl Default for DiffListSource {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    source: DiffListSource,
    split: bool,
    gutter: bool,
}

/// One memo per native list; heights and scroll position remain in ListState.
#[derive(Default)]
pub struct DiffListLayout {
    key: Cell<Option<LayoutKey>>,
    estimate: Cell<Option<(f32, Pixels)>>,
}

impl DiffListLayout {
    pub fn sync(
        &self,
        list: &ListState,
        source: DiffListSource,
        split: bool,
        gutter: bool,
        count: usize,
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
                list.clone().with_uniform_item_height(height);
                if list.item_count() != count {
                    list.reset(count);
                } else if previous.gutter != gutter {
                    list.remeasure();
                }
            }
            _ => list.reset_with_uniform_height(count, height),
        }
        self.key.set(Some(key));
    }

    pub fn clear(&self, list: &ListState) {
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
    use gpui::{ListAlignment, ListOffset};

    #[test]
    fn same_count_sources_reset_but_repaints_and_gutters_keep_anchor() {
        let list = ListState::new(0, ListAlignment::Top, px(0.));
        let layout = DiffListLayout::default();
        let source = DiffListSource::default();
        layout.sync(&list, source, false, false, 100);
        list.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(3.),
        });
        layout.sync(&list, source, false, false, 100);
        assert_eq!(list.logical_scroll_top().item_ix, 12);
        layout.sync(&list, source, false, true, 100);
        assert_eq!(list.logical_scroll_top().item_ix, 12);
        layout.sync(&list, DiffListSource::default(), false, true, 100);
        assert_eq!(list.logical_scroll_top().item_ix, 0);
        list.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(0.),
        });
        layout.sync(&list, source, true, true, 100);
        assert_eq!(list.logical_scroll_top().item_ix, 0);
        layout.clear(&list);
        assert_eq!(list.item_count(), 0);
    }

    #[test]
    fn expansion_splice_does_not_reset_the_existing_anchor() {
        let list = ListState::new(0, ListAlignment::Top, px(0.));
        let layout = DiffListLayout::default();
        let source = DiffListSource::default();
        layout.sync(&list, source, false, true, 100);
        list.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(3.),
        });
        list.splice(4..4, 1);
        layout.sync(&list, source, false, true, 101);
        assert_eq!(list.logical_scroll_top().item_ix, 13);
        assert_eq!(list.logical_scroll_top().offset_in_item, px(3.));
    }
}
