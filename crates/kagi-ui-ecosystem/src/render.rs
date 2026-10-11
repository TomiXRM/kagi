//! Code Ecosystem view rendering (ADR-0119), split out of `ecosystem/mod.rs`
//! to keep the entity logic and the element tree under the LOC budget.
//!
//! Native Analyze selectors and actions, with mode-specific evidence views.

use super::*;
use gpui::{relative, AnyElement};
use gpui_component::tooltip::Tooltip;

/// Root element of the full-screen ecosystem view.
pub(super) fn render_ecosystem(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> AnyElement {
    div()
        .relative()
        .track_focus(
            view.navigation
                .root
                .as_ref()
                .expect("pane focus initialized"),
        )
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(theme().bg_base))
        .child(render_header(view, cx))
        .child(render_body(view, cx))
        .into_any_element()
}

/// Top toolbar: title, mode toggle, granularity toggle, Copy diagnostic, close.
///
/// The controls are grouped into a left cluster (title + mode + sub-view) and a
/// right cluster (window + format + copy/help/close), separated by a flexible
/// spacer. The row is allowed to **wrap**: on a narrow / zoomed-in window the
/// right cluster drops to a second line as a unit instead of being clipped off
/// the right edge (which hid the Copy / ✕ buttons before).
fn render_header(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    // Left cluster: title + mode toggle + the mode-specific sub-view toggle.
    let left = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .child(
            div()
                .text_size(theme::scaled_px(14.0))
                .text_color(rgb(theme().text_main))
                .child(Msg::Ecosystem.t()),
        )
        .child(render_mode_toggle(view, cx))
        .when(view.data.mode == EcosystemMode::Hotspots, |d| {
            d.child(render_view_toggle(view, cx))
        })
        .when(view.data.mode == EcosystemMode::Coupling, |d| {
            d.child(render_coupling_toggle(view, cx))
        });

    // Right cluster: window + export format + actions. `flex_wrap` lets the
    // controls themselves spill onto a second line on a narrow / zoomed-in
    // window instead of being clipped off the right edge (which hid the
    // format chips, Copy, ? and ✕ — user report). `justify_end` keeps them
    // right-aligned when they do.
    let right = div()
        .flex()
        .flex_wrap()
        .justify_end()
        .items_center()
        .gap_2()
        .child(render_granularity_toggle(view, cx))
        .child(render_format_toggle(view, cx))
        .children(render_actions(view, cx));

    // `justify_between` (instead of a flexible spacer) splits the two clusters
    // to the edges on a wide window but, combined with `flex_wrap`, lets the
    // right cluster drop to its own line when they no longer fit — a `flex_1`
    // spacer absorbed the slack and suppressed that wrap, so the right cluster
    // overflowed the edge instead.
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(left)
        .child(right)
        .into_any_element()
}

fn render_actions(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> [gpui::Stateful<gpui::Div>; 3] {
    let enabled = view.data.ecosystem.is_some() && view.data.mode != EcosystemMode::Health;
    let copy = controls::button(
        "eco-copy",
        Msg::EcoCopyDiagnostic.t(),
        Msg::EcoCopyDiagnostic.t(),
        enabled,
        view.navigation.handle("eco-copy", cx),
    )
    .on_click(cx.listener(move |v, _, _, cx| {
        if enabled {
            v.copy_diagnostic(cx);
        }
    }));
    let opener = view.navigation.handle("eco-help", cx);
    let help = controls::button("eco-help", "?", Msg::EcoHelpTitle.t(), true, opener.clone())
        .on_click(cx.listener(move |v, _, _, cx| v.request_help(opener.clone(), cx)));
    let close = controls::button(
        "eco-close",
        "✕",
        Msg::RemoteClose.t(),
        true,
        view.navigation.handle("eco-close", cx),
    )
    .on_click(cx.listener(|v, _, _, cx| v.request_close(cx)));
    [copy, help, close]
}

/// Selected, roving keyboard segments; all reuse the original view setters.
fn render_mode_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let choices: Vec<_> = EcosystemMode::ALL
        .iter()
        .map(|&mode| (mode, mode.label(), format!("eco-mode-{}", mode.label())))
        .collect();
    controls::tabs(
        "eco-modes",
        &view.navigation.mode,
        &choices,
        view.data.mode,
        view.navigation.root.clone(),
        EcosystemView::set_mode,
        cx,
    )
}

fn render_view_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    controls::tabs(
        "eco-views",
        &view.navigation.view,
        &[
            (false, Msg::EcoList.t(), "eco-view-list".into()),
            (true, Msg::EcoMap.t(), "eco-view-map".into()),
        ],
        view.data.map,
        view.navigation.root.clone(),
        EcosystemView::set_map,
        cx,
    )
}

