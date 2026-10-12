//! Bounded repository-tab scrolling and the all-tabs popup.
use super::*;
use gpui::{Focusable as _, ScrollHandle};
use gpui_component::{
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
};

#[derive(Default)]
pub(super) struct Layout {
    available: f32,
    scroll: ScrollHandle,
    reveal: Option<(usize, usize, u32, u32)>,
}

pub(super) fn viewport(
    tabs: &super::super::keyboard_nav::TabList,
    slots: usize,
    target: usize,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> (gpui::Stateful<gpui::Div>, gpui::Pixels) {
    let layout = window.use_keyed_state("repository-tab-layout", cx, |_, _| Layout::default());
    let (available, scroll) = {
        let state = layout.read(cx);
        (state.available, state.scroll.clone())
    };
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::tab_strip::record_scroll(scroll.clone());
    let width = ((available / slots.max(1) as f32) - 1.)
        .clamp(
            f32::from(theme::scaled_px(TAB_MIN_W)).ceil(),
            f32::from(theme::scaled_px(TAB_MAX_W)).floor(),
        )
        .floor();
    let reveal = (target, slots, available.to_bits(), theme::zoom().to_bits());
    layout.update(cx, |state, _| {
        if state.reveal != Some(reveal) {
            state.scroll.scroll_to_item(target);
            state.reveal = Some(reveal);
        }
    });
    (
        tabs.list(div().id("tab-scroll-viewport"))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll()
            .track_scroll(&scroll),
        px(width),
    )
}

pub(super) fn probe(
    app: gpui::WeakEntity<KagiApp>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let layout = window.use_keyed_state("repository-tab-layout", cx, |_, _| Layout::default());
    gpui::canvas(
        move |bounds, _, cx| {
            let changed = layout.update(cx, |state, _| {
                let width = f32::from(bounds.size.width);
                if (state.available - width).abs() < 0.1 {
                    return false;
                }
                state.available = width;
                true
            });
            if changed {
                let _ = app.update(cx, |_, cx| cx.notify());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

struct MenuState {
    open: bool,
    trigger: gpui::FocusHandle,
    previous: Option<gpui::FocusHandle>,
    menu: Option<gpui::Entity<PopupMenu>>,
    popover: Option<gpui::Entity<gpui_component::popover::PopoverState>>,
}

fn menu_state(window: &mut Window, cx: &mut gpui::App) -> gpui::Entity<MenuState> {
    window.use_keyed_state("repository-tab-menu", cx, |_, cx| MenuState {
        open: false,
        trigger: cx.focus_handle(),
        previous: None,
        menu: None,
        popover: None,
    })
}
impl MenuState {
    fn owns_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        window.focused(cx).is_none()
            || self.trigger.is_focused(window)
            || self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.focus_handle(cx).contains_focused(window, cx))
    }
}

pub(super) fn dismiss(window: &mut Window, cx: &mut Context<KagiApp>) {
    let state = menu_state(window, cx);
    let popover = state.read(cx).popover.clone();
    if let Some(popover) = popover {
        popover.update(cx, |state, cx| state.dismiss(window, cx));
    }
    state.update(cx, |state, _| {
        state.open = false;
        state.menu = None;
        state.popover = None;
        state.previous = None;
    });
}

#[derive(gpui::IntoElement)]
struct Trigger {
    selected: bool,
    state: gpui::WeakEntity<MenuState>,
    app: gpui::WeakEntity<KagiApp>,
    focus: gpui::FocusHandle,
}
impl gpui_component::Selectable for Trigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.selected
    }
}
impl gpui::RenderOnce for Trigger {
    fn render(self, _: &mut Window, _: &mut gpui::App) -> impl IntoElement {
        let pointer = self.state.clone();
        let trigger = super::super::render_header::toolbar_overflow::trigger(
            "tab-overflow",
            Msg::TabOverflow.t(),
            self.selected,
            &self.focus,
        )
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            let focus = window.focused(cx);
            let _ = pointer.update(cx, |state, _| {
                if !state.open {
                    state.previous = focus;
                }
            });
        })
        .on_click(move |event, window, cx| {
            if event.is_keyboard() {
                let focus = window.focused(cx);
                let _ = self.state.update(cx, |state, _| {
                    state.previous = focus;
                    state.open = true;
                });
                let _ = self.app.update(cx, |_, cx| cx.notify());
            }
        });
        super::super::e2e::measure_control("tab-overflow", trigger)
    }
}
#[derive(gpui::IntoElement)]
pub(super) struct Menu {
    app: gpui::WeakEntity<KagiApp>,
    tabs: Vec<(u64, SharedString)>,
    home: bool,
    state: gpui::Entity<MenuState>,
}

