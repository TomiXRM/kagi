//! Code Ecosystem view rendering (ADR-0119), split out of `ecosystem/mod.rs`
//! to keep the entity logic and the element tree under the LOC budget.
//!
//! All GPUI-native: a header (mode + granularity toggles, Copy diagnostic,
//! close) over a risk-ranked Hotspots list. The Coupling / Ownership modes and
//! the circle-pack / heatmap paint are stubs pending later tickets.

use super::*;
use gpui::{relative, AnyElement};
use kagi_domain::hotspot::{Ecosystem, FileMetric};

/// Cap on rendered rows — the power-law means the top slice carries the signal,
/// and it bounds the element count without virtualization.
const MAX_ROWS: usize = 300;

/// Root element of the full-screen ecosystem view.
pub(super) fn render_ecosystem(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(theme().bg_base))
        .child(render_header(view, cx))
        .child(render_body(view, cx))
        .into_any_element()
}

/// Top toolbar: title, mode toggle, granularity toggle, Copy diagnostic, close.
fn render_header(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let copy_enabled = view.data.ecosystem.is_some();
    let copy_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.copy_diagnostic(cx));
    let close_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.request_close(cx));

    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .text_size(theme::scaled_px(14.0))
                .text_color(rgb(theme().text_main))
                .child(Msg::Ecosystem.t()),
        )
        .child(render_mode_toggle(view, cx))
        .child(div().flex_1())
        .child(render_granularity_toggle(view, cx))
        .child(
            text_button("eco-copy", Msg::EcoCopyDiagnostic.t(), copy_enabled).on_click(copy_click),
        )
        .child(text_button("eco-close", "✕", true).on_click(close_click))
        .into_any_element()
}

/// Segmented mode switch (Hotspots / Coupling / Ownership).
fn render_mode_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let active = view.data.mode;
    let mut row = div().flex().items_center().gap_1();
    for mode in EcosystemMode::ALL {
        let click = cx.listener(move |v, _: &gpui::ClickEvent, _w, cx| v.set_mode(mode, cx));
        row = row.child(
            chip(
                mode.label(),
                mode == active,
                format!("eco-mode-{}", mode.label()),
            )
            .on_click(click),
        );
    }
    row.into_any_element()
}

/// Granularity (window) switch, mirroring the Activity tab.
fn render_granularity_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let active = view.data.granularity;
    let mut row = div().flex().items_center().gap_1();
    for g in Granularity::ALL {
        let click = cx.listener(move |v, _: &gpui::ClickEvent, _w, cx| v.set_granularity(g, cx));
        row = row
            .child(chip(g.label(), g == active, format!("eco-gran-{}", g.label())).on_click(click));
    }
    row.into_any_element()
}

/// Body: loading / error / the mode panel.
fn render_body(view: &EcosystemView, _cx: &mut Context<EcosystemView>) -> AnyElement {
    let inner = if view.data.loading {
        loading_view()
    } else if let Some(err) = &view.data.error {
        centered(&format!("{}: {}", Msg::EcoLoadFailed.t(), err))
    } else {
        match view.data.mode {
            EcosystemMode::Hotspots => match &view.data.ecosystem {
                Some(eco) if !eco.files.is_empty() => render_hotspot_list(eco),
                _ => centered(Msg::EcoEmpty.t()),
            },
            _ => centered(Msg::EcoComingSoon.t()),
        }
    };
    div()
        .flex_1()
        .min_h(px(0.0))
        .child(inner)
        .into_any_element()
}

/// The risk-ranked Hotspots list (top [`MAX_ROWS`]).
fn render_hotspot_list(eco: &Ecosystem) -> AnyElement {
    let max_risk = eco
        .files
        .first()
        .map(|f| f.risk)
        .unwrap_or(1.0)
        .max(f64::MIN_POSITIVE);
    let mut list = div()
        .id("eco-list")
        .flex()
        .flex_col()
        .size_full()
        .overflow_y_scroll();
    for (i, f) in eco.files.iter().take(MAX_ROWS).enumerate() {
        list = list.child(render_row(i + 1, f, max_risk));
    }
    list.into_any_element()
}

