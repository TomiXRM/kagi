//! The pull-request navigator — the sidebar page PR mode owns.
//!
//! Split out of `pr_mode.rs` when the navigator's sections pushed that file
//! past its LOC ceiling. It is one sidebar page's content (ADR-0199), built
//! whether the page is on screen or is the neighbour a gesture is sliding
//! toward, so everything here stays a pure read of `ui().pr_list_rows()`.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::{PrAttention, PrGroup, PullRequest, ReviewState};
use kagi_domain::list_filter::apply_prs;
use kagi_domain::pr_list::PrSection;

use super::i18n::Msg;
use super::pr_attention::attention_color;
use super::pr_mode::{focus_border, PrFocus};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::workspace_mode::SectionCount;
use super::KagiApp;

/// A fold-and-count header, the shape the navigator's sections share with the
/// Graph page's sections: a disclosure mark, the name, the count. `None`: who
/// "me" is on the PRs' host is not known yet, so no count is claimed (#906).
fn render_section_header(
    section: PrSection,
    count: Option<usize>,
    open: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    super::workspace_mode::sidebar_section_header(
        ("pr-mode-section", section.index()),
        pr_section_label(section),
        match count {
            Some(n) => SectionCount::Known { n, more: false },
            None => SectionCount::Unknown,
        },
        open,
        section == PrSection::Inbox,
        cx,
        move |this, _, _w, cx| this.pr_mode_toggle_section(section, cx),
    )
}

/// Who "me" is for each PR row: the `gh` login on that row's host (#906), not
/// the github.com account — an Enterprise repository's PRs are judged by
/// that server's identity. Looked up once per run of rows on the same
/// repository (a list is one repository), not once per row.
pub(super) fn row_viewers<'a>(app: &'a KagiApp, rows: &'a [PullRequest]) -> Vec<Option<&'a str>> {
    let mut last: Option<(&str, Option<&str>)> = None;
    rows.iter()
        .map(|pr| match last {
            Some((repo, login)) if repo == pr.base_repo => login,
            _ => {
                let login = app.host_login(&pr.base_repo);
                last = Some((&pr.base_repo, login));
                login
            }
        })
        .collect()
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
    let local: Vec<String> = app.view().branches.iter().map(|(n, _)| n.clone()).collect();
    let rows = app.ui().pr_list_rows();
    let viewers = row_viewers(app, rows);
    let indices = apply_prs(rows, &app.ui().github_pr_filter, |pr| {
        app.pr_status_availability(pr)
    });
    let open = app
        .pr_mode()
        .map(|m| m.sections_open)
        .unwrap_or([true, false, false, false]);
    PrSection::ALL
        .into_iter()
        .map(|section| {
            let members: Vec<PrListRow> = indices
                .iter()
                .map(|&index| (&rows[index], viewers[index]))
                .filter(|(pr, me)| {
                    section.accepts_with_status(pr, *me, &local, app.pr_status_availability(pr))
                })
                .map(|(pr, me)| {
                    let group = pr.group_for(me, &local);
                    let (attention, _why) = pr.attention_with_status(
                        group == PrGroup::Mine,
                        group == PrGroup::ReviewRequested,
                        app.pr_status_availability(pr),
                    );
                    PrListRow {
                        pr: pr.clone(),
                        attention,
                    }
                })
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
// must stay a pure read of `ui().pr_list_rows()`.
pub(super) fn render_pr_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let focused = app.pr_mode().map(|m| m.focus) == Some(PrFocus::List);
    let focus_click = cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
        this.pr_mode_focus(PrFocus::List, cx);
    });
    let all = app.ui().pr_list_rows();
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
    // #906: every section depends on who "me" is; until the login on the
    // PRs' host is known, the rows still list but no count is claimed.
    let viewer_known = row_viewers(app, all).iter().all(Option::is_some);
    for (section, open, members) in pr_sections(app) {
        let count = viewer_known.then_some(members.len());
        body = body.child(render_section_header(section, count, open, cx));
        if !open {
            continue;
        }
        for row in members {
            let stacked = row.pr.is_stacked_on(all);
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
    use kagi_domain::github::IssueState;
    let state = match pr.state {
        IssueState::Closed => Msg::IssueStateClosed.t(),
        IssueState::Unknown => Msg::IssueStateUnknown.t(),
        IssueState::Open if pr.is_draft => Msg::PrHomeDraft.t(),
        IssueState::Open => Msg::PrHomeOpen.t(),
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
        // Line 1 - the title, full width, with air under it (user request).
        .child(
            div()
                .w_full()
                .truncate()
                .mb(px(3.))
                .text_sm()
                .line_height(theme::scaled_px(18.))
                .text_color(rgb(theme().text_main))
                .child(safe_text(&pr.title)),
        )
        // Line 2 - the state at the left, the number hard against the right.
        // The head branch is not here: it repeated what the title says, in the
        // room the number now uses (user request).
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .w_full()
                .text_size(theme::scaled_px(10.))
                .line_height(theme::scaled_px(13.))
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
                }))
                // The number is the row's right edge: every row ends with one,
                // so a column of them reads as a column.
                .child(div().flex_1().min_w(px(0.)))
                .child(
                    div()
                        .flex_shrink_0()
                        .child(SharedString::from(format!("#{}", pr.number))),
                ),
        );
    let row = super::workspace_mode::sidebar_list_row(is_active)
        .id(("pr-mode-card", pr.number as usize))
        // The row fits its two lines and its padding - it is not given a fixed
        // height. A fixed 42px did fit two bare line boxes, but not once the
        // rows gained the mock's vertical padding: the text then overflowed
        // its own row and drew across the hairline below it (user report).
        // Tight line boxes keep it compact without a magic number to keep in
        // sync.
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.7))
        .child(edge)
        .child(card);
    super::e2e::measure_control(format!("pr-mode-card-{}", pr.number), row)
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// The PR numbers the navigator lists under `section`, from the same
    /// projection the renderer draws (#906).
    pub fn pr_section_numbers_for_e2e(&self, section: PrSection) -> Vec<u64> {
        pr_sections(self)
            .into_iter()
            .find(|(candidate, _, _)| *candidate == section)
            .map(|(_, _, members)| members.iter().map(|row| row.pr.number).collect())
            .unwrap_or_default()
    }

    /// Whether the `gh` login read for `host` is in flight or has landed
    /// (#906); a failed read is forgotten and reads `false` again.
    pub fn host_login_requested_for_e2e(&self, host: Option<&str>) -> bool {
        self.github_host_login_requests
            .iter()
            .any(|requested| requested.as_deref() == host)
    }
}
