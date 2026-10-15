use super::*;
use gpui::px;

#[test]
fn the_first_render_is_at_its_end_without_moving() {
    let now = Instant::now();
    let mut motion = Reveal::default();
    motion.sync(true, now, false);
    assert_eq!(motion.visible(now), 1.);
    assert!(!motion.animating(now));
}

#[test]
fn opening_eases_out_over_180ms_and_closing_eases_in_over_150ms() {
    let start = Instant::now();
    let mut motion = Reveal::default();
    motion.sync(false, start, false);
    motion.sync(true, start, false);
    assert_eq!(motion.visible(start), 0.);
    assert!(motion.visible(start + Duration::from_millis(90)) > 0.8);
    assert!(motion.animating(start + Duration::from_millis(179)));
    assert_eq!(motion.visible(start + OPEN), 1.);
    assert!(!motion.animating(start + OPEN));
    let closing = start + Duration::from_millis(500);
    motion.sync(false, closing, false);
    assert!(motion.visible(closing + Duration::from_millis(75)) > 0.8);
    assert_eq!(motion.visible(closing + CLOSE), 0.);
}

#[test]
fn a_toggle_in_mid_flight_turns_around_from_the_current_height() {
    let start = Instant::now();
    let mut motion = Reveal::default();
    motion.sync(false, start, false);
    motion.sync(true, start, false);
    let turn = start + Duration::from_millis(60);
    let height = motion.visible(turn);
    motion.sync(false, turn, false);
    assert_eq!(motion.visible(turn), height);
    let back = reveal_duration(false, height);
    assert!(motion.animating(turn + back - Duration::from_millis(1)));
    assert_eq!(motion.visible(turn + back), 0.);
}

#[test]
fn reduce_motion_lands_at_once_even_in_mid_flight() {
    let start = Instant::now();
    let mut motion = Reveal::default();
    motion.sync(true, start, false);
    motion.sync(false, start, true);
    assert_eq!(motion.visible(start), 0.);
    assert!(!motion.animating(start));
    motion.sync(true, start, false);
    motion.sync(true, start + Duration::from_millis(20), true);
    assert_eq!(motion.visible(start + Duration::from_millis(20)), 1.);
}

#[test]
fn keyed_transition_restarts_generation_without_a_value_jump() {
    let now = Instant::now();
    let mut transition = KeyedTransition::new("a", 0.);
    assert!(transition.retarget("b", 1., now, TRANSITION, Easing::OutCubic, false));
    assert_eq!(transition.generation(), 1);
    assert_eq!(transition.sample(now).progress, 0.);
    assert!(!transition.sample(now).done);
    let mid = now + TRANSITION / 2;
    let value = transition.sample(mid).value;
    assert_eq!(value, 0.875);
    assert!(!transition.retarget("b", 1., mid, TRANSITION, Easing::OutCubic, false));
    assert_eq!(transition.generation(), 1);
    assert!(transition.retarget("c", 0., mid, TRANSITION, Easing::InCubic, false));
    assert_eq!(transition.generation(), 2);
    assert_eq!(transition.key(), &"c");
    assert_eq!(transition.sample(mid).value, value);
    assert_eq!(transition.sample(mid).progress, 0.);
    assert_eq!(transition.sample(mid + TRANSITION).value, 0.);
    assert!(transition.sample(mid + TRANSITION).done);
    // A new logical key restarts even when its numeric target is unchanged.
    assert!(transition.retarget("d", 0., mid, TRANSITION, Easing::OutCubic, false));
    assert_eq!(transition.generation(), 3);
    assert_eq!(transition.sample(mid).progress, 0.);
}

#[test]
fn transition_zero_duration_instant_and_clock_bounds() {
    let now = Instant::now();
    let mut transition = KeyedTransition::new(false, 0.);
    transition.retarget(true, 1., now, Duration::ZERO, Easing::OutCubic, false);
    assert!(transition.sample(now).done);
    assert_eq!(transition.sample(now).value, 1.);
    transition.retarget(false, 0., now, TRANSITION, Easing::InCubic, false);
    assert_eq!(
        transition.sample(now - Duration::from_millis(1)).progress,
        0.
    );
    transition.retarget(false, 0., now, TRANSITION, Easing::InCubic, true);
    assert!(transition.sample(now).done);
    assert_eq!(transition.sample(now).value, 0.);
    assert_eq!(Easing::OutCubic.apply(-1.), 0.);
    assert_eq!(Easing::InCubic.apply(2.), 1.);
}

#[test]
fn row_splice_inserts_n_rows_with_cumulative_offsets() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(["a", "b", "c"], true, now, false);
    assert!(splice.advance(now, false));
    assert_eq!(splice.row(&"tail", px(20.), now, |_| true).offset, px(-60.));
    assert_eq!(splice.row(&"head", px(20.), now, |_| false).offset, px(0.));
    assert_eq!(
        splice.row(&"b", px(20.), now, |key| *key == "a").offset,
        px(-20.)
    );
    assert_eq!(splice.row(&"b", px(20.), now, |_| false).opacity, 0.);
    let mid = now + TRANSITION / 2;
    assert_eq!(splice.row(&"tail", px(20.), mid, |_| true).offset, px(-7.5));
    assert_eq!(splice.row(&"b", px(20.), mid, |_| false).opacity, 0.875);
    assert!(!splice.advance(now + TRANSITION, false));
    let end = splice.row(&"tail", px(20.), now + TRANSITION, |_| true);
    assert_eq!(end.offset, px(0.));
    assert_eq!(end.opacity, 1.);
}

