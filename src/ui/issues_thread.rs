//! The retained Issue projection rendered through one outer virtual list.
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::text::{TextSelectionGroupElement as _, TextView};
use kagi_domain::github::IssueState;

use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::{i18n::Msg, issue_conversation::IssueConversation, KagiApp};

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

fn header(cx: &mut Context<KagiApp>) -> AnyElement {
    let click = cx.listener(|app: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
        app.return_to_issues_home(window, cx);
    });
    super::e2e::measure_control(
        "issue-thread-back",
        div()
            .id("issue-thread-back")
            .w_full()
            .px(theme::scaled_px(super::timeline_row::GUTTER))
            .py_2()
            .border_b_1()
            .border_color(rgb(theme().selected))
            .cursor_pointer()
            .text_sm()
            .text_color(rgb(theme().color_branch))
            .hover(|style| style.text_color(rgb(theme().text_main)))
            .on_click(click)
            .child(Msg::IssueBackToList.t()),
    )
    .into_any_element()
}

fn detail_status(app: &KagiApp) -> Option<AnyElement> {
    let ui = app.ui();
    if let Some(message) = ui.github_issue_detail_error.as_deref() {
        Some(status(
            "issue-mode-detail-error",
            safe_text(message),
            theme().color_blocker,
        ))
    } else if ui.github_issue_detail_loading == ui.selected_github_issue {
        Some(status(
            "issue-mode-detail-loading",
            Msg::IssueLoading.t(),
            theme().text_muted,
        ))
    } else {
        None
    }
}

pub(super) fn render_thread(
    app: &KagiApp,

    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let Some(conversation) = app.selected_issue_conversation() else {
        return pending_thread(app, cx);
    };
    let consumer_generation = app.ui().issue_conversation_gen;
    let session = app.active_session();
    IssueConversation::bind_window(
        &conversation,
        &cx.entity(),
        session,
        consumer_generation,
        window,
        cx,
    );
    let owner = conversation.read(cx);
    let state = owner.list.clone();
    let group = owner.group.clone();
    let generation = owner.generation();
    let weak_owner = conversation.downgrade();
    let weak_app = cx.entity().downgrade();
    let render_owner = weak_owner.clone();
    let render = cx.processor(move |app: &mut KagiApp, index: usize, window, cx| {
        if app.active_session() != session || app.ui().issue_conversation_gen != consumer_generation
        {
            return div().into_any_element();
        }
        let Some(entity) = render_owner.upgrade() else {
            return div().into_any_element();
        };
        let owner = entity.read(cx);
        if owner.generation() != generation {
            return div().into_any_element();
        }
        if index == 0 {
            return header(cx);
        }
        if index == owner.posts.len() + 1 {
            return super::issues_composer::render_composer(app, Some(owner.number), window, cx);
        }
        post(app, owner, index - 1, cx)
    });
    let scrollbar = state.clone();
    let viewport = div()
        .id("issue-thread")
        .flex_1()
        .min_h(px(0.))
        .h_full()
        .w_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(super::e2e::measure_inside("issue-thread"))
        .child(super::render_helpers::with_vertical_scrollbar(
            "issue-thread-scroll",
            &scrollbar,
            gpui::list(state, move |index, window, cx| render(index, window, cx))
                .flex_1()
                .min_h(px(0.)),
            true,
        ))
        // Request chrome must not change a retained post's list geometry.
        // Keep it visible without moving the scroll viewport's origin.
        .children(detail_status(app).map(|status| div().flex_none().child(status)));
    viewport
        .text_selection_group(group, move |distance, _, cx| {
            let Some(app) = weak_app.upgrade() else {
                return;
            };
            let current = app.read(cx);
            if current.active_session() != session
                || current.ui().issue_conversation_gen != consumer_generation
                || current.home_in_front()
                || current.has_active_modal()
                || current.menu_overlay.is_some()
                || current.conflict_body_visible()
                || current.workspace_mode() != super::workspace_mode::WorkspaceMode::Issues
                || current.has_modal_or_visible_plan(cx)
            {
                return;
            }
            let Some(owner) = weak_owner.upgrade() else {
                return;
            };
            let state = owner.read(cx);
            if !state.active() || state.generation() != generation {
                return;
            }
            state.list.scroll_by(distance);
            app.update(cx, |_, cx| cx.notify());
        })
        .into_any_element()
}

