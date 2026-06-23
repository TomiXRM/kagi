//! Per-mode list / row renderers for the Analyze view (ADR-0119), split out of
//! `render.rs` to keep each render module under the LOC budget. Covers the
//! textual Hotspots / Coupling / Ownership rows and the Coupling Mermaid-source
//! panel; the Map (treemap) and force-directed Graph live in `viz.rs` / `graph.rs`.
//!
//! Shared row widgets (`scroll_path_cell`, `stat`, `text_button`) stay in
//! `render.rs` and are reused from here.

use super::render::{scroll_path_cell, stat, text_button};
use super::*;
use gpui::{relative, AnyElement};
use kagi_domain::hotspot::{CouplingEdge, CouplingPair, Ecosystem, FileMetric, FileOwnership};

/// Cap on rendered rows — the power-law means the top slice carries the signal,
/// and it bounds the element count without virtualization.
const MAX_ROWS: usize = 300;

/// The risk-ranked Hotspots list (top [`MAX_ROWS`]).
pub(super) fn render_hotspot_list(eco: &Ecosystem) -> AnyElement {
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

/// The Mermaid sub-view: the coupling flowchart **source**, with a one-click
/// "Open in mermaid.live" that renders it in the browser. Kagi is GPUI-native
/// (no embedded web renderer), so the diagram is shown as text here and handed
/// off to mermaid.live for the rendered picture the user liked.
pub(super) fn render_coupling_mermaid_view(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> AnyElement {
    let source = view.coupling_mermaid_source();
    let open_click = cx.listener(|v, _: &gpui::ClickEvent, _w, cx| v.open_in_mermaid_live(cx));

    // Action bar: open-in-browser button + a short hint.
    let bar = div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            text_button("eco-mermaid-open", Msg::EcoOpenMermaidLive.t(), true).on_click(open_click),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().text_sub))
                .child(Msg::EcoMermaidHint.t()),
        );

    // Source panel: one div per line preserves the layout (GPUI doesn't honour
    // embedded newlines in a single text node); monospaced, scrollable.
    let mut code = div()
        .id("eco-mermaid-src")
        .flex()
        .flex_col()
        .size_full()
        .overflow_scroll()
        .px_3()
        .py_2();
    for line in source.lines() {
        code = code.child(
            div()
                .font_family("monospace")
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().text_main))
                .whitespace_nowrap()
                .child(line.to_string()),
        );
    }

    div()
        .flex()
        .flex_col()
        .size_full()
        .child(bar)
        .child(div().flex_1().min_h(px(0.0)).child(code))
        .into_any_element()
}

/// The change-coupling list: file pairs that change together, top first. A row
/// click expands the left file's full 1:many partner set beneath it.
pub(super) fn render_coupling_list(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> AnyElement {
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
pub(super) fn render_ownership_list(owners: &[FileOwnership]) -> AnyElement {
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
