//! Read-only GitHub Issues workspace (ADR-0198).
//!
//! Rendering consumes the session-owned Issue snapshot in `TabUiState`. Network
//! requests are started only by workspace-mode and row-selection handlers.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::scroll::ScrollableElement;
use kagi_domain::github::{Issue, IssueState};

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

fn render_issue_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let ui = app.ui();
    let issue_count = ui.github_issues.len().min(ISSUE_LIST_LIMIT);
    let selected = ui.selected_github_issue;
    let loading = ui.github_issues_loading;
    let loaded = ui.github_issues_loaded;
    let error = ui.github_issues_error.as_deref();
    let presentation = list_presentation(loading, loaded, error.is_some(), issue_count);
    let swipe = cx.listener(KagiApp::sidebar_scroll);

    let mut body = div()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scrollbar()
        .flex()
        .flex_col();

    body = match presentation {
        IssueListPresentation::Loading => body.child(status_text(
            "issue-mode-list-loading",
            "Loading issues…",
            theme().text_muted,
        )),
        IssueListPresentation::Error => body.child(status_text(
            "issue-mode-list-error",
            safe_text(error.unwrap_or_default()),
            theme().color_blocker,
        )),
        IssueListPresentation::Empty => body.child(status_text(
            "issue-mode-list-empty",
            "No open issues",
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
                        .child("Refreshing issues…"),
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
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar))
        .on_scroll_wheel(swipe)
        .child(super::workspace_mode::render_sidebar_mode_nav(
            super::workspace_mode::WorkspaceMode::Issues,
            cx,
        ))
        .child(body)
        .into_any_element()
}

fn issue_state_text(state: IssueState) -> &'static str {
    match state {
        IssueState::Open => "Open",
        IssueState::Closed => "Closed",
        IssueState::Unknown => "Unknown",
    }
}

fn selected_issue(app: &KagiApp) -> Option<&Issue> {
    let ui = app.ui();
    let number = ui.selected_github_issue?;
    ui.github_issue_details
        .get(&number)
        .or_else(|| ui.github_issues.iter().find(|issue| issue.number == number))
}

fn render_center(app: &KagiApp) -> AnyElement {
    let ui = app.ui();
    let selected = ui.selected_github_issue;
    let loading = selected.is_some() && ui.github_issue_detail_loading == selected;
    let error = selected.and(ui.github_issue_detail_error.as_deref());
    let detail = selected.and_then(|number| ui.github_issue_details.get(&number));

    let mut center = div()
        .id("issue-mode-center-pane")
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .overflow_y_scrollbar()
        .flex()
        .flex_col()
        .bg(rgb(theme().bg_base));

    if selected.is_none() {
        return center
            .child(status_text(
                "issue-mode-detail-empty",
                "Select an issue",
                theme().text_muted,
            ))
            .into_any_element();
    }
    if loading {
        return center
            .child(status_text(
                "issue-mode-detail-loading",
                "Loading issue…",
                theme().text_muted,
            ))
            .into_any_element();
    }
    if let Some(message) = error {
        return center
            .child(status_text(
                "issue-mode-detail-error",
                safe_text(message),
                theme().color_blocker,
            ))
            .into_any_element();
    }
    let Some(issue) = detail else {
        return center
            .child(status_text(
                "issue-mode-detail-empty",
                "Issue details unavailable",
                theme().text_muted,
            ))
            .into_any_element();
    };

    center = center
        .p_4()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(theme().text_main))
                        .whitespace_normal()
                        .child(safe_text(&issue.title)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(safe_text(&format!(
                            "#{} · {} · @{}",
                            issue.number,
                            issue_state_text(issue.state),
                            issue.author
                        ))),
                ),
        )
        .child(
            div()
                .p_3()
                .rounded_md()
                .bg(rgb(theme().panel))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .whitespace_normal()
                .child(if issue.body.is_empty() {
                    SharedString::from("No description provided.")
                } else {
                    safe_text(&issue.body)
                }),
        );

    if !issue.comments.is_empty() {
        center = center.child(
            div()
                .pt_2()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(format!(
                    "Comments ({})",
                    issue.comments.len()
                ))),
        );
        for comment in &issue.comments {
            center =
                center.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(rgb(theme().panel))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_xs().text_color(rgb(theme().text_muted)).child(
                            safe_text(&format!("@{} · {}", comment.author, comment.created_at)),
                        ))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(theme().text_main))
                                .whitespace_normal()
                                .child(safe_text(&comment.body)),
                        ),
                );
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
                .child("None")
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
                    .child("Issue metadata"),
            )
            .into_any_element();
    };

    rail = rail
        .child(metadata_group(
            "State",
            vec![SharedString::from(issue_state_text(issue.state))],
        ))
        .child(metadata_group("Author", vec![safe_text(&issue.author)]))
        .child(metadata_group(
            "Labels",
            issue
                .labels
                .iter()
                .map(|label| safe_text(&label.name))
                .collect(),
        ))
        .child(metadata_group(
            "Assignees",
            issue.assignees.iter().map(|name| safe_text(name)).collect(),
        ))
        .child(metadata_group(
            "Created",
            vec![safe_text(&issue.created_at)],
        ))
        .child(metadata_group(
            "Updated",
            vec![safe_text(&issue.updated_at)],
        ));
    rail.into_any_element()
}

/// Render the three-column, read-only Issues workspace.
pub fn render_issues_mode(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> AnyElement {
    let left = div()
        .id("issue-mode-left-pane")
        .w(theme::scaled_px(app.sidebar.width))
        .flex_shrink_0()
        .h_full()
        .child(super::e2e::measure_control(
            "issue-mode-left-pane",
            render_issue_list(app, cx),
        ));
    let center = super::e2e::measure_control("issue-mode-center-pane", render_center(app));
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
