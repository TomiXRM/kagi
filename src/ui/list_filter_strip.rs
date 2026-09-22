//! Shared GitHub list controls. Intent and input resources belong to TabUiState;
//! filtering stays in kagi-domain and reads stay in the existing orchestrators.
use std::collections::BTreeSet;

use gpui::{
    div, prelude::*, rgb, AnyElement, Context, Entity, Pixels, Point, SharedString, Subscription,
    Window,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    Sizable,
};
use kagi_domain::list_filter::{
    ChecksFilter, DraftFilter, ListFilter, ListSort, SortDirection, SortField, StateFilter,
};

use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    i18n::Msg,
    render_helpers::safe_text,
    theme::{self, theme},
    workspace_mode::WorkspaceMode,
    KagiApp,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ListKind {
    Prs,
    Issues,
}

impl ListKind {
    fn index(self) -> usize {
        match self {
            Self::Prs => 0,
            Self::Issues => 1,
        }
    }
    pub(super) fn current(app: &KagiApp) -> Option<Self> {
        match app.workspace_mode() {
            WorkspaceMode::Prs if app.pr_mode().is_some_and(|mode| mode.active.is_none()) => {
                Some(Self::Prs)
            }
            WorkspaceMode::Issues if app.ui().selected_github_issue.is_none() => Some(Self::Issues),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum Field {
    State,
    Labels,
    Author,
    Draft,
    Checks,
    Sort,
}

#[derive(Clone)]
pub(super) struct FilterMenu {
    kind: ListKind,
    field: Field,
    position: Point<Pixels>,
}

#[derive(Default)]
pub(super) struct FilterControls {
    inputs: [Option<Entity<InputState>>; 2],
    subscriptions: [Option<Subscription>; 2],
    placeholder_lang: Option<super::i18n::Lang>,
    pub menu: Option<FilterMenu>,
}

fn common(app: &KagiApp, kind: ListKind) -> &ListFilter {
    match kind {
        ListKind::Prs => &app.ui().github_pr_filter.common,
        ListKind::Issues => &app.ui().github_issue_filter.common,
    }
}

fn sort(app: &KagiApp, kind: ListKind) -> ListSort {
    match kind {
        ListKind::Prs => app.ui().github_pr_filter.sort,
        ListKind::Issues => app.ui().github_issue_filter.sort,
    }
}

impl KagiApp {
    pub(super) fn sync_list_filter_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(owner), Some(kind)) = (self.active_session(), ListKind::current(self)) else {
            return;
        };
        let lang = super::i18n::lang();
        if self.ui().filter_controls.placeholder_lang != Some(lang) {
            for input in self.ui().filter_controls.inputs.iter().flatten() {
                input.update(cx, |input, cx| {
                    input.set_placeholder(Msg::ListFilterTitle.t(), window, cx)
                });
            }
            self.with_ui(|ui| ui.filter_controls.placeholder_lang = Some(lang));
        }
        if self.ui().filter_controls.inputs[kind.index()].is_some() {
            return;
        }
        let text = common(self, kind).text.clone();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(Msg::ListFilterTitle.t()));
        input.update(cx, |input, cx| input.set_value(text, window, cx));
        let subscription = cx.subscribe(&input, move |app, input, event, cx| {
            if !matches!(event, InputEvent::Change)
                || app.active_session() != Some(owner)
                || ListKind::current(app) != Some(kind)
            {
                return;
            }
            let text = input.read(cx).value().to_string();
            match kind {
                ListKind::Prs => {
                    let mut filter = app.ui().github_pr_filter.clone();
                    filter.common.text = text;
                    app.set_github_pr_filter(filter, cx);
                }
                ListKind::Issues => {
                    let mut filter = app.ui().github_issue_filter.clone();
                    filter.common.text = text;
                    app.set_github_issue_filter(filter, cx);
                }
            }
        });
        self.with_ui(|ui| {
            ui.filter_controls.inputs[kind.index()] = Some(input);
            ui.filter_controls.subscriptions[kind.index()] = Some(subscription);
        });
    }
}

// Extracted from pr_dashboard's filter/sort chips: one shared chrome.
fn chip(id: &'static str, active: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .px_2()
        .py_px()
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme().selected))
        .when(active, |el| el.bg(rgb(theme().selected)))
        .text_xs()
        .text_color(rgb(if active {
            theme().text_main
        } else {
            theme().text_sub
        }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme().surface)))
}

