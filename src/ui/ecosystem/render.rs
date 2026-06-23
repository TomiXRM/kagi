//! Code Ecosystem view rendering (ADR-0119), split out of `ecosystem/mod.rs`
//! to keep the entity logic and the element tree under the LOC budget.
//!
//! All GPUI-native: a header (mode + granularity toggles, Copy diagnostic,
//! close) over a risk-ranked Hotspots list. The Coupling / Ownership modes and
//! the circle-pack / heatmap paint are stubs pending later tickets.

use super::*;
use gpui::{relative, AnyElement};
use kagi_domain::hotspot::{CouplingEdge, CouplingPair, Ecosystem, FileMetric, FileOwnership};

/// Cap on rendered rows — the power-law means the top slice carries the signal,
/// and it bounds the element count without virtualization.
const MAX_ROWS: usize = 300;

/// Root element of the full-screen ecosystem view.
pub(super) fn render_ecosystem(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> AnyElement {
    div()
        .relative()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(theme().bg_base))
        .child(render_header(view, cx))
        .child(render_body(view, cx))
        .when(view.data.help_open, |d| d.child(render_help(cx)))
        .into_any_element()
}

/// "How to read Analyze" help overlay — a centered, scrollable card over a dim
/// backdrop (click the backdrop or ✕ to close).
fn render_help(cx: &mut Context<EcosystemView>) -> AnyElement {
    let close = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.toggle_help(cx));
    let backdrop_close = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.toggle_help(cx));

    let card = div()
        .id("eco-help-card")
        // Occlude so clicks inside the card don't fall through to the backdrop
        // (which would toggle help a second time and cancel the ✕ close).
        .occlude()
        .flex()
        .flex_col()
        .gap_3()
        .w(theme::scaled_px(580.0))
        .max_h(relative(0.82))
        .overflow_y_scroll()
        .p_4()
        .bg(rgb(theme().modal))
        .border_1()
        .border_color(rgb(theme().surface))
        .rounded(theme::scaled_px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(theme::scaled_px(16.0))
                        .text_color(rgb(theme().text_main))
                        .child(Msg::EcoHelpTitle.t()),
                )
                .child(text_button("eco-help-close", "✕", true).on_click(close)),
        )
        .child(help_section_list());

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .id("eco-help-backdrop")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.7)
                .on_click(backdrop_close),
        )
        .child(card)
        .into_any_element()
}

/// Top toolbar: title, mode toggle, granularity toggle, Copy diagnostic, close.
fn render_header(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let copy_enabled = view.data.ecosystem.is_some();
    let copy_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.copy_diagnostic(cx));
    let help_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.toggle_help(cx));
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
        .when(view.data.mode == EcosystemMode::Hotspots, |d| {
            d.child(render_view_toggle(view, cx))
        })
        .when(view.data.mode == EcosystemMode::Coupling, |d| {
            d.child(render_coupling_toggle(view, cx))
        })
        .child(div().flex_1())
        .child(render_granularity_toggle(view, cx))
        .child(render_format_toggle(view, cx))
        .child(
            text_button("eco-copy", Msg::EcoCopyDiagnostic.t(), copy_enabled).on_click(copy_click),
        )
        .child(text_button("eco-help", "?", true).on_click(help_click))
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

/// List ⇄ Map sub-view switch, shown only in Hotspots mode.
fn render_view_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let map = view.data.map;
    let list_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.set_map(false, cx));
    let map_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.set_map(true, cx));
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(vsep())
        .child(chip(Msg::EcoList.t(), !map, "eco-view-list".into()).on_click(list_click))
        .child(chip(Msg::EcoMap.t(), map, "eco-view-map".into()).on_click(map_click))
        .into_any_element()
}

/// List ⇄ Graph sub-view switch, shown only in Coupling mode.
fn render_coupling_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let graph = view.data.coupling_graph_on;
    let list_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.set_coupling_graph(false, cx));
    let graph_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.set_coupling_graph(true, cx));
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(vsep())
        .child(chip(Msg::EcoList.t(), !graph, "eco-coup-list".into()).on_click(list_click))
        .child(chip(Msg::EcoGraph.t(), graph, "eco-coup-graph".into()).on_click(graph_click))
        .into_any_element()
}