/// One ranked file row: `#rank  path  commits  LOC  risk-bar`.
fn render_row(rank: usize, f: &FileMetric, max_risk: f64) -> AnyElement {
    let frac = (f.risk / max_risk).clamp(0.0, 1.0) as f32;
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .w(theme::scaled_px(36.0))
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().text_muted))
                .child(format!("#{rank}")),
        )
        .child(
            div()
                .flex_1()
                .text_size(theme::scaled_px(13.0))
                .text_color(rgb(theme().text_main))
                .child(f.path.clone()),
        )
        .child(stat(&format!("{} commits", f.commits)))
        .child(stat(&format!("{} LOC", f.loc)))
        .child(
            div()
                .w(theme::scaled_px(120.0))
                .h(theme::scaled_px(6.0))
                .bg(rgb(theme().surface))
                .child(
                    div()
                        .h_full()
                        .w(relative(frac))
                        .bg(rgb(theme().color_warning)),
                ),
        )
        .into_any_element()
}

// ── small shared element helpers ────────────────────────────────

/// A toggle chip; highlighted when `active`.
fn chip(label: &str, active: bool, id: String) -> gpui::Stateful<gpui::Div> {
    let (bg, fg) = if active {
        (theme().accent, theme().bg_base)
    } else {
        (theme().surface, theme().text_sub)
    };
    div()
        .id(SharedString::from(id))
        .px_2()
        .py_1()
        .rounded(theme::scaled_px(4.0))
        .bg(rgb(bg))
        .text_size(theme::scaled_px(12.0))
        .text_color(rgb(fg))
        .cursor_pointer()
        .child(label.to_string())
}

/// A header text button (Copy diagnostic / close).
fn text_button(id: &'static str, label: &str, enabled: bool) -> gpui::Stateful<gpui::Div> {
    let fg = if enabled {
        theme().text_main
    } else {
        theme().text_muted
    };
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(theme::scaled_px(4.0))
        .bg(rgb(theme().surface))
        .text_size(theme::scaled_px(12.0))
        .text_color(rgb(fg))
        .cursor_pointer()
        .child(label.to_string())
}

/// A muted right-aligned stat cell.
fn stat(text: &str) -> gpui::Div {
    div()
        .text_size(theme::scaled_px(12.0))
        .text_color(rgb(theme().text_sub))
        .child(text.to_string())
}

/// Loading state: a Claude-style spinning loader + an indeterminate progress
/// bar (the mine reports no increments) + a "large repos take a while" hint.
fn loading_view() -> AnyElement {
    use gpui::AnimationExt as _;
    let spinner = gpui::svg()
        .path("icons/loader-circle.svg")
        .w(theme::scaled_px(30.0))
        .h(theme::scaled_px(30.0))
        .text_color(rgb(theme().accent))
        .with_animation(
            "eco-spinner",
            gpui::Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |svg, delta| {
                svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        );

    // Indeterminate bar: a 30%-wide segment sweeping across the track.
    let bar = div()
        .w(theme::scaled_px(220.0))
        .h(theme::scaled_px(4.0))
        .rounded(theme::scaled_px(2.0))
        .bg(rgb(theme().surface))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(0.3))
                .bg(rgb(theme().accent))
                .with_animation(
                    "eco-bar",
                    gpui::Animation::new(std::time::Duration::from_millis(1300)).repeat(),
                    |el, delta| el.ml(relative(0.7 * delta)),
                ),
        );

    div()
        .flex()
        .flex_col()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(spinner)
        .child(
            div()
                .text_size(theme::scaled_px(14.0))
                .text_color(rgb(theme().text_main))
                .child(Msg::EcoLoading.t()),
        )
        .child(bar)
        .child(
            div()
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().text_muted))
                .child(Msg::EcoLoadingHint.t()),
        )
        .into_any_element()
}

/// A centered single-line message filling the body.
fn centered(text: &str) -> AnyElement {
    div()
        .flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_size(theme::scaled_px(13.0))
        .text_color(rgb(theme().text_muted))
        .child(text.to_string())
        .into_any_element()
}