fn pending_thread(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let state = app.ui().issue_thread_pending_list.clone();
    state.remeasure_items(0..2);
    let session = app.active_session();
    let number = app.ui().selected_github_issue;
    let generation = app.ui().issue_conversation_gen;
    let render = cx.processor(move |app: &mut KagiApp, index: usize, window, cx| {
        if app.active_session() != session
            || app.ui().selected_github_issue != number
            || app.ui().issue_conversation_gen != generation
        {
            return div().into_any_element();
        }
        if index == 0 {
            return div()
                .w_full()
                .flex()
                .flex_col()
                .child(header(cx))
                .children(detail_status(app))
                .when(
                    app.ui().github_issue_detail_loading.is_none()
                        && app.ui().github_issue_detail_error.is_none(),
                    |row| {
                        row.child(status(
                            "issue-mode-detail-empty",
                            Msg::IssueDetailsUnavailable.t(),
                            theme().text_muted,
                        ))
                    },
                )
                .into_any_element();
        }
        super::issues_composer::render_composer(app, number, window, cx)
    });
    let scrollbar = state.clone();
    super::render_helpers::with_vertical_scrollbar(
        "issue-thread-pending-scroll",
        &scrollbar,
        gpui::list(state, move |index, window, cx| render(index, window, cx))
            .flex_1()
            .min_h(px(0.)),
        true,
    )
    .into_any_element()
}

fn post(app: &KagiApp, owner: &IssueConversation, index: usize, cx: &gpui::App) -> AnyElement {
    let Some(post) = owner.posts.get(index) else {
        return div().into_any_element();
    };
    let id = post.comment_index.map_or_else(
        || format!("issue-thread-body-{}", owner.number),
        |index| format!("issue-thread-comment-{}-{index}", owner.number),
    );
    let mut content =
        super::timeline_row::content_column()
            .gap_2()
            .child(super::timeline_row::meta(
                &post.author,
                &super::timeline_row::age(&post.created_at),
            ));
    if index == 0 {
        content = content.child(
            div()
                .text_size(theme::scaled_px(20.))
                .line_height(theme::scaled_px(28.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_main))
                .whitespace_normal()
                .child(safe_text(&format!("#{} {}", owner.number, owner.title))),
        );
    }
    content = content.child(super::e2e::measure_control(
        format!("{id}-md"),
        TextView::new(&post.text)
            .selectable(true)
            .scrollable(false)
            .style(super::timeline_row::markdown_style(15., cx))
            .font_features(kagi_ui_editor::markdown::literal_text_features()),
    ));
    if index == 0 {
        let (label, color) = match owner.state {
            IssueState::Open => (Msg::IssueStateOpen.t(), theme().color_success),
            IssueState::Closed => (Msg::IssueStateClosed.t(), theme().text_muted),
            IssueState::Unknown => (Msg::IssueStateUnknown.t(), theme().text_muted),
        };
        content = content.child(super::timeline_row::state_dot(label, color));
    }
    let row = super::timeline_row::row(
        ("issue-conversation-post", post.text.entity_id()),
        &post.author,
        app.issue_repo_host(),
        &app.avatars.images,
        content,
    );
    let mut result = div().w_full().flex().flex_col();
    if index == 1 {
        result = result.child(
            div()
                .px(theme::scaled_px(super::timeline_row::GUTTER))
                .py_2()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_label))
                .child(super::i18n::issue_comments(owner.posts.len() - 1)),
        );
    }
    result.child(row).into_any_element()
}
