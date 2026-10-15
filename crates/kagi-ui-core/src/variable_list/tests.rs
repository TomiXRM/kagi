use super::*;

fn list(count: usize) -> VariableListState {
    let list = VariableListState::new(count, px(0.));
    list.set_estimate(px(20.));
    list.0.borrow_mut().viewport = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.)));
    list
}

fn measure(list: &VariableListState, ix: usize, height: f64) {
    list.0.borrow_mut().measure(ix, height);
}

#[test]
fn prefix_inverse_uses_actual_and_nonzero_estimated_slots() {
    let list = list(20_005);
    measure(&list, 2, 75.);
    let state = list.0.borrow();
    assert_eq!(state.prefix(3), 115.);
    for (pixel, ix) in [
        (0., 0),
        (19.9, 0),
        (20., 1),
        (40., 2),
        (114.9, 2),
        (115., 3),
    ] {
        assert_eq!(state.inverse(pixel), ix);
    }
    assert_eq!(state.inverse(state.total()), 20_004);
    assert_eq!(list.measured_height(10_000), None);
    assert_eq!(
        list.bounds_for_item(2),
        None,
        "a measurement is not a painted witness"
    );
}

#[test]
fn estimate_survives_reset_splice_and_empty_source() {
    let list = list(100);
    list.splice(3..3, 2);
    assert_eq!(list.0.borrow().total(), 2040.);
    list.reset_with_estimate(100, px(30.));
    assert_eq!(list.0.borrow().total(), 3000.);
    list.reset(0);
    assert_eq!(list.0.borrow().total(), 0.);
    list.reset(10);
    assert_eq!(list.0.borrow().total(), 300.);
}

#[test]
fn offscreen_same_count_replacement_invalidates_only_affected_measurements() {
    let list = list(100);
    measure(&list, 2, 44.);
    measure(&list, 90, 800.);
    list.scroll_to(ListOffset {
        item_ix: 30,
        offset_in_item: px(5.),
    });
    let key = list.0.borrow().rows[30].key;
    list.splice(90..91, 1);
    assert_eq!(list.item_count(), 100);
    assert_eq!(list.measured_height(90), None);
    assert_eq!(list.measured_height(2), Some(px(44.)));
    assert_eq!(list.0.borrow().rows[30].key, key);
    assert_eq!(list.logical_scroll_top().item_ix, 30);
    assert_eq!(list.logical_scroll_top().offset_in_item, px(5.));
}

#[test]
fn insert_delete_before_reading_preserves_key_and_fraction() {
    let list = list(100);
    measure(&list, 30, 80.);
    list.scroll_to(ListOffset {
        item_ix: 30,
        offset_in_item: px(20.),
    });
    let key = list.0.borrow().rows[30].key;
    list.splice(4..4, 3);
    assert_eq!(list.logical_scroll_top().item_ix, 33);
    assert_eq!(list.logical_scroll_top().offset_in_item, px(20.));
    assert_eq!(list.0.borrow().rows[33].key, key);
    list.splice(2..7, 0);
    assert_eq!(list.logical_scroll_top().item_ix, 28);
    assert_eq!(list.0.borrow().rows[28].key, key);
    list.splice(28..29, 0);
    assert_eq!(
        list.logical_scroll_top().item_ix,
        28,
        "deleted key chooses next survivor"
    );
    assert_eq!(
        list.logical_scroll_top().offset_in_item,
        px(5.),
        "same quarter of survivor"
    );
}

#[test]
fn width_font_zoom_gutter_invalidation_resolves_fraction_against_actual_height() {
    let list = list(100);
    measure(&list, 10, 80.);
    list.scroll_to(ListOffset {
        item_ix: 10,
        offset_in_item: px(30.),
    });
    list.remeasure(); // width / font / line-height / gutter generation
    list.set_estimate(px(35.)); // zoom in the same update must not recapture estimate fraction
    assert_eq!(list.measured_height(10), None);
    measure(&list, 10, 160.);
    assert_eq!(list.logical_scroll_top().item_ix, 10);
    assert_eq!(list.logical_scroll_top().offset_in_item, px(60.));
    list.set_estimate(px(35.));
    assert_eq!(
        list.measured_height(10),
        Some(px(160.)),
        "unchanged repaint keeps cache"
    );
}