fn menu_chip(
    app: &KagiApp,
    kind: ListKind,
    field: Field,
    id: &'static str,
    label: String,
    active: bool,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let owner = app.active_session();
    super::e2e::measure_control(
        id,
        chip(id, active)
            .child(safe_text(&label))
            .on_click(
                cx.listener(move |app, event: &gpui::ClickEvent, window, cx| {
                    if app.active_session() != owner {
                        return;
                    }
                    if let Some(focus) = &app.root_focus {
                        window.focus(focus, cx);
                    }
                    app.with_ui(|ui| {
                        ui.filter_controls.menu = Some(FilterMenu {
                            kind,
                            field,
                            position: event.position(),
                        })
                    });
                    cx.notify();
                }),
            ),
    )
}

pub(super) fn render_strip(
    app: &KagiApp,
    kind: ListKind,
    count: usize,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let filter = common(app, kind);
    let order = sort(app, kind);
    let more = kind == ListKind::Issues && app.ui().github_issues_cursor.is_some() && count > 0;
    let count = if more {
        format!("({count}+)")
    } else {
        count.to_string()
    };
    let mut strip = div()
        .id("list-filter-strip")
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .child(SharedString::from(match kind {
                    ListKind::Prs => Msg::PrPaneTitle.t(),
                    ListKind::Issues => Msg::WorkspaceIssues.t(),
                })),
        )
        .child(super::e2e::measure_control(
            "list-filter-count",
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(count),
        ))
        .child(menu_chip(
            app,
            kind,
            Field::State,
            "list-filter-state",
            format!("{}: {}", Msg::PrColState.t(), state_label(filter.state)),
            filter.state != StateFilter::All,
            cx,
        ))
        .child(menu_chip(
            app,
            kind,
            Field::Labels,
            "list-filter-label",
            format!(
                "{}: {}",
                Msg::PrRailLabels.t(),
                if filter.labels.is_empty() {
                    Msg::ListFilterAll.t().to_string()
                } else {
                    filter.labels.join(", ")
                }
            ),
            !filter.labels.is_empty(),
            cx,
        ))
        .child(menu_chip(
            app,
            kind,
            Field::Author,
            "list-filter-author",
            format!(
                "{}: {}",
                Msg::PrColAuthor.t(),
                filter.author.as_deref().unwrap_or(Msg::ListFilterAll.t())
            ),
            filter.author.is_some(),
            cx,
        ));
    if kind == ListKind::Prs {
        let filter = &app.ui().github_pr_filter;
        strip = strip
            .child(menu_chip(
                app,
                kind,
                Field::Draft,
                "list-filter-draft",
                format!("{}: {}", Msg::PrHomeDraft.t(), draft_label(filter.draft)),
                filter.draft != DraftFilter::All,
                cx,
            ))
            .child(menu_chip(
                app,
                kind,
                Field::Checks,
                "list-filter-checks",
                format!("{}: {}", Msg::PrColChecks.t(), checks_label(filter.checks)),
                filter.checks != ChecksFilter::All,
                cx,
            ));
    }
    if let Some(input) = &app.ui().filter_controls.inputs[kind.index()] {
        strip = strip.child(super::e2e::measure_control(
            "list-filter-text",
            div()
                .w(theme::scaled_px(180.))
                .child(Input::new(input).small()),
        ));
    }
    let owner = app.active_session();
    strip = strip
        .child(menu_chip(
            app,
            kind,
            Field::Sort,
            "list-filter-sort",
            sort_label(order.field).to_string(),
            false,
            cx,
        ))
        .child(super::e2e::measure_control(
            "list-filter-direction",
            chip("list-filter-direction", false)
                .child(SharedString::from(match order.direction {
                    SortDirection::Ascending => Msg::ListSortAscending.t(),
                    SortDirection::Descending => Msg::ListSortDescending.t(),
                }))
                .on_click(cx.listener(move |app, _, _, cx| {
                    if app.active_session() == owner {
                        apply_choice(app, kind, Choice::Direction, cx);
                    }
                })),
        ))
        .child(super::e2e::measure_control(
            "list-filter-clear",
            chip("list-filter-clear", false)
                .child(SharedString::from(Msg::ListFilterClear.t()))
                .on_click(cx.listener(move |app, _, window, cx| {
                    if app.active_session() != owner {
                        return;
                    }
                    match kind {
                        ListKind::Prs => {
                            app.set_github_pr_filter(super::tab_view::default_pr_filter(), cx)
                        }
                        ListKind::Issues => {
                            app.set_github_issue_filter(super::tab_view::default_issue_filter(), cx)
                        }
                    }
                    if let Some(input) = app.ui().filter_controls.inputs[kind.index()].clone() {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                })),
        ))
        .child(
            chip("list-filter-refresh", false)
                .child(SharedString::from(Msg::PrRefresh.t()))
                .on_click(cx.listener(move |app, _, _, cx| {
                    if app.active_session() != owner {
                        return;
                    }
                    match kind {
                        ListKind::Prs => app.refresh_github_prs(cx),
                        ListKind::Issues => app.refresh_github_issues(cx),
                    }
                })),
        );
    strip.into_any_element()
}

