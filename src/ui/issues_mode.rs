//! Composer-first GitHub Issues workspace (ADR-0198 / ADR-0201).
//!
//! Rendering consumes the session-owned Issue snapshot in `TabUiState`. Network
//! requests are started only by workspace-mode and row-selection handlers.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::{scroll::ScrollableElement, Icon, Sizable};
use kagi_domain::github::{Issue, IssueListTab, IssueState};
use kagi_domain::list_filter::apply_issues;

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::tab_view::TabUiState;
use super::theme::{self, theme};
use super::KagiApp;

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
    if count > 0 {
        IssueListPresentation::Issues
    } else if loading && !loaded {
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

fn issue_indices(app: &KagiApp, tab: IssueListTab) -> Vec<usize> {
    let ui = app.ui();
    let mut indices = apply_issues(&ui.github_issues, &ui.github_issue_filter);
    indices.retain(|&index| {
        tab.accepts(
            &ui.github_issues[index],
            app.github_login.as_deref(),
            &ui.github_issue_mentions,
        )
    });
    indices
}

fn issues_for_tab(app: &KagiApp, tab: IssueListTab) -> Vec<&Issue> {
    issue_indices(app, tab)
        .into_iter()
        .map(|index| &app.ui().github_issues[index])
        .collect()
}

/// #753: is the visible list a *client-side subset* of the rows this session
/// holds?
///
/// The state chip is a server collection — changing it restarts the request,
/// so the rows that come back already answer it — and sorting only reorders,
/// so neither one makes a loaded page a partial answer. Labels, author, title
/// and any tab other than `RecentlyUpdated` do: the row at the tail is then
/// the last *match*, not the last row, and letting layout keep paging on its
/// behalf drains the repository hunting for further matches. Continuation
/// under those predicates is a user request, never a side effect of drawing.
fn client_membership_active(ui: &TabUiState) -> bool {
    let filter = &ui.github_issue_filter.common;
    !filter.labels.is_empty()
        || filter.author.is_some()
        || !filter.text.is_empty()
        || ui.github_issue_tab != IssueListTab::RecentlyUpdated
}

/// One sidebar page's content (ADR-0199): built for the Issues page whether it
/// is on screen or the neighbour a gesture is sliding toward, so it must stay a
/// pure read of `ui().github_issues`.
pub(super) fn render_issue_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let ui = app.ui();
    let issue_count = ui.github_issues.len();
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
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(super::render_overlay::sync_spinner(
                            10.,
                            theme().text_muted,
                            "issue-sidebar-refresh-spinner",
                        ))
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
                    (issues.len(), ui.github_issues_cursor.is_some()),
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
                for issue in issues.into_iter().take(100) {
                    let number = issue.number;
                    let active_row = selected == Some(number);
                    let select = cx.listener(
                        move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                            this.load_github_issue_detail(number, window, cx);
                        },
                    );
                    let age = super::timeline_row::age(&issue.updated_at);
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
                                .child(safe_text(&issue.title)),
                        )
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
                                .child(div().flex_shrink_0().child(safe_text(&format!(
                                    "{age} · {}",
                                    super::i18n::issue_comments(issue.comment_count)
                                ))))
                                .child(div().flex_1().min_w(px(0.)))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .child(SharedString::from(format!("#{}", issue.number))),
                                ),
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

/// Status is a separate virtual row so refreshing or retrying never replaces
/// the retained Issue rows.
fn render_main_list_status(
    app: &KagiApp,
    filtered_count: usize,
    _cx: &mut Context<KagiApp>,
) -> AnyElement {
    let ui = app.ui();
    let presentation = list_presentation(
        ui.github_issues_loading,
        ui.github_issues_loaded,
        ui.github_issues_error.is_some(),
        filtered_count,
    );
    let error = ui.github_issues_error.as_deref();
    let mut list = div()
        .id("issue-main-list-status")
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
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(super::render_overlay::sync_spinner(
                            10.,
                            theme().text_muted,
                            "issue-main-refresh-spinner",
                        ))
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
            if filtered_count == 0 {
                list = list.child(status_text(
                    "issue-main-list-empty",
                    Msg::IssuesFilterEmpty.t(),
                    theme().text_muted,
                ));
            }

            list
        }
    };

    list.into_any_element()
}

