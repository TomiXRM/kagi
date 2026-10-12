//! Document-tab presentation; session ownership remains in `tabs.rs`.
use super::super::keyboard_nav::{Activation, TabList};
use super::*;

impl KagiApp {
    pub(in crate::ui) fn tab_strip_visible(&self) -> bool {
        self.tabs.len() + usize::from(self.home.is_some()) > 1
    }

    /// Multiple documents share a bounded strip below the unified toolbar.
    pub fn render_tab_strip(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.tab_strip_visible() {
            overflow::dismiss(window, cx);
            self.tab_strip_focus.sync(&[], cx);
            self.tab_strip_focus
                .release_closed(self.root_focus.as_ref(), window, cx);
            return None;
        }
        let mut keys: Vec<_> = self.tabs.iter().map(|tab| tab.session.tab().0).collect();
        let repos = keys.len();
        let home_slot = self.home.is_some().then_some(repos);
        if home_slot.is_some() {
            keys.push(u64::MAX);
        }
        let selected = if self.home_in_front() {
            home_slot
        } else {
            Some(self.active_tab)
        };
        let list = self.document_tab_list(keys, selected, repos, cx);
        self.tab_strip_focus
            .release_closed(self.root_focus.as_ref(), window, cx);
        let focused = self.tab_strip_focus.focused(window);
        let (mut viewport, width) = overflow::viewport(
            &list,
            repos + usize::from(home_slot.is_some()),
            focused.or(selected).unwrap_or(0),
            window,
            cx,
        );
        viewport = self.document_tab_cells(viewport, &list, selected, width, window, cx);
        viewport = viewport.child(overflow::probe(cx.weak_entity(), window, cx));
        let menu = overflow::menu(
            cx.weak_entity(),
            self.tabs
                .iter()
                .map(|tab| {
                    (
                        tab.session.tab().0,
                        super::super::render_helpers::safe_text(&tab.name),
                    )
                })
                .collect(),
            home_slot.is_some(),
            window,
            cx,
        );
        Some(
            div()
                .id("tab-strip")
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .h(theme::scaled_px(TAB_STRIP_H))
                .flex_shrink_0()
                .bg(rgb(theme().chrome_surface()))
                .border_b_1()
                .border_color(rgb(theme().surface))
                .child(super::super::e2e::measure_inside("repo-tab-strip"))
                .child(viewport)
                .child(menu)
                .into_any_element(),
        )
    }

    fn document_tab_cells(
        &self,
        mut viewport: gpui::Stateful<gpui::Div>,
        list: &TabList,
        selected: Option<usize>,
        width: gpui::Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let focused = self.tab_strip_focus.focused(window);
        let window_active = self.window_active_for_render(window);
        for (slot, tab) in self.tabs.iter().enumerate() {
            viewport = viewport.child(repository_tab(
                list,
                slot,
                tab,
                selected == Some(slot),
                focused == Some(slot),
                window_active,
                width,
                cx,
            ));
        }
        if self.home.is_some() {
            let slot = self.tabs.len();
            viewport = viewport.child(home_tab(
                list,
                slot,
                selected == Some(slot),
                focused == Some(slot),
                window_active,
                width,
                cx,
            ));
        }
        viewport
    }

