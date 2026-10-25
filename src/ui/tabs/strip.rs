//! Document-tab presentation; session ownership remains in `tabs.rs`.
use super::super::keyboard_nav::{Activation, TabList};
use super::*;

/// The sidebar toggle and its gap before the first tab, outside UI zoom like
/// the traffic lights it follows.
const SIDEBAR_TOGGLE_W: f32 = 32.;

impl KagiApp {
    /// Document tabs share the native titlebar, including a single document.
    pub fn render_tab_strip(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut keys: Vec<_> = self.tabs.iter().map(|tab| tab.session.tab().0).collect();
        let repos = keys.len();
        let home_slot = (self.home.is_some() || self.home_in_front()).then_some(repos);
        if home_slot.is_some() {
            keys.push(u64::MAX);
        }
        let selected = if self.home_in_front() {
            home_slot
        } else {
            self.tabs.get(self.active_tab).map(|_| self.active_tab)
        };
        let list = self.document_tab_list(keys, selected, repos, cx);
        let has_add = !self.tabs.is_empty() || self.remote_view.is_some();
        let has_sidebar_toggle = !super::super::compact::for_window(window);
        let scale = window.scale_factor();
        let snap = |value: gpui::Pixels| gpui::px((f32::from(value) * scale).round() / scale);
        // GPUI snaps each control separately, not their combined reference width.
        let reserved_space = snap(theme::scaled_px(8.))
            + if has_add {
                snap(theme::scaled_px(28.))
            } else {
                gpui::px(0.)
            }
            + snap(gpui::px(if cfg!(target_os = "macos") { 80. } else { 0. }))
            + if has_sidebar_toggle {
                snap(gpui::px(SIDEBAR_TOGGLE_W))
            } else {
                gpui::px(0.)
            };
        self.tab_strip_focus
            .release_closed(self.root_focus.as_ref(), window, cx);
        let focused = self.tab_strip_focus.focused(window);
        let (mut viewport, width, has_overflow) = overflow::viewport(
            &list,
            repos + usize::from(home_slot.is_some()),
            overflow::signature(&self.tabs, home_slot.is_some(), window.scale_factor()),
            selected,
            focused,
            window,
            cx,
        );
        viewport = self.document_tab_cells(viewport, &list, selected, width, window, cx);
        let probe = overflow::probe(
            cx.weak_entity(),
            repos + usize::from(home_slot.is_some()),
            width.is_none(),
            has_overflow,
            reserved_space,
            window,
            cx,
        );
        let menu = self.document_tab_menu(has_overflow, window, cx);
        div()
            .id("tab-strip")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .when(cfg!(target_os = "macos"), |bar| bar.pl(gpui::px(80.)))
            .h(gpui::px(TAB_STRIP_H))
            .flex_shrink_0()
            .bg(rgb(theme().chrome_surface()))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .child(super::super::e2e::measure_inside("repo-tab-strip"))
            .when(has_sidebar_toggle, |bar| {
                bar.child(
                    div()
                        .flex_shrink_0()
                        .w(gpui::px(SIDEBAR_TOGGLE_W))
                        .child(super::super::pr_layout::sidebar_toggle(self, cx)),
                )
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_row()
                    .items_center()
                    .min_w_0()
                    .h_full()
                    .flex_shrink(1.)
                    .when(has_overflow, |tabs| tabs.flex_1())
                    .child(viewport)
                    .children(menu),
            )
            .child(probe)
            .when(has_add, |bar| bar.child(self.render_tab_add(cx)))
            .child(
                self.render_chrome_spacer("titlebar-drag-region", cx)
                    // Overflow needs the full remaining tab budget, not half
                    // of it shared with a second growing flex item.
                    .when(has_overflow, |spacer| {
                        spacer.flex_none().w(theme::scaled_px(8.))
                    }),
            )
            .into_any_element()
    }

    fn document_tab_menu(
        &self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<overflow::Menu> {
        if visible {
            Some(overflow::menu(
                cx.weak_entity(),
                overflow::entries(&self.tabs),
                self.home.is_some(),
                window,
                cx,
            ))
        } else {
            overflow::dismiss(self.root_focus.clone(), window, cx);
            None
        }
    }

    fn document_tab_cells(
        &self,
        mut viewport: gpui::Stateful<gpui::Div>,
        list: &TabList,
        selected: Option<usize>,
        width: Option<gpui::Pixels>,
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
        if self.home.is_some() || self.home_in_front() {
            let slot = self.tabs.len();
            viewport = viewport.child(home_tab(
                list,
                slot,
                selected == Some(slot),
                focused == Some(slot),
                window_active,
                width,
                !self.tabs.is_empty(),
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
    width: Option<gpui::Pixels>,
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
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::tab_strip::record_description(format!("tab-close-{slot}"), &path);
    let close = close_button(
        ("tab-close", slot),
        ("tab-close-hover-help", slot),
        close_name,
        Some(path.clone()),
        cx.listener(move |app, _, _, cx| {
            if let Some(slot) = app.tabs.iter().position(|tab| tab.session == key) {
                app.close_tab(slot, cx);
            }
        }),
    );
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
        .child(tab_label(
            format!("repo-tab-label-{slot}"),
            label,
            width.is_some(),
        ))
        .child(close)
        .hover_tooltip_builder(("repo-tab-hover-help", slot), move |window, cx| {
            Tooltip::new(path.clone()).build(window, cx)
        })
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn home_tab(
    list: &TabList,
    slot: usize,
    selected: bool,
    focused: bool,
    window_active: bool,
    width: Option<gpui::Pixels>,
    closeable: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cell = list
        .cell(slot, Msg::HomeTabTitle.t(), div().id("home-tab"))
        .group("home-tab");
    tab_cell(cell, selected, window_active, width)
        .child(super::super::e2e::measure_inside("home-tab"))
        .child(tab_label(
            "home-tab-label".into(),
            format!("⌂ {}", Msg::HomeTabTitle.t()).into(),
            width.is_some(),
        ))
        .when(closeable, |cell| {
            // The Home-only window has no document behind it to return to.
            let close_name = i18n::tab_close_name(Msg::HomeTabTitle.t());
            #[cfg(feature = "gui-e2e")]
            super::super::e2e::tab_strip::record("home-tab-close", gpui::Role::Button, &close_name);
            let close = close_button(
                "home-tab-close",
                "home-tab-close-hover-help",
                close_name,
                None,
                cx.listener(|app, _, _, cx| app.close_home_tab(cx)),
            );
            let close = div()
                .id("home-tab-close-reveal")
                .opacity(if selected || focused { 1. } else { 0. })
                .group_hover("home-tab", |style| style.opacity(1.))
                .child(super::super::e2e::measure_control("home-tab-close", close));
            cell.child(close)
        })
        .into_any_element()
}

fn close_button(
    id: impl Into<gpui::ElementId>,
    tooltip_id: impl Into<gpui::ElementId>,
    name: String,
    path: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    // A visible child, not Button.label: the kit's label would overwrite the
    // independent name with × during RenderOnce.
    super::super::button_style::named_button_with_description(
        tooltip_id,
        Button::new(id)
            .child("×")
            .ghost()
            .xsmall()
            .min_w(px(20.))
            .min_h(px(20.))
            .ml(theme::scaled_px(4.))
            .tab_stop(false)
            .on_click(on_click),
        name.into(),
        None,
        path,
        false,
    )
}

fn tab_cell(
    cell: gpui::Stateful<gpui::Div>,
    selected: bool,
    window_active: bool,
    width: Option<gpui::Pixels>,
) -> gpui::Stateful<gpui::Div> {
    let (bg, fg) = tab_colors(&theme(), selected);
    let min = if width.is_some() {
        px(f32::from(theme::scaled_px(TAB_MIN_W)).ceil())
    } else {
        theme::scaled_px(TAB_MIN_W)
    };
    cell.relative()
        .flex()
        .flex_row()
        .items_center()
        .h_full()
        .min_w(min)
        .max_w(width.unwrap_or_else(|| theme::scaled_px(TAB_MAX_W)))
        .flex_shrink_0()
        .px(super::super::keyboard_nav::inset(8.))
        .gap_1()
        .bg(bg)
        .text_size(px(f32::from(theme::scaled_px(14.)).min(14.)))
        .text_color(rgb(fg))
        .mr(px(1.))
        .when(selected && tab_has_rail(&theme()), |cell| {
            cell.child(tab_rail(&theme(), window_active))
        })
        .cursor(gpui::CursorStyle::PointingHand)
}

fn tab_label(id: String, label: SharedString, compressed: bool) -> gpui::Div {
    div()
        .relative()
        .flex_1()
        .when(compressed, |label| label.min_w_0())
        .overflow_hidden()
        .text_ellipsis_middle()
        .whitespace_nowrap()
        .child(super::super::e2e::measure_inside(id))
        .child(label)
}
