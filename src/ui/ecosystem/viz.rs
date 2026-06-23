//! Ecosystem "Map" heatmap rendering (ADR-0119, T-ECO-VIZ-004).
//!
//! A treemap where each file is a tile **sized by LOC** (complexity) and
//! **coloured by risk** (green → amber → red) — the CodeScene hot-spot map,
//! GPUI-native. Tiles are plain `div`s positioned with `relative()` lengths
//! from the pure [`kagi_domain::hotspot_layout`] unit-square layout, so labels
//! and (future) hit-testing stay as ordinary elements (no canvas).

use super::*;
use gpui::{relative, AnyElement};
use kagi_domain::hotspot::Ecosystem;
use kagi_domain::hotspot_layout::{treemap, Rect};

/// Cap on tiles drawn — keeps the element count bounded; the power-law means
/// the largest/riskiest files carry the signal anyway.
const MAP_TILES: usize = 150;

/// Render the Hotspots treemap heatmap (size = LOC, colour = risk).
pub(super) fn render_hotspot_map(eco: &Ecosystem) -> AnyElement {
    let files: Vec<_> = eco.files.iter().take(MAP_TILES).collect();
    // Size by LOC; fall back to churn so zero-LOC files still get a slice.
    let weights: Vec<f64> = files
        .iter()
        .map(|f| (f.loc as f64).max(f.commits as f64).max(1.0))
        .collect();
    let rects = treemap(&weights);

    let mut canvas = div()
        .id("eco-map")
        .relative()
        .size_full()
        .overflow_hidden()
        .bg(rgb(theme().bg_base));
    for (f, r) in files.iter().zip(rects.iter()) {
        if r.area() <= 0.0 {
            continue;
        }
        canvas = canvas.child(tile(f.path.as_str(), f.risk, r));
    }
    canvas.into_any_element()
}

/// One treemap tile: a heat-coloured rectangle, labelled with the file's base
/// name when it is large enough to fit text.
fn tile(path: &str, risk: f64, r: &Rect) -> AnyElement {
    let mut el = div()
        .absolute()
        .left(relative(r.x as f32))
        .top(relative(r.y as f32))
        .w(relative(r.w as f32))
        .h(relative(r.h as f32))
        .bg(rgb(heat(risk)))
        .border_1()
        .border_color(rgb(theme().bg_base))
        .overflow_hidden();

    // Only label tiles with room for it (avoid a wall of clipped text).
    if r.w > 0.07 && r.h > 0.045 {
        let name = path.rsplit('/').next().unwrap_or(path);
        el = el.child(
            div()
                .px_1()
                .text_size(theme::scaled_px(11.0))
                .text_color(rgb(LABEL))
                .child(name.to_string()),
        );
    }
    el.into_any_element()
}

/// Near-black label colour that reads on the green/amber/red heat range.
const LABEL: u32 = 0x10_10_10;

/// Risk → heat colour: success (low) → warning (mid) → blocker (high).
fn heat(risk: f64) -> u32 {
    let t = (risk as f32).clamp(0.0, 1.0);
    if t < 0.5 {
        lerp_rgb(theme().color_success, theme().color_warning, t * 2.0)
    } else {
        lerp_rgb(
            theme().color_warning,
            theme().color_blocker,
            (t - 0.5) * 2.0,
        )
    }
}

/// Linear interpolation between two `0xRRGGBB` colours.
fn lerp_rgb(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let chan = |sh: u32| -> u32 {
        let av = ((a >> sh) & 0xff) as f32;
        let bv = ((b >> sh) & 0xff) as f32;
        ((av + (bv - av) * t).round() as u32) & 0xff
    };
    (chan(16) << 16) | (chan(8) << 8) | chan(0)
}
