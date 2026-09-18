//! Sidebar gesture navigation (ADR-0199).
//!
//! One trackpad gesture moves only the sidebar. The state machine owns the
//! gesture (`Idle → Tracking → Settling → Idle`), the resisted visual offset,
//! and the page the gesture committed to (`pending`). It never owns the page
//! the main pane shows: the UI derives that from its own mode state and only
//! switches it when [`SidebarSwipe::tick`] reports the settle finished.
//!
//! Pages are indices into an ordered list the UI supplies at gesture start.
//! `origin` is fixed for the whole gesture, so a gesture can only ever land on
//! `origin - 1`, `origin`, or `origin + 1`.

/// Raw gesture progress is compressed with `tanh(progress * RESISTANCE)`, so
/// the sidebar always moves less than the fingers and never past one page.
pub const RESISTANCE: f32 = 0.35;
/// `|raw_dx / width|` at or beyond which a release commits to the neighbour.
pub const COMMIT_PROGRESS: f32 = 0.20;

/// Axis lock. The gesture is horizontal only once it has moved this far *and*
/// dominates the vertical travel: scrolling a list is the common case, so a
/// diagonal gesture resolves vertical rather than sitting undecided.
const AXIS_CLAIM_DISTANCE: f32 = 8.0;
const HORIZONTAL_DOMINANCE: f32 = 1.2;

/// Critically damped spring for the settle (`damping = 2 * sqrt(stiffness)`):
/// no overshoot, so the sidebar never passes its snap target. Settling time is
/// proportional to `1 / sqrt(stiffness)`, so ω = 60 rad/s lands the page twice
/// as fast as the 30 rad/s cut and 3× as fast as the first one — chaining
/// gestures (Graph → PRs → Issues) must not feel like waiting (user request).
const STIFFNESS: f32 = 3600.0;
const DAMPING: f32 = 120.0;
/// Integration step cap; a longer frame is split into this many sub-steps.
///
/// Semi-implicit Euler needs `damping * step < 2` to decay instead of ringing,
/// and at ω = 60 a whole 60 Hz frame sits exactly on that boundary. Four
/// sub-steps per frame (`damping * step = 0.5`) keep the settle monotone and
/// frame-rate independent; raising ω again means lowering this too.
const MAX_STEP: f32 = 1.0 / 240.0;
/// Rest thresholds (px, px/s) at which the settle is declared finished.
const REST_DISTANCE: f32 = 0.5;
const REST_VELOCITY: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Axis {
    #[default]
    Undecided,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Phase {
    #[default]
    Idle,
    Tracking {
        axis: Axis,
        raw_x: f32,
        raw_y: f32,
    },
    Settling {
        velocity: f32,
    },
}

/// One frame of a settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    /// Not settling; nothing to animate.
    Idle,
    /// Still moving; draw `offset()` and tick again next frame.
    Running,
    /// Rested at the snap target. Carries the page the gesture committed to
    /// (`None` = snapped back to `origin`). The offset is already normalised
    /// to `0` for the new page arrangement.
    Finished(Option<usize>),
}

#[derive(Debug, Default)]
pub struct SidebarSwipe {
    phase: Phase,
    origin: usize,
    page_count: usize,
    width: f32,
    offset: f32,
    pending: Option<usize>,
}

impl SidebarSwipe {
    /// Begin tracking from `origin` of `page_count` pages laid out `width`
    /// pixels wide. Ignored while a settle is still running: the previous
    /// gesture's page has not been applied yet, so a new origin would lie.
    pub fn start(&mut self, origin: usize, page_count: usize, width: f32) {
        if matches!(self.phase, Phase::Settling { .. }) {
            return;
        }
        if !width.is_finite() || width <= 0.0 || origin >= page_count {
            self.cancel();
            return;
        }
        *self = Self {
            phase: Phase::Tracking {
                axis: Axis::Undecided,
                raw_x: 0.0,
                raw_y: 0.0,
            },
            origin,
            page_count,
            width,
            offset: 0.0,
            pending: None,
        };
    }

