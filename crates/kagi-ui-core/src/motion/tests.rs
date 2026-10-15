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
fn insert_offsets_add_and_inserted_opacity_settles() {
    let now = Instant::now();
    let mut first = InsertOffset::new("commit-a", now, false);
    let mut second = InsertOffset::new("worktree-path", now, false);
    assert_eq!(first.key(), &"commit-a");
    assert_eq!(
        first
            .sample(&"commit-a", false, px(29.), now, false)
            .opacity,
        0.
    );
    assert_eq!(
        first.sample(&"before", false, px(29.), now, false).offset,
        px(0.)
    );
    let a = first.sample(&"below", true, px(29.), now, false);
    let b = second.sample(&"below", true, px(29.), now, false);
    assert_eq!(a.offset + b.offset, px(-58.));
    assert_eq!(a.opacity, 1.);
    let middle = first.sample(&"commit-a", false, px(29.), now + TRANSITION / 2, false);
    assert_eq!(middle.opacity, 0.875);
    assert!(!middle.done);
    let end = first.sample(&"below", true, px(29.), now + TRANSITION, false);
    assert_eq!(end.offset, px(0.));
    assert!(end.done);
    let instant = second.sample(&"worktree-path", false, px(29.), now, true);
    assert_eq!(instant.opacity, 1.);
    assert!(instant.done);
    assert!(second.sample(&"below", true, px(29.), now, false).done);
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