#[test]
fn row_splice_removes_n_rows_and_positions_retained_ghosts() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(["a", "b", "c"], false, now, false);
    assert_eq!(splice.row(&"tail", px(20.), now, |_| true).offset, px(60.));
    let ghost = splice
        .ghost(&"c", Some(&"head"), [&"a", &"b"], px(20.), now)
        .unwrap();
    assert_eq!(ghost.after, Some(&"head"));
    assert_eq!(ghost.offset, px(60.));
    assert_eq!(ghost.opacity, 1.);
    let mid = now + TRANSITION / 2;
    assert_eq!(splice.row(&"tail", px(20.), mid, |_| true).offset, px(7.5));
    let ghost = splice
        .ghost(&"c", Some(&"head"), [&"a", &"b"], px(20.), mid)
        .unwrap();
    assert_eq!(ghost.offset, px(25.));
    assert_eq!(ghost.opacity, 0.125);
    let first = splice.ghost(&"a", None, [], px(20.), mid).unwrap();
    assert_eq!(
        first.offset,
        px(0.),
        "unanchored ghost starts at list origin"
    );
    assert!(splice
        .ghost(&"c", Some(&"head"), [&"a", &"b"], px(20.), now + TRANSITION)
        .is_none());
    assert!(!splice.advance(now + TRANSITION, false));
    assert_eq!(
        splice
            .row(&"tail", px(20.), now + TRANSITION, |_| true)
            .offset,
        px(0.)
    );
}

#[test]
fn overlapping_row_splices_preserve_existing_presentation() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(["a", "b"], true, now, false);
    let mid = now + TRANSITION / 2;
    // Final tail slot is after two new rows; visual position is 35px.
    let before = px(40.) + splice.row(&"tail", px(20.), mid, |_| true).offset;
    splice.retarget(["c"], true, mid, false);
    let after = px(60.) + splice.row(&"tail", px(20.), mid, |_| true).offset;
    assert_eq!(
        before, after,
        "another insertion must not jump existing rows"
    );
    assert_eq!(splice.row(&"a", px(20.), mid, |_| false).opacity, 0.875);
    // An unrelated removal likewise shifts the final layout baseline, not the
    // current presentation or the already-running insertion generations.
    splice.retarget(["old"], false, mid, false);
    assert_eq!(
        px(40.) + splice.row(&"tail", px(20.), mid, |_| true).offset,
        before
    );
    assert_eq!(
        splice
            .row(&"a", px(20.), now + TRANSITION, |_| false)
            .opacity,
        1.
    );
    assert!(
        splice.advance(now + TRANSITION, false),
        "later batch still moves"
    );
    assert!(!splice.advance(mid + TRANSITION, false));
}

#[test]
fn row_splice_reverses_opening_and_closing_without_position_or_opacity_jumps() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(["a", "b"], true, now, false);
    let turn = now + TRANSITION / 2;
    let before = px(40.) + splice.row(&"tail", px(20.), turn, |_| true).offset;
    let opacity = splice.row(&"a", px(20.), turn, |_| false).opacity;
    splice.retarget(["a", "b"], false, turn, false);
    assert_eq!(splice.row(&"tail", px(20.), turn, |_| true).offset, before);
    let first = splice.ghost(&"a", None, [], px(20.), turn).unwrap();
    let second = splice.ghost(&"b", None, [&"a"], px(20.), turn).unwrap();
    assert_eq!(first.opacity, opacity);
    assert_eq!(second.offset, px(20.) * opacity);
    let back = turn + TRANSITION / 2;
    let before = splice.row(&"tail", px(20.), back, |_| true).offset;
    let opacity = splice.ghost(&"a", None, [], px(20.), back).unwrap().opacity;
    splice.retarget(["a", "b"], true, back, false);
    assert_eq!(
        px(40.) + splice.row(&"tail", px(20.), back, |_| true).offset,
        before
    );
    assert_eq!(splice.row(&"a", px(20.), back, |_| false).opacity, opacity);
    assert!(splice.ghost(&"a", None, [], px(20.), back).is_none());
}

#[test]
fn ghost_anchor_can_be_removed_or_reinserted_mid_flight() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(["b"], false, now, false);
    let mid = now + TRANSITION / 2;
    let before = splice.ghost(&"b", Some(&"a"), [], px(20.), mid).unwrap();
    splice.retarget(["a"], false, mid, false);
    let after = splice.ghost(&"b", None, [&"a"], px(20.), mid).unwrap();
    assert_eq!(
        before.offset, after.offset,
        "caller reanchors removed predecessor"
    );
    let back = mid + TRANSITION / 4;
    let before = splice.ghost(&"b", None, [&"a"], px(20.), back).unwrap();
    splice.retarget(["a"], true, back, false);
    let after = splice.ghost(&"b", Some(&"a"), [], px(20.), back).unwrap();
    assert_eq!(
        before.offset, after.offset,
        "partial anchor contributes its current presence"
    );
}

