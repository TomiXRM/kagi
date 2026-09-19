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
pub(super) fn render_pr_properties(app: &KagiApp, pr: &PullRequest) -> gpui::AnyElement {
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

    // One row per property: a fixed-width name, then the value. The names line
    // up so the column reads as a table rather than as a list of sentences.
    let row = |name: &str, value: gpui::AnyElement| {
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
            .child(div().flex_1().min_w(px(0.)).child(value))
    };

    let mut col = div()
        .id("pr-mode-properties")
        .w_full()
        .flex()
        .flex_col()
        .child(row(Msg::PrRailReviewers.t(), people(&pr.reviewers)))
        .child(row(Msg::PrRailAssignees.t(), people(&pr.assignees)))
        .child(row(Msg::PrRailLabels.t(), labels));
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
        ));
    }
    col.into_any_element()
}

/// The comment composer, pinned at the foot of the PR's page (ADR-0200).
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
    use gpui_component::Disableable as _;
    let input = app.pr_comment_input.clone()?;
    let number = app
        .pr_mode()
        .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
        .map(|t| t.pr.number)?;
    let empty = input.read(cx).value().trim().is_empty();
    let held = app.repo_path.as_ref().is_some_and(|repo| {
        app.transport_holds
            .contains(repo, &format!("pr-comment #{number}"))
    });
    let post = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.start_pr_comment(cx);
    });
    Some(
        div()
            .id("pr-mode-composer")
            .flex_shrink_0()
            .w_full()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .border_t_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .child(gpui_component::input::Input::new(&input))
            .child(
                div().flex().flex_row().items_center().justify_end().child(
                    gpui_component::button::Button::new("pr-comment-post")
                        .label(Msg::PrCommentPost.t())
                        .disabled(empty || held)
                        .on_click(post),
                ),
            )
            .into_any_element(),
    )
    .map(|el| super::e2e::measure_control("pr-mode-composer", el))
}
