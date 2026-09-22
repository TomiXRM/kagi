//! The PR's own page furniture: its properties, and the box you write a
//! comment in (ADR-0200 §8/§9).
//!
//! Split out of `pr_mode.rs` on a feature boundary when that file passed its
//! LOC ceiling. These are renderers over `KagiApp` with no state of their own -
//! the composer's text lives on the `PrTab` it was typed for and its input
//! entity on `KagiApp`, both owned by `pr_mode`.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::github::PullRequest;

use super::i18n::Msg;
use super::pr_mode::label_color;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// The property names' column, so their values line up as a table.
const PROPERTY_NAME_W: f32 = 78.0;

/// The PR's properties, as the mock's label/value rows (ADR-0200): who is
/// reviewing it, who owns it, how it is labelled, and whether a worktree here
/// has it checked out.
///
/// This is the *only* renderer of these facts. github.com keeps them in a
/// right-hand column; kagi keeps them at the top of the PR's own page, because
/// a column of metadata beside the diff is what the reader loses width to.
///
/// Logins carry their avatar when the background pass has resolved one
/// (`ensure_pr_avatars`), and their initial circle until then - a name with a
/// face beside it is how a reviewer list is read at a glance.
pub(super) fn render_pr_properties(
    app: &KagiApp,
    pr: &PullRequest,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // A worktree here that has the PR's head branch checked out: the answer to
    // "can I just go and look at this?", which no GitHub field can give.
    let worktree = app
        .view()
        .worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(pr.head.as_str()))
        .map(|w| {
            let dirt = w.wip.as_ref().map(|wip| wip.total()).unwrap_or(0);
            (
                w.name.clone(),
                match dirt {
                    0 => Msg::PrRailWorktreeClean.t().to_string(),
                    n => super::i18n::unstaged_not_included(n),
                },
            )
        });

    let avatars = &app.avatars.images;
    let people = |logins: &[String]| -> gpui::AnyElement {
        if logins.is_empty() {
            return div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrRailNone.t()))
                .into_any_element();
        }
        let mut row = div().flex().flex_row().flex_wrap().items_center().gap_2();
        for login in logins {
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(kagi_ui_core::commit_header::avatar_circle(
                        16., login, login, avatars,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_sub))
                            .child(safe_text(login)),
                    ),
            );
        }
        row.into_any_element()
    };

    let labels: gpui::AnyElement = if pr.labels.is_empty() {
        div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(Msg::PrRailNone.t()))
            .into_any_element()
    } else {
        let mut pills = div().flex().flex_row().flex_wrap().gap_1();
        for label in &pr.labels {
            pills = pills.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(label_color(&label.color)))
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(safe_text(&label.name)),
            );
        }
        pills.into_any_element()
    };

    // One row per property: a fixed-width name, then the value - which is the
    // editor's click target (ADR-0200 §11). A field with no editor - the
    // worktree line, which is local truth - passes `None` and is inert.
    let row = |name: &str, value: gpui::AnyElement, field: Option<super::modals::PrField>| {
        div()
            .flex()
            .flex_row()
            .items_start()
            .gap_2()
            .py_1()
            .child(
                div()
                    .w(theme::scaled_px(PROPERTY_NAME_W))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(name.to_string())),
            )
            // The value is the editor's way in - "なし" is an invitation to add,
            // an existing value an invitation to change. There is no gear:
            // the value already says what a gear would (user request).
            .child(match field {
                Some(field) => {
                    let open =
                        cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                            this.open_pr_fields_modal(field, cx);
                        });
                    div()
                        .id(match field {
                            super::modals::PrField::Reviewers => "pr-field-open-reviewers",
                            super::modals::PrField::Assignees => "pr-field-open-assignees",
                            super::modals::PrField::Labels => "pr-field-open-labels",
                        })
                        .flex_1()
                        .min_w(px(0.))
                        .px_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(theme().surface)))
                        .on_click(open)
                        .child(super::e2e::measure_control(
                            match field {
                                super::modals::PrField::Reviewers => "pr-field-value-reviewers",
                                super::modals::PrField::Assignees => "pr-field-value-assignees",
                                super::modals::PrField::Labels => "pr-field-value-labels",
                            },
                            value,
                        ))
                        .into_any_element()
                }
                None => div().flex_1().min_w(px(0.)).child(value).into_any_element(),
            })
    };

    // Boxed, like the description and the comments below it: a bare list of
    // rows floating on the page read as page furniture rather than as the
    // PR's own facts (user request).
    let mut col = div()
        .id("pr-mode-properties")
        .w_full()
        .rounded_lg()
        .bg(rgb(super::pr_mode::card_bg()))
        .border_1()
        .border_color(super::pr_attention::card_border())
        .px_4()
        .py_2()
        .flex()
        .flex_col()
        .child(row(
            Msg::PrRailReviewers.t(),
            people(&pr.reviewers),
            Some(super::modals::PrField::Reviewers),
        ))
        .child(row(
            Msg::PrRailAssignees.t(),
            people(&pr.assignees),
            Some(super::modals::PrField::Assignees),
        ))
        .child(row(
            Msg::PrRailLabels.t(),
            labels,
            Some(super::modals::PrField::Labels),
        ));
    if let Some((name, state)) = worktree {
        col = col.child(row(
            Msg::PrRailWorktree.t(),
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&name)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(state)),
                )
                .into_any_element(),
            None,
        ));
    }
    col.into_any_element()
}