#[test]
fn row_splice_instant_settles_insertions_removals_and_active_batches() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(["a", "b"], true, now, true);
    splice.retarget(["old"], false, now, true);
    assert!(!splice.advance(now, false));
    assert_eq!(splice.row(&"a", px(20.), now, |_| true).opacity, 1.);
    assert!(splice.ghost(&"old", None, [], px(20.), now).is_none());
    splice.retarget(["c"], true, now, false);
    splice.retarget(["d"], false, now, false);
    assert!(splice.advance(now, false));
    assert!(!splice.advance(now, true));
    assert!(
        !splice.advance(now, false),
        "re-enabling motion cannot revive a batch"
    );
    assert_eq!(splice.row(&"tail", px(20.), now, |_| true).offset, px(0.));
    assert!(splice.ghost(&"d", None, [], px(20.), now).is_none());
}

#[test]
fn oversized_row_splice_settles_entire_owner_without_consuming_huge_batch() {
    let now = Instant::now();
    for present in [true, false] {
        let mut splice = RowSplice::default();
        splice.retarget([usize::MAX], false, now, false);
        let mut consumed = 0;
        splice.retarget((0..10_000).inspect(|_| consumed += 1), present, now, false);
        assert_eq!(
            consumed, MAX_ANIMATED_ROWS,
            "one older key already occupied capacity"
        );
        assert!(splice.rows.is_empty(), "overflow settles old and new keys");
        assert!(!splice.advance(now, false));
        let row = splice.row(&0, px(20.), now, |_| true);
        assert_eq!(row.offset, px(0.));
        assert_eq!(row.opacity, 1.);
        assert!(splice.ghost(&usize::MAX, None, [], px(20.), now).is_none());
        assert!(splice.ghost(&0, None, [], px(20.), now).is_none());
    }
}

#[test]
fn row_splice_sampling_and_ghost_iteration_are_bounded() {
    let now = Instant::now();
    let mut splice = RowSplice::default();
    splice.retarget(0..MAX_ANIMATED_ROWS, true, now, false);
    assert_eq!(splice.rows.len(), MAX_ANIMATED_ROWS);
    // Retargeting existing keys at capacity is not an overflow.
    splice.retarget(0..MAX_ANIMATED_ROWS, false, now + TRANSITION, false);
    assert!(splice.advance(now + TRANSITION, false));
    let mut visits = 0;
    splice.row(&usize::MAX, px(20.), now + TRANSITION, |_| {
        visits += 1;
        true
    });
    assert_eq!(visits, MAX_ANIMATED_ROWS);
    // Even an incorrectly supplied historical iterator cannot create an
    // unbounded per-frame scan. Correct callers pass only active removed keys.
    let unknown = usize::MAX;
    let mut ghost_visits = 0;
    let between = std::iter::repeat_n(&unknown, 10_000).inspect(|_| ghost_visits += 1);
    assert!(splice
        .ghost(&0, None, between, px(20.), now + TRANSITION)
        .is_some());
    assert_eq!(ghost_visits, MAX_ANIMATED_ROWS);
    splice.retarget([MAX_ANIMATED_ROWS], false, now + TRANSITION, false);
    assert!(
        splice.rows.is_empty(),
        "one additional distinct key settles all"
    );
}

#[test]
fn measured_reveal_retains_closing_content_and_accepts_new_measurement() {
    let now = Instant::now();
    let mut reveal = Reveal::default();
    reveal.sync(true, now, false);
    assert_eq!(reveal.sample(px(200.), now).height, px(200.));
    reveal.sync(false, now, false);
    assert!(reveal.sample(px(200.), now).retain_content);
    let mid = now + CLOSE / 2;
    let sample = reveal.sample(px(200.), mid);
    assert_eq!(sample.fraction, 0.875);
    assert_eq!(sample.height, px(175.));
    assert!(sample.retain_content);
    assert_eq!(reveal.sample(px(400.), mid).height, px(350.));
    reveal.sync(true, mid, false);
    assert_eq!(reveal.sample(px(200.), mid).height, sample.height);
    reveal.sync(false, mid, true);
    let closed = reveal.sample(px(200.), mid);
    assert_eq!(closed.height, px(0.));
    assert!(!closed.retain_content);
    assert!(closed.done);
}

#[cfg(feature = "gui-e2e")]
#[test]
fn gui_clock_is_shared_and_default_policy_is_instant() {
    set_clock_for_e2e(None);
    assert!(instant());
    let frozen = Instant::now();
    set_clock_for_e2e(Some(frozen));
    assert_eq!(now(), frozen);
    assert_eq!(instant(), crate::theme::reduce_motion());
    set_clock_for_e2e(None);
    assert!(instant());
}