    /// Drop the gesture and any pending page; the sidebar returns to `0`.
    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    /// Accumulate raw input. Only a horizontal gesture moves the sidebar; the
    /// axis is claimed once and kept for the rest of the gesture.
    pub fn move_by(&mut self, x: f32, y: f32) {
        let Phase::Tracking {
            mut axis,
            mut raw_x,
            mut raw_y,
        } = self.phase
        else {
            return;
        };
        if !x.is_finite() || !y.is_finite() {
            self.cancel();
            return;
        }
        raw_x += x;
        raw_y += y;
        if axis == Axis::Undecided {
            let ax = raw_x.abs();
            let ay = raw_y.abs();
            if ax > AXIS_CLAIM_DISTANCE && ax > ay * HORIZONTAL_DOMINANCE {
                axis = Axis::Horizontal;
            } else if ay > AXIS_CLAIM_DISTANCE {
                axis = Axis::Vertical;
            }
        }
        self.phase = Phase::Tracking { axis, raw_x, raw_y };
        self.offset = match (axis, self.neighbour_for(raw_x)) {
            (Axis::Horizontal, Some(_)) => self.width * (raw_x / self.width * RESISTANCE).tanh(),
            _ => 0.0,
        };
    }

    /// Fingers lifted: decide on the raw distance, then settle from the
    /// current offset. The page is only reported by [`Self::tick`].
    ///
    /// A gesture that never became horizontal moved nothing, so it ends
    /// outright — settling a zero offset would hold the wheel for a frame after
    /// every list scroll.
    pub fn release(&mut self) {
        let Phase::Tracking { axis, raw_x, .. } = self.phase else {
            return;
        };
        if axis != Axis::Horizontal {
            self.cancel();
            return;
        }
        self.pending = if (raw_x / self.width).abs() >= COMMIT_PROGRESS {
            self.neighbour_for(raw_x)
        } else {
            None
        };
        self.phase = Phase::Settling { velocity: 0.0 };
    }

    /// Advance the settle by `dt` seconds.
    pub fn tick(&mut self, dt: f32) -> Settle {
        let Phase::Settling { mut velocity } = self.phase else {
            return Settle::Idle;
        };
        let target = self.snap_target();
        let mut remaining = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        while remaining > 0.0 {
            let step = remaining.min(MAX_STEP);
            remaining -= step;
            let displacement = self.offset - target;
            let accel = -STIFFNESS * displacement - DAMPING * velocity;
            velocity += accel * step;
            self.offset += velocity * step;
        }
        self.phase = Phase::Settling { velocity };
        if (self.offset - target).abs() < REST_DISTANCE && velocity.abs() < REST_VELOCITY {
            self.offset = target;
            return self.complete();
        }
        Settle::Running
    }

    /// Jump straight to the snap target (reduced motion, or a headless
    /// harness that cannot wait for frames).
    pub fn complete(&mut self) -> Settle {
        if !matches!(self.phase, Phase::Settling { .. }) {
            return Settle::Idle;
        }
        let page = self.pending.take();
        *self = Self::default();
        Settle::Finished(page)
    }

    /// Sidebar translation in pixels: positive reveals the previous page.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Page the gesture has committed to; only set from release to finish.
    pub fn pending(&self) -> Option<usize> {
        self.pending
    }

    pub fn is_idle(&self) -> bool {
        self.phase == Phase::Idle
    }

    pub fn is_settling(&self) -> bool {
        matches!(self.phase, Phase::Settling { .. })
    }

    /// Whether the gesture owns the wheel, so the page under it must not
    /// scroll.
    ///
    /// True only once the axis has resolved *horizontal*, and for the whole
    /// settle. That is what removes the vertical component of a sideways swipe:
    /// the page beneath cannot consume the `y` delta while the sidebar is being
    /// dragged. Claiming it any earlier stopped a list mid-scroll on a gesture
    /// that merely had a slight sideways component (user report): the axis is
    /// undecided below `AXIS_CLAIM_DISTANCE`, and an undecided gesture has no
    /// grounds to take the wheel off the page.
    pub fn owns_wheel(&self) -> bool {
        match self.phase {
            Phase::Idle => false,
            Phase::Tracking { axis, .. } => axis == Axis::Horizontal,
            Phase::Settling { .. } => true,
        }
    }