#[test]
fn unmeasured_fraction_survives_metric_change() {
    let list = list(100);
    list.scroll_to(ListOffset {
        item_ix: 10,
        offset_in_item: px(10.),
    });
    list.set_estimate(px(40.));
    measure(&list, 10, 60.);
    assert_eq!(list.logical_scroll_top().offset_in_item, px(30.));
}

#[test]
fn drag_extent_is_frozen_and_endpoint_requests_true_end() {
    let list = list(100);
    ScrollbarHandle::start_drag(&list);
    let extent = ScrollbarHandle::content_size(&list);
    measure(&list, 0, 600.);
    assert_eq!(ScrollbarHandle::content_size(&list), extent);
    ScrollbarHandle::set_offset(&list, point(px(0.), px(-900.)));
    assert_eq!(
        list.logical_scroll_top().item_ix,
        16,
        "interior maps through current prefix inverse"
    );
    ScrollbarHandle::set_offset(&list, point(px(0.), -(extent.height - px(100.))));
    assert!(matches!(list.0.borrow().pending, Some(Intent::End)));
    ScrollbarHandle::end_drag(&list);
    assert!(ScrollbarHandle::content_size(&list).height > extent.height);
}

#[test]
fn end_resolution_is_one_shot_with_oversized_tail_and_no_append_follow() {
    let list = list(100);
    list.scroll_by(px(20_000_000.));
    assert!(matches!(list.0.borrow().pending, Some(Intent::End)));
    measure(&list, 99, 800.);
    {
        let mut state = list.0.borrow_mut();
        state.resolve_end();
        assert!(state.pending.is_none());
        assert_eq!(state.offset(), state.total() - 100.);
    }
    let before = list.logical_scroll_top();
    assert_eq!(before.item_ix, 99);
    assert_eq!(before.offset_in_item, px(700.));
    list.splice(100..100, 1);
    assert_eq!(list.logical_scroll_top().item_ix, before.item_ix);
    assert_eq!(
        list.logical_scroll_top().offset_in_item,
        before.offset_in_item
    );
    assert!(list.0.borrow().pending.is_none());
    list.scroll_by(px(-20_000_000.));
    assert_eq!(list.logical_scroll_top().item_ix, 0);
    assert_eq!(list.logical_scroll_top().offset_in_item, px(0.));
}

#[test]
fn source_reset_drops_anchor_pending_intent_drag_and_measurements() {
    let list = list(100);
    measure(&list, 40, 100.);
    list.scroll_to_reveal_item(40);
    ScrollbarHandle::start_drag(&list);
    list.reset_with_estimate(100, px(25.));
    assert!(list.0.borrow().pending.is_none());
    assert!(list.0.borrow().drag_extent.is_none());
    assert_eq!(list.measured_height(40), None);
    assert_eq!(list.logical_scroll_top().item_ix, 0);
    let weak = Rc::downgrade(&list.0);
    drop(list);
    assert!(
        weak.upgrade().is_none(),
        "geometry has no background/self-retaining owner"
    );
}

#[test]
fn scrollbar_endpoint_uses_the_exposed_pixel_extent_not_f64_rounding() {
    let list = list(20_005);
    list.set_estimate(px(21.8123));
    measure(&list, 0, 21.731);
    ScrollbarHandle::start_drag(&list);
    let endpoint = ScrollbarHandle::content_size(&list).height - list.viewport_bounds().size.height;
    ScrollbarHandle::set_offset(&list, point(px(0.), -endpoint));
    assert!(
        matches!(list.0.borrow().pending, Some(Intent::End)),
        "the public scrollbar's exact endpoint must resolve the real last row"
    );
}

#[test]
fn empty_end_does_not_become_follow_on_later_append() {
    let list = list(0);
    list.scroll_to_end();
    list.splice(0..0, 100);
    assert!(list.0.borrow().pending.is_none());
    assert_eq!(list.logical_scroll_top().item_ix, 0);
}