    fn document_tab_list(
        &self,
        keys: Vec<u64>,
        selected: Option<usize>,
        repos: usize,
        cx: &mut Context<Self>,
    ) -> TabList {
        let slots = keys.len();
        let app = cx.weak_entity();
        TabList::new(
            &self.tab_strip_focus,
            keys,
            (0..slots).collect(),
            selected,
            Activation::Manual,
            self.root_focus.clone(),
            move |slot, _, window, cx| {
                let _ = app.update(cx, |app, cx| {
                    if slot < repos {
                        let left = app.send_home_back();
                        app.return_to_tab(slot, left, cx);
                        cx.notify();
                    } else {
                        let kept = app.tab_strip_focus.focused_cell(window);
                        app.open_home_tab(window, cx);
                        if let Some(cell) = kept {
                            window.focus(&cell, cx);
                        }
                    }
                });
            },
            cx,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn repository_tab(
    list: &TabList,
    slot: usize,
    tab: &RepoTab,
    selected: bool,
    focused: bool,
    window_active: bool,
    width: gpui::Pixels,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let name = super::super::render_helpers::safe_text(&tab.name);
    let path = super::super::render_helpers::safe_text(&tab.path.display().to_string());
    let label = if tab.remote.is_some() {
        format!("☁ {name}").into()
    } else if tab.is_worktree {
        format!("🌲 {name}").into()
    } else {
        name.clone()
    };
    let key = tab.session;
    let close_name = i18n::tab_close_name(&name);
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::tab_strip::record(
        format!("tab-close-{slot}"),
        gpui::Role::Button,
        &close_name,
    );
    let close = close_button(("tab-close", slot), close_name).on_click(cx.listener(
        move |app, _, _, cx| {
            if let Some(slot) = app.tabs.iter().position(|tab| tab.session == key) {
                app.close_tab(slot, cx);
            }
        },
    ));
    let close = div()
        .id(("tab-close-reveal", slot))
        .opacity(if selected || focused { 1. } else { 0. })
        .group_hover(path.clone(), |style| style.opacity(1.))
        .child(super::super::e2e::measure_control(
            format!("tab-close-{slot}"),
            close,
        ));
    let cell = list
        .cell(slot, &name, div().id(("repo-tab", slot)))
        .group(path.clone())
        .aria_description(path.clone());
    tab_cell(cell, selected, window_active, width)
        .child(super::super::e2e::measure_inside(format!(
            "repo-tab-{slot}"
        )))
        .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
        .child(tab_label(format!("repo-tab-label-{slot}"), label))
        .child(close)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn home_tab(
    list: &TabList,
    slot: usize,
    selected: bool,
    focused: bool,
    window_active: bool,
    width: gpui::Pixels,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let close_name = i18n::tab_close_name("Home");
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::tab_strip::record("home-tab-close", gpui::Role::Button, &close_name);
    let close = close_button("home-tab-close", close_name)
        .on_click(cx.listener(|app, _, _, cx| app.close_home_tab(cx)));
    let close = div()
        .id("home-tab-close-reveal")
        .opacity(if selected || focused { 1. } else { 0. })
        .group_hover("home-tab", |style| style.opacity(1.))
        .child(super::super::e2e::measure_control("home-tab-close", close));
    let cell = list
        .cell(slot, Msg::HomeTabTitle.t(), div().id("home-tab"))
        .group("home-tab");
    tab_cell(cell, selected, window_active, width)
        .child(super::super::e2e::measure_inside("home-tab"))
        .child(tab_label(
            "home-tab-label".into(),
            format!("⌂ {}", Msg::HomeTabTitle.t()).into(),
        ))
        .child(close)
        .into_any_element()
}

fn close_button(id: impl Into<gpui::ElementId>, name: String) -> Button {
    // A visible child, not Button.label: the kit's label would overwrite the
    // independent name with × during RenderOnce.
    super::super::button_style::named_button(
        Button::new(id)
            .child("×")
            .ghost()
            .xsmall()
            .min_w(px(20.))
            .min_h(px(20.))
            .ml(theme::scaled_px(4.))
            .tab_stop(false),
        name,
        None,
    )
}

fn tab_cell(
    cell: gpui::Stateful<gpui::Div>,
    selected: bool,
    window_active: bool,
    width: gpui::Pixels,
) -> gpui::Stateful<gpui::Div> {
    let (bg, fg) = tab_colors(&theme(), selected, window_active);
    cell.relative()
        .flex()
        .flex_row()
        .items_center()
        .h_full()
        .w(width)
        .flex_shrink_0()
        .px(super::super::keyboard_nav::inset(8.))
        .gap_1()
        .bg(bg)
        .text_sm()
        .text_color(rgb(fg))
        .mr(px(1.))
        .when(selected && tab_has_rail(&theme()), |cell| {
            cell.child(tab_rail(&theme(), window_active))
        })
        .cursor(gpui::CursorStyle::PointingHand)
}

fn tab_label(id: String, label: SharedString) -> gpui::Div {
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis_middle()
        .whitespace_nowrap()
        .child(super::super::e2e::measure_inside(id))
        .child(label)
}
