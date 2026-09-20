//! Composer-first GitHub Issues workspace (ADR-0198 / ADR-0201).
//!
//! Rendering consumes the session-owned Issue snapshot in `TabUiState`. Network
//! requests are started only by workspace-mode and row-selection handlers.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::{scroll::ScrollableElement, Icon, Sizable};
use kagi_domain::github::{filtered_issues, Issue, IssueListTab, IssueState};

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

const ISSUE_LIST_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueListPresentation {
    Loading,
    Error,
    Empty,
    Issues,
}

fn list_presentation(
    loading: bool,
    loaded: bool,
    has_error: bool,
    count: usize,
) -> IssueListPresentation {
    if loading && !loaded {
        IssueListPresentation::Loading
    } else if has_error && !loaded {
        IssueListPresentation::Error
    } else if loaded && count == 0 {
        IssueListPresentation::Empty
    } else {
        IssueListPresentation::Issues
    }
}

fn divider() -> gpui::Div {
    div()
        .w(theme::scaled_px(1.0))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().selected))
}

fn status_text(id: &'static str, text: impl Into<SharedString>, color: u32) -> AnyElement {
    super::e2e::measure_control(
        id,
        div()
            .id(id)
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .px_3()
            .text_sm()
            .text_color(rgb(color))
            .child(text.into()),
    )
    .into_any_element()
}

fn issues_for_tab(app: &KagiApp, tab: IssueListTab) -> Vec<&Issue> {
    let ui = app.ui();
    filtered_issues(
        &ui.github_issues,
        tab,
        app.github_login.as_deref(),
        &ui.github_issue_mentions,
    )
}

/// One sidebar page's content (ADR-0199): built for the Issues page whether it
/// is on screen or the neighbour a gesture is sliding toward, so it must stay a
/// pure read of `ui().github_issues`.
pub(super) fn render_issue_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let ui = app.ui();
    let issue_count = ui.github_issues.len().min(ISSUE_LIST_LIMIT);
    let selected = ui.selected_github_issue;
    let loading = ui.github_issues_loading;
    let loaded = ui.github_issues_loaded;
    let error = ui.github_issues_error.as_deref();
    let presentation = list_presentation(loading, loaded, error.is_some(), issue_count);

    let mut body = div()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scrollbar()
        .flex()
        .flex_col();

    body = match presentation {
        IssueListPresentation::Loading => body.child(status_text(
            "issue-mode-list-loading",
            Msg::IssuesLoadingList.t(),
            theme().text_muted,
        )),
        IssueListPresentation::Error => body.child(status_text(
            "issue-mode-list-error",
            safe_text(error.unwrap_or_default()),
            theme().color_blocker,
        )),
        IssueListPresentation::Empty | IssueListPresentation::Issues => {
            if loading {
                body = body.child(
                    div()
                        .id("issue-mode-list-refreshing")
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(Msg::IssuesRefreshing.t()),
                );
            }
            if let Some(message) = error {
                body = body.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(rgb(theme().color_blocker))
                        .whitespace_normal()
                        .child(safe_text(message)),
                );
            }
            let tabs = IssueListTab::ALL.map(|tab| (tab, issues_for_tab(app, tab)));
            for (tab, issues) in tabs {
                let active = tab == ui.github_issue_tab;
                let header = super::workspace_mode::sidebar_section_header(
                    ("issue-filter-tab", tab.index()),
                    issue_tab_label(tab),
                    issues.len(),
                    active,
                    false,
                    cx,
                    move |app, _, _, cx| app.select_github_issue_tab(tab, cx),
                );
                body = body.child(super::e2e::measure_control(
                    format!("issue-filter-tab-{}", tab.index()),
                    header,
                ));
                if !active {
                    continue;
                }
                if issues.is_empty() {
                    let (id, message) = if issue_count == 0 {
                        ("issue-mode-list-empty", Msg::IssuesEmpty.t())
                    } else {
                        ("issue-filter-empty", Msg::IssuesFilterEmpty.t())
                    };
                    body = body.child(super::e2e::measure_control(
                        id,
                        div()
                            .id(id)
                            .px_3()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(message),
                    ));
                }
                for issue in issues.into_iter().take(ISSUE_LIST_LIMIT) {
                    let number = issue.number;
                    let active_row = selected == Some(number);
                    let select = cx.listener(
                        move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                            this.load_github_issue_detail(number, window, cx);
                        },
                    );
                    let age = kagi_ui_core::time_parse::iso_to_epoch(&issue.updated_at)
                        .map(|at| {
                            kagi_ui_core::time::relative_time(
                                at,
                                kagi_ui_core::time::now_unix_secs(),
                            )
                        })
                        .unwrap_or_else(|| issue.updated_at.clone());
                    let content = div()
                        .flex_1()
                        .min_w(px(0.))
                        .px_3()
                        .py(px(6.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .mb(px(3.))
                                .text_sm()
                                .line_height(theme::scaled_px(18.))
                                .text_color(rgb(theme().text_main))
                                .child(safe_text(&format!("#{} {}", issue.number, issue.title))),
                        )
                        .child(
                            div()
                                .text_size(theme::scaled_px(10.))
                                .line_height(theme::scaled_px(13.))
                                .text_color(rgb(theme().text_muted))
                                .child(safe_text(&format!(
                                    "{age} · {}",
                                    super::i18n::issue_comments(issue.comment_count)
                                ))),
                        );
                    let row = super::workspace_mode::sidebar_list_row(active_row)
                        .id(("issue-mode-card", number as usize))
                        .on_click(select)
                        .child(content);
                    body = body.child(super::e2e::measure_control(
                        format!("issue-mode-card-{number}"),
                        row,
                    ));
                }
            }
            body
        }
    };

    div()
        .id("issue-mode-list")
        .w_full()
        .h_full()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar))
        .child(body)
        .into_any_element()
}