#[derive(Clone)]
enum Choice {
    State(StateFilter),
    Label(Option<String>),
    Author(Option<String>),
    Draft(DraftFilter),
    Checks(ChecksFilter),
    Sort(SortField),
    Direction,
}

fn apply_choice(app: &mut KagiApp, kind: ListKind, choice: Choice, cx: &mut Context<KagiApp>) {
    match kind {
        ListKind::Prs => {
            let mut filter = app.ui().github_pr_filter.clone();
            match choice {
                Choice::Draft(value) => filter.draft = value,
                Choice::Checks(value) => filter.checks = value,
                choice => apply_common_choice(choice, &mut filter.common, &mut filter.sort),
            }
            app.set_github_pr_filter(filter, cx);
        }
        ListKind::Issues => {
            let mut filter = app.ui().github_issue_filter.clone();
            apply_common_choice(choice, &mut filter.common, &mut filter.sort);
            app.set_github_issue_filter(filter, cx);
        }
    }
}

fn apply_common_choice(choice: Choice, common: &mut ListFilter, order: &mut ListSort) {
    match choice {
        Choice::State(value) => common.state = value,
        Choice::Label(None) => common.labels.clear(),
        Choice::Label(Some(value)) => {
            if let Some(index) = common.labels.iter().position(|label| label == &value) {
                common.labels.remove(index);
            } else {
                common.labels.push(value);
            }
        }
        Choice::Author(value) => common.author = value,
        Choice::Draft(_) | Choice::Checks(_) => {
            unreachable!("PR-only choices are handled by the PR filter")
        }
        Choice::Sort(value) => order.field = value,
        Choice::Direction => {
            order.direction = match order.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            }
        }
    }
}

fn item(choice: Choice, label: &str, selected: bool) -> MenuItem<Choice> {
    MenuItem {
        action: choice,
        label: safe_text(&format!("{}{}", if selected { "[x] " } else { "" }, label)),
        state: ItemState::Enabled,
        dangerous: false,
    }
}

fn candidates(app: &KagiApp, kind: ListKind, field: Field) -> BTreeSet<&str> {
    let mut values = BTreeSet::new();
    match kind {
        ListKind::Issues => {
            for row in &app.ui().github_issues {
                if matches!(field, Field::Labels) {
                    values.extend(row.labels.iter().map(|label| label.name.as_str()));
                } else {
                    values.insert(row.author.as_str());
                }
            }
        }
        ListKind::Prs => {
            for row in &app.ui().github_prs {
                if matches!(field, Field::Labels) {
                    values.extend(row.labels.iter().map(|label| label.name.as_str()));
                } else {
                    values.insert(row.author.as_str());
                }
            }
        }
    }
    let filter = common(app, kind);
    if matches!(field, Field::Labels) {
        values.extend(filter.labels.iter().map(String::as_str));
    } else {
        values.extend(filter.author.as_deref());
    }
    values.remove("");
    values
}