fn render_coupling_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let choices = [
        (CouplingView::List, Msg::EcoList.t(), "eco-coup-list".into()),
        (
            CouplingView::Graph,
            Msg::EcoGraph.t(),
            "eco-coup-graph".into(),
        ),
        (
            CouplingView::Mermaid,
            Msg::EcoMermaid.t(),
            "eco-coup-mermaid".into(),
        ),
    ];
    controls::tabs(
        "eco-coupling-views",
        &view.navigation.coupling_view,
        &choices,
        view.data.coupling_view,
        view.navigation.root.clone(),
        EcosystemView::set_coupling_view,
        cx,
    )
}

fn render_format_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let mut choices = vec![
        (ExportFormat::Markdown, "MD", "eco-fmt-MD".into()),
        (ExportFormat::Json, "JSON", "eco-fmt-JSON".into()),
    ];
    if view.data.mode == EcosystemMode::Coupling {
        choices.push((ExportFormat::Mermaid, "Mermaid", "eco-fmt-Mermaid".into()));
    }
    controls::tabs(
        "eco-formats",
        &view.navigation.format,
        &choices,
        view.data.export_format,
        view.navigation.root.clone(),
        EcosystemView::set_export_format,
        cx,
    )
}

fn render_granularity_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let choices: Vec<_> = Granularity::ALL
        .iter()
        .map(|&g| (g, g.label(), format!("eco-gran-{}", g.label())))
        .collect();
    controls::tabs(
        "eco-periods",
        &view.navigation.period,
        &choices,
        view.data.granularity,
        view.navigation.root.clone(),
        EcosystemView::set_granularity,
        cx,
    )
}

/// Body: loading / error / the mode panel.
fn render_body(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    // Health has its own read, independent of the history mine's loading state.
    let inner = if view.data.mode == EcosystemMode::Health {
        super::health::render_health(view, cx)
    } else if view.data.loading {
        loading_view()
    } else if let Some(err) = &view.data.error {
        centered(&format!("{}: {}", Msg::EcoLoadFailed.t(), err))
    } else {
        match view.data.mode {
            EcosystemMode::Hotspots => match &view.data.ecosystem {
                Some(eco) if !eco.files.is_empty() => {
                    if view.data.map {
                        super::viz::render_hotspot_map(eco)
                    } else {
                        super::lists::render_hotspot_list(eco, view, cx)
                    }
                }
                _ => centered(Msg::EcoEmpty.t()),
            },
            EcosystemMode::Coupling => {
                if view.data.couplings.is_empty() {
                    centered(Msg::EcoEmpty.t())
                } else {
                    match view.data.coupling_view {
                        CouplingView::Graph if view.data.coupling_graph.is_some() => {
                            super::graph::render_coupling_graph(view, cx)
                        }
                        CouplingView::Mermaid => {
                            super::lists::render_coupling_mermaid_view(view, cx)
                        }
                        _ => super::lists::render_coupling_list(view, cx),
                    }
                }
            }
            EcosystemMode::Ownership => {
                if view.data.ownership.is_empty() {
                    centered(Msg::EcoEmpty.t())
                } else {
                    super::lists::render_ownership_list(&view.data.ownership)
                }
            }
            EcosystemMode::Health => super::health::render_health(view, cx),
        }
    };
    div()
        .flex_1()
        .min_h(px(0.0))
        .child(inner)
        .into_any_element()
}

/// The left "name" cell of a list row. The cell takes the flexible space, but a
/// `flex_1` child's flex **min-width defaults to its content width** — so a very
/// long path (e.g. deep STM32 build artifacts, 200+ chars) refuses to shrink and
/// shoves the fixed numeric columns + risk bar off the right edge where they
/// vanish (user report). The fix is the same one the commit-list centre column
/// uses (`render_body.rs`): the flex participant carries `min_w(0)` +
/// `overflow_hidden`, which lets it shrink below the path's intrinsic width and
/// keeps every right-hand column on-screen. A nested `overflow_x_scroll` child
/// preserves the earlier "scroll the long path inside its cell" behaviour.
pub(super) fn scroll_path_cell(id: String, text: String) -> gpui::Stateful<gpui::Div> {
    // `flex_1` + `min_w(0)` + `overflow_x_scroll` is the proven shrink pattern
    // (cf. the commit-list centre column); it lets the cell shrink below the
    // path's intrinsic width so every right-hand column stays on-screen. The
    // path scrolls horizontally *inside* this clipped cell, and a hover tooltip
    // shows the full path so a clipped name is always readable (user request:
    // "hidden text is unusable").
    let full = SharedString::from(text.clone());
    div()
        .id(SharedString::from(id))
        .flex_1()
        .min_w(px(0.0))
        .overflow_x_scroll()
        .whitespace_nowrap()
        .text_size(theme::scaled_px(13.0))
        .text_color(rgb(theme().text_main))
        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
        .child(text)
}

