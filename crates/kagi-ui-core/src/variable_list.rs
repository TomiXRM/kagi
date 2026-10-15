//! Kagi-owned variable-height geometry, shared by retained pane owners (#1479-S5C).
//! Only numeric heights/keys survive a frame. Public GPUI canvas/layout APIs build
//! visible rows; neither GPUI's list implementation nor eager text measurement is used.

use gpui::{
    canvas, point, px, size, AnyElement, App, AvailableSpace, Bounds, ContentMask, Font,
    HitboxBehavior, IntoElement, ListOffset, Pixels, Point, ScrollWheelEvent, Size, Styled, Window,
};
use gpui_component::scroll::ScrollbarHandle;
use std::{
    cell::RefCell,
    ops::Range,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone)]
pub struct VariableListState(Rc<RefCell<Geometry>>);

struct Row {
    key: u64,
    measured: Option<f64>,
}

#[derive(Clone, Copy, Default)]
struct Anchor {
    index: usize,
    fraction: f64,
}

#[derive(Clone, Copy)]
enum Intent {
    End,
    Reveal(usize),
}

#[derive(PartialEq)]
struct Metrics {
    width: Pixels,
    rem: Pixels,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
}

struct Geometry {
    owner: u64,
    next_key: u64,
    rows: Vec<Row>,
    // Fenwick sums use f64 so distant rows don't lose subpixel precision.
    sums: Vec<f64>,
    estimate: f64,
    anchor: Anchor,
    pending: Option<Intent>,
    drag_extent: Option<f64>,
    viewport: Bounds<Pixels>,
    metrics: Option<Metrics>,
    overdraw: f64,
    painted: Vec<(usize, Bounds<Pixels>)>,
}

impl Geometry {
    fn height(&self, ix: usize) -> f64 {
        self.rows[ix].measured.unwrap_or(self.estimate)
    }

    fn prefix(&self, mut end: usize) -> f64 {
        let mut sum = 0.;
        while end > 0 {
            sum += self.sums[end];
            end &= end - 1;
        }
        sum
    }

    fn total(&self) -> f64 {
        self.prefix(self.rows.len())
    }

    fn inverse(&self, offset: f64) -> usize {
        let mut ix = 0;
        let mut sum = 0.;
        let mut bit = self.rows.len().checked_next_power_of_two().unwrap_or(0);
        while bit > 0 {
            let next = ix + bit;
            if next < self.sums.len() && sum + self.sums[next] <= offset {
                sum += self.sums[next];
                ix = next;
            }
            bit >>= 1;
        }
        ix.min(self.rows.len().saturating_sub(1))
    }

    fn rebuild(&mut self) {
        self.sums.clear();
        self.sums.resize(self.rows.len() + 1, 0.);
        for ix in 1..self.sums.len() {
            self.sums[ix] += self.height(ix - 1);
            let parent = ix + (ix & ix.wrapping_neg());
            if parent < self.sums.len() {
                self.sums[parent] += self.sums[ix];
            }
        }
    }

    fn measure(&mut self, ix: usize, height: f64) {
        let height = height.max(0.01);
        let change = height - self.height(ix);
        self.rows[ix].measured = Some(height);
        let mut node = ix + 1;
        while node < self.sums.len() {
            self.sums[node] += change;
            node += node & node.wrapping_neg();
        }
    }

    fn offset(&self) -> f64 {
        if self.rows.is_empty() {
            return 0.;
        }
        self.prefix(self.anchor.index) + self.height(self.anchor.index) * self.anchor.fraction
    }

    fn set_pixel(&mut self, offset: f64) {
        if self.rows.is_empty() {
            self.anchor = Anchor::default();
            return;
        }
        let offset = offset.max(0.).min(self.maximum());
        let index = self.inverse(offset);
        self.anchor = Anchor {
            index,
            fraction: ((offset - self.prefix(index)) / self.height(index)).clamp(0., 1.),
        };
    }

    fn maximum(&self) -> f64 {
        (self.total() - f64::from(self.viewport.size.height)).max(0.)
    }

    fn invalidate(&mut self) {
        // Anchor is already fractional: consecutive width/rem/gutter changes in
        // one update must not recapture it from a replacement estimate.
        for row in &mut self.rows {
            row.measured = None;
        }
        self.painted.clear();
        self.rebuild();
    }

    fn resolve_end(&mut self) {
        self.set_pixel(self.maximum());
        self.pending = None;
    }
}