pub(super) fn render_menu(
    app: &KagiApp,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> Option<AnyElement> {
    let menu = app.ui().filter_controls.menu.as_ref()?;
    if ListKind::current(app) != Some(menu.kind) {
        return None;
    }
    let kind = menu.kind;
    let owner = app.active_session();
    let filter = common(app, kind);
    let (title, items) = match menu.field {
        Field::State => (
            Msg::PrColState.t(),
            [StateFilter::Open, StateFilter::Closed, StateFilter::All]
                .into_iter()
                .map(|value| {
                    item(
                        Choice::State(value),
                        state_label(value),
                        filter.state == value,
                    )
                })
                .collect(),
        ),
        Field::Labels => {
            let mut items = vec![item(
                Choice::Label(None),
                Msg::ListFilterAll.t(),
                filter.labels.is_empty(),
            )];
            for value in candidates(app, kind, Field::Labels) {
                items.push(item(
                    Choice::Label(Some(value.to_owned())),
                    value,
                    filter.labels.iter().any(|label| label == value),
                ));
            }
            (Msg::PrRailLabels.t(), items)
        }
        Field::Author => {
            let mut items = vec![item(
                Choice::Author(None),
                Msg::ListFilterAll.t(),
                filter.author.is_none(),
            )];
            for value in candidates(app, kind, Field::Author) {
                items.push(item(
                    Choice::Author(Some(value.to_owned())),
                    value,
                    filter.author.as_deref() == Some(value),
                ));
            }
            (Msg::PrColAuthor.t(), items)
        }
        Field::Draft => (
            Msg::PrHomeDraft.t(),
            [DraftFilter::All, DraftFilter::Ready, DraftFilter::Draft]
                .into_iter()
                .map(|value| {
                    item(
                        Choice::Draft(value),
                        draft_label(value),
                        app.ui().github_pr_filter.draft == value,
                    )
                })
                .collect(),
        ),
        Field::Checks => (
            Msg::PrColChecks.t(),
            [
                ChecksFilter::All,
                ChecksFilter::Passing,
                ChecksFilter::Failing,
                ChecksFilter::Pending,
            ]
            .into_iter()
            .map(|value| {
                item(
                    Choice::Checks(value),
                    checks_label(value),
                    app.ui().github_pr_filter.checks == value,
                )
            })
            .collect(),
        ),
        Field::Sort => (
            Msg::ListFilterSort.t(),
            [
                SortField::Updated,
                SortField::Created,
                SortField::Number,
                SortField::Comments,
            ]
            .into_iter()
            .map(|value| {
                item(
                    Choice::Sort(value),
                    sort_label(value),
                    sort(app, kind).field == value,
                )
            })
            .collect(),
        ),
    };
    Some(super::menu_overlay::render_menu_overlay(
        "list-filter-menu",
        "list-filter-option",
        260.,
        "",
        menu.position,
        title.into(),
        vec![MenuGroup { title: None, items }],
        move |app, _, _| {
            if app.active_session() == owner {
                app.with_ui(|ui| ui.filter_controls.menu = None);
            }
        },
        move |app, choice, _, cx| {
            if app.active_session() != owner {
                return;
            }
            let keep_open = matches!(choice, Choice::Label(_));
            apply_choice(app, kind, choice, cx);
            if !keep_open {
                app.with_ui(|ui| ui.filter_controls.menu = None);
            }
        },
        window,
        cx,
    ))
}

fn state_label(value: StateFilter) -> &'static str {
    match value {
        StateFilter::Open => Msg::IssueStateOpen.t(),
        StateFilter::Closed => Msg::IssueStateClosed.t(),
        StateFilter::All => Msg::ListFilterAll.t(),
    }
}
fn draft_label(value: DraftFilter) -> &'static str {
    match value {
        DraftFilter::Ready => Msg::ListFilterReady.t(),
        DraftFilter::Draft => Msg::PrHomeDraft.t(),
        DraftFilter::All => Msg::ListFilterAll.t(),
    }
}
fn checks_label(value: ChecksFilter) -> &'static str {
    match value {
        ChecksFilter::Passing => Msg::ListFilterPassing.t(),
        ChecksFilter::Failing => Msg::ListFilterFailing.t(),
        ChecksFilter::Pending => Msg::PrQueuePending.t(),
        ChecksFilter::All => Msg::ListFilterAll.t(),
    }
}
fn sort_label(value: SortField) -> &'static str {
    match value {
        SortField::Updated => Msg::PrHomeSortUpdated.t(),
        SortField::Created => Msg::PrHomeSortCreated.t(),
        SortField::Number => Msg::PrHomeSortNumber.t(),
        SortField::Comments => Msg::ListSortComments.t(),
    }
}