fn issue_tab_label(tab: IssueListTab) -> &'static str {
    match tab {
        IssueListTab::AssignedToMe => Msg::IssuesAssignedToMe.t(),
        IssueListTab::CreatedByMe => Msg::IssuesCreatedByMe.t(),
        IssueListTab::MentioningMe => Msg::IssuesMentioningMe.t(),
        IssueListTab::RecentlyUpdated => Msg::IssuesRecentlyUpdated.t(),
    }
}

/// The Issues home keeps the original full open-Issue feed below Composer.
/// `RecentlyUpdated` is the canonical all-Issue projection, so sorting and
/// limits stay shared with the sidebar without inheriting its selected filter.
fn render_main_issue_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let ui = app.ui();
    let issues = issues_for_tab(app, IssueListTab::RecentlyUpdated);
    let presentation = list_presentation(
        ui.github_issues_loading,
        ui.github_issues_loaded,
        ui.github_issues_error.is_some(),
        issues.len(),
    );
    let error = ui.github_issues_error.as_deref();
    let mut list = div()
        .id("issue-main-list")
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .overflow_hidden();

    list = match presentation {
        IssueListPresentation::Loading => list.child(status_text(
            "issue-main-list-loading",
            Msg::IssuesLoadingList.t(),
            theme().text_muted,
        )),
        IssueListPresentation::Error => list.child(status_text(
            "issue-main-list-error",
            safe_text(error.unwrap_or_default()),
            theme().color_blocker,
        )),
        IssueListPresentation::Empty | IssueListPresentation::Issues => {
            if ui.github_issues_loading {
                list = list.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(Msg::IssuesRefreshing.t()),
                );
            }
            if let Some(message) = error {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(rgb(theme().color_blocker))
                        .whitespace_normal()
                        .child(safe_text(message)),
                );
            }
            if issues.is_empty() {
                list = list.child(status_text(
                    "issue-main-list-empty",
                    Msg::IssuesEmpty.t(),
                    theme().text_muted,
                ));
            }
            for issue in issues.into_iter().take(ISSUE_LIST_LIMIT) {
                let number = issue.number;
                let select = cx.listener(
                    move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                        this.load_github_issue_detail(number, window, cx);
                    },
                );
                let age = kagi_ui_core::time_parse::iso_to_epoch(&issue.updated_at)
                    .map(|at| {
                        kagi_ui_core::time::relative_time(at, kagi_ui_core::time::now_unix_secs())
                    })
                    .unwrap_or_else(|| issue.updated_at.clone());
                let (state_label, state_color) = match issue.state {
                    IssueState::Open => (Msg::IssueStateOpen.t(), theme().color_success),
                    IssueState::Closed => (Msg::IssueStateClosed.t(), theme().text_muted),
                    IssueState::Unknown => (Msg::IssueStateUnknown.t(), theme().text_muted),
                };
                let content = div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(theme().text_main))
                                    .child(safe_text(&format!("@{}", issue.author))),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme().text_muted))
                                    .child(safe_text(&format!("#{} · {age}", issue.number))),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .whitespace_normal()
                            .text_size(theme::scaled_px(15.))
                            .line_height(theme::scaled_px(22.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(theme().text_main))
                            .child(safe_text(&issue.title)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::scaled_px(18.))
                            .pt_1()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Icon::empty().path("icons/message-square.svg").with_size(
                                            gpui_component::Size::Size(theme::scaled_px(14.)),
                                        ),
                                    )
                                    .child(issue.comment_count.to_string()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .w(theme::scaled_px(8.))
                                            .h(theme::scaled_px(8.))
                                            .rounded_full()
                                            .bg(rgb(state_color)),
                                    )
                                    .child(state_label),
                            ),
                    );
                let row = div()
                    .id(("issue-main-row", number as usize))
                    .w_full()
                    .min_h(theme::scaled_px(104.))
                    .flex()
                    .flex_row()
                    .gap(theme::scaled_px(14.))
                    .px(theme::scaled_px(24.))
                    .py(theme::scaled_px(16.))
                    .border_b_1()
                    .border_color(rgb(theme().selected))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(theme().surface)))
                    .on_click(select)
                    .child(kagi_ui_core::commit_header::avatar_circle_with_initials(
                        40.,
                        &issue.author,
                        &issue.author,
                        &app.avatars.images,
                    ))
                    .child(content);
                list = list.child(super::e2e::measure_control(
                    format!("issue-main-row-{number}"),
                    row,
                ));
            }
            list
        }
    };

    super::e2e::measure_control("issue-main-list", list).into_any_element()
}