    fn snap_target(&self) -> f32 {
        match self.pending {
            Some(page) if page < self.origin => self.width,
            Some(_) => -self.width,
            None => 0.0,
        }
    }

    /// The adjacent page a rightward (`raw_x > 0`, previous) or leftward
    /// (next) drag heads for, if the origin has one on that side.
    fn neighbour_for(&self, raw_x: f32) -> Option<usize> {
        if raw_x > 0.0 {
            self.origin.checked_sub(1)
        } else if raw_x < 0.0 && self.origin + 1 < self.page_count {
            Some(self.origin + 1)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 300.0;

    fn tracking(origin: usize, x: f32, y: f32) -> SidebarSwipe {
        let mut swipe = SidebarSwipe::default();
        swipe.start(origin, 3, W);
        swipe.move_by(x, y);
        swipe
    }

    fn settle(swipe: &mut SidebarSwipe) -> Option<usize> {
        for _ in 0..600 {
            if let Settle::Finished(page) = swipe.tick(1.0 / 60.0) {
                return page;
            }
        }
        panic!("settle never rested");
    }

    #[test]
    fn sidebar_moves_less_than_the_fingers_and_never_past_one_page() {
        let swipe = tracking(1, -90.0, 0.0);
        let offset = swipe.offset();
        assert!(offset < 0.0 && offset.abs() < 90.0, "offset={offset}");
        assert!((offset - W * (-0.3f32 * RESISTANCE).tanh()).abs() < 1e-3);

        // `tanh` asymptotes at one page: the sidebar can reach the neighbour's
        // position but never travels past it, however far the fingers go.
        let huge = tracking(1, -100_000.0, 0.0);
        assert!(huge.offset() >= -W && huge.offset() < -0.9 * W);
    }

    #[test]
    fn twenty_percent_commits_and_less_snaps_back() {
        let mut swipe = tracking(1, -(W * 0.2), 0.0);
        swipe.release();
        assert_eq!(swipe.pending(), Some(2));
        assert_eq!(settle(&mut swipe), Some(2));

        let mut swipe = tracking(1, -(W * 0.2) + 0.5, 0.0);
        swipe.release();
        assert_eq!(swipe.pending(), None);
        assert_eq!(settle(&mut swipe), None);

        let mut swipe = tracking(1, W * 0.25, 0.0);
        swipe.release();
        assert_eq!(settle(&mut swipe), Some(0));
    }

    #[test]
    fn one_gesture_moves_at_most_one_page_from_its_origin() {
        let mut swipe = tracking(0, -(W * 5.0), 0.0);
        swipe.move_by(-(W * 5.0), 0.0);
        swipe.release();
        assert_eq!(swipe.pending(), Some(1));
        assert_eq!(settle(&mut swipe), Some(1));

        // Momentum after release has no Started phase, so it is ignored.
        swipe.move_by(-(W * 5.0), 0.0);
        assert_eq!(swipe.offset(), 0.0);
        assert!(swipe.is_idle());
    }

    #[test]
    fn a_horizontal_gesture_owns_the_wheel_and_a_vertical_one_hands_it_back() {
        let mut idle = SidebarSwipe::default();
        assert!(!idle.owns_wheel(), "no gesture, no claim");

        // The page keeps the wheel until the gesture is provably horizontal,
        // and the claim is held through the settle so momentum cannot scroll it.
        idle.start(1, 3, W);
        assert!(
            !idle.owns_wheel(),
            "a gesture with no axis yet takes nothing"
        );
        idle.move_by(-4.0, 3.0);
        assert!(
            !idle.owns_wheel(),
            "an undecided axis must leave the list scrolling"
        );
        idle.move_by(-60.0, 0.0);
        assert!(idle.owns_wheel());
        idle.release();
        assert!(idle.owns_wheel());
        settle(&mut idle);
        assert!(!idle.owns_wheel(), "a settled sidebar returns the wheel");

        // A slight sideways component must not stop a list scroll (user
        // report): anything that is not dominated by `x` resolves vertical, and
        // releasing it ends the gesture without a settle to hold the wheel.
        for (x, y) in [(0.0, 40.0), (12.0, 12.0), (20.0, 30.0)] {
            let mut vertical = tracking(1, x, y);
            assert!(
                !vertical.owns_wheel(),
                "({x}, {y}) belongs to the page under it"
            );
            assert_eq!(vertical.offset(), 0.0);
            vertical.release();
            assert!(vertical.is_idle(), "({x}, {y}) must not settle");
        }
    }

    #[test]
    fn edges_have_no_neighbour_to_reveal() {
        let swipe = tracking(0, W, 0.0);
        assert_eq!(swipe.offset(), 0.0);
        let mut swipe = tracking(2, -W, 0.0);
        assert_eq!(swipe.offset(), 0.0);
        swipe.release();
        assert_eq!(settle(&mut swipe), None);

        let mut single = SidebarSwipe::default();
        single.start(0, 1, W);
        single.move_by(-W, 0.0);
        single.release();
        assert_eq!(settle(&mut single), None);
    }

    #[test]
    fn release_continues_from_the_tracked_offset_without_a_jump() {
        let mut swipe = tracking(1, -(W * 0.5), 0.0);
        let tracked = swipe.offset();
        swipe.release();
        assert_eq!(swipe.offset(), tracked);
        assert!(swipe.is_settling());

        let mut last = tracked;
        loop {
            match swipe.tick(1.0 / 60.0) {
                Settle::Running => {
                    let now = swipe.offset();
                    assert!(now <= last + 1e-3 && now >= -W, "monotone toward -W");
                    last = now;
                }
                Settle::Finished(page) => {
                    assert_eq!(page, Some(2));
                    break;
                }
                Settle::Idle => panic!("settling"),
            }
        }
        assert_eq!(swipe.offset(), 0.0);
    }

    #[test]
    fn page_is_reported_only_when_the_settle_finishes() {
        let mut swipe = tracking(1, -W, 0.0);
        swipe.release();
        assert_eq!(swipe.tick(1.0 / 60.0), Settle::Running);
        assert!(swipe.pending().is_some());
        assert_eq!(swipe.complete(), Settle::Finished(Some(2)));
        assert_eq!(swipe.pending(), None);
        assert_eq!(swipe.tick(1.0), Settle::Idle);
    }

    #[test]
    fn a_new_gesture_cannot_start_mid_settle() {
        let mut swipe = tracking(1, -W, 0.0);
        swipe.release();
        swipe.start(1, 3, W);
        swipe.move_by(W, 0.0);
        assert!(swipe.is_settling());
        assert_eq!(swipe.pending(), Some(2));
    }

    #[test]
    fn vertical_and_diagonal_input_never_moves_the_sidebar() {
        for (x, y) in [(0.0, 100.0), (60.0, 60.0), (-8.0, 0.0)] {
            let mut swipe = tracking(1, x, y);
            assert_eq!(swipe.offset(), 0.0, "({x}, {y})");
            swipe.release();
            assert_eq!(swipe.pending(), None);
        }
        // Once vertical is claimed, a later horizontal burst stays ignored.
        let mut swipe = tracking(1, 0.0, 40.0);
        swipe.move_by(-W, 0.0);
        assert_eq!(swipe.offset(), 0.0);
    }

    #[test]
    fn cancellation_and_invalid_input_reset_everything() {
        let mut swipe = tracking(1, -W, 0.0);
        swipe.cancel();
        assert!(swipe.is_idle());
        assert_eq!(swipe.offset(), 0.0);
        swipe.release();
        assert_eq!(swipe.tick(1.0), Settle::Idle);

        let mut swipe = tracking(1, -W, 0.0);
        swipe.move_by(f32::NAN, 0.0);
        assert!(swipe.is_idle());

        let mut swipe = SidebarSwipe::default();
        swipe.start(3, 3, W);
        assert!(swipe.is_idle());
        swipe.start(0, 3, 0.0);
        assert!(swipe.is_idle());
    }
}
