//! The pull-request navigator — the sidebar page PR mode owns.
//!
//! Split out of `pr_mode.rs` when the navigator's sections pushed that file
//! past its LOC ceiling. It is one sidebar page's content (ADR-0199), built
//! whether the page is on screen or is the neighbour a gesture is sliding
//! toward, so everything here must stay a pure read of `ui().github_prs`.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::{stack_order, PrAttention, PrGroup, PullRequest, ReviewState};
use kagi_domain::pr_list::PrSection;

use super::i18n::Msg;
use super::pr_mode::{attention_color, focus_border, PrFocus};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// A fold-and-count header, the shape the navigator's sections share with the
/// Graph page's sections: a disclosure mark, the name, the count.
fn render_section_header(
    section: PrSection,
    count: usize,
    open: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_toggle_section(section, cx);
    });
    div()
        .id(("pr-mode-section", section.index()))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .pt_2()
        .pb_1()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(toggle)
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(if open {
                    "\u{25BE}"
                } else {
                    "\u{25B8}"
                })),
        )
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(pr_section_label(section))),
        )
        .child(div().flex_1().min_w(px(0.)))
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(if count > 0 && section == PrSection::Inbox {
                    theme().color_branch
                } else {
                    theme().text_muted
                }))
                .child(SharedString::from(count.to_string())),
        )
        .into_any_element()
}

pub(super) fn pr_section_label(section: PrSection) -> &'static str {
    match section {
        PrSection::Inbox => Msg::PrSectionInbox.t(),
        PrSection::Mine => Msg::PrSectionMine.t(),
        PrSection::Review => Msg::PrSectionReview.t(),
        PrSection::Assigned => Msg::PrSectionAssigned.t(),
    }
}

/// One visible row of the navigator.
///
/// Sections are filters, so the same PR can be two rows — under `MY PRS` and
/// again under `REVIEW`. The list is built once and used by both the renderer
/// and ↑/↓ stepping, so what the arrows walk is exactly what is on screen.
/// The attention verdict rides along because both consumers want it and it is
/// derived from the PR, not fetched. The reason behind that verdict is not: the
/// row shows the state and the branch (ADR-0200), and the section header above
/// it already names the bucket.
#[derive(Clone)]
pub(super) struct PrListRow {
    pub pr: PullRequest,
    pub attention: PrAttention,
}

/// Every section header in order, each with its members and whether it is
/// unfolded. Headers are always returned — a section with nothing in it says
/// so with a zero, which is information.
pub(super) fn pr_sections(app: &KagiApp) -> Vec<(PrSection, bool, Vec<PrListRow>)> {
    let me = app.github_login.clone();
    let local: Vec<String> = app.view().branches.iter().map(|(n, _)| n.clone()).collect();
    let open = app
        .pr_mode()
        .map(|m| m.sections_open)
        .unwrap_or([true, false, false, false]);
    PrSection::ALL
        .into_iter()
        .map(|section| {
            let mut members: Vec<PrListRow> = app
                .ui()
                .github_prs
                .iter()
                .filter(|pr| section.accepts(pr, me.as_deref(), &local))
                .map(|pr| {
                    let group = pr.group_for(me.as_deref(), &local);
                    let (attention, _why) =
                        pr.attention(group == PrGroup::Mine, group == PrGroup::ReviewRequested);
                    PrListRow {
                        pr: pr.clone(),
                        attention,
                    }
                })
                .collect();
            // Stack order within a section so a chain still reads top-down.
            let prs: Vec<PullRequest> = members.iter().map(|r| r.pr.clone()).collect();
            let order = stack_order(&prs);
            members = order
                .into_iter()
                .map(|(ix, _)| members[ix].clone())
                .collect();
            (section, open[section.index()], members)
        })
        .collect()
}

/// The navigator's visible rows, top to bottom — folded sections contribute
/// none. Shared by the renderer and ↑/↓ stepping so they cannot disagree.
pub(super) fn pr_list_order(app: &KagiApp) -> Vec<PullRequest> {
    pr_sections(app)
        .into_iter()
        .filter(|(_, open, _)| *open)
        .flat_map(|(_, _, members)| members.into_iter().map(|r| r.pr))
        .collect()
}

// ── Left: PR list, grouped, stack-ordered ────────────────────
//
// One sidebar page's content (ADR-0199): built here for the PR page whether it
// is the page on screen or the neighbour a gesture is sliding toward, so it
// must stay a pure read of `ui().github_prs`.
pub(super) fn render_pr_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let focused = app.pr_mode().map(|m| m.focus) == Some(PrFocus::List);
    let focus_click = cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
        this.pr_mode_focus(PrFocus::List, cx);
    });
    let all = app.ui().github_prs.clone();
    let active_pr = app
        .pr_mode()
        .and_then(|m| m.active.and_then(|i| m.tabs.get(i)))
        .map(|t| t.pr.number);

    let mut body = div()
        .id("pr-mode-list-body")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    if all.is_empty() {
        body = body.child(
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrPaneEmpty.t())),
        );
    }
    for (section, open, members) in pr_sections(app) {
        body = body.child(render_section_header(section, members.len(), open, cx));
        if !open {
            continue;
        }
        for row in members {
            let stacked = row.pr.is_stacked_on(&all);
            body = body.child(render_pr_card(
                &row.pr,
                row.attention,
                stacked,
                active_pr,
                cx,
            ));
        }
    }

    focus_border(
        div()
            .id("pr-mode-list")
            .w_full()
            .h_full()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(rgb(theme().sidebar))
            .on_mouse_down(gpui::MouseButton::Left, focus_click),
        focused,
    )
    .child(body)
    .into_any_element()
}

