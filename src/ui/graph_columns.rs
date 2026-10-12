//! Viewport fitting for Graph's reference-space divider preferences (#1317).

use super::{theme, BADGE_COL_MIN, GRAPH_COL_MIN, INNER_DIV_W};

pub(super) const SUBJECT_MIN: f32 = 120.;
pub(super) const AUTHOR_MIN: f32 = 40.;
pub(super) const AUTHOR_PREFERRED: f32 = 96.;
pub(super) const TIME_WIDTH: f32 = 48.;

/// Keep preferred widths when there is room; compress lanes before ref labels.
/// This is presentation only: never replace or persist the user's preferences.
pub(super) fn fit(badge: f32, graph: f32, center_width: f32, row_padding: f32) -> (f32, f32) {
    let scale = theme::scaled(1.);
    // Reserve the separate 18px avatar + 4px margin even when the avatar is
    // currently a graph node. The provenance glyph is text_sm (14px) with
    // unscaled 3px padding and 1px borders on each side, like its renderer.
    let message = SUBJECT_MIN + AUTHOR_MIN + TIME_WIDTH + 22. + 14. + 8. / scale;
    let budget = ((center_width - row_padding) / scale - message - 2. * INNER_DIV_W)
        .max(BADGE_COL_MIN + GRAPH_COL_MIN);
    let graph = graph.min((budget - badge).max(GRAPH_COL_MIN));
    let badge = badge.min((budget - graph).max(BADGE_COL_MIN));
    (badge, graph)
}
