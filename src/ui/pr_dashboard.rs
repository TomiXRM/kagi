//! PR mode's home screen — the dashboard the center shows when no PR tab is
//! open, plus the two bar buttons (`All PRs`, `Refresh`) the tab header
//! borrows.
//!
//! Split out of `pr_mode.rs`. Pure renderers over the fetched PR list; the
//! only state they touch is `pr_mode.active` (via `pr_mode_home`).
//!
//! ```text
//! ┌ Pull Requests · owner/repo ──────────────── [⟳ Refresh] ┐
//! │  ● 3        ● 2          ● 1                            │  attention tiles
//! │  NEEDS YOU  IN PROGRESS  READY                          │
//! │ Mine 3                                                  │
//! │ ▌ feat: PR merge from kagi                 ✓  approved  │  one card per PR
//! │   #247 · @tomixrm · feat/pr → main · ready to merge     │
//! └─────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::{stack_order, PrAttention, PrGroup, PrReason, PullRequest, ReviewState};

use super::i18n::Msg;
use super::pr_mode::{
    attention_color, card_bg, card_border, card_pane_bg, ci_glyph, focus_queue, queue_bucket_label,
    reason_text, MAX_INDENT_DEPTH,
};
use super::theme::{self, theme};
use super::KagiApp;

/// A header-bar button: icon + label, the same weight as the existing
/// `GitHub ↗` one so the row reads as one set of controls.
fn bar_button(
    id: &'static str,
    icon: &'static str,
    label: String,
    click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .px_2()
        .py_px()
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme().selected))
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .child(
            gpui::svg()
                .path(icon)
                .flex_shrink_0()
                .w(theme::scaled_px(12.))
                .h(theme::scaled_px(12.))
                .text_color(rgb(theme().text_sub)),
        )
        .child(SharedString::from(label))
}

/// Back to the dashboard. Opening a PR replaced the center for good — there
/// was no way back to the home screen short of closing the tab (user report).
pub(super) fn home_button(cx: &mut Context<KagiApp>) -> gpui::Stateful<gpui::Div> {
    let click = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_home(cx);
    });
    bar_button(
        "pr-mode-home",
        "icons/git-pull-request.svg",
        format!("\u{2190} {}", Msg::PrAllPrs.t()),
        click,
    )
}

/// Fetch the PR list now instead of waiting out the 60s ticker (user request).
pub(super) fn refresh_button(cx: &mut Context<KagiApp>) -> gpui::Stateful<gpui::Div> {
    let click = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_refresh(cx);
    });
    bar_button(
        "pr-mode-refresh",
        "icons/refresh-cw.svg",
        Msg::PrRefresh.t().to_string(),
        click,
    )
}

pub(super) fn render_dashboard(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .active_view
        .branches
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let all = app.github_prs.clone();
    let buckets = focus_queue(app);
    // Attention is what colours a card and writes its "why" line; the queue
    // already computes both, so the dashboard reads them off it by number
    // rather than classifying a second time.
    let att: HashMap<u64, (PrAttention, PrReason)> = buckets
        .iter()
        .flat_map(|(a, m)| m.iter().map(|(p, r)| (p.number, (*a, r.clone()))))
        .collect();

    let mut body = div()
        .id("pr-mode-dashboard")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .pb_4();

    if all.is_empty() {
        body = body.child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .py_8()
                .child(
                    gpui::svg()
                        .path("icons/inbox.svg")
                        .w(theme::scaled_px(28.))
                        .h(theme::scaled_px(28.))
                        .text_color(rgb(theme().text_muted)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(Msg::PrPaneEmpty.t())),
                ),
        );
    } else {
        body = body.child(render_tiles(&buckets));
    }

    for (group, title) in [
        (PrGroup::Mine, Msg::PrGroupMine.t()),
        (PrGroup::ReviewRequested, Msg::PrGroupReview.t()),
        (PrGroup::Others, Msg::PrGroupOthers.t()),
    ] {
        let members: Vec<PullRequest> = all
            .iter()
            .filter(|p| p.group_for(login.as_deref(), &local) == group)
            .cloned()
            .collect();
        if members.is_empty() {
            continue;
        }
        body = body.child(group_header(title, members.len()));
        for (ix, depth) in stack_order(&members) {
            let pr = &members[ix];
            let (bucket, why) = att
                .get(&pr.number)
                .cloned()
                .unwrap_or((PrAttention::Dormant, PrReason::None));
            body = body.child(render_card(pr, depth, bucket, &why, cx));
        }
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(card_pane_bg()))
        .child(render_hero(app, cx))
        .child(body)
        .into_any_element()
}

