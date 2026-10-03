//! The bottom panel's open / close motion (#950).
//!
//! Only the height of the panel's outer box moves: its content keeps the
//! saved height and is clipped, so the Terminal's grid (and with it the PTY
//! size) never changes while the panel slides. Opening takes
//! [`OPEN`] with an ease-out curve, closing [`CLOSE`] with an ease-in curve.
//! A toggle in mid-flight turns around from the current height, and the time
//! it takes is scaled by the distance left, so repeated Cmd+J never jumps.
//! With `reduce_motion` (or a first render) the panel is simply at its end.

use std::time::{Duration, Instant};

/// Time to open the panel from fully closed.
pub(crate) const OPEN: Duration = Duration::from_millis(180);
/// Time to close the panel from fully open.
pub(crate) const CLOSE: Duration = Duration::from_millis(150);

/// The time the motion is read at. The GUI runner stands a clock in
/// ([`super::e2e::set_panel_motion_clock`]) so a scenario can stop the panel
/// in mid-flight; without one, a runner scenario sees the panel at its end.
pub(crate) fn now() -> Instant {
    #[cfg(feature = "gui-e2e")]
    if let Some(now) = super::e2e::panel_motion_clock() {
        return now;
    }
    Instant::now()
}

/// Whether the panel jumps to its end: `reduce_motion`, or a runner
/// scenario that did not ask for the motion.
pub(crate) fn instant() -> bool {
    #[cfg(feature = "gui-e2e")]
    if super::e2e::panel_motion_clock().is_none() {
        return true;
    }
    super::theme::reduce_motion()
}

/// Where the panel is: the target it moves to, the visible fraction it left
/// from, and when it started. `None` target: nothing rendered yet.
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelMotion {
    target: Option<bool>,
    from: f32,
    started: Option<Instant>,
}

impl PanelMotion {
    /// Follow `open`, the panel's state as the rest of the app sets it. A
    /// change starts a motion from wherever the panel is now; `instant`
    /// (reduce motion) lands at the end at once.
    pub(crate) fn sync(&mut self, open: bool, now: Instant, instant: bool) {
        if self.target == Some(open) {
            if instant {
                self.started = None;
            }
            return;
        }
        let first = self.target.is_none();
        self.from = self.visible(now);
        self.target = Some(open);
        self.started = (!first && !instant).then_some(now);
    }

    /// The visible fraction of the panel's height, 0 (closed) to 1 (open).
    pub(crate) fn visible(&self, now: Instant) -> f32 {
        let Some(open) = self.target else {
            return 0.;
        };
        let Some(started) = self.started else {
            return if open { 1. } else { 0. };
        };
        let duration = duration(open, self.from);
        if duration.is_zero() {
            return if open { 1. } else { 0. };
        }
        let t = (now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32())
            .clamp(0., 1.);
        position(open, self.from, t)
    }

    /// Whether the panel is still moving and needs another frame.
    pub(crate) fn animating(&self, now: Instant) -> bool {
        match (self.target, self.started) {
            (Some(open), Some(started)) => {
                now.saturating_duration_since(started) < duration(open, self.from)
            }
            _ => false,
        }
    }
}

/// The time a motion from `from` takes: the full time scaled by the
/// distance left, so a turn-around in mid-flight is as quick as the way back.
fn duration(open: bool, from: f32) -> Duration {
    let (full, left) = if open {
        (OPEN, 1. - from)
    } else {
        (CLOSE, from)
    };
    // Whole microseconds: an f32 product of 180ms would land a hair past it.
    Duration::from_micros((full.as_micros() as f32 * left.clamp(0., 1.)).round() as u64)
}

/// The visible fraction `t` (0..1) of the way through a motion from `from`:
/// ease-out (cubic) while opening, ease-in (cubic) while closing.
fn position(open: bool, from: f32, t: f32) -> f32 {
    if open {
        let eased = 1. - (1. - t).powi(3);
        from + (1. - from) * eased
    } else {
        from * (1. - t.powi(3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(motion: &PanelMotion, start: Instant, ms: u64) -> f32 {
        motion.visible(start + Duration::from_millis(ms))
    }

    #[test]
    fn the_first_render_is_at_its_end_without_moving() {
        let now = Instant::now();
        let mut motion = PanelMotion::default();
        motion.sync(true, now, false);
        assert_eq!(motion.visible(now), 1.);
        assert!(!motion.animating(now));
    }

    #[test]
    fn opening_eases_out_over_180ms_and_closing_eases_in_over_150ms() {
        let start = Instant::now();
        let mut motion = PanelMotion::default();
        motion.sync(false, start, false);
        motion.sync(true, start, false);
        assert_eq!(at(&motion, start, 0), 0.);
        // Ease-out: past the midpoint of the height at half the time.
        assert!(at(&motion, start, 90) > 0.8);
        assert!(motion.animating(start + Duration::from_millis(179)));
        assert_eq!(at(&motion, start, 180), 1.);
        assert!(!motion.animating(start + Duration::from_millis(180)));

        let closing = start + Duration::from_millis(500);
        motion.sync(false, closing, false);
        // Ease-in: still mostly open at half the time.
        assert!(motion.visible(closing + Duration::from_millis(75)) > 0.8);
        assert_eq!(motion.visible(closing + Duration::from_millis(150)), 0.);
    }

    #[test]
    fn a_toggle_in_mid_flight_turns_around_from_the_current_height() {
        let start = Instant::now();
        let mut motion = PanelMotion::default();
        motion.sync(false, start, false);
        motion.sync(true, start, false);
        let turn = start + Duration::from_millis(60);
        let height = motion.visible(turn);
        motion.sync(false, turn, false);
        assert_eq!(motion.visible(turn), height, "no jump at the turn");
        // The way back is the distance left, at the closing speed.
        let back = duration(false, height);
        assert!(motion.animating(turn + back - Duration::from_millis(1)));
        assert_eq!(motion.visible(turn + back), 0.);
    }

    #[test]
    fn reduce_motion_lands_at_once_even_in_mid_flight() {
        let start = Instant::now();
        let mut motion = PanelMotion::default();
        motion.sync(true, start, false);
        motion.sync(false, start, true);
        assert_eq!(motion.visible(start), 0.);
        assert!(!motion.animating(start));
        motion.sync(true, start, false);
        motion.sync(true, start + Duration::from_millis(20), true);
        assert_eq!(motion.visible(start + Duration::from_millis(20)), 1.);
    }
}
