//! Co-author popup keyboard state, owned by the existing CommitPanelView.

use std::rc::Rc;

use gpui::{
    actions, div, prelude::*, px, rgb, AnyElement, App, Context, FocusHandle, KeyBinding,
    ListAlignment, ListState, Role, SharedString, Window,
};

use super::commit_panel::CommitPanelView;
use super::e2e::commit_panel_labels as labels;
use super::i18n::Msg;
use super::keyboard_nav::{self, RowFocus, RowKeys, RowList, RowScroll};
use super::render_helpers::with_system_vertical_scrollbar;
use super::theme::{self, theme};

pub(super) const ROW_HEIGHT: f32 = 40.;

actions!(kagi_coauthor_menu, [CoauthorDismiss]);

const MENU_CONTEXT: &str = "KagiCoauthorMenu";

/// Like context menus, Tab closes this popup instead of visiting controls
/// behind it. A scoped action outranks Root's Tab binding, which runs before
/// capture_key_down; Escape remains owned by the existing FrontLayer chain.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", CoauthorDismiss, Some(MENU_CONTEXT)),
        KeyBinding::new("shift-tab", CoauthorDismiss, Some(MENU_CONTEXT)),
    ]);
}

struct CoauthorRow {
    id: SharedString,
    name: SharedString,
    email: SharedString,
    label: SharedString,
}

pub(crate) struct CoauthorNavigation {
    keys: RowKeys,
    focus: RowFocus,
    scroll: ListState,
    rows: Option<Rc<RowList>>,
    display: Rc<[CoauthorRow]>,
    empty_focus: FocusHandle,
    focus_first: bool,
}

impl CoauthorNavigation {
    fn new(candidates: &[kagi_git::AuthorCandidate], cx: &App) -> Self {
        Self {
            keys: candidates
                .iter()
                .enumerate()
                .map(|(i, _)| (i.to_string(), i))
                .collect(),
            focus: RowFocus::default(),
            scroll: ListState::new(candidates.len(), ListAlignment::Top, px(0.)),
            rows: None,
            display: candidates
                .iter()
                .enumerate()
                .map(|(i, candidate)| CoauthorRow {
                    id: format!("cp-coauthor-row-{i}").into(),
                    name: candidate.name.clone().into(),
                    email: candidate.email.clone().into(),
                    label: format!("{} <{}>", candidate.name, candidate.email).into(),
                })
                .collect(),
            empty_focus: cx.focus_handle(),
            focus_first: true,
        }
    }
}

impl CommitPanelView {
    /// FrontLayer still clears coauthor_menu. Reconcile after that existing
    /// close path, when a Window is available, rather than intercepting Escape.
    pub(crate) fn reconcile_coauthor_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(candidates) = self.coauthor_menu.as_deref() else {
            if self.coauthor_navigation.take().is_some_and(|navigation| {
                navigation.focus.focused(window).is_some()
                    || navigation.empty_focus.is_focused(window)
                    || window.focused(cx).is_none()
            }) {
                self.controls_focus.get(5, cx).focus(window, cx);
            }
            return;
        };
        let navigation = self
            .coauthor_navigation
            .get_or_insert_with(|| CoauthorNavigation::new(candidates, cx));
        let scroll = RowScroll::List(navigation.scroll.clone());
        navigation.rows = Some(Rc::new(navigation.focus.rows(
            navigation.keys.clone(),
            &scroll,
            Some(&navigation.empty_focus),
            window,
            cx,
        )));
        if std::mem::take(&mut navigation.focus_first)
            && !navigation.focus.focus_first(&scroll, window, cx)
        {
            navigation.empty_focus.focus(window, cx);
        }
    }

    pub(crate) fn activate_coauthor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(candidate) = self
            .coauthor_menu
            .as_ref()
            .and_then(|rows| rows.get(index))
            .cloned()
        else {
            return;
        };
        // Close before inserting: a repeated activation can no longer select
        // an author, and add_coauthor retains its existing trailer deduplication.
        self.coauthor_menu = None;
        self.add_coauthor(&candidate, window, cx);
        self.reconcile_coauthor_menu(window, cx);
        cx.notify();
    }
}