/// "Copy diagnostic" output-format switch (MD / JSON, plus Mermaid in Coupling
/// mode where the 1:many co-change structure is best read as a graph).
fn render_format_toggle(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let active = view.data.export_format;
    let mut formats = vec![ExportFormat::Markdown, ExportFormat::Json];
    if view.data.mode == EcosystemMode::Coupling {
        formats.push(ExportFormat::Mermaid);
    }
    let mut row = div().flex().items_center().gap_1().child(vsep());
    for fmt in formats {
        let click =
            cx.listener(move |v, _: &gpui::ClickEvent, _w, cx| v.set_export_format(fmt, cx));
        row = row.child(
            chip(
                fmt.label(),
                fmt == active,
                format!("eco-fmt-{}", fmt.label()),
            )
            .on_click(click),
        );
    }
    row.into_any_element()
}

/// A thin vertical divider separating the mode toggle from the sub-view toggle.
fn vsep() -> gpui::Div {
    div()
        .w(px(1.0))
        .h(theme::scaled_px(16.0))
        .mx_1()
        .bg(rgb(theme().text_muted))
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
fn render_body(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let inner = if view.data.loading {
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
                        render_hotspot_list(eco)
                    }
                }
                _ => centered(Msg::EcoEmpty.t()),
            },
            EcosystemMode::Coupling => {
                if view.data.couplings.is_empty() {
                    centered(Msg::EcoEmpty.t())
                } else if view.data.coupling_graph_on && view.data.coupling_graph.is_some() {
                    super::graph::render_coupling_graph(view, cx)
                } else {
                    render_coupling_list(view, cx)
                }
            }
            EcosystemMode::Ownership => {
                if view.data.ownership.is_empty() {
                    centered(Msg::EcoEmpty.t())
                } else {
                    render_ownership_list(&view.data.ownership)
                }
            }
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
        .child(scroll_path_cell(
            format!("eco-hot-path-{rank}"),
            f.path.clone(),
        ))
        .child(stat(&format!("{} commits", f.commits)))
        .child(stat(&format!("{} LOC", f.loc)))
        .child(
            div()
                .flex_shrink_0()
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

/// The change-coupling list: file pairs that change together, top first. A row
/// click expands the left file's full 1:many partner set beneath it.
fn render_coupling_list(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    let pairs = &view.data.couplings;
    let max_together = pairs.first().map(|p| p.together).unwrap_or(1).max(1);
    let focus = view.data.coupling_focus;
    let mut list = div()
        .id("eco-coupling-list")
        .flex()
        .flex_col()
        .size_full()
        .overflow_y_scroll();
    for (i, p) in pairs.iter().enumerate() {
        let a = p.a.clone();
        let click =
            cx.listener(move |v, _: &gpui::ClickEvent, _w, cx| v.toggle_coupling(i, a.clone(), cx));
        list = list
            .child(render_coupling_row(i + 1, p, max_together, focus == Some(i)).on_click(click));
        if focus == Some(i) {
            list = list.child(render_partners(&p.a, &view.data.coupling_partners));
        }
    }
    list.into_any_element()
}

/// One coupling row: `#rank  a ⇄ b   N together   degree-bar  degree%`.
fn render_coupling_row(
    rank: usize,
    p: &CouplingPair,
    max_together: u32,
    expanded: bool,
) -> gpui::Stateful<gpui::Div> {
    let frac = (p.together as f32 / max_together as f32).clamp(0.0, 1.0);
    div()
        .id(SharedString::from(format!("eco-coup-row-{rank}")))
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .cursor_pointer()
        .when(expanded, |d| d.bg(rgb(theme().selected)))
        .child(
            div()
                .flex_shrink_0()
                .w(theme::scaled_px(36.0))
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().text_muted))
                .child(format!("#{rank}")),
        )
        .child(scroll_path_cell(
            format!("eco-coup-path-{rank}"),
            format!("{}  ⇄  {}", p.a, p.b),
        ))
        .child(stat(&format!("{}×", p.together)))
        .child(
            div()
                .flex_shrink_0()
                .w(theme::scaled_px(120.0))
                .h(theme::scaled_px(6.0))
                .bg(rgb(theme().surface))
                .child(
                    div()
                        .h_full()
                        .w(relative(frac))
                        .bg(rgb(theme().color_branch)),
                ),
        )
        .child(stat(&format!("{:.0}%", p.degree * 100.0)))
}