/// Secondary text action used by the Mermaid source panel.
pub(super) fn text_button(
    id: &'static str,
    label: &str,
    enabled: bool,
) -> gpui_component::button::Button {
    use gpui_component::{button::ButtonVariants as _, Disableable as _, Sizable as _};
    gpui_component::button::Button::new(id)
        .small()
        .ghost()
        .label(label.to_string())
        .disabled(!enabled)
}

/// A muted, fixed (non-shrinking) stat cell — stays visible at the right edge.
pub(super) fn stat(text: &str) -> gpui::Div {
    div()
        .flex_shrink_0()
        .text_size(theme::scaled_px(12.0))
        .text_color(rgb(theme().text_sub))
        .child(text.to_string())
}

/// The "How to read Analyze" section blocks (heading + body), shared by the
/// help overlay and the loading screen.
fn help_section_list() -> gpui::Div {
    let mut col = div().flex().flex_col().gap_3();
    for (heading, body) in kagi_ui_core::i18n::eco_help_sections() {
        col = col.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(theme::scaled_px(13.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(theme().accent))
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(theme::scaled_px(12.5))
                        .text_color(rgb(theme().text_sub))
                        .child(body),
                ),
        );
    }
    col
}

/// Loading state: a Claude-style spinning loader + an indeterminate progress
/// bar (the mine reports no increments) + a hint, and — while the user waits —
/// the "How to read Analyze" explainer so the wait doubles as onboarding.
fn loading_view() -> AnyElement {
    // The animated block stays OUTSIDE any scroll container — `with_animation`
    // doesn't tick inside an `overflow_y_scroll` element. Only the help scrolls.
    div()
        .flex()
        .flex_col()
        .size_full()
        .items_center()
        .child(loading_progress())
        // While the mine runs, show the explainer (the wait = onboarding).
        .child(
            div()
                .id("eco-loading-help")
                .flex_1()
                .min_h(px(0.0))
                .w(theme::scaled_px(620.0))
                .overflow_y_scroll()
                .pt_2()
                .pb_4()
                .child(
                    div()
                        .pb_2()
                        .text_size(theme::scaled_px(14.0))
                        .text_color(rgb(theme().text_main))
                        .child(Msg::EcoHelpTitle.t()),
                )
                .child(help_section_list()),
        )
        .into_any_element()
}

/// The animated progress cluster (spinner + label + indeterminate bar + hint)
/// shown above the explainer while the repo is being mined.
fn loading_progress() -> gpui::Div {
    use gpui::AnimationExt as _;
    // Reduce motion: render spinner + bar static — skip `.with_animation`
    // entirely so no per-frame tick is requested (the pinned GPUI build does
    // not honor reduce-motion itself). Mirrors `render_body::render_loading_placeholder`.
    let reduce_motion = theme::reduce_motion();
    let spinner_base = gpui::svg()
        .path("icons/loader-circle.svg")
        .w(theme::scaled_px(30.0))
        .h(theme::scaled_px(30.0))
        .text_color(rgb(theme().accent));
    let spinner: gpui::AnyElement = if reduce_motion {
        spinner_base.into_any_element()
    } else {
        spinner_base
            .with_animation(
                "eco-spinner",
                gpui::Animation::new(std::time::Duration::from_millis(900)).repeat(),
                |svg, delta| {
                    svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                        delta * std::f32::consts::TAU,
                    )))
                },
            )
            .into_any_element()
    };

    // Indeterminate bar: a 30%-wide segment sweeping across the track.
    let bar_seg = div().h_full().w(relative(0.3)).bg(rgb(theme().accent));
    let bar_seg: gpui::AnyElement = if reduce_motion {
        bar_seg.into_any_element()
    } else {
        bar_seg
            .with_animation(
                "eco-bar",
                gpui::Animation::new(std::time::Duration::from_millis(1300)).repeat(),
                |el, delta| el.ml(relative(0.7 * delta)),
            )
            .into_any_element()
    };
    let bar = div()
        .w(theme::scaled_px(220.0))
        .h(theme::scaled_px(4.0))
        .rounded(theme::scaled_px(2.0))
        .bg(rgb(theme().surface))
        .overflow_hidden()
        .child(bar_seg);

    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .pt_8()
        .pb_4()
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
}

/// A centered single-line message filling the body.
pub(super) fn centered(text: &str) -> AnyElement {
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