impl VariableListState {
    pub fn new(count: usize, overdraw: Pixels) -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let state = Self(Rc::new(RefCell::new(Geometry {
            owner: NEXT_OWNER.fetch_add(1, Ordering::Relaxed),
            next_key: 0,
            rows: Vec::new(),
            sums: vec![0.],
            estimate: 20.,
            anchor: Anchor::default(),
            pending: None,
            drag_extent: None,
            viewport: Bounds::default(),
            metrics: None,
            overdraw: f64::from(overdraw).max(0.),
            painted: Vec::new(),
        })));
        state.reset(count);
        state
    }

    /// Reset is a source replacement, never append/follow intent.
    pub fn reset(&self, count: usize) {
        let mut state = self.0.borrow_mut();
        state.rows.clear();
        for _ in 0..count {
            let key = state.next_key;
            state.next_key += 1;
            state.rows.push(Row {
                key,
                measured: None,
            });
        }
        state.anchor = Anchor::default();
        state.pending = None;
        state.drag_extent = None;
        state.painted.clear();
        state.rebuild();
    }

    pub fn reset_with_estimate(&self, count: usize, height: Pixels) {
        self.0.borrow_mut().estimate = f64::from(height).max(0.01);
        self.reset(count);
    }

    pub fn set_estimate(&self, height: Pixels) {
        let mut state = self.0.borrow_mut();
        let height = f64::from(height).max(0.01);
        if state.estimate != height {
            state.estimate = height;
            state.invalidate();
        }
    }

    pub fn remeasure(&self) {
        self.0.borrow_mut().invalidate();
    }

    /// Replaced slots receive fresh source-local keys and nonzero estimates.
    /// Survivors retain keys/measurements; a deleted anchor chooses the next
    /// survivor (or the preceding final survivor), retaining its intra-row fraction.
    /// Replacements keep an anchor/Reveal in the corresponding replacement slot.
    pub fn splice(&self, range: Range<usize>, count: usize) {
        let mut state = self.0.borrow_mut();
        assert!(range.start <= range.end && range.end <= state.rows.len());
        let old_len = state.rows.len();
        let new_len = old_len - range.len() + count;
        let relocate = |ix: usize| {
            if old_len == 0 {
                0
            } else if ix < range.start {
                ix
            } else if ix >= range.end {
                ix - range.len() + count
            } else if count > 0 {
                range.start + (ix - range.start).min(count - 1)
            } else {
                (range.start + count).min(new_len.saturating_sub(1))
            }
        };
        state.anchor.index = relocate(state.anchor.index).min(new_len.saturating_sub(1));
        if let Some(Intent::Reveal(ix)) = state.pending {
            state.pending = Some(Intent::Reveal(relocate(ix)));
        }
        let first_key = state.next_key;
        state.next_key += count as u64;
        state.rows.splice(
            range,
            (0..count).map(|ix| Row {
                key: first_key + ix as u64,
                measured: None,
            }),
        );
        if new_len == 0 {
            state.anchor = Anchor::default();
            state.pending = None;
        }
        state.painted.clear();
        state.rebuild();
    }

    pub fn item_count(&self) -> usize {
        self.0.borrow().rows.len()
    }
    pub fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().viewport
    }
    pub fn estimated_height(&self) -> Pixels {
        px(self.0.borrow().estimate as f32)
    }
    pub fn max_offset(&self) -> Pixels {
        px(self.0.borrow().maximum() as f32)
    }
    pub fn pixel_offset(&self) -> Pixels {
        px(self.0.borrow().offset() as f32)
    }

    /// Only actual measurements; estimates never masquerade as observations.
    pub fn measured_height(&self, ix: usize) -> Option<Pixels> {
        self.0
            .borrow()
            .rows
            .get(ix)?
            .measured
            .map(|height| px(height as f32))
    }

    /// Bounds from the most recent frame's painted viewport, not cached estimates.
    pub fn bounds_for_item(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.0
            .borrow()
            .painted
            .iter()
            .find_map(|(index, bounds)| (*index == ix).then_some(*bounds))
    }

    pub fn logical_scroll_top(&self) -> ListOffset {
        let state = self.0.borrow();
        ListOffset {
            item_ix: state.anchor.index,
            offset_in_item: px(if state.rows.is_empty() {
                0.
            } else {
                (state.height(state.anchor.index) * state.anchor.fraction) as f32
            }),
        }
    }

    pub fn scroll_to(&self, position: ListOffset) {
        let mut state = self.0.borrow_mut();
        if state.rows.is_empty() {
            return;
        }
        if position.item_ix >= state.rows.len() {
            state.pending = Some(Intent::End);
            return;
        }
        // An explicit row target must survive underestimated successors. Only
        // actual layout may decide there is too little content below it.
        state.anchor = Anchor {
            index: position.item_ix,
            fraction: f64::from(position.offset_in_item) / state.height(position.item_ix),
        };
        state.pending = None;
    }

    pub fn scroll_to_reveal_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        if ix < state.rows.len() {
            state.pending = Some(Intent::Reveal(ix));
        }
    }

    pub fn scroll_to_end(&self) {
        let mut state = self.0.borrow_mut();
        state.pending = (!state.rows.is_empty()).then_some(Intent::End);
    }

    /// Returns whether this pane consumed the movement. An event reaching the
    /// edge belongs here; only the next outward event chains to its parent.
    pub fn scroll_by(&self, distance: Pixels) -> bool {
        let mut state = self.0.borrow_mut();
        let current = state.offset();
        let next = (current + f64::from(distance)).clamp(0., state.maximum());
        if next == current {
            return false;
        }
        if distance > px(0.) && next >= state.maximum() {
            state.pending = Some(Intent::End);
        } else {
            state.pending = None;
            state.set_pixel(next);
        }
        true
    }
}

