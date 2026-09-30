//! Analyze's Health axis (#358, ADR-0205): one row per finding — what it
//! means, and an "Enable…" button that asks the host to plan the fix. The
//! host opens the plan's confirmation; nothing is written from this pane.

use super::*;
use gpui::AnyElement;
use kagi_ui_core::i18n::plan::maintenance::{
    checking_text, enable_label, finding_text, healthy_text,
};

/// The Health body for the current read state.
pub(super) fn render_health(view: &EcosystemView, cx: &mut Context<EcosystemView>) -> AnyElement {
    match &view.data.health {
        None => super::render::centered(checking_text()),
        Some(Err(error)) => {
            super::render::centered(&format!("{}: {error}", Msg::EcoLoadFailed.t()))
        }
        Some(Ok(findings)) if findings.is_empty() => super::render::centered(healthy_text()),
        Some(Ok(findings)) => {
            let mut list = div().flex().flex_col().gap_2().p_3();
            for (ix, finding) in findings.iter().enumerate() {
                list = list.child(finding_row(ix, *finding, cx));
            }
            list.into_any_element()
        }
    }
}

fn finding_row(ix: usize, finding: HealthFinding, cx: &mut Context<EcosystemView>) -> gpui::Div {
    let fix = finding.fix();
    let click =
        cx.listener(move |view, _: &gpui::ClickEvent, _w, cx| view.request_health_fix(fix, cx));
    div()
        .flex()
        .items_center()
        .gap_3()
        .p_3()
        .rounded(theme::scaled_px(6.0))
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(theme::scaled_px(13.0))
                .text_color(rgb(theme().text_main))
                .child(finding_text(finding)),
        )
        .child(
            div()
                .id(SharedString::from(format!("eco-health-fix-{ix}")))
                .flex_shrink_0()
                .px_3()
                .py_1()
                .rounded(theme::scaled_px(4.0))
                .bg(rgb(theme().accent))
                .text_size(theme::scaled_px(12.0))
                .text_color(rgb(theme().bg_base))
                .cursor_pointer()
                .child(enable_label())
                .on_click(click),
        )
}
