//! Composer-first GitHub Issues workspace (ADR-0198 / ADR-0201).
//!
//! Rendering consumes the session-owned Issue snapshot in `TabUiState`. Network
//! requests are started only by workspace-mode and row-selection handlers.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::scroll::ScrollableElement;
use kagi_domain::github::{Issue, IssueState};

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

const ISSUE_LIST_LIMIT: usize = 100;
const ISSUE_META_W: f32 = 240.0;

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
        IssueListPresentation::Empty => body.child(status_text(
            "issue-mode-list-empty",
            Msg::IssuesEmpty.t(),
            theme().text_muted,
        )),
        IssueListPresentation::Issues => {
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
            for issue in ui.github_issues.iter().take(ISSUE_LIST_LIMIT) {
                let number = issue.number;
                let active = selected == Some(number);
                let select = cx.listener(
                    move |this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
                        this.load_github_issue_detail(number, cx);
                    },
                );
                body =
                    body.child(
                        div()
                            .id(("issue-row", number as usize))
                            .px_3()
                            .py_2()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .cursor_pointer()
                            .when(active, |el| el.bg(rgb(theme().selected)))
                            .when(!active, |el| el.hover(|st| st.bg(rgb(theme().surface))))
                            .on_click(select)
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(theme().text_main))
                                    .whitespace_normal()
                                    .child(safe_text(&issue.title)),
                            )
                            .child(div().text_xs().text_color(rgb(theme().text_muted)).child(
                                safe_text(&format!("#{} · @{}", issue.number, issue.author)),
                            )),
                    );
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

fn issue_state_text(state: IssueState) -> &'static str {
    match state {
        IssueState::Open => Msg::IssueStateOpen.t(),
        IssueState::Closed => Msg::IssueStateClosed.t(),
        IssueState::Unknown => Msg::IssueStateUnknown.t(),
    }
}

fn selected_issue(app: &KagiApp) -> Option<&Issue> {
    let ui = app.ui();
    let number = ui.selected_github_issue?;
    ui.github_issue_details
        .get(&number)
        .or_else(|| ui.github_issues.iter().find(|issue| issue.number == number))
}

fn render_center(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let selected = app.ui().selected_github_issue;
    let editors = &app.ui().issue_composer.editors;
    let focused = if editors.get(&None).is_some_and(|editor| editor.focused) {
        Some(None)
    } else {
        selected.and_then(|number| {
            editors
                .get(&Some(number))
                .is_some_and(|editor| editor.focused)
                .then_some(Some(number))
        })
    };
    let mut center = div()
        .id("issue-mode-center-pane")
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .overflow_y_scrollbar()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .bg(rgb(theme().bg_base));
    if let Some(number) = focused {
        center = center.child(super::issues_composer::render_composer(app, number, cx));
    } else {
        center = center.child(super::issues_composer::render_composer(app, None, cx));
        if let Some(number) = selected {
            center = center
                .child(super::issues_thread::render_thread(app, cx))
                .child(super::issues_composer::render_composer(
                    app,
                    Some(number),
                    cx,
                ));
        } else {
            center = center.child(render_issue_list(app, cx));
        }
    }
    center.into_any_element()
}

fn metadata_group(title: &'static str, values: Vec<SharedString>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_label))
                .child(title),
        )
        .child(if values.is_empty() {
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(Msg::IssuesNone.t())
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(values.into_iter().map(|value| {
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .whitespace_normal()
                        .child(value)
                }))
                .into_any_element()
        })
        .into_any_element()
}

fn render_metadata(app: &KagiApp) -> AnyElement {
    let issue = selected_issue(app);
    let mut rail = div()
        .id("issue-mode-right-pane")
        .w(theme::scaled_px(ISSUE_META_W))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scrollbar()
        .p_3()
        .flex()
        .flex_col()
        .gap_4()
        .bg(rgb(theme().panel));
    let Some(issue) = issue else {
        return rail
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(Msg::IssueMetadata.t()),
            )
            .into_any_element();
    };

    rail = rail
        .child(metadata_group(
            Msg::IssueFieldState.t(),
            vec![SharedString::from(issue_state_text(issue.state))],
        ))
        .child(metadata_group(
            Msg::IssueFieldAuthor.t(),
            vec![safe_text(&issue.author)],
        ))
        .child(metadata_group(
            Msg::IssueFieldLabels.t(),
            issue
                .labels
                .iter()
                .map(|label| safe_text(&label.name))
                .collect(),
        ))
        .child(metadata_group(
            Msg::IssueFieldAssignees.t(),
            issue.assignees.iter().map(|name| safe_text(name)).collect(),
        ))
        .child(metadata_group(
            Msg::IssueFieldCreated.t(),
            vec![safe_text(&issue.created_at)],
        ))
        .child(metadata_group(
            Msg::IssueFieldUpdated.t(),
            vec![safe_text(&issue.updated_at)],
        ));
    rail.into_any_element()
}

/// Render the three-column, read-only Issues workspace.
pub fn render_issues_mode(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let left = super::e2e::measure_control(
        "issue-mode-left-pane",
        super::workspace_mode::render_sidebar_pages(
            app,
            super::workspace_mode::WorkspaceMode::Issues,
            cx,
        ),
    );
    let center = super::e2e::measure_control("issue-mode-center-pane", render_center(app, cx));
    let right = super::e2e::measure_control("issue-mode-right-pane", render_metadata(app));

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
        .child(divider())
        .child(right)
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
