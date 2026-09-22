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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use gpui::{div, prelude::*, px, rgb, uniform_list, Context, SharedString};
use kagi_domain::github::{PrAttention, PrDetailAvailability, PrReason, PullRequest};
use kagi_domain::list_filter::apply_prs;

use super::i18n::Msg;
use super::pr_attention::{
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
    let all = &ui.github_prs;
    let rows = apply_prs(all, &ui.github_pr_filter, |pr| {
        app.pr_status_availability(pr)
    });
    let filtered_count = rows.len();
    let numbers: HashSet<_> = rows.iter().map(|&index| all[index].number).collect();
    let now = kagi_ui_core::time::now_unix_secs();
    let mut buckets = focus_queue(app);
    for (_, members) in &mut buckets {
        members.retain(|(pr, _)| numbers.contains(&pr.number));
    }
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
            let rows: Rc<Vec<(usize, PrAttention, PrReason)>> = Rc::new(
                rows.into_iter()
                    .map(|index| {
                        let (bucket, why) = att
                            .get(&all[index].number)
                            .cloned()
                            .unwrap_or((PrAttention::Dormant, PrReason::None));
                        (index, bucket, why)
                    })
                    .collect(),
            );
            let row_count = rows.len();
            let render_rows = rows.clone();
            let scroll = app
                .pr_mode()
                .map(|mode| mode.dashboard_scroll.clone())
                .unwrap_or_default();
            let owner = app.active_session();
            let generation = ui.github_prs_gen;
            body = body.child(
                uniform_list(
                    "pr-home-list",
                    row_count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        if this.active_session() != owner || this.ui().github_prs_gen != generation
                        {
                            return Vec::new();
                        }
                        let start = range.start.saturating_sub(2);
                        let end = (range.end + 2).min(render_rows.len());
                        let visible: BTreeSet<u64> = render_rows[start..end]
                            .iter()
                            .filter_map(|(index, _, _)| {
                                this.ui().github_prs.get(*index).map(|pr| pr.number)
                            })
                            .collect();
                        this.observe_visible_prs(visible, cx);
                        range
                            .filter_map(|index| {
                                render_rows.get(index).and_then(|(source, bucket, why)| {
                                    let pr = this.ui().github_prs.get(*source)?;
                                    // Borrowed, not cloned: a `clone()` here
                                    // copied the whole avatar map for every
                                    // visible row, every frame (#750 review).
                                    let status = this.pr_status_availability(pr);
                                    Some(render_table_row(
                                        pr,
                                        *bucket,
                                        why,
                                        status,
                                        now,
                                        &this.avatars.images,
                                        cx,
                                    ))
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
        .child(super::list_filter_strip::render_strip(
            app,
            super::list_filter_strip::ListKind::Prs,
            filtered_count,
            cx,
        ))
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
/// Row padding for the triage table: denser than the feed's `ROW_PY`, because
/// this page is scanned rather than read (PM Tier B, #750). With the shared
/// 40px avatar and the hairline the row settles at 65px.
const ROW_PY: f32 = 12.0;

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
        .px(theme::scaled_px(super::timeline_row::GUTTER))
        .gap(theme::scaled_px(super::timeline_row::GAP))
        .border_b_1()
        .border_color(rgb(theme().selected))
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme().text_label))
        // The rows' avatar column has no name; the header reserves its width,
        // and the labels sit in one strip like the row's cells, so the two
        // line up by construction rather than by two lists of paddings.
        .child(
            div()
                .w(theme::scaled_px(super::timeline_row::AVATAR))
                .flex_shrink_0(),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_row()
                .items_center()
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
                .child(col(COL_AGE, Msg::PrColAge.t())),
        )
}

/// One PR as a table row: identity, then the four facts the reader triages on.
///
/// The counts and the age come straight off the fetched list (ADR-0200), so a
/// row says how big a PR is and how stale it is without opening it. The
/// leading avatar, gutter and hairline are the shared timeline chrome (#750) —
/// the page stays a table, it just stops being a different table.
fn render_table_row(
    pr: &PullRequest,
    bucket: PrAttention,
    why: &PrReason,
    status: PrDetailAvailability,
    now: i64,
    avatars: &kagi_ui_core::avatar::AvatarImages,
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
    use kagi_domain::github::IssueState;
    let state = match pr.state {
        IssueState::Closed => Msg::IssueStateClosed.t().to_string(),
        IssueState::Unknown => Msg::IssueStateUnknown.t().to_string(),
        IssueState::Open if pr.is_draft => Msg::PrDraft.t().to_string(),
        IssueState::Open => reason_text(why),
    };
    // The cells fill everything right of the shared row's avatar column. The
    // strip must take the table's width, not its own content's: without
    // `flex_1` + `overflow_hidden` a long title sets the column's intrinsic
    // width and the fixed columns land at a different x on each row (user
    // report).
    let cells = div()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
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
                // `w_full` + `overflow_hidden`: a long title otherwise sets
                // the column's intrinsic width and pushes the fixed columns
                // to the right out of alignment (user report, zed).
                .overflow_hidden()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&pr.title)),
                )
                .child(
                    div()
                        .w_full()
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
                // Blank only for a fetched zero; a count not fetched yet says
                // so, like the checks cell (user report).
                .child(SharedString::from(match status {
                    PrDetailAvailability::Fresh if pr.changed_files > 0 => {
                        pr.changed_files.to_string()
                    }
                    PrDetailAvailability::Fresh => String::new(),
                    _ => "\u{2026}".to_string(),
                })),
        )
        .child(
            div()
                .w(theme::scaled_px(COL_AGE))
                .flex_shrink_0()
                .child(SharedString::from(age)),
        );
    super::timeline_row::clickable(super::timeline_row::row(
        ("pr-home-row", pr.number as usize),
        &pr.author,
        avatars,
        cells,
    ))
    .relative()
    .child(super::e2e::measure_inside(format!(
        "pr-home-row-{}",
        pr.number
    )))
    .items_center()
    .flex_shrink_0()
    // A triage table is denser than a feed, so the row carries 12px rather
    // than the feed's 16px — and no fixed height: the avatar, that padding
    // and the hairline decide it (a 64px box clipped the 40px avatar by 1px,
    // #750 review). `uniform_list` measures its first item.
    .py(theme::scaled_px(ROW_PY))
    .on_click(click)
    .on_mouse_down(gpui::MouseButton::Right, menu)
    .when(pr.is_draft, |el| el.opacity(0.75))
    .into_any_element()
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
