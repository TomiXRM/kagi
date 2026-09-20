//! Issue conversation rendering over the existing session-owned detail cache.
//!
//! The workspace owns scrolling and appends its Reply composer after this
//! content. This module neither fetches data nor owns a second copy of it.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::ActiveTheme as _;
use kagi_domain::github::IssueState;

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

fn status(id: &'static str, text: impl Into<SharedString>, color: u32) -> AnyElement {
    super::e2e::measure_control(
        id,
        div()
            .id(id)
            .w_full()
            .py_4()
            .text_sm()
            .text_color(rgb(color))
            .whitespace_normal()
            .child(text.into()),
    )
    .into_any_element()
}

fn back_to_issues(cx: &mut Context<KagiApp>) -> AnyElement {
    let click = cx.listener(|app: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
        app.return_to_issues_home(cx);
    });
    super::e2e::measure_control(
        "issue-thread-back",
        div()
            .id("issue-thread-back")
            .cursor_pointer()
            .text_sm()
            .text_color(rgb(theme().color_branch))
            .hover(|style| style.text_color(rgb(theme().text_main)))
            .on_click(click)
            .child(Msg::IssueBackToList.t()),
    )
    .into_any_element()
}

fn thread_shell(back: AnyElement, content: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(back)
        .child(content)
        .into_any_element()
}

/// Render the body followed by chronological comments, without a nested scroll
/// container. The active session is the sole source of selection and detail.
pub(super) fn render_thread(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let ui = app.ui();
    let Some(number) = ui.selected_github_issue else {
        return status(
            "issue-mode-detail-empty",
            Msg::IssueSelect.t(),
            theme().text_muted,
        );
    };
    if ui.github_issue_detail_loading == Some(number)
        && !ui.github_issue_details.contains_key(&number)
    {
        return thread_shell(
            back_to_issues(cx),
            status(
                "issue-mode-detail-loading",
                Msg::IssueLoading.t(),
                theme().text_muted,
            ),
        );
    }
    if let Some(message) = ui
        .github_issue_detail_error
        .as_deref()
        .filter(|_| !ui.github_issue_details.contains_key(&number))
    {
        return thread_shell(
            back_to_issues(cx),
            status(
                "issue-mode-detail-error",
                safe_text(message),
                theme().color_blocker,
            ),
        );
    }
    let Some(issue) = ui.github_issue_details.get(&number) else {
        return thread_shell(
            back_to_issues(cx),
            status(
                "issue-mode-detail-empty",
                Msg::IssueDetailsUnavailable.t(),
                theme().text_muted,
            ),
        );
    };
    let mut content = div().flex().flex_col().gap_2();
    if let Some(message) = ui.github_issue_detail_error.as_deref() {
        content = content.child(status(
            "issue-mode-detail-error",
            safe_text(message),
            theme().color_blocker,
        ));
    } else if ui.github_issue_detail_loading == Some(number) {
        content = content.child(status(
            "issue-mode-detail-loading",
            Msg::IssueLoading.t(),
            theme().text_muted,
        ));
    }
    thread_shell(
        back_to_issues(cx),
        content
            .child(render_conversation(app, issue, cx))
            .into_any_element(),
    )
}

fn render_conversation(
    app: &KagiApp,
    issue: &kagi_domain::github::Issue,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let number = issue.number;
    let state = match issue.state {
        IssueState::Open => Msg::IssueStateOpen.t(),
        IssueState::Closed => Msg::IssueStateClosed.t(),
        IssueState::Unknown => Msg::IssueStateUnknown.t(),
    };
    let mut thread = div()
        .id("issue-thread")
        .w_full()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_main))
                .whitespace_normal()
                .child(safe_text(&format!("#{} {}", number, issue.title))),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(state),
        )
        .child(post(
            app,
            format!("issue-thread-body-{number}"),
            &issue.author,
            &issue.created_at,
            if issue.body.trim().is_empty() {
                Msg::IssueNoDescription.t()
            } else {
                &issue.body
            },
            cx,
        ));
    if !issue.comments.is_empty() {
        thread = thread.child(
            div()
                .pt_2()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(super::i18n::issue_comments(
                    issue.comments.len(),
                ))),
        );
    }
    // Sort references, not bodies. Stable ordering preserves API order for
    // equal timestamps; gh returns UTC ISO timestamps suitable for comparison.
    let mut comments: Vec<_> = issue.comments.iter().enumerate().collect();
    comments.sort_by(|(_, left), (_, right)| left.created_at.cmp(&right.created_at));
    for (index, comment) in comments {
        thread = thread.child(post(
            app,
            format!("issue-thread-comment-{number}-{index}"),
            &comment.author,
            &comment.created_at,
            &comment.body,
            cx,
        ));
    }
    super::e2e::measure_control("issue-thread", thread).into_any_element()
}

/// Both the initial post and replies use the PR conversation's Markdown
/// pipeline: normalize GitHub text, pad inline code, flatten HTML, then render.
fn post(
    app: &KagiApp,
    id: String,
    author: &str,
    created_at: &str,
    body: &str,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let body = kagi_domain::message::sanitize_markdown_for_view(body);
    let body = kagi_ui_editor::markdown::pad_inline_code(&body);
    let style = TextViewStyle {
        heading_base_font_size: theme::scaled_px(15.),
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        ..Default::default()
    };
    div()
        .id(SharedString::from(id.clone()))
        .w_full()
        .min_w(px(0.))
        .p_3()
        .rounded_md()
        .bg(rgb(theme().panel))
        .flex()
        .flex_col()
        .gap_2()
        .text_color(rgb(theme().text_main))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(kagi_ui_core::commit_header::avatar_circle(
                    18.,
                    author,
                    author,
                    &app.avatars.images,
                ))
                .child(safe_text(&format!("@{author}")))
                .child(div().flex_1())
                .child(safe_text(created_at)),
        )
        .child(
            TextView::markdown(
                SharedString::from(format!("{id}-md")),
                SharedString::from(kagi_ui_core::markdown::flatten_html_blocks(&body)),
            )
            .plugin(kagi_ui_core::markdown::MarkdownImages::remote())
            .selectable(true)
            .style(style),
        )
        .into_any_element()
}
