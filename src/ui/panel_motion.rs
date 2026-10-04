//! How the bottom panel (#950), the left sidebar and the right pane (#955)
//! open and close — one place for the timing all three share.
//!
//! Only the size of a pane's outer box moves (height for the bottom panel,
//! width for the side panes): its content keeps its saved size and is
//! clipped ([`clip`]), so the Terminal's grid (and with it the PTY size) and
//! the panes' layouts never change while they slide. Opening takes [`OPEN`]
//! with an ease-out curve, closing [`CLOSE`] with an ease-in curve. A toggle
//! in mid-flight turns around from the current size, and the time it takes
//! is scaled by the distance left, so repeated toggles never jump. With
//! `reduce_motion` (or a first render) a pane is simply at its end.

use std::time::{Duration, Instant};

use gpui::{div, prelude::*, Div, ElementId, Pixels, Stateful};

/// Which way a pane's outer box grows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Height (the bottom panel).
    Vertical,
    /// Width (the side panes).
    Horizontal,
}

/// Which edge of the outer box the full-size content hangs from: the edge
/// that stays put is the far one, so the content travels with the near edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// Top (vertical) or left (horizontal).
    Start,
    /// Bottom (vertical) or right (horizontal).
    End,
}

/// The outer box of a sliding pane: `full` along `axis`, cut to
/// `full × fraction`, with `child` kept at `full` and hung from `anchor`.
/// Only this box's size changes, never the child's.
pub(crate) fn clip(
    id: impl Into<ElementId>,
    axis: Axis,
    anchor: Anchor,
    full: Pixels,
    fraction: f32,
    child: impl IntoElement,
) -> Stateful<Div> {
    let cut = full * fraction.clamp(0., 1.);
    let outer = div().id(id).relative().flex_shrink_0().overflow_hidden();
    let inner = div().absolute();
    let (outer, inner) = match axis {
        Axis::Vertical => (outer.w_full().h(cut), inner.left_0().right_0().h(full)),
        Axis::Horizontal => (outer.h_full().w(cut), inner.top_0().bottom_0().w(full)),
    };
    let inner = match (axis, anchor) {
        (Axis::Vertical, Anchor::Start) => inner.top_0(),
        (Axis::Vertical, Anchor::End) => inner.bottom_0(),
        (Axis::Horizontal, Anchor::Start) => inner.left_0(),
        (Axis::Horizontal, Anchor::End) => inner.right_0(),
    };
    outer.child(inner.child(child))
}

/// The bottom and left panes follow their toggles; the right pane follows
/// its resolved slot's visibility. Its last Inspector row is exit-only content.
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelMotions {
    pub bottom: PanelMotion,
    pub sidebar: PanelMotion,
    pub right: PanelMotion,
    /// The right pane last drawn, so it can still be drawn while it closes
    /// after the layout has already moved on to `Hidden`.
    pub right_pane: Option<super::workspace::RightPane>,
    /// Last Inspector row, retained only while its clip retracts after Esc.
    pub inspector_row: Option<usize>,
    sidebar_toggle: Option<bool>,
}

impl PanelMotions {
    /// Follow the left sidebar: `shown` is whether the layout draws it,
    /// `toggle` the user's View → Toggle Sidebar state.
    pub(crate) fn sync_sidebar(&mut self, shown: bool, toggle: bool, now: Instant, instant: bool) {
        follow(
            &mut self.sidebar,
            &mut self.sidebar_toggle,
            shown,
            toggle,
            now,
            instant,
        );
    }

    /// Home, tab changes and Conflict replace the entire workspace surface.
    pub(crate) fn reset_right(&mut self) {
        self.right = PanelMotion::default();
        self.right_pane = None;
        self.inspector_row = None;
    }