impl ScrollbarHandle for VariableListState {
    fn offset(&self) -> Point<Pixels> {
        point(px(0.), -self.pixel_offset())
    }
    fn set_offset(&self, offset: Point<Pixels>) {
        let mut state = self.0.borrow_mut();
        // The widget maps pointer positions in public Pixels (f32). Compare
        // with that same exposed endpoint, not the more precise prefix sum.
        let extent = px(state.drag_extent.unwrap_or_else(|| state.total()) as f32);
        let maximum = f64::from((extent - state.viewport.size.height).max(px(0.)));
        let target = f64::from(-offset.y).clamp(0., maximum);
        if maximum > 0. && target >= maximum {
            state.pending = Some(Intent::End);
        } else {
            state.pending = None;
            state.set_pixel(target);
        }
    }
    fn content_size(&self) -> Size<Pixels> {
        let state = self.0.borrow();
        size(
            state.viewport.size.width,
            px(state.drag_extent.unwrap_or_else(|| state.total()) as f32),
        )
    }
    fn start_drag(&self) {
        let mut state = self.0.borrow_mut();
        state.drag_extent = Some(state.total());
    }
    fn end_drag(&self) {
        self.0.borrow_mut().drag_extent = None;
    }
}

struct LaidOutRow {
    ix: usize,
    key: u64,
    element: AnyElement,
}

fn measure_row(
    state: &VariableListState,
    ix: usize,
    render: &mut impl FnMut(usize, &mut Window, &mut App) -> AnyElement,
    window: &mut Window,
    cx: &mut App,
) -> LaidOutRow {
    let (key, width) = {
        let state = state.0.borrow();
        (state.rows[ix].key, state.viewport.size.width)
    };
    window.with_element_namespace(("row", key), |window| {
        let mut element = render(ix, window, cx);
        let measured = element.layout_as_root(
            size(AvailableSpace::Definite(width), AvailableSpace::MinContent),
            window,
            cx,
        );
        state.0.borrow_mut().measure(ix, f64::from(measured.height));
        LaidOutRow { ix, key, element }
    })
}

fn finish_layout(
    state: &VariableListState,
    rows: &mut Vec<LaidOutRow>,
    measure: &mut impl FnMut(usize) -> LaidOutRow,
) {
    let (fill, count, coverage) = {
        let geometry = state.0.borrow();
        (
            matches!(geometry.pending, Some(Intent::End)) || geometry.offset() > geometry.maximum(),
            geometry.rows.len(),
            f64::from(geometry.viewport.size.height) + geometry.overdraw,
        )
    };
    if fill {
        // #1479-S5C: reflow/collapse and near-tail jumps can leave less than a
        // viewport below the anchor. Resolve against actual tail measurements
        // now, not as an accidental jump on the next wheel event.
        // layout_rows constructs a contiguous interval. Reuse its elements;
        // only newly exposed predecessors need layout in this frame.
        let existing = rows.iter().fold(count..0, |range, row| {
            range.start.min(row.ix)..range.end.max(row.ix + 1)
        });
        let mut covered = 0.;
        for ix in (0..count).rev() {
            if !existing.contains(&ix) {
                rows.push(measure(ix));
            }
            covered += state.0.borrow().height(ix);
            if covered >= coverage {
                break;
            }
        }
        state.0.borrow_mut().resolve_end();
    }
    rows.sort_unstable_by_key(|row| row.ix);
}

