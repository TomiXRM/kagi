//! Main feed keyboard rows share the existing Composer/paging viewport.
use super::*;
use super::{render_issue_page_tail, render_main_list_status};
use crate::ui::{
    e2e, issues_composer, keyboard_nav, list_a11y, list_filter_strip, render_helpers, timeline_row,
};
use kagi_ui_core::variable_list::{variable_list, VariableListState};
fn render_main_issue_row(
    app: &KagiApp,
    issue: &Issue,
    rows: &keyboard_nav::RowList,
    at: usize,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let number = issue.number;
    let owner = app.active_session();
    let repo = app.repo_path.clone();
    let select = cx.listener(
        move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
            if this.active_session() != owner
                || this.repo_path != repo
                || this.home_in_front()
                || this.workspace_mode() != crate::ui::workspace_mode::WorkspaceMode::Issues
                || !this
                    .ui()
                    .issue_view(this.issue_host_login())
                    .order
                    .iter()
                    .any(|&index| this.ui().github_issues[index].number == number)
            {
                return;
            }
            this.load_github_issue_detail(number, window, cx);
        },
    );
    let (state_label, state_color) = match issue.state {
        IssueState::Open => (Msg::IssueStateOpen.t(), theme().color_success),
        IssueState::Closed => (Msg::IssueStateClosed.t(), theme().text_muted),
        IssueState::Unknown => (Msg::IssueStateUnknown.t(), theme().text_muted),
    };
    let content = issue_content(issue, state_label, state_color);
    let row = timeline_row::clickable(timeline_row::row(
        ("issue-main-row", number as usize),
        &issue.author,
        app.issue_repo_host(),
        &app.avatars.images,
        content,
    ))
    .min_h(theme::scaled_px(104.))
    .on_click(select);
    let name = crate::ui::i18n::issue_row_name(number, &safe_text(&issue.title), state_label);
    let row = rows
        .row(
            at,
            list_a11y::list_item("issue-keyboard-list", row, at, rows.len(), name),
        )
        .key_context("KagiRowList");
    e2e::measure_control(format!("issue-main-row-{number}"), row).into_any_element()
}

/// Composer and variable-height Issue rows share one scrolling viewport.
/// Appending splices the tail instead of resetting the current scroll anchor.
pub(super) fn render_main_issue_list(
    app: &KagiApp,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let ui = app.ui();
    let state = ui.github_issues_list.clone();
    let order = Rc::clone(&ui.issue_view(app.issue_host_login()).order);
    let filtered_count = order.len();
    // Composer, shared strip, status, Issue rows, then the loading/retry tail.
    let count = filtered_count + 4;
    if state.item_count() != count {
        reconcile_feed(
            &state,
            None,
            &ui.issue_view(app.issue_host_login()).row_keys,
        );
    }
    state.set_estimate(theme::scaled_px(104.));
    state.invalidate_rows(0..3);
    state.invalidate_rows(count - 1..count);
    let keys = ui.issue_view(app.issue_host_login()).row_keys.clone();
    let rows = ui.issue_rows.rows(
        keys.iter().map(|(key, index)| (key.as_str(), *index)),
        &crate::ui::keyboard_nav::RowScroll::Owned(state.clone()),
        app.root_focus.as_ref(),
        window,
        cx,
    );
    let owner = app.active_session();
    let render = cx.processor(move |app: &mut KagiApp, index: usize, window, cx| {
        if app.active_session() != owner {
            return div().into_any_element();
        }
        match index {
            0 => issues_composer::render_composer(app, None, window, cx),
            1 if crate::ui::compact_sidebar::owns_list_filter(window, cx) => {
                // The navigator is the sole projection of these shared Input
                // and chip focus owners until its Popover is dismissed.
                div().into_any_element()
            }
            1 => list_filter_strip::render_strip(
                app,
                list_filter_strip::ListKind::Issues,
                filtered_count,
                cx,
            ),
            2 => render_main_list_status(app, filtered_count, cx),
            index if index == count - 1 => render_issue_page_tail(app, filtered_count, cx),
            index => order
                .get(index - 3)
                .and_then(|&index| app.ui().github_issues.get(index))
                .map(|issue| render_main_issue_row(app, issue, &rows, index - 3, cx))
                .unwrap_or_else(|| div().into_any_element()),
        }
    });
    let scrollbar = state.clone();
    list_a11y::plain_list(
        "issue-keyboard-list",
        div().id("issue-main-list"),
        Msg::WorkspaceIssues.t(),
    )
    .flex_1()
    .min_h(px(0.))
    .h_full()
    .w_full()
    .overflow_hidden()
    .flex()
    .flex_col()
    .child(e2e::measure_inside("issue-main-list"))
    .child(render_helpers::with_vertical_scrollbar(
        "issue-main-list-scroll",
        &scrollbar,
        variable_list(state, move |index, window, cx| render(index, window, cx))
            .flex_1()
            .min_h(px(0.)),
        true,
    ))
    .into_any_element()
}

fn issue_content(issue: &Issue, state_label: &'static str, state_color: u32) -> gpui::Div {
    let age = timeline_row::age(&issue.updated_at);
    timeline_row::content_column()
        .gap_1()
        .child(timeline_row::meta(
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
                .child(timeline_row::state_dot(state_label, state_color)),
        )
}

/// Reconcile only on projection publication; unchanged paints do no key scans.
/// The cache retains the previous projection until this handoff, even after its
/// source epoch is invalidated.
pub(super) fn reconcile_feed(
    state: &VariableListState,
    previous: Option<&IssueViewCache>,
    keys: &crate::ui::keyboard_nav::RowKeys,
) {
    let previous = previous.filter(|old| state.item_count() == old.row_keys.len() + 4);
    let old_slots: std::collections::HashMap<_, _> = previous
        .into_iter()
        .flat_map(|old| {
            old.row_keys
                .iter()
                .map(|(key, index)| (key.as_str(), *index))
        })
        .collect();
    let mut retained = Vec::with_capacity(keys.len() + 4);
    retained.extend((0..3).map(|index| previous.map(|_| index)));
    retained.extend(
        keys.iter()
            .map(|(key, _)| old_slots.get(key.as_str()).copied()),
    );
    retained.push(previous.map(|old| old.row_keys.len() + 3));
    state.reconcile_rows(&retained);
}