/// A Focus-Queue card: identity, size, and — the point — the one line that
/// says what state it is in and why. Replaces the one-line row: at a glance
/// the user should know which PR to touch next without decoding glyphs.
/// Classify a PR as agent-created from its author login and head branch
/// (issue #337). PRs carry no commit trailers/committer, so only the
/// author-login and branch-prefix routes apply. `None` = no badge.
///
/// ponytail: built-in patterns only (empty `extra`) — reusing the shared
/// classifier avoids per-frame `Settings::load()` I/O on every card. Add the
/// settings-extensible list here too if PR-specific agents ever need it.
fn pr_provenance(pr: &PullRequest) -> Option<kagi_domain::provenance::Provenance> {
    let author = kagi_domain::commit::Signature {
        name: pr.author.clone(),
        email: String::new(),
        time: 0,
    };
    kagi_domain::provenance::classify_provenance(&[], &author, &author, Some(pr.head.as_str()), &[])
}

fn render_pr_card(
    pr: &PullRequest,
    bucket: PrAttention,
    stacked: bool,
    active_pr: Option<u64>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let is_active = active_pr == Some(pr.number);
    let accent = attention_color(bucket);
    let pr_click = pr.clone();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_open(&pr_click, cx);
    });
    let pr_menu = pr.clone();
    let menu = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
            this.with_ui(|ui| ui.pr_menu = Some((pr_menu.clone(), e.position)));
            cx.stop_propagation();
            cx.notify();
        },
    );

    // The mock's row (ADR-0200): `#N title` on the first line, then a state
    // dot and the branch on the second. The number joins the title because
    // together they are how the PR is named out loud; the branch is what tells
    // two similarly titled PRs apart, and it is what a worktree is called.
    //
    // The dot carries the attention colour, so the bucket is still visible per
    // row now that the left border is selection rather than attention. Two
    // facts ride at the right of the second line because they change what you
    // do next: the failed/total check count and the agent badge.
    let state = if pr.is_draft {
        Msg::PrHomeDraft.t()
    } else {
        Msg::PrHomeOpen.t()
    };
    let checks = match (pr.checks.len(), pr.failed_checks()) {
        (0, _) => String::new(),
        (total, 0) => format!("\u{2713}{}", total),
        (total, failed) => format!("\u{2717}{}/{}", failed, total),
    };
    let review_mark = match pr.review {
        ReviewState::Approved => Some(("\u{2714}", theme().color_success)),
        ReviewState::ChangesRequested => Some(("\u{21BA}", theme().color_warning)),
        _ => None,
    };
    // The accent edge is its own 2px strip rather than a left border: the row
    // needs a hairline under it *and* an edge beside it, and one element has
    // one border colour.
    let edge = div()
        .w(px(2.))
        .h_full()
        .flex_shrink_0()
        .when(is_active, |el| el.bg(rgb(accent)));
    let card = div()
        .flex_1()
        .min_w(px(0.))
        .px_3()
        .py(px(6.))
        .flex()
        .flex_col()
        .justify_center()
        .gap_px()
        // Line 1 - `#N title`, full width.
        .child(
            div()
                .w_full()
                .truncate()
                .text_sm()
                .line_height(theme::scaled_px(18.))
                .text_color(rgb(theme().text_main))
                .child(safe_text(&format!("#{} {}", pr.number, pr.title))),
        )
        // Line 2 - state, branch, and the two facts that change what you do.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .w_full()
                .text_xs()
                .line_height(theme::scaled_px(15.))
                .text_color(rgb(theme().text_muted))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(accent))
                        .child(SharedString::from(format!("\u{25CF} {}", state))),
                )
                .when(stacked, |el| {
                    el.child(div().flex_shrink_0().child(SharedString::from("\u{21B3}")))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(rgb(theme().color_branch))
                        .child(safe_text(&pr.head)),
                )
                .when(!checks.is_empty(), |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .text_color(rgb(if pr.failed_checks() > 0 {
                                theme().color_blocker
                            } else {
                                theme().text_muted
                            }))
                            .child(SharedString::from(checks.clone())),
                    )
                })
                .children(review_mark.map(|(g, c)| {
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(c))
                        .child(SharedString::from(g))
                }))
                // Issue #337: "agent-created" badge - the glyph only; the agent
                // is conveyed by hue (Claude -> orange, others -> violet).
                // Nothing when unclassifiable.
                .children(pr_provenance(pr).map(|prov| {
                    use kagi_domain::provenance::AgentKind;
                    let hue = match prov.agent {
                        AgentKind::ClaudeCode => 0xf97316,
                        _ => 0x8b5cf6,
                    };
                    let (bg, border, _text) = theme::badge_style(hue);
                    div()
                        .flex_shrink_0()
                        .px(px(3.))
                        .rounded_sm()
                        .bg(gpui::rgba(bg))
                        .border_1()
                        .border_color(gpui::rgba(border))
                        .child(SharedString::from("🤖"))
                })),
        );
    let row = div()
        .id(("pr-mode-card", pr.number as usize))
        .flex()
        .flex_row()
        // The row fits its two lines and its padding - it is not given a fixed
        // height. A fixed 42px did fit two bare line boxes, but not once the
        // rows gained the mock's vertical padding: the text then overflowed
        // its own row and drew across the hairline below it (user report).
        // Tight line boxes keep it compact without a magic number to keep in
        // sync.
        .overflow_hidden()
        .cursor_pointer()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .when(is_active, |el| el.bg(rgb(theme().selected)))
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.7))
        .child(edge)
        .child(card);
    super::e2e::measure_control(format!("pr-mode-card-{}", pr.number), row)
}
