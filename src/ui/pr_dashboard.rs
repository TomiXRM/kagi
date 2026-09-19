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

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use gpui::{div, prelude::*, px, rgb, uniform_list, Context, SharedString};
use kagi_domain::github::{PrAttention, PrDetailAvailability, PrReason, PullRequest};
use kagi_domain::pr_list::{sort_prs, PrListFilter, PrSection, PrSort};

use super::i18n::Msg;
use super::pr_mode::{
    attention_color, card_border, ci_glyph, focus_queue, queue_bucket_label, reason_text,
};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// The dashboard's page: the app's own base background, not the chrome grey.
/// It reads as a *page*, and `surface` there was a muddy mid-tone that made
/// the whole screen look washed out (user report). The reading cards in
/// `pr_mode` keep their own inverted relationship — this is the home screen
/// only.
fn page_bg() -> u32 {
    theme().bg_base
}

/// A card on that page. No grey anywhere (user request): solid white in light
/// themes, and in dark ones plain black over the page. A partly transparent
/// white still mixed with what was behind it and came out grey, which is why
/// the light theme kept looking washed out.
fn dash_card_bg() -> gpui::Hsla {
    if theme().dark {
        gpui::hsla(0., 0., 0., 0.35)
    } else {
        gpui::hsla(0., 0., 1., 1.)
    }
}

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
    let ui = app.ui();
    let all = ui.github_prs.clone();
    let filter = app.pr_mode().map(|m| m.filter).unwrap_or_default();
    let sort = app.pr_mode().map(|m| m.sort).unwrap_or_default();
    let now = kagi_ui_core::time::now_unix_secs();
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
                        .text_color(rgb(if ui.github_error.is_some() {
                            theme().color_blocker
                        } else {
                            theme().text_muted
                        }))
                        // #506: three different empty screens — a failed fetch,
                        // a repo with no GitHub remote, and a real empty inbox.
                        .child(SharedString::from(if ui.github_error.is_some() {
                            Msg::PrFetchFailed.t()
                        } else if ui.github_unavailable {
                            Msg::PrGithubUnavailable.t()
                        } else {
                            Msg::PrPaneEmpty.t()
                        })),
                )
                .children(ui.github_error.clone().map(|e| {
                    div()
                        .max_w(theme::scaled_px(420.))
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(e)
                }))
                .child(refresh_button(cx)),
        );
    } else {
        // #506: the list survived a failed fetch, so it is last-known data —
        // say so above the tiles instead of showing it as fresh.
        if let Some(detail) = ui.github_error.clone() {
            body = body.child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from(Msg::PrFetchStale.t())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(detail),
                    ),
            );
        }
        body = body.child(render_tiles(&buckets));
        body = body.child(render_column_header());
        let mut rows: Vec<PullRequest> = all
            .iter()
            .filter(|pr| filter.accepts(pr))
            .cloned()
            .collect();
        sort_prs(&mut rows, sort);
        if rows.is_empty() {
            body = body.child(
                div()
                    .px_4()
                    .py_3()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::PrPaneEmpty.t())),
            );
        } else {
            let rows: Rc<Vec<(PullRequest, PrAttention, PrReason)>> = Rc::new(
                rows.into_iter()
                    .map(|pr| {
                        let (bucket, why) = att
                            .get(&pr.number)
                            .cloned()
                            .unwrap_or((PrAttention::Dormant, PrReason::None));
                        (pr, bucket, why)
                    })
                    .collect(),
            );
            let row_count = rows.len();
            let render_rows = rows.clone();
            let scroll = app
                .pr_mode()
                .map(|mode| mode.dashboard_scroll.clone())
                .unwrap_or_else(gpui::UniformListScrollHandle::new);
            body = body.child(
                uniform_list(
                    "pr-home-list",
                    row_count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        let start = range.start.saturating_sub(2);
                        let end = (range.end + 2).min(render_rows.len());
                        let visible: BTreeSet<u64> = render_rows[start..end]
                            .iter()
                            .map(|(pr, _, _)| pr.number)
                            .collect();
                        this.observe_visible_prs(visible, cx);
                        range
                            .filter_map(|index| {
                                render_rows.get(index).map(|(pr, bucket, why)| {
                                    render_table_row(
                                        pr,
                                        *bucket,
                                        why,
                                        this.pr_status_availability(pr),
                                        now,
                                        cx,
                                    )
                                })
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&scroll)
                .flex_1()
                .min_h(px(0.)),
            );
        }
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(page_bg()))
        .child(render_hero(app, cx))
        .child(body)
        .into_any_element()
}