fn render_main_issue_row(app: &KagiApp, issue: &Issue, cx: &mut Context<KagiApp>) -> AnyElement {
    let number = issue.number;
    let select = cx.listener(
        move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
            this.load_github_issue_detail(number, window, cx);
        },
    );
    let age = super::timeline_row::age(&issue.updated_at);
    let (state_label, state_color) = match issue.state {
        IssueState::Open => (Msg::IssueStateOpen.t(), theme().color_success),
        IssueState::Closed => (Msg::IssueStateClosed.t(), theme().text_muted),
        IssueState::Unknown => (Msg::IssueStateUnknown.t(), theme().text_muted),
    };
    let content = super::timeline_row::content_column()
        .gap_1()
        .child(super::timeline_row::meta(
            &issue.author,
            &format!("#{} · {age}", issue.number),
        ))
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
                            Icon::empty()
                                .path("icons/message-square.svg")
                                .with_size(gpui_component::Size::Size(theme::scaled_px(14.))),
                        )
                        .child(issue.comment_count.to_string()),
                )
                .child(super::timeline_row::state_dot(state_label, state_color)),
        );
    let row = super::timeline_row::clickable(super::timeline_row::row(
        ("issue-main-row", number as usize),
        &issue.author,
        &app.avatars.images,
        content,
    ))
    .min_h(theme::scaled_px(104.))
    .on_click(select);
    super::e2e::measure_control(format!("issue-main-row-{number}"), row).into_any_element()
}

/// Composer and variable-height Issue rows share one scrolling viewport.
/// Appending splices the tail instead of resetting the current scroll anchor.
fn render_main_issue_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let ui = app.ui();
    let state = ui.github_issues_list.clone();
    let order = issue_indices(app, ui.github_issue_tab);
    let filtered_count = order.len();
    // Composer, shared strip, status, Issue rows, then the loading/retry tail.
    let count = filtered_count + 4;
    let old_count = state.item_count();
    if count > old_count {
        state.splice(old_count..old_count, count - old_count);
        state.remeasure_items(old_count.saturating_sub(1)..count);
        // Unmeasured rows otherwise contribute zero height, so wheel/scrollbar
        // movement stops at the few rows measured in the initial viewport.
        state
            .clone()
            .with_uniform_item_height(theme::scaled_px(104.));
    } else if count < old_count {
        state.splice(count..old_count, 0);
    }
    state.remeasure_items(0..3);
    state.remeasure_items(count - 1..count);
    let owner = app.active_session();
    let repo = app.repo_path.clone();
    let on_scroll = cx.processor(
        move |app: &mut KagiApp, visible: std::ops::Range<usize>, _, cx| {
            let (Some(owner), Some(repo)) = (owner, repo.as_ref()) else {
                return;
            };
            let Some(ui) = app.ui.get(&owner) else {
                return;
            };
            // Read the filter here, not from the closure's capture: a
            // predicate switched on after this list rendered must still
            // suppress the fetch this event would have started.
            if app.active_session() == Some(owner)
                && ui.github_issues_error.is_none()
                && !client_membership_active(ui)
                && filtered_count > 0
                && visible.end.saturating_sub(3) >= filtered_count
            {
                app.load_more_github_issues_for(owner, repo.clone(), cx);
            }
        },
    );
    state.set_scroll_handler(move |event, window, cx| {
        on_scroll(event.visible_range.clone(), window, cx);
    });
    let render = cx.processor(move |app: &mut KagiApp, index: usize, _, cx| {
        if app.active_session() != owner {
            return div().into_any_element();
        }
        match index {
            0 => super::issues_composer::render_composer(app, None, cx),
            1 => super::list_filter_strip::render_strip(
                app,
                super::list_filter_strip::ListKind::Issues,
                filtered_count,
                cx,
            ),
            2 => render_main_list_status(app, filtered_count, cx),
            index if index == count - 1 => render_issue_page_tail(app, filtered_count, cx),
            index => order
                .get(index - 3)
                .and_then(|&index| app.ui().github_issues.get(index))
                .map(|issue| render_main_issue_row(app, issue, cx))
                .unwrap_or_else(|| div().into_any_element()),
        }
    });
    let scrollbar = state.clone();
    div()
        .id("issue-main-list")
        .flex_1()
        .min_h(px(0.))
        .h_full()
        .w_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(super::e2e::measure_inside("issue-main-list"))
        .child(super::render_helpers::with_vertical_scrollbar(
            "issue-main-list-scroll",
            &scrollbar,
            gpui::list(state, move |index, window, cx| render(index, window, cx))
                .flex_1()
                .min_h(px(0.)),
            true,
        ))
        .into_any_element()
}

