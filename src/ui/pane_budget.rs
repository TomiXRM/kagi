//! Presentation-only outer pane fitting, parallel to Graph's columns (#1355).
use super::{theme, SIDEBAR_MIN};

pub(super) const CENTER_MIN: f32 = 320.;
// Smaller than the 240px drag-preference floor: existing minima cannot fit
// 940px at 167%. This floor never replaces the preferred Inspector width.
const INSPECTOR_MIN: f32 = 160.;
const DIVIDER: f32 = 4.;

#[derive(Clone, Copy, Default)]
pub(super) struct Fit {
    pub sidebar: f32,
    pub right: f32,
    budget: Option<f32>,
    sidebar_visible: bool,
    right_visible: bool,
}

impl Fit {
    pub fn refit(self, sidebar: f32, right: f32) -> Self {
        let Some(budget) = self.budget else {
            return Self {
                sidebar,
                right,
                ..self
            };
        };
        let left = if self.sidebar_visible { sidebar } else { 0. };
        let right_used = if self.right_visible { right } else { 0. };
        // Inspector gives up room first, then navigation. Below both floors,
        // the centre uses the remaining room rather than forcing overflow.
        let right_used = right_used.min((budget - left).max(INSPECTOR_MIN));
        let left = left.min((budget - right_used).max(SIDEBAR_MIN));
        Self {
            sidebar: if self.sidebar_visible { left } else { sidebar },
            right: if self.right_visible {
                right_used
            } else {
                right
            },
            ..self
        }
    }
}

pub(super) fn fit(
    sidebar: f32,
    right: f32,
    window_width: f32,
    sidebar_visible: bool,
    right_visible: bool,
    protect_center: bool,
) -> Fit {
    let dividers = (u8::from(sidebar_visible) + u8::from(right_visible)) as f32 * DIVIDER;
    Fit {
        budget: protect_center.then(|| window_width / theme::scaled(1.) - dividers - CENTER_MIN),
        sidebar_visible,
        right_visible,
        ..Default::default()
    }
    .refit(sidebar, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(width: f32, left: bool, right: bool) -> Fit {
        fit(240., 360., theme::scaled(width), left, right, true)
    }

    #[test]
    fn pane_budget_compresses_inspector_before_navigation() {
        let wide = at(1000., true, true);
        assert_eq!((wide.sidebar, wide.right), (240., 360.));
        let inspector_only = at(800., true, true);
        assert_eq!(inspector_only.sidebar, 240.);
        assert!((inspector_only.right - 232.).abs() < 0.001);
        let both = at(625., true, true);
        assert!((both.sidebar - 137.).abs() < 0.001);
        assert_eq!(both.right, INSPECTOR_MIN);
        let tiny = at(400., true, true);
        assert_eq!((tiny.sidebar, tiny.right), (SIDEBAR_MIN, INSPECTOR_MIN));
    }

    #[test]
    fn pane_budget_hidden_panes_do_not_consume_room() {
        let left_only = at(600., true, false);
        assert_eq!((left_only.sidebar, left_only.right), (240., 360.));
        let right_only = at(600., false, true);
        assert_eq!(right_only.sidebar, 240.);
        assert!((right_only.right - 276.).abs() < 0.001);
        let neither = at(100., false, false);
        assert_eq!((neither.sidebar, neither.right), (240., 360.));
    }

    #[test]
    fn pane_budget_refits_capped_drags_without_replacing_preferences() {
        let compressed = at(625., true, true);
        let expansion = compressed.refit(170., 240.);
        assert!((expansion.sidebar - compressed.sidebar).abs() < 0.001);
        assert_eq!(expansion.right, compressed.right);
        let shrink = compressed.refit(120., 360.);
        assert_eq!(shrink.sidebar, 120.);
        assert!((shrink.right - 177.).abs() < 0.001);
        let commit_panel = fit(240., 100., 500., true, true, false);
        assert_eq!((commit_panel.sidebar, commit_panel.right), (240., 100.));
        assert_eq!(commit_panel.refit(270., 250.).right, 250.);
    }
}