// Exercise the production layout algorithm with deterministic row measurements.
// Native cases separately exercise actual GPUI elements and painted bounds.
fn layout(list: &VariableListState, height: impl Fn(usize) -> f64) -> Vec<LaidOutRow> {
    layout_rows(list, |ix| {
        measure(list, ix, height(ix));
        LaidOutRow {
            ix,
            key: list.0.borrow().rows[ix].key,
            element: gpui::div().into_any_element(),
        }
    })
}

fn assert_filled_end(list: &VariableListState, rows: &[LaidOutRow]) {
    let state = list.0.borrow();
    assert!(
        (state.offset() - state.maximum()).abs() < 0.001,
        "tail must bottom-align after layout: offset={}, maximum={}",
        state.offset(),
        state.maximum()
    );
    assert_eq!(rows.last().unwrap().ix, state.rows.len() - 1);
    assert!(state.prefix(rows[0].ix) <= state.offset());
    assert!(state.pending.is_none(), "clamping is one-shot, not follow");
}

#[test]
fn near_tail_scroll_to_fills_viewport_from_actual_predecessors() {
    let list = list(100);
    list.scroll_to(ListOffset {
        item_ix: 99,
        offset_in_item: px(0.),
    });
    let rows = layout(&list, |_| 20.);
    assert_filled_end(&list, &rows);
    let before = list.pixel_offset();
    list.scroll_by(px(-5.));
    assert_eq!(list.pixel_offset(), before - px(5.));
}

#[test]
fn end_then_shrinking_metrics_fills_viewport_without_tail_follow() {
    let list = list(100);
    list.scroll_to_end();
    layout(&list, |_| 40.);
    list.remeasure();
    let rows = layout(&list, |_| 20.);
    assert_filled_end(&list, &rows);
    let before = list.logical_scroll_top();
    list.splice(100..100, 10);
    assert_eq!(list.logical_scroll_top().item_ix, before.item_ix);
    assert_eq!(
        list.logical_scroll_top().offset_in_item,
        before.offset_in_item
    );
    assert!(list.0.borrow().pending.is_none());
}

#[test]
fn near_tail_card_collapse_fills_viewport_from_actual_predecessors() {
    let list = list(100);
    list.scroll_to_end();
    layout(&list, |ix| if ix == 99 { 200. } else { 20. });
    list.splice(99..100, 1);
    let rows = layout(&list, |_| 20.);
    assert_filled_end(&list, &rows);
}

#[test]
fn same_count_card_reload_keeps_reading_fraction_and_pending_reveal() {
    let list = list(100);
    measure(&list, 40, 200.);
    list.scroll_to(ListOffset {
        item_ix: 40,
        offset_in_item: px(50.),
    });
    list.scroll_to_reveal_item(40);
    let old_key = list.0.borrow().rows[40].key;
    list.splice(40..41, 1);
    measure(&list, 40, 400.);
    assert_eq!(
        list.logical_scroll_top().item_ix,
        40,
        "replacement is the same reading slot"
    );
    assert_eq!(list.logical_scroll_top().offset_in_item, px(100.));
    let state = list.0.borrow();
    assert!(matches!(state.pending, Some(Intent::Reveal(40))));
    assert_ne!(
        state.rows[40].key, old_key,
        "replacement still invalidates element identity"
    );
}

#[test]
fn near_tail_jump_keeps_target_when_actual_successors_exceed_estimates() {
    let list = list(100);
    list.scroll_to(ListOffset {
        item_ix: 96,
        offset_in_item: px(0.),
    });
    let rows = layout(&list, |_| 40.);
    assert_eq!(
        list.logical_scroll_top().item_ix,
        96,
        "an explicit target must not become End from estimated remaining height"
    );
    assert_eq!(list.logical_scroll_top().offset_in_item, px(0.));
    assert_eq!(rows.first().unwrap().ix, 96);
    assert!(list.0.borrow().offset() < list.0.borrow().maximum());
}