/// The PR's own headline, at the top of its page (mock 7a): `#N` and the
/// title, then one meta row - state, author, branch pair, size.
///
/// The toolbar above shows the title too, but truncated into a strip shared
/// with the buttons, which is not where a reader looks for what they opened
/// (user report). Here it has the width of the page, and the facts that
/// answer "what am I looking at" sit under it rather than in four places.
pub(super) fn render_pr_headline(app: &KagiApp, pr: &PullRequest) -> gpui::AnyElement {
    // Draft / merged / open, as GitHub states it. `gh pr list` gives kagi only
    // open PRs plus the draft flag, so those are the two states it can claim.
    let (state, state_ink) = if pr.is_draft {
        (Msg::PrHomeDraft.t(), theme().text_muted)
    } else {
        (Msg::PrHomeOpen.t(), theme().color_success)
    };
    let chip = |text: SharedString, ink: u32, border: u32| {
        div()
            .flex_shrink_0()
            .px_2()
            .py(px(1.))
            .rounded_sm()
            .border_1()
            .border_color(rgb(border))
            .text_xs()
            .text_color(rgb(ink))
            .child(text)
    };
    let age = (!pr.updated_at.is_empty())
        .then(|| kagi_ui_core::time_parse::iso_to_epoch(&pr.updated_at))
        .flatten()
        .map(|at| {
            kagi_ui_core::time::relative_time(at, kagi_ui_core::time::now_unix_secs()).to_string()
        });
    div()
        .id("pr-mode-headline")
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(format!("#{}", pr.number))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_lg()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&pr.title)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(chip(
                    SharedString::from(state.to_string()),
                    state_ink,
                    theme().selected,
                ))
                .child(kagi_ui_core::commit_header::avatar_circle(
                    16.,
                    &pr.author,
                    &pr.author,
                    &app.avatars.images,
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_sub))
                        .child(safe_text(&pr.author)),
                )
                .children(age.map(|age| {
                    div()
                        .flex_shrink_0()
                        .child(SharedString::from(age))
                        .into_any_element()
                }))
                .child(chip(
                    safe_text(&format!("{} \u{2192} {}", pr.head, pr.base)),
                    theme().color_branch,
                    theme().selected,
                ))
                // The size of the change, which is what decides whether this
                // is a read-now or a read-later.
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(theme().color_success))
                        .child(SharedString::from(format!("+{}", pr.additions))),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(theme().color_blocker))
                        .child(SharedString::from(format!("\u{2212}{}", pr.deletions))),
                )
                .child(div().flex_shrink_0().child(SharedString::from(format!(
                    "{} {}",
                    pr.changed_files,
                    Msg::PrModeFiles.t()
                )))),
        )
        .into_any_element()
}