pub(super) fn menu(
    app: gpui::WeakEntity<KagiApp>,
    tabs: Vec<(u64, SharedString)>,
    home: bool,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Menu {
    Menu {
        app,
        tabs,
        home,
        state: menu_state(window, cx),
    }
}
impl gpui::RenderOnce for Menu {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let state = self.state.clone();
        let changed = state.downgrade();
        let changed_app = self.app.clone();
        let open = state.read(cx).open;
        Popover::new("repository-tab-popover")
            .appearance(false)
            .overlay_closable(false)
            .anchor(gpui::Anchor::TopRight)
            .open(open)
            .on_open_change(move |open, window, cx| {
                let (previous, restore) = changed
                    .update(cx, |state, cx| {
                        let restore = !open && state.owns_focus(window, cx);
                        state.open = *open;
                        if !open {
                            state.menu = None;
                            state.popover = None;
                            (state.previous.take(), restore)
                        } else {
                            (None, false)
                        }
                    })
                    .unwrap_or((None, false));
                let _ = changed_app.update(cx, |app, cx| {
                    if restore {
                        if let Some(focus) = previous.or_else(|| app.root_focus.clone()) {
                            focus.focus(window, cx);
                        }
                    }
                    cx.notify();
                });
            })
            .trigger(Trigger {
                selected: false,
                state: state.downgrade(),
                app: self.app.clone(),
                focus: state.read(cx).trigger.clone(),
            })
            .content(move |_, window, cx| self.content(&state, window, cx))
            .into_any_element()
    }
}

impl Menu {
    fn content(
        &self,
        state: &gpui::Entity<MenuState>,
        window: &mut Window,
        cx: &mut Context<gpui_component::popover::PopoverState>,
    ) -> gpui::Entity<PopupMenu> {
        if let Some(menu) = state.read(cx).menu.clone() {
            return menu;
        }
        let app = self.app.clone();
        let entries = self.tabs.clone();
        let width = (window.viewport_size().width - px(34.))
            .max(px(0.))
            .min(theme::scaled_px(400.));
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu.min_w(width.min(theme::scaled_px(200.))).max_w(width);
            for (index, (key, label)) in entries.into_iter().enumerate() {
                let item_app = app.clone();
                menu = menu.item(
                    tab_item(index, label, width).on_click(move |_, window, cx| {
                        let _ = item_app.update(cx, |app, cx| {
                            if let Some(slot) =
                                app.tabs.iter().position(|tab| tab.session.tab().0 == key)
                            {
                                let left = app.send_home_back();
                                app.return_to_tab(slot, left, cx);
                                if let Some(root) = &app.root_focus {
                                    root.focus(window, cx);
                                }
                                cx.notify();
                            }
                        });
                    }),
                );
            }
            if self.home {
                let item_app = app.clone();
                menu = menu.item(
                    tab_item(usize::MAX, Msg::HomeTabTitle.t().into(), width).on_click(
                        move |_, window, cx| {
                            let _ = item_app.update(cx, |app, cx| app.open_home_tab(window, cx));
                        },
                    ),
                );
            }
            menu
        });
        let popover_state = cx.entity();
        let popover = popover_state.downgrade();
        state.update(cx, |state, _| {
            state.menu = Some(menu.clone());
            state.popover = Some(popover_state);
        });
        menu.focus_handle(cx).focus(window, cx);
        let dismissed = state.downgrade();
        window
            .subscribe(&menu, cx, move |_, _: &gpui::DismissEvent, window, cx| {
                let _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                let _ = dismissed.update(cx, |state, _| state.menu = None);
            })
            .detach();
        menu
    }
}
fn tab_item(index: usize, label: SharedString, width: gpui::Pixels) -> PopupMenuItem {
    PopupMenuItem::element(move |window, _| {
        #[cfg(feature = "gui-e2e")]
        super::super::e2e::tab_strip::record(
            format!("tab-overflow-item-{index}"),
            gpui::Role::MenuItem,
            &label,
        );
        div()
            .id(("tab-overflow-item", index))
            .role(gpui::Role::MenuItem)
            .aria_label(label.clone())
            .w_full()
            .max_w((width - window.rem_size() * 0.5 - px(16.)).max(px(0.)))
            .child(div().truncate().child(label.clone()))
    })
}