/// Column widths (unscaled px), shared by the header and every row so the two
/// cannot drift. `TITLE / BRANCH` is the flexible one.
const COL_NO: f32 = 56.0;
const COL_STATE: f32 = 116.0;
const COL_AUTHOR: f32 = 116.0;
const COL_CHECKS: f32 = 72.0;
const COL_FILES: f32 = 52.0;
const COL_AGE: f32 = 52.0;
const ROW_H: f32 = 44.0;

fn col(width: f32, label: &'static str) -> gpui::Div {
    div()
        .w(theme::scaled_px(width))
        .flex_shrink_0()
        .child(SharedString::from(label))
}

/// The column names. Domain words, so they are the same in both languages
/// (ADR-0048) — they go through `Msg` only so the table has one vocabulary.
fn render_column_header() -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(26.))
        .px_4()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme().text_label))
        .child(col(COL_NO, Msg::PrColNo.t()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(SharedString::from(Msg::PrColTitle.t())),
        )
        .child(col(COL_STATE, Msg::PrColState.t()))
        .child(col(COL_AUTHOR, Msg::PrColAuthor.t()))
        .child(col(COL_CHECKS, Msg::PrColChecks.t()))
        .child(col(COL_FILES, Msg::PrColFiles.t()))
        .child(col(COL_AGE, Msg::PrColAge.t()))
}

/// One PR as a table row: identity, then the four facts the reader triages on.
///
/// The counts and the age come straight off the fetched list (ADR-0200), so a
/// row says how big a PR is and how stale it is without opening it.
fn render_table_row(
    pr: &PullRequest,
    bucket: PrAttention,
    why: &PrReason,
    status: PrDetailAvailability,
    now: i64,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let open = pr.clone();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_open(&open, cx);
    });
    let menu_pr = pr.clone();
    let menu = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
            this.with_ui(|ui| ui.pr_menu = Some((menu_pr.clone(), e.position)));
            cx.stop_propagation();
            cx.notify();
        },
    );
    let (glyph, glyph_ink) = ci_glyph(pr.ci);
    let checks = match status {
        PrDetailAvailability::Fresh => match (pr.checks.len(), pr.failed_checks()) {
            (0, _) => glyph.to_string(),
            (total, 0) => format!("{glyph}{total}"),
            (total, failed) => format!("\u{2717}{failed}/{total}"),
        },
        PrDetailAvailability::Missing | PrDetailAvailability::Loading => {
            Msg::PrWhyPending.t().to_string()
        }
        PrDetailAvailability::Stale => format!("{}*", Msg::PrWhyPending.t()),
    };
    // An age needs an instant; the list's stamp is text (see `PullRequest`).
    let age = kagi_ui_core::time_parse::iso_to_epoch(&pr.updated_at)
        .map(|at| kagi_ui_core::time::relative_time(at, now))
        .unwrap_or_default();
    let state = if pr.is_draft {
        Msg::PrDraft.t().to_string()
    } else {
        reason_text(why)
    };
    div()
        .id(("pr-home-row", pr.number as usize))
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(ROW_H))
        .px_4()
        .border_b_1()
        .border_color(rgb(theme().panel))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.75))
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .child(
            div()
                .w(theme::scaled_px(COL_NO))
                .flex_shrink_0()
                .child(SharedString::from(format!("#{}", pr.number))),
        )
        // Title over its branch pair: the two lines that identify the PR.
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pr_4()
                .flex()
                .flex_col()
                .gap_px()
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&pr.title)),
                )
                .child(
                    div()
                        .truncate()
                        .child(safe_text(&format!("{} \u{2192} {}", pr.head, pr.base))),
                ),
        )
        .child(
            div()
                .w(theme::scaled_px(COL_STATE))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(attention_color(bucket)))
                .child(SharedString::from(state)),
        )
        .child(
            div()
                .w(theme::scaled_px(COL_AUTHOR))
                .flex_shrink_0()
                .truncate()
                .child(safe_text(&format!("@{}", pr.author))),
        )
        .child(
            div()
                .w(theme::scaled_px(COL_CHECKS))
                .flex_shrink_0()
                .text_color(rgb(if pr.failed_checks() > 0 {
                    theme().color_blocker
                } else {
                    glyph_ink
                }))
                .child(SharedString::from(checks)),
        )
        .child(
            div()
                .w(theme::scaled_px(COL_FILES))
                .flex_shrink_0()
                .child(SharedString::from(if pr.changed_files > 0 {
                    pr.changed_files.to_string()
                } else {
                    String::new()
                })),
        )
        .child(
            div()
                .w(theme::scaled_px(COL_AGE))
                .flex_shrink_0()
                .child(SharedString::from(age)),
        )
        .into_any_element()
}