/// The checks card on the PR page (mock 7a folded / 7b open): one line that
/// answers "can this merge", and the per-check list behind a disclosure.
///
/// `None` when the PR reported no checks - an empty card says nothing. It is
/// on the page rather than in the swimlane pane because whether CI passed is
/// the second thing a reader wants after the title, not a fact filed away in
/// a companion pane (mock 7a).
pub(super) fn render_checks_card(
    app: &KagiApp,
    pr: &PullRequest,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let checks = pr.checks.clone();
    if checks.is_empty() {
        return None;
    }
    let failed = pr.failed_checks();
    let pending = checks
        .iter()
        .filter(|c| c.state == kagi_domain::github::CiState::Pending)
        .count();
    let open = app.pr_mode().map(|m| m.checks_open).unwrap_or(false);
    let (glyph, ink, headline) = if failed > 0 {
        (
            "\u{2717}",
            theme().color_blocker,
            Msg::PrChecksFailed.t().to_string(),
        )
    } else if pending > 0 {
        (
            "\u{25CF}",
            theme().color_warning,
            Msg::PrChecksRunning.t().to_string(),
        )
    } else {
        (
            "\u{2713}",
            theme().color_success,
            Msg::PrChecksPassed.t().to_string(),
        )
    };
    let toggle = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_toggle_checks(cx);
    });
    let summary = div()
        .id("pr-mode-checks-summary")
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(toggle)
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(ink))
                .child(SharedString::from(glyph)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(headline)),
                )
                .child(div().text_xs().text_color(rgb(theme().text_muted)).child(
                    SharedString::from(super::i18n::pr_checks_counts(checks.len(), failed)),
                )),
        )
        // The disclosure marker is the state, so folded and open are told
        // apart without reading the rows.
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(if open {
                    "\u{2304}"
                } else {
                    "\u{203a}"
                })),
        );

    let mut card = div()
        .id("pr-mode-checks")
        .w_full()
        .rounded_lg()
        .bg(rgb(super::pr_mode::card_bg()))
        .border_1()
        .border_color(super::pr_attention::card_border())
        .flex()
        .flex_col()
        .child(summary);
    if open {
        // Failures first: the actionable ones must not need scrolling.
        let mut ordered = checks.clone();
        ordered.sort_by_key(|c| match c.state {
            kagi_domain::github::CiState::Failure => 0,
            kagi_domain::github::CiState::Pending => 1,
            _ => 2,
        });
        for (i, c) in ordered.iter().enumerate() {
            let (g, colour) = super::pr_attention::ci_glyph(c.state);
            let url = c.url.clone();
            let has_url = !url.is_empty();
            let open_in_browser =
                cx.listener(move |_this: &mut KagiApp, _: &gpui::ClickEvent, _w, _cx| {
                    if !url.is_empty() {
                        let _ = std::process::Command::new("open").arg(&url).spawn();
                    }
                });
            let label = if c.workflow.is_empty() || c.workflow == c.name {
                c.name.clone()
            } else {
                format!("{} / {}", c.workflow, c.name)
            };
            card = card.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_1()
                    .border_t_1()
                    .border_color(rgb(theme().selected))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(rgb(colour))
                            .child(SharedString::from(g)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_xs()
                            .text_color(rgb(theme().text_sub))
                            .child(safe_text(&label)),
                    )
                    .when(has_url, |el| {
                        el.child(
                            div()
                                .id(("pr-mode-check-open", i))
                                .flex_shrink_0()
                                .px_2()
                                .rounded_sm()
                                .border_1()
                                .border_color(rgb(theme().selected))
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(theme().surface)))
                                .on_click(open_in_browser)
                                .child(SharedString::from(Msg::PrChecksOpenInBrowser.t())),
                        )
                    }),
            );
        }
    }
    Some(super::e2e::measure_control("pr-mode-checks", card))
}