fn render_center(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let selected = app.ui().selected_github_issue;
    let editors = &app.ui().issue_composer.editors;
    let mut center = div()
        .id("issue-mode-center-pane")
        .relative()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .overflow_y_scrollbar()
        .flex()
        .flex_col()
        .bg(rgb(theme().bg_base))
        // Keep the pane's `h_full` / `flex_1` chain intact. Wrapping this in
        // `measure_control` gives its scroll viewport an auto-sized parent and
        // can lay lower Issue rows outside the clickable window.
        .child(super::e2e::measure_inside("issue-mode-center-pane"));
    match selected {
        Some(number) => {
            if editors
                .get(&Some(number))
                .is_some_and(|editor| editor.focused)
            {
                center = center.child(super::issues_composer::render_composer(
                    app,
                    Some(number),
                    cx,
                ));
            } else {
                center = center
                    .child(super::issues_thread::render_thread(app, cx))
                    .child(super::issues_composer::render_composer(
                        app,
                        Some(number),
                        cx,
                    ));
            }
        }
        None => {
            center = center.child(super::issues_composer::render_composer(app, None, cx));
            if !editors.get(&None).is_some_and(|editor| editor.focused) {
                center = center.child(render_main_issue_list(app, cx));
            }
        }
    }
    center.into_any_element()
}

/// Render the sidebar navigator and Composer/Thread main workspace.
pub fn render_issues_mode(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    app.ensure_issue_avatars(cx);
    let left = super::e2e::measure_control(
        "issue-mode-left-pane",
        super::workspace_mode::render_sidebar_pages(
            app,
            super::workspace_mode::WorkspaceMode::Issues,
            cx,
        ),
    );
    let center = render_center(app, cx);
    div()
        .id("issue-mode-layout")
        .flex()
        .flex_row()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .bg(rgb(theme().bg_base))
        .child(left)
        .child(divider())
        .child(center)
        .into_any_element()
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Deterministic empty-success state for the visual recovery harness.
    pub fn show_empty_issues_for_e2e(&mut self, cx: &mut Context<Self>) {
        self.with_ui(|ui| {
            ui.github_issues_gen = ui.github_issues_gen.wrapping_add(1);
            ui.github_issues.clear();
            ui.github_issues_loaded = true;
            ui.github_issues_loading = false;
            ui.github_issues_error = None;
            ui.selected_github_issue = None;
            ui.github_issue_detail_loading = None;
            ui.github_issue_detail_error = None;
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_status_distinguishes_loading_empty_success_and_error() {
        assert_eq!(
            list_presentation(true, false, false, 0),
            IssueListPresentation::Loading
        );
        assert_eq!(
            list_presentation(false, true, false, 0),
            IssueListPresentation::Empty
        );
        assert_eq!(
            list_presentation(false, false, true, 0),
            IssueListPresentation::Error
        );
        assert_eq!(
            list_presentation(false, true, false, 1),
            IssueListPresentation::Issues
        );
        assert_eq!(
            list_presentation(true, true, false, 1),
            IssueListPresentation::Issues
        );
    }
}
