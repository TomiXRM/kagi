//! Composer-first GitHub Issues workspace (ADR-0198 / ADR-0201).
//!
//! Rendering consumes the session-owned Issue snapshot in `TabUiState`. Network
//! requests are started only by workspace-mode and row-selection handlers.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::{scroll::ScrollableElement, Icon, Sizable};
use kagi_domain::github::{Issue, IssueListTab, IssueState};
use kagi_domain::list_filter::{apply_issues, IssueFilter};
use std::{cell::Ref, rc::Rc};

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::tab_view::TabUiState;
use super::theme::{self, theme};
use super::workspace_mode::SectionCount;
use super::KagiApp;

mod center;
use center::render_center;
mod feed;
use feed::render_main_issue_list;

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

/// Disposable projection of one session's Issue evidence and filter intent.
pub(super) struct IssueViewCache {
    revision: (u64, u64, usize),
    filter: IssueFilter,
    tab: IssueListTab,
    login: Option<String>,
    mentions: Vec<u64>,
    pub(super) order: Rc<Vec<usize>>,
    row_keys: super::keyboard_nav::RowKeys,
    pub(super) tab_counts: [usize; 4],
    language: super::i18n::Lang,
}

#[cfg(any(test, feature = "gui-e2e"))]
thread_local! {
    static ISSUE_VIEW_RECOMPUTATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(any(test, feature = "gui-e2e"))]
pub(super) fn issue_view_recomputations() -> usize {
    ISSUE_VIEW_RECOMPUTATIONS.with(|count| count.get())
}

impl TabUiState {
    pub(super) fn invalidate_issue_view(&mut self) {
        self.github_issues_epoch = self.github_issues_epoch.wrapping_add(1);
    }

    pub(super) fn issue_view(&self, login: Option<&str>) -> Ref<'_, IssueViewCache> {
        let revision = (
            self.github_issues_epoch,
            self.github_issues_gen,
            self.github_issues.len(),
        );
        let stale = self.issue_view.borrow().as_ref().is_none_or(|view| {
            view.revision != revision
                || view.filter != self.github_issue_filter
                || view.tab != self.github_issue_tab
                || view.login.as_deref() != login
                || view.mentions != self.github_issue_mentions
                || view.language != super::i18n::lang()
        });
        if stale {
            let mut order = apply_issues(&self.github_issues, &self.github_issue_filter);
            #[cfg(any(test, feature = "gui-e2e"))]
            ISSUE_VIEW_RECOMPUTATIONS.with(|count| count.set(count.get() + 1));
            let mut tab_counts = [0; 4];
            order.retain(|&index| {
                let mut keep = false;
                for tab in IssueListTab::ALL {
                    let accepted = tab.accepts(
                        &self.github_issues[index],
                        login,
                        &self.github_issue_mentions,
                    );
                    tab_counts[tab.index()] += usize::from(accepted);
                    if tab == self.github_issue_tab {
                        keep = accepted;
                    }
                }
                keep
            });
            klog!(
                "issues: view recomputed loaded={} visible={}",
                self.github_issues.len(),
                order.len()
            );
            let row_keys: super::keyboard_nav::RowKeys = order
                .iter()
                .enumerate()
                .map(|(at, &index)| (self.github_issues[index].number.to_string(), at + 3))
                .collect();
            {
                let previous = self.issue_view.borrow();
                // Accepted pages only add new Issue numbers within a request
                // generation. Other publications can change offscreen metadata.
                let invalidate = previous.as_ref().is_some_and(|old| {
                    (old.revision != revision
                        && !(old.revision.1 == revision.1 && old.revision.2 < revision.2))
                        || old.language != super::i18n::lang()
                });
                feed::reconcile_feed(&self.github_issues_list, previous.as_ref(), &row_keys);
                if invalidate {
                    self.github_issues_list
                        .invalidate_rows(3..row_keys.len() + 3);
                }
            }
            *self.issue_view.borrow_mut() = Some(IssueViewCache {
                revision,
                filter: self.github_issue_filter.clone(),
                tab: self.github_issue_tab,
                login: login.map(str::to_owned),
                mentions: self.github_issue_mentions.clone(),
                row_keys,
                order: Rc::new(order),
                tab_counts,
                language: super::i18n::lang(),
            });
        }
        Ref::map(self.issue_view.borrow(), |view| {
            view.as_ref().expect("Issue view prepared")
        })
    }
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
    render_list(app, false, cx)
}

pub(super) fn render_compact_issue_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    render_list(app, true, cx)
}

