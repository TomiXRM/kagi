//! Pane-specific layout and exit-content ownership. Shared timing, clipping and
//! reversible reveal live in `kagi_ui_core::motion`.

use std::time::Instant;

use kagi_ui_core::motion::Reveal;

/// The bottom and left panes follow their toggles; the right pane follows
/// its resolved slot's visibility. Its last Inspector row is exit-only content.
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelMotions {
    pub bottom: Reveal,
    pub left: Reveal,
    pub right: Reveal,
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
            &mut self.left,
            &mut self.sidebar_toggle,
            shown,
            toggle,
            now,
            instant,
        );
    }

    /// Home, tab changes and Conflict replace the entire workspace surface.
    pub(crate) fn reset_right(&mut self) {
        self.right = Reveal::default();
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
        self.bottom.animating(now) || self.left.animating(now) || self.right.animating(now)
    }
}

/// A side pane slides only when its own toggle changed with it; appearing or
/// disappearing with the toggle unchanged is a layout change, and jumps. A
/// motion already under way is left to finish.
fn follow(
    motion: &mut Reveal,
    last_toggle: &mut Option<bool>,
    shown: bool,
    toggle: bool,
    now: Instant,
    instant: bool,
) {
    let toggled = last_toggle.is_some_and(|last| last != toggle);
    *last_toggle = Some(toggle);
    let layout_change = motion.target().is_some_and(|target| target != shown) && !toggled;
    motion.sync(shown, now, instant || layout_change);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_side_pane_slides_on_its_toggle_and_jumps_on_a_layout_change() {
        let start = Instant::now();
        let mut panes = PanelMotions::default();
        panes.sync_sidebar(true, true, start, false);
        assert_eq!(panes.left.visible(start), 1., "the first frame is the end");

        // The user hides it: it slides, and the next frame does not cut it.
        panes.sync_sidebar(false, false, start, false);
        let mid = start + Duration::from_millis(50);
        panes.sync_sidebar(false, false, mid, false);
        assert!(panes.left.animating(mid));
        let shown = panes.left.visible(mid);
        assert!(0. < shown && shown < 1., "closing in mid-flight: {shown}");

        // Shown again by its toggle, then hidden by a takeover: a jump.
        let back = start + Duration::from_millis(500);
        panes.sync_sidebar(true, true, back, false);
        let settled = back + Duration::from_millis(500);
        panes.sync_sidebar(true, true, settled, false);
        panes.sync_sidebar(false, true, settled, false);
        assert_eq!(panes.left.visible(settled), 0.);
        assert!(!panes.left.animating(settled));
        panes.sync_sidebar(true, true, settled, false);
        assert_eq!(
            panes.left.visible(settled),
            1.,
            "the takeover ends at once too"
        );
    }
}