fn layout_rows(
    state: &VariableListState,
    mut measure: impl FnMut(usize) -> LaidOutRow,
) -> Vec<LaidOutRow> {
    let (count, pending, viewport, overdraw) = {
        let state = state.0.borrow();
        (
            state.rows.len(),
            state.pending,
            f64::from(state.viewport.size.height),
            state.overdraw,
        )
    };
    let mut rows = Vec::new();
    if count == 0 || viewport <= 0. {
        return rows;
    }
    if matches!(pending, Some(Intent::End)) {
        finish_layout(state, &mut rows, &mut measure);
        return rows;
    }
    if let Some(Intent::Reveal(ix)) = pending {
        rows.push(measure(ix));
        let align_bottom = {
            let geometry = state.0.borrow();
            geometry.height(ix) <= viewport
                && geometry.prefix(ix + 1) > geometry.offset() + viewport
        };
        if align_bottom {
            // A reveal must measure the viewport preceding its destination
            // before aligning it. Resolving against estimated predecessors
            // would push the caret below the viewport as those rows grow.
            let mut covered = state.0.borrow().height(ix);
            for previous in (0..ix).rev() {
                if covered >= viewport + overdraw {
                    break;
                }
                rows.push(measure(previous));
                covered += state.0.borrow().height(previous);
            }
            {
                let mut geometry = state.0.borrow_mut();
                let offset = geometry.prefix(ix + 1) - viewport;
                geometry.set_pixel(offset);
                geometry.pending = None;
            }
            rows.reverse();
            let mut covered = 0.;
            for next in ix + 1..count {
                if covered >= overdraw {
                    break;
                }
                rows.push(measure(next));
                covered += state.0.borrow().height(next);
            }
            finish_layout(state, &mut rows, &mut measure);
            return rows;
        }
        let mut geometry = state.0.borrow_mut();
        let top = geometry.prefix(ix);
        if top < geometry.offset() || geometry.height(ix) > viewport {
            geometry.set_pixel(top);
        }
        geometry.pending = None;
    }
    let revealed = match pending {
        Some(Intent::Reveal(ix)) => Some(ix),
        _ => None,
    };
    let anchor = state.0.borrow().anchor.index;
    if revealed != Some(anchor) {
        rows.push(measure(anchor));
    }
    // Measure backward overdraw and forward coverage from the reading key. The
    // anchor fraction is evaluated against its freshly measured actual height.
    let mut ix = anchor;
    while ix > 0 {
        let geometry = state.0.borrow();
        let enough = geometry.prefix(ix) <= geometry.offset() - overdraw;
        drop(geometry);
        if enough {
            break;
        }
        ix -= 1;
        if revealed != Some(ix) {
            rows.push(measure(ix));
        }
    }
    for ix in anchor + 1..count {
        let geometry = state.0.borrow();
        let enough = geometry.prefix(ix) >= geometry.offset() + viewport + overdraw;
        drop(geometry);
        if enough {
            break;
        }
        if revealed != Some(ix) {
            rows.push(measure(ix));
        }
    }
    finish_layout(state, &mut rows, &mut measure);
    rows
}

/// A lazy variable-height Element built exclusively on public GPUI canvas and
/// root layout APIs. The caller keeps its row renderer, focus and selection.
pub fn variable_list(
    state: VariableListState,
    mut render: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> impl IntoElement + Styled {
    let paint_state = state.clone();
    canvas(
        move |bounds, window, cx| {
            let text = window.text_style();
            let rem = window.rem_size();
            let metrics = Metrics {
                width: bounds.size.width,
                rem,
                font: text.font(),
                font_size: text.font_size.to_pixels(rem),
                line_height: text.line_height_in_pixels(rem),
            };
            let owner = {
                let mut geometry = state.0.borrow_mut();
                if geometry.metrics.as_ref().is_some_and(|old| *old != metrics) {
                    geometry.invalidate();
                }
                geometry.metrics = Some(metrics);
                geometry.viewport = bounds;
                geometry.painted.clear();
                geometry.owner
            };
            window.with_element_namespace(("variable-list", owner), |window| {
                let mut rows = layout_rows(&state, |ix| {
                    measure_row(&state, ix, &mut render, window, cx)
                });
                let hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                    for row in &mut rows {
                        let row_bounds = {
                            let geometry = state.0.borrow();
                            Bounds::new(
                                point(
                                    bounds.left(),
                                    bounds.top()
                                        + px((geometry.prefix(row.ix) - geometry.offset()) as f32),
                                ),
                                size(bounds.size.width, px(geometry.height(row.ix) as f32)),
                            )
                        };
                        window.with_element_namespace(("row", row.key), |window| {
                            row.element.prepaint_at(row_bounds.origin, window, cx)
                        });
                        if row_bounds.bottom() > bounds.top() && row_bounds.top() < bounds.bottom()
                        {
                            state.0.borrow_mut().painted.push((row.ix, row_bounds));
                        }
                    }
                    hitbox
                });
                (owner, rows, hitbox)
            })
        },
        move |bounds, (owner, mut rows, hitbox), window, cx| {
            let line_height = window.line_height();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase.bubble()
                    && hitbox.should_handle_scroll(window)
                    && paint_state.scroll_by(-event.delta.pixel_delta(line_height).y)
                {
                    window.refresh();
                    cx.stop_propagation();
                }
            });
            window.with_element_namespace(("variable-list", owner), |window| {
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for row in &mut rows {
                        window.with_element_namespace(("row", row.key), |window| {
                            row.element.paint(window, cx)
                        });
                    }
                });
            });
        },
    )
    .w_full()
}

#[cfg(test)]
mod tests;