fn render_list(app: &KagiApp, compact: bool, cx: &mut Context<KagiApp>) -> AnyElement {
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
            safe_text(&crate::ui::i18n::op::user_text::diagnostic(
                error.unwrap_or_default(),
            )),
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
                        .child(safe_text(&crate::ui::i18n::op::user_text::diagnostic(
                            message,
                        ))),
                );
            }
            // #906: "me" is the login on the Issues repository's host, not
            // the github.com account. Until it is read, the tabs that mean
            // "yours" claim neither a count nor an empty list.
            let viewer = app.issue_host_login();
            let (order, tab_counts) = {
                let view = ui.issue_view(viewer);
                (Rc::clone(&view.order), view.tab_counts)
            };
            for tab in IssueListTab::ALL {
                let active = tab == ui.github_issue_tab;
                let unknown = viewer.is_none() && issue_count > 0 && tab.needs_viewer();
                let header = super::workspace_mode::sidebar_section_header(
                    ("issue-filter-tab", tab.index()),
                    issue_tab_label(tab),
                    if unknown {
                        SectionCount::Unknown
                    } else {
                        SectionCount::Known {
                            n: tab_counts[tab.index()],
                            more: ui.github_issues_cursor.is_some(),
                        }
                    },
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
                if order.is_empty() {
                    let (id, message) = if issue_count == 0 {
                        ("issue-mode-list-empty", Msg::IssuesEmpty.t())
                    } else if unknown {
                        ("issue-filter-viewer-unknown", Msg::IssuesViewerUnknown.t())
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
                for &index in order.iter().take(100) {
                    let issue = &ui.github_issues[index];
                    let number = issue.number;
                    let active_row = selected == Some(number);
                    let select = cx.listener(
                        move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                            this.load_github_issue_detail(number, window, cx);
                        },
                    );
                    let age = super::timeline_row::age(&issue.updated_at);
                    let on_accent = theme().selection_uses_on_accent(active_row, app.window_active);
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
                                .text_color(if on_accent {
                                    theme().selection_foreground(app.window_active)
                                } else {
                                    rgb(theme().text_main).into()
                                })
                                .child(safe_text(&issue.title)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .w_full()
                                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                                .line_height(theme::scaled_px(13.))
                                .text_color(if on_accent {
                                    theme().selection_secondary_foreground(app.window_active)
                                } else {
                                    rgb(theme().text_muted).into()
                                })
                                .when(cfg!(feature = "gui-e2e"), |label| {
                                    label.child(super::e2e::measure_inside(format!(
                                        "issue-secondary-{number}"
                                    )))
                                })
                                .child(
                                    div()
                                        .flex_shrink(1.)
                                        .min_w(px(0.))
                                        // Expand only the clip, not the metadata line's layout slot.
                                        .py(theme::scaled_px(2.))
                                        .my(theme::scaled_px(-2.))
                                        .truncate()
                                        .child(safe_text(&format!(
                                            "{age} · {}",
                                            super::i18n::issue_comments(issue.comment_count)
                                        )))
                                        .when(cfg!(feature = "gui-e2e"), |label| {
                                            label.child(super::e2e::measure_inside(format!(
                                                "issue-activity-{number}"
                                            )))
                                        }),
                                )
                                .child(div().flex_1().min_w(px(0.)))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .child(SharedString::from(format!("#{}", issue.number)))
                                        .when(cfg!(feature = "gui-e2e"), |label| {
                                            label.child(super::e2e::measure_inside(format!(
                                                "issue-number-{number}"
                                            )))
                                        }),
                                ),
                        );
                    let row =
                        super::workspace_mode::sidebar_list_row(active_row, app.window_active)
                            .id(("issue-mode-card", number as usize))
                            .on_click(select)
                            .child(content);
                    let row = if compact {
                        super::compact_sidebar::Leaf {
                            id: ("compact-issue-row", number as usize).into(),
                            label: format!("#{number} {}", safe_text(&issue.title)),
                            row,
                            app: cx.weak_entity(),
                        }
                        .into_any_element()
                    } else {
                        row.into_any_element()
                    };
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
            safe_text(&crate::ui::i18n::op::user_text::diagnostic(
                error.unwrap_or_default(),
            )),
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
                        .child(safe_text(&crate::ui::i18n::op::user_text::diagnostic(
                            message,
                        ))),
                );
            }
            if filtered_count == 0 {
                // #906: a "yours" tab with an unknown host login is not empty.
                let unknown = ui.github_issue_tab.needs_viewer()
                    && !ui.github_issues.is_empty()
                    && app.issue_host_login().is_none();
                list = list.child(if unknown {
                    status_text(
                        "issue-main-list-viewer-unknown",
                        Msg::IssuesViewerUnknown.t(),
                        theme().text_muted,
                    )
                } else {
                    status_text(
                        "issue-main-list-empty",
                        Msg::IssuesFilterEmpty.t(),
                        theme().text_muted,
                    )
                });
            }

            list
        }
    };

    list.into_any_element()
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
                    .child(div().whitespace_normal().child(safe_text(
                        &crate::ui::i18n::op::user_text::diagnostic(error),
                    )))
                    .child(super::list_filter_strip::page_button(
                        "issue-main-page-retry",
                        Msg::IssuesRetryLoadMore.t(),
                        cx,
                        move |app, _, _, cx| {
                            app.load_more_github_issues_for(owner, repo.clone(), cx);
                        },
                    ));
            } else if !ui.github_issues_loading {
                if filtered_count == 0 || client_membership_active(ui) {
                    // A client-side predicate makes these rows a subset, so
                    // reaching their tail is not evidence that the next page
                    // is wanted. Offer the continuation instead of taking it.
                    tail = tail.child(super::list_filter_strip::page_button(
                        "issue-filter-load-more",
                        Msg::ListLoadMore.t(),
                        cx,
                        move |app, _, _, cx| {
                            if app.active_session() == Some(owner) {
                                app.load_more_github_issues_for(owner, repo.clone(), cx);
                            }
                        },
                    ));
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

/// Render the Composer/Thread main workspace; the navigator is the shell's
/// sidebar (#1790).
///
/// `panel` is the window's bottom panel. It sits under the main workspace
/// only, so the navigator keeps the full body height.
pub fn render_issues_mode(
    app: &mut KagiApp,
    panel: Option<AnyElement>,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    app.ensure_issue_avatars(cx);
    let center = render_center(app, window, cx);
    let center = match panel {
        None => center,
        Some(panel) => div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .child(center)
            .child(panel)
            .into_any_element(),
    };
    div()
        .id("issue-mode-layout")
        .flex()
        .flex_row()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .bg(rgb(theme().bg_base))
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