/// Title row: what this screen is, which repo it is for, and Refresh.
fn render_hero(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::Div {
    let repo = app
        .repo_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(
            gpui::svg()
                .path("icons/git-pull-request.svg")
                .flex_shrink_0()
                .w(theme::scaled_px(16.))
                .h(theme::scaled_px(16.))
                .text_color(rgb(theme().color_branch)),
        )
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::PrPaneTitle.t())),
        )
        .when(!repo.is_empty(), |el| {
            el.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(repo)),
            )
        })
        .child(div().flex_1().min_w(px(0.)))
        .child(refresh_button(cx))
}

/// One badge per non-empty attention bucket — dot, count, label, all on one
/// line. Stacking the label under the count made the strip as tall as two
/// cards for information that is three words wide (user request); one row at
/// normal text size is the height it needs.
fn render_tiles(buckets: &[(PrAttention, Vec<(PullRequest, PrReason)>)]) -> gpui::Div {
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_4()
        .pt_3();
    for (a, members) in buckets {
        let c = attention_color(*a);
        row = row.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(card_border())
                .bg(rgb(card_bg()))
                .text_sm()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(c))
                        .child(SharedString::from("\u{25CF}")),
                )
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(members.len().to_string())),
                )
                .child(
                    div()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(queue_bucket_label(*a))),
                ),
        );
    }
    row
}

fn group_header(title: &str, count: usize) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_4()
        .pt_4()
        .pb_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .px_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(count.to_string())),
        )
}

/// A dashboard card. Same information the old table row carried, laid out as
/// a card: title on its own line at full width, everything else on a quiet
/// second line, state as a coloured stripe rather than a column of glyphs.
fn render_card(
    pr: &PullRequest,
    depth: usize,
    bucket: PrAttention,
    why: &PrReason,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let accent = attention_color(bucket);
    let (g, c) = ci_glyph(pr.ci);
    let (rv, rvc) = match pr.review {
        ReviewState::Approved => (Msg::PrReviewApproved.t(), theme().color_success),
        ReviewState::ChangesRequested => (Msg::PrReviewChanges.t(), theme().color_warning),
        ReviewState::ReviewRequired => (Msg::PrReviewRequired.t(), theme().text_sub),
        ReviewState::None => ("", theme().text_muted),
    };
    let pr_open = pr.clone();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_open(&pr_open, cx);
    });
    let pr_menu = pr.clone();
    let menu = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
            this.pr_menu = Some((pr_menu.clone(), e.position));
            cx.stop_propagation();
            cx.notify();
        },
    );
    let title = if pr.is_draft {
        format!("{} ({})", pr.title, Msg::PrDraft.t())
    } else {
        pr.title.clone()
    };
    let reason = reason_text(why);
    let dot = || {
        div()
            .flex_shrink_0()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from("\u{00B7}"))
    };
    div()
        .id(("pr-dash-card", pr.number as usize))
        // Indent marks a stack; the cap stops a deep chain from squeezing the
        // card into nothing.
        .ml(theme::scaled_px(
            16. + depth.min(MAX_INDENT_DEPTH) as f32 * 18.,
        ))
        .mr_4()
        .mb_2()
        .flex()
        .flex_row()
        // A flex child in a column shrinks below its own content by default,
        // so on a full list every card was squeezed into the one under it
        // (user report). Cards keep their two-line height; the column
        // scrolls instead.
        .flex_shrink_0()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(card_border())
        .bg(rgb(card_bg()))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.65))
        // The state, as a stripe. One card = one colour, so a screen of them
        // stays readable.
        .child(
            div()
                .w(theme::scaled_px(3.))
                .flex_shrink_0()
                .bg(rgb(accent)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap_px()
                .px_3()
                .py_2()
                // Line 1 — CI, title, review decision. The CI mark leads:
                // titles are ragged, so a right-hand glyph column never lined
                // up and you had to hunt for it card by card (user request).
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .w(theme::scaled_px(12.))
                                .text_color(rgb(c))
                                .child(SharedString::from(g)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_sm()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(title)),
                        )
                        .when(!rv.is_empty(), |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(rgb(rvc))
                                    .child(SharedString::from(rv.to_string())),
                            )
                        }),
                )
                // Line 2 — identity and the "why", all muted.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(theme().color_branch))
                                .child(SharedString::from(format!("#{}", pr.number))),
                        )
                        .child(dot())
                        .child(
                            div()
                                .flex_shrink_0()
                                .max_w(theme::scaled_px(120.))
                                .truncate()
                                .child(SharedString::from(format!("@{}", pr.author))),
                        )
                        .child(dot())
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .child(SharedString::from(format!(
                                    "{} \u{2192} {}",
                                    pr.head, pr.base
                                ))),
                        )
                        .when(!reason.is_empty(), |el| {
                            el.child(dot()).child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(rgb(accent))
                                    .child(SharedString::from(reason.clone())),
                            )
                        }),
                ),
        )
        .into_any_element()
}