fn render_issue_page_tail(
    app: &KagiApp,
    filtered_count: usize,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let ui = app.ui();
    let mut tail = div().id("issue-main-page-tail").px_3().py_2().text_xs();
    if ui.github_issues_loading_more {
        tail = tail
            .flex()
            .items_center()
            .gap_1()
            .text_color(rgb(theme().text_muted))
            .child(super::render_overlay::sync_spinner(
                10.,
                theme().text_muted,
                "issue-main-page-spinner",
            ))
            .child(Msg::IssuesLoadingMore.t());
    } else if ui.github_issues_cursor.is_some() {
        if let (Some(owner), Some(repo)) = (app.active_session(), app.repo_path.clone()) {
            if let Some(error) = &ui.github_issues_error {
                tail = tail
                    .text_color(rgb(theme().color_blocker))
                    .child(div().whitespace_normal().child(safe_text(error)))
                    .child(
                        div()
                            .id("issue-main-page-retry")
                            .cursor_pointer()
                            .py_2()
                            .text_color(rgb(theme().text_main))
                            .child(Msg::IssuesRetryLoadMore.t())
                            .child(super::e2e::measure_inside("issue-main-page-retry"))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.load_more_github_issues_for(owner, repo.clone(), cx);
                            })),
                    );
            } else if !ui.github_issues_loading {
                if filtered_count == 0 || client_membership_active(ui) {
                    // A client-side predicate makes these rows a subset, so
                    // reaching their tail is not evidence that the next page
                    // is wanted. Offer the continuation instead of taking it.
                    tail = tail.child(
                        div()
                            .id("issue-filter-load-more")
                            .cursor_pointer()
                            .py_2()
                            .text_color(rgb(theme().text_main))
                            .child(Msg::ListLoadMore.t())
                            .child(super::e2e::measure_inside("issue-filter-load-more"))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if app.active_session() == Some(owner) {
                                    app.load_more_github_issues_for(owner, repo.clone(), cx);
                                }
                            })),
                    );
                } else {
                    // Layout may expose the tail without a wheel event (resize,
                    // scrollbar drag, or a short page). Ignore overdraw outside
                    // the actual clip and defer I/O to the event boundary.
                    let entity = cx.entity().downgrade();
                    let generation = ui.github_issues_gen;
                    let cursor = ui.github_issues_cursor.clone();
                    tail = tail.child(
                        gpui::canvas(
                            move |bounds, window, cx| {
                                if bounds.intersects(&window.content_mask().bounds) {
                                    let entity = entity.clone();
                                    let repo = repo.clone();
                                    let cursor = cursor.clone();
                                    cx.defer(move |cx| {
                                        let _ = entity.update(cx, |app, cx| {
                                            // The filter may have become active
                                            // between paint and this deferred
                                            // run; re-check it with the rest.
                                            if app.active_session() == Some(owner)
                                                && app.ui().github_issues_gen == generation
                                                && app.ui().github_issues_cursor == cursor
                                                && app.ui().github_issues_error.is_none()
                                                && !client_membership_active(app.ui())
                                                && app.ui().selected_github_issue.is_none()
                                                && app.workspace_mode()
                                                    == super::workspace_mode::WorkspaceMode::Issues
                                            {
                                                app.load_more_github_issues_for(owner, repo, cx);
                                            }
                                        });
                                    });
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .w_full()
                        .h(px(1.)),
                    );
                }
            }
        }
    }
    tail.into_any_element()
}

fn render_center(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let selected = app.ui().selected_github_issue;
    let editors = &app.ui().issue_composer.editors;
    let home_focused = editors.get(&None).is_some_and(|editor| editor.focused);
    let mut center = div()
        .id("issue-mode-center-pane")
        .relative()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .overflow_hidden()
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
            if home_focused {
                center = center.child(super::issues_composer::render_composer(app, None, cx));
            } else {
                center = center.child(render_main_issue_list(app, cx));
            }
        }
    }
    if selected.is_some() || home_focused {
        center.overflow_y_scrollbar().into_any_element()
    } else {
        center.into_any_element()
    }
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
            ui.github_issues_loading_more = false;
            ui.github_issues_cursor = None;
            ui.github_issues_error = None;
            ui.selected_github_issue = None;
            ui.github_issue_detail_loading = None;
            ui.github_issue_detail_error = None;
        });
        cx.notify();
    }
}

#[cfg(test)]
#[path = "issues_mode_tests.rs"]
mod tests;