    /// One right slot for Inspector, Compare and Commit Panel. Every change
    /// between shown and hidden slides while the Graph is on screen; whole
    /// workspace transitions jump, and swapping shown content does not move.
    pub(crate) fn sync_right(
        &mut self,
        pane: super::workspace::RightPane,
        selected: Option<usize>,
        graph_body: bool,
        now: Instant,
        instant: bool,
    ) {
        // A different center owns the whole surface. Do not seed a hidden
        // target here: re-entering the Graph must be a first (instant) frame.
        if !graph_body {
            self.reset_right();
            return;
        }
        use super::workspace::RightPane;
        let shown = matches!(
            pane,
            RightPane::Inspector | RightPane::Compare | RightPane::CommitPanel
        );
        self.right.sync(shown, now, instant);
        if shown {
            self.right_pane = Some(pane);
            if pane == RightPane::Inspector {
                self.inspector_row = selected;
            }
        }
    }

    /// Whether any pane still moves and needs another frame.
    pub(crate) fn animating(&self, now: Instant) -> bool {
        self.bottom.animating(now) || self.sidebar.animating(now) || self.right.animating(now)
    }
}

/// A side pane slides only when its own toggle changed with it; appearing or
/// disappearing with the toggle unchanged is a layout change, and jumps. A
/// motion already under way is left to finish.
fn follow(
    motion: &mut PanelMotion,
    last_toggle: &mut Option<bool>,
    shown: bool,
    toggle: bool,
    now: Instant,
    instant: bool,
) {
    let toggled = last_toggle.is_some_and(|last| last != toggle);
    *last_toggle = Some(toggle);
    let layout_change = motion.target.is_some_and(|target| target != shown) && !toggled;
    motion.sync(shown, now, instant || layout_change);
}

/// Time to open a pane from fully closed — the bottom panel, the sidebar and
/// the right pane alike.
pub(crate) const OPEN: Duration = Duration::from_millis(180);
/// Time to close a pane from fully open.
pub(crate) const CLOSE: Duration = Duration::from_millis(150);

/// The time the motion is read at. The GUI runner stands a clock in
/// ([`super::e2e::set_panel_motion_clock`]) so a scenario can stop a pane
/// in mid-flight; without one, a runner scenario sees every pane at its end.
pub(crate) fn now() -> Instant {
    #[cfg(feature = "gui-e2e")]
    if let Some(now) = super::e2e::panel_motion_clock() {
        return now;
    }
    Instant::now()
}

/// Whether a pane jumps to its end: `reduce_motion`, or a runner
/// scenario that did not ask for the motion.
pub(crate) fn instant() -> bool {
    #[cfg(feature = "gui-e2e")]
    if super::e2e::panel_motion_clock().is_none() {
        return true;
    }
    super::theme::reduce_motion()
}

/// Where one pane is: the target it moves to, the visible fraction it left
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

    /// The visible fraction of the pane's size, 0 (closed) to 1 (open).
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

    /// Whether the pane is still moving and needs another frame.
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

    #[test]
    fn a_side_pane_slides_on_its_toggle_and_jumps_on_a_layout_change() {
        let start = Instant::now();
        let mut panes = PanelMotions::default();
        panes.sync_sidebar(true, true, start, false);
        assert_eq!(
            panes.sidebar.visible(start),
            1.,
            "the first frame is the end"
        );

        // The user hides it: it slides, and the next frame does not cut it.
        panes.sync_sidebar(false, false, start, false);
        let mid = start + Duration::from_millis(50);
        panes.sync_sidebar(false, false, mid, false);
        assert!(panes.sidebar.animating(mid));
        let shown = panes.sidebar.visible(mid);
        assert!(0. < shown && shown < 1., "closing in mid-flight: {shown}");

        // Shown again by its toggle, then hidden by a takeover: a jump.
        let back = start + Duration::from_millis(500);
        panes.sync_sidebar(true, true, back, false);
        let settled = back + Duration::from_millis(500);
        panes.sync_sidebar(true, true, settled, false);
        panes.sync_sidebar(false, true, settled, false);
        assert_eq!(panes.sidebar.visible(settled), 0.);
        assert!(!panes.sidebar.animating(settled));
        panes.sync_sidebar(true, true, settled, false);
        assert_eq!(
            panes.sidebar.visible(settled),
            1.,
            "the takeover ends at once too"
        );
    }
}