pub(super) fn render_content(
    view: &CommitPanelView,
    height: gpui::Pixels,
    window: &Window,
    cx: &mut Context<CommitPanelView>,
) -> Option<AnyElement> {
    let navigation = view.coauthor_navigation.as_ref()?;
    let rows = navigation.rows.as_ref()?.clone();
    let selected = navigation
        .focus
        .focused(window)
        .and_then(|key| key.parse::<usize>().ok());
    let wrapper = rows
        .list(super::list_a11y::list_box(
            "cp-coauthor-menu",
            div()
                .id("cp-coauthor-menu")
                .w_full()
                .h(height)
                .flex()
                .flex_col(),
            Msg::AddCoAuthor.t(),
        ))
        .key_context("KagiRowList KagiCoauthorMenu")
        .on_action(cx.listener(|view, _: &CoauthorDismiss, window, cx| {
            view.coauthor_menu = None;
            view.reconcile_coauthor_menu(window, cx);
            cx.notify();
        }))
        .role(labels::role("cp-coauthor-menu", Role::ListBox))
        .aria_label(labels::name("cp-coauthor-menu", Msg::AddCoAuthor.t()));
    let content = if navigation.display.is_empty() {
        render_empty(&navigation.empty_focus)
    } else {
        let display = navigation.display.clone();
        let entity = cx.entity();
        let count = display.len();
        gpui::list(navigation.scroll.clone(), move |index, window, cx| {
            render_option(
                &display[index],
                (index, count),
                &rows,
                selected,
                entity.clone(),
                window,
                cx,
            )
        })
        .w_full()
        .h(height)
        .into_any_element()
    };
    Some(
        super::e2e::measure_control(
            "cp-coauthor-menu",
            wrapper.child(with_system_vertical_scrollbar(
                "cp-coauthor-viewport",
                &navigation.scroll,
                content,
                cx,
            )),
        )
        .into_any_element(),
    )
}

fn render_option(
    row: &CoauthorRow,
    (index, count): (usize, usize),
    rows: &RowList,
    selected: Option<usize>,
    entity: gpui::Entity<CommitPanelView>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    if selected == Some(index) {
        if let Some(handle) = window.focused(cx) {
            labels::focus(&row.id, &handle);
        }
    }
    let option = rows
        .row(
            index,
            super::list_a11y::list_option(
                "cp-coauthor-menu",
                div().id(row.id.clone()),
                index,
                count,
                row.label.clone(),
                selected == Some(index),
            ),
        )
        .role(labels::role(&row.id, Role::ListBoxOption))
        .aria_label(labels::name(&row.id, row.label.clone()))
        .aria_selected(labels::selected(&row.id, selected == Some(index)))
        .w_full()
        .h(theme::scaled_px(ROW_HEIGHT))
        .flex()
        .flex_col()
        .justify_center()
        .px(keyboard_nav::inset(8.))
        .py(keyboard_nav::inset(4.))
        .overflow_hidden()
        .when(selected == Some(index), |row| row.bg(rgb(theme().selected)))
        .hover(|style| style.bg(rgb(theme().selected)).cursor_default())
        .on_click(move |_, window, cx| {
            entity.update(cx, |view, cx| view.activate_coauthor(index, window, cx));
        })
        .child(
            div()
                .w_full()
                .truncate()
                .text_xs()
                .text_color(rgb(theme().text_main))
                .child(row.name.clone()),
        )
        .child(
            div()
                .w_full()
                .truncate()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(row.email.clone()),
        );
    super::e2e::measure_control(&*row.id, option)
}

fn render_empty(focus: &FocusHandle) -> AnyElement {
    labels::focus("cp-coauthor-empty", focus);
    let empty = keyboard_nav::focusable(
        div()
            .id("cp-coauthor-empty")
            .track_focus(focus)
            .role(labels::role("cp-coauthor-empty", Role::Status))
            .aria_label(labels::name("cp-coauthor-empty", Msg::NoRecentAuthors.t()))
            .w_full()
            .h(theme::scaled_px(ROW_HEIGHT))
            .px(keyboard_nav::inset(8.))
            .flex()
            .items_center()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(Msg::NoRecentAuthors.t())),
    );
    super::e2e::measure_control("cp-coauthor-empty", empty)
}