/// The expanded 1:many panel: `focus`'s co-change partners, indented under the
/// clicked row (`→ partner   N×   P(partner|focus)%`).
fn render_partners(focus: &str, partners: &[CouplingEdge]) -> AnyElement {
    let max_together = partners.first().map(|e| e.together).unwrap_or(1).max(1);
    let mut panel = div()
        .flex()
        .flex_col()
        .bg(rgb(theme().bg_row_alt))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .px_3()
                .py_1()
                .pl_8()
                .text_size(theme::scaled_px(11.0))
                .text_color(rgb(theme().text_muted))
                .child(format!("{} {}", Msg::EcoCouplesWith.t(), focus)),
        );
    for (j, e) in partners.iter().enumerate() {
        panel = panel.child(render_partner_row(j, e, max_together));
    }
    panel.into_any_element()
}

/// One partner row inside the 1:many expansion.
fn render_partner_row(idx: usize, e: &CouplingEdge, max_together: u32) -> AnyElement {
    let frac = (e.together as f32 / max_together as f32).clamp(0.0, 1.0);
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_1()
        .pl_8()
        .child(
            div()
                .flex_shrink_0()
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().text_muted))
                .child("↳"),
        )
        .child(scroll_path_cell(
            format!("eco-coup-partner-{idx}"),
            e.partner.clone(),
        ))
        .child(stat(&format!("{}×", e.together)))
        .child(
            div()
                .flex_shrink_0()
                .w(theme::scaled_px(120.0))
                .h(theme::scaled_px(6.0))
                .bg(rgb(theme().surface))
                .child(
                    div()
                        .h_full()
                        .w(relative(frac))
                        .bg(rgb(theme().color_branch)),
                ),
        )
        .child(stat(&format!("{:.0}%", e.ratio * 100.0)))
        .into_any_element()
}

/// The ownership list: single-owner / high-share files first (bus-factor risk).
fn render_ownership_list(owners: &[FileOwnership]) -> AnyElement {
    let mut list = div()
        .id("eco-ownership-list")
        .flex()
        .flex_col()
        .size_full()
        .overflow_y_scroll();
    for (i, o) in owners.iter().enumerate() {
        list = list.child(render_ownership_row(i + 1, o));
    }
    list.into_any_element()
}

/// One ownership row: `#rank  path   owner   share%   N authors`.
/// A single author is flagged in the blocker colour (bus factor of one).
fn render_ownership_row(rank: usize, o: &FileOwnership) -> AnyElement {
    let solo = o.authors <= 1;
    let authors_color = if solo {
        theme().color_warning
    } else {
        theme().text_sub
    };
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
        .child(scroll_path_cell(
            format!("eco-own-path-{rank}"),
            o.path.clone(),
        ))
        .child(stat(&o.primary_author))
        .child(stat(&format!("{:.0}%", o.primary_share * 100.0)))
        .child(
            div()
                .flex_shrink_0()
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(authors_color))
                .child(format!(
                    "{} author{}",
                    o.authors,
                    if o.authors == 1 { "" } else { "s" }
                )),
        )
        .into_any_element()
}

// ── small shared element helpers ────────────────────────────────

/// The left "name" cell of a list row: takes the flexible space but, crucially,
/// `min_w(0)` + `overflow_x_scroll` so a very long path scrolls **inside the
/// cell** instead of pushing the fixed numeric columns / bar off the right edge
/// (user request). `whitespace_nowrap` keeps it on one line so it scrolls.
fn scroll_path_cell(id: String, text: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id))
        .flex_1()
        .min_w(px(0.0))
        .overflow_x_scroll()
        .whitespace_nowrap()
        .text_size(theme::scaled_px(13.0))
        .text_color(rgb(theme().text_main))
        .child(text)
}

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

/// A muted, fixed (non-shrinking) stat cell — stays visible at the right edge.
fn stat(text: &str) -> gpui::Div {
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
    for (heading, body) in crate::ui::i18n::eco_help_sections() {
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

    // The animated block stays OUTSIDE any scroll container — `with_animation`
    // doesn't tick inside an `overflow_y_scroll` element. Only the help scrolls.
    div()
        .flex()
        .flex_col()
        .size_full()
        .items_center()
        .child(
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
                ),
        )
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