/// The list's header strip: what this is, how much of it there is, which slice
/// is showing, and in what order.
fn render_hero(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::Div {
    let all = &app.ui().github_prs;
    let counts = |filter: PrListFilter| all.iter().filter(|pr| filter.accepts(pr)).count();
    let active = app.pr_mode().map(|m| m.filter).unwrap_or_default();
    let sort = app.pr_mode().map(|m| m.sort).unwrap_or_default();
    // "need your review" counts review requests, which is what the chip's own
    // section in the navigator lists — one definition, two places.
    let mine_to_review = all
        .iter()
        .filter(|pr| {
            PrSection::Review.accepts(
                pr,
                app.github_login.as_deref(),
                &[], // local branches do not make a review request
            )
        })
        .count();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::PrPaneTitle.t())),
        )
        .when(mine_to_review > 0, |el| {
            el.child(div().text_xs().text_color(rgb(theme().color_branch)).child(
                SharedString::from(format!("{} {}", mine_to_review, Msg::PrHomeNeedsReview.t())),
            ))
        })
        .child(div().flex_1().min_w(px(0.)))
        .child(filter_chip(PrListFilter::Open, active, counts, cx))
        .child(filter_chip(PrListFilter::Draft, active, counts, cx))
        .child(sort_chip(sort, cx))
        .child(refresh_button(cx))
}

/// One slice chip, carrying its own count. The active one is filled, the way
/// the workspace navigator marks the page you are on.
fn filter_chip(
    filter: PrListFilter,
    active: PrListFilter,
    count: impl Fn(PrListFilter) -> usize,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let label = match filter {
        PrListFilter::Open => Msg::PrHomeOpen.t(),
        PrListFilter::Draft => Msg::PrHomeDraft.t(),
    };
    let id = match filter {
        PrListFilter::Open => "pr-home-filter-open",
        PrListFilter::Draft => "pr-home-filter-draft",
    };
    let on = filter == active;
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_set_filter(filter, cx);
    });
    div()
        .id(id)
        .flex_shrink_0()
        .px_2()
        .py_px()
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme().selected))
        .when(on, |el| el.bg(rgb(theme().selected)))
        .text_xs()
        .text_color(rgb(if on {
            theme().text_main
        } else {
            theme().text_sub
        }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .child(SharedString::from(format!("{label} {}", count(filter))))
}

/// The order chip: one control that cycles, so the strip carries a single
/// sort affordance rather than three competing ones.
fn sort_chip(sort: PrSort, cx: &mut Context<KagiApp>) -> gpui::Stateful<gpui::Div> {
    let click = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_cycle_sort(cx);
    });
    div()
        .id("pr-home-sort")
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
        .child(SharedString::from(match sort {
            PrSort::Updated => Msg::PrHomeSortUpdated.t(),
            PrSort::Created => Msg::PrHomeSortCreated.t(),
            PrSort::Number => Msg::PrHomeSortNumber.t(),
        }))
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
                .bg(dash_card_bg())
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