/// The comment composer, pinned at the foot of the PR's page (ADR-0200), in
/// the Issues timeline's chrome (#750): the viewer's avatar, one box with one
/// placeholder, edit/preview as a single icon toggle, and an amber POST that
/// is amber only while it can be pressed.
///
/// `None` until the window-bearing render pass has built the input
/// (`sync_pr_comment_input`): `InputState::new` needs a `&mut Window`, and this
/// renderer has none. POST is disabled while the box is empty and while a
/// transport hold is parked on this PR's comment - an unproven post must not be
/// retried blindly.
pub(super) fn render_composer(
    app: &KagiApp,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    use gpui_component::{Disableable as _, Sizable as _};
    let input = app.pr_comment_input.clone()?;
    let number = app
        .pr_mode()
        .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
        .map(|t| t.pr.number)?;
    // The box is the truth, not the tab's parked copy: a preview of what was
    // parked would show yesterday's text while you type today's. The value is
    // already a `SharedString`; keep it, don't rebuild it every frame.
    let typed = input.read(cx).value().clone();
    let empty = typed.trim().is_empty();
    let preview = app.pr_mode().is_some_and(|m| m.comment_preview);
    let held = app.repo_path.as_ref().is_some_and(|repo| {
        app.transport_holds
            .contains(repo, &format!("pr-comment #{number}"))
    });
    let review_held = app.repo_path.as_ref().is_some_and(|repo| {
        app.transport_holds
            .contains(repo, &format!("pr-review #{number}"))
    });
    let post = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.start_pr_comment(cx);
    });
    let approve = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.start_pr_review(kagi_domain::github::ReviewVerdict::Approve, cx);
    });
    let request_changes = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.start_pr_review(kagi_domain::github::ReviewVerdict::RequestChanges, cx);
    });
    // Preview renders the typed text; the box itself is never rebuilt or
    // re-`set_value`d by the toggle, so undo history survives a round trip.
    let body: gpui::AnyElement = if preview {
        super::e2e::measure_control(
            "pr-composer-preview",
            div()
                .min_h(px(56.))
                .child(super::timeline_row::body_markdown(
                    ("pr-comment-preview", number as usize),
                    typed.as_ref(),
                    super::timeline_row::markdown_style(15., cx),
                )),
        )
    } else {
        super::timeline_row::body_input(&input).into_any_element()
    };
    let viewer = app.github_login.as_deref().unwrap_or("?");
    let mut content = super::timeline_row::content_column()
        .gap_2()
        .child(body)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(super::e2e::measure_control(
                    "pr-composer-mode-toggle",
                    super::timeline_row::mode_toggle(
                        "pr-composer-mode-toggle-button",
                        preview,
                        cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                            this.pr_comment_toggle_preview(cx);
                        }),
                    ),
                ))
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        // GitHub requires words on a "request changes" review and
                        // allows a wordless approval, so only one of the three is
                        // usable with an empty box (mock 7a).
                        // The verdicts carry their colour; a plain comment is the
                        // composer's own amber submit.
                        .child(
                            super::button_style::KagiButton::accent_icon(
                                "pr-review-request-changes",
                                "icons/review-request-changes.svg",
                                Msg::PrReviewRequestChanges.t(),
                                theme().color_warning,
                                cx,
                            )
                            .small()
                            .disabled(empty || review_held)
                            .on_click(request_changes),
                        )
                        .child(
                            super::button_style::KagiButton::accent_icon(
                                "pr-review-approve",
                                "icons/review-approve.svg",
                                Msg::PrReviewApprove.t(),
                                theme().color_success,
                                cx,
                            )
                            .small()
                            .disabled(review_held)
                            .on_click(approve),
                        )
                        .child(super::timeline_row::submit(
                            "pr-comment-post",
                            Msg::PrCommentPost.t(),
                            empty || held,
                            post,
                        )),
                ),
        );
    // What the box holds lives on the PR's tab for as long as the app runs
    // (`PrTab::comment_draft`) - it is kept, not filed away on disk like an
    // Issue draft. Say so only when there is something to keep.
    if !empty {
        content = content.child(super::timeline_row::draft_status(
            Msg::ComposerDraftSaved.t(),
        ));
    }
    Some(
        super::timeline_row::composer_frame(
            "pr-mode-composer",
            viewer,
            &app.avatars.images,
            content,
        )
        .flex_shrink_0()
        .border_t_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().panel))
        .into_any_element(),
    )
    .map(|el| super::e2e::measure_control("pr-mode-composer", el))
}

impl KagiApp {
    /// Flip the pinned composer between its box and its markdown preview
    /// (#750). The input entity is untouched, so undo survives the trip.
    pub fn pr_comment_toggle_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(mode) = self.pr_mode_mut() {
            mode.comment_preview = !mode.comment_preview;
        }
        cx.notify();
    }
}
