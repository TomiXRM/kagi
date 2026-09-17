//! Phase-bounded sidebar swipe decisions.
//!
//! Only a completed touch gesture can select an adjacent page. Rejected,
//! cancelled, and momentum-only input snaps back to the current page.

const COMMIT_DISTANCE: f32 = 60.0;
const HORIZONTAL_DOMINANCE: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSwipeResult {
    Previous,
    Next,
    SnapBack,
}

#[derive(Default)]
pub struct SidebarSwipe {
    tracking: bool,
    x: f32,
    vertical_travel: f32,
}

impl SidebarSwipe {
    pub fn start(&mut self) {
        *self = Self {
            tracking: true,
            ..Self::default()
        };
    }

    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    pub fn move_by(&mut self, x: f32, y: f32) {
        if !self.tracking {
            return;
        }
        if !x.is_finite() || !y.is_finite() {
            self.cancel();
            return;
        }
        self.x += x;
        self.vertical_travel += y.abs();
    }

    /// Select at finger release, never while the gesture is still moving.
    pub fn finish(&mut self) -> SidebarSwipeResult {
        let result = if self.tracking
            && self.x.abs() >= COMMIT_DISTANCE
            && self.x.abs() > self.vertical_travel * HORIZONTAL_DOMINANCE
        {
            if self.x < 0.0 {
                SidebarSwipeResult::Next
            } else {
                SidebarSwipeResult::Previous
            }
        } else {
            SidebarSwipeResult::SnapBack
        };
        self.cancel();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish(x: f32, y: f32) -> SidebarSwipeResult {
        let mut swipe = SidebarSwipe::default();
        swipe.start();
        swipe.move_by(x, y);
        swipe.finish()
    }

    #[test]
    fn commits_one_adjacent_page_only_at_release() {
        let mut swipe = SidebarSwipe::default();
        swipe.start();
        swipe.move_by(-40.0, 2.0);
        swipe.move_by(-20.0, -2.0);
        assert_eq!(swipe.finish(), SidebarSwipeResult::Next);
        swipe.move_by(-100.0, 0.0);
        assert_eq!(swipe.finish(), SidebarSwipeResult::SnapBack);

        assert_eq!(finish(60.0, 0.0), SidebarSwipeResult::Previous);
        assert_eq!(finish(-60.0, 0.0), SidebarSwipeResult::Next);
    }

    #[test]
    fn snap_boundary_rejects_short_vertical_and_diagonal_motion() {
        for (x, y) in [
            (59.99, 0.0),
            (-59.99, 0.0),
            (0.0, 100.0),
            (60.0, 40.0),
            (80.0, 80.0),
        ] {
            assert_eq!(finish(x, y), SidebarSwipeResult::SnapBack);
        }
        assert_eq!(finish(60.0, 39.99), SidebarSwipeResult::Previous);
    }

    #[test]
    fn cancellation_reversal_and_invalid_input_snap_back() {
        let mut swipe = SidebarSwipe::default();
        swipe.start();
        swipe.move_by(100.0, 0.0);
        swipe.cancel();
        assert_eq!(swipe.finish(), SidebarSwipeResult::SnapBack);

        swipe.start();
        swipe.move_by(80.0, 1.0);
        swipe.move_by(-40.0, 1.0);
        assert_eq!(swipe.finish(), SidebarSwipeResult::SnapBack);

        swipe.start();
        swipe.move_by(100.0, 0.0);
        swipe.move_by(f32::NAN, 0.0);
        assert_eq!(swipe.finish(), SidebarSwipeResult::SnapBack);
    }
}
