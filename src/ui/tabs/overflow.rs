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
    reveal: Option<(usize, usize, u32, u32, u32)>,
    selected: Option<usize>,
    focused: Option<usize>,
    signature: Option<u64>,
    natural: f32,
    reserved: f32,
}

pub(super) fn signature(tabs: &[RepoTab], home: bool, scale: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    theme::zoom().to_bits().hash(&mut hash);
    scale.to_bits().hash(&mut hash);
    home.hash(&mut hash);
    if home {
        (i18n::lang() as u8).hash(&mut hash);
    }
    for tab in tabs {
        (&tab.name, tab.remote.is_some(), tab.is_worktree).hash(&mut hash);
    }
    hash.finish()
}

pub(super) fn viewport(
    tabs: &super::super::keyboard_nav::TabList,
    slots: usize,
    signature: u64,
    selected: Option<usize>,
    focused: Option<usize>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> (gpui::Stateful<gpui::Div>, Option<gpui::Pixels>, bool) {
    let layout = window.use_keyed_state("repository-tab-layout", cx, |_, _| Layout::default());
    layout.update(cx, |state, _| {
        if state.signature != Some(signature) {
            state.signature = Some(signature);
            state.natural = 0.;
            state.reserved = f32::from(theme::scaled_px(36.));
        }
    });
    let (available, natural, reserved, scroll) = {
        let state = layout.read(cx);
        (
            state.available,
            state.natural,
            state.reserved,
            state.scroll.clone(),
        )
    };
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::tab_strip::record_scroll(scroll.clone());
    let overflow = natural > available + 0.1;
    let width = overflow.then(|| {
        let available = available - reserved;
        px(((available / slots.max(1) as f32) - 1.)
            .clamp(
                f32::from(theme::scaled_px(TAB_MIN_W)).ceil(),
                f32::from(theme::scaled_px(TAB_MAX_W)).floor(),
            )
            .floor())
    });
    layout.update(cx, |state, _| {
        // Selection changes outrank old focus and stay revealed next frame.
        let target = if state.selected != selected {
            selected.unwrap_or(0)
        } else if state.focused != focused {
            focused.or(selected).unwrap_or(0)
        } else {
            state.reveal.map(|reveal| reveal.0).unwrap_or(0)
        };
        state.selected = selected;
        state.focused = focused;
        let reveal = (
            target,
            slots,
            available.to_bits(),
            theme::zoom().to_bits(),
            width.map(|width| f32::from(width).to_bits()).unwrap_or(0),
        );
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
            .map(|mut viewport| {
                viewport.style().restrict_scroll_to_axis = Some(true);
                viewport
            })
            .track_scroll(&scroll),
        width,
        overflow,
    )
}

pub(super) fn probe(
    app: gpui::WeakEntity<KagiApp>,
    slots: usize,
    natural_layout: bool,
    overflow: bool,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let layout = window.use_keyed_state("repository-tab-layout", cx, |_, _| Layout::default());
    gpui::canvas(
        move |bounds, _, cx| {
            let changed = layout.update(cx, |state, _| {
                // Measure the whole strip directly; the trigger's snapped
                // width comes from the actual viewport, not an estimate.
                let available = f32::from(bounds.size.width);
                let reserved = if overflow {
                    (available - f32::from(state.scroll.bounds().size.width)).max(0.)
                } else {
                    f32::from(theme::scaled_px(36.))
                };
                let natural = natural_layout
                    .then(|| {
                        let first = state.scroll.bounds_for_item(0)?;
                        let last = state.scroll.bounds_for_item(slots.checked_sub(1)?)?;
                        Some(f32::from(last.right() - first.left() + px(1.)).ceil())
                    })
                    .flatten();
                let changed = (state.available - available).abs() >= 0.1
                    || (state.reserved - reserved).abs() >= 0.1
                    || natural.is_some_and(|width| (state.natural - width).abs() >= 0.1);
                state.available = available;
                state.reserved = reserved;
                if let Some(natural) = natural {
                    state.natural = natural;
                }
                changed
            });
            if changed {
                let _ = app.update(cx, |_, cx| cx.notify());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
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

pub(super) fn dismiss(
    root: Option<gpui::FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) {
    let state = menu_state(window, cx);
    let popover = state.read(cx).popover.clone();
    if let Some(popover) = popover {
        // Root render still borrows KagiApp. Keep both owners alive until its
        // borrow is released, then dismiss without losing the kit registration.
        window.defer(cx, move |window, cx| {
            let restore = state.read(cx).owns_focus(window, cx);
            popover.update(cx, |state, cx| state.dismiss(window, cx));
            if restore {
                if let Some(root) = root {
                    root.focus(window, cx);
                }
            }
            state.update(cx, |state, _| {
                state.open = false;
                state.menu = None;
                state.popover = None;
                state.previous = None;
            });
        });
        return;
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
        let trigger = super::super::render_header::overflow_trigger::trigger(
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

pub(super) struct MenuTab {
    key: u64,
    name: SharedString,
    path: SharedString,
    remote: bool,
    worktree: bool,
}

pub(super) fn entries(tabs: &[RepoTab]) -> Vec<MenuTab> {
    tabs.iter()
        .map(|tab| MenuTab {
            key: tab.session.tab().0,
            name: super::super::render_helpers::safe_text(&tab.name),
            path: super::super::render_helpers::safe_text(&tab.path.display().to_string()),
            remote: tab.remote.is_some(),
            worktree: tab.is_worktree,
        })
        .collect()
}

impl MenuTab {
    fn parent(&self) -> &std::path::Path {
        let path = std::path::Path::new(self.path.as_ref());
        path.parent().unwrap_or(path)
    }

    fn label(&self, duplicate: bool, shared_parent: bool) -> SharedString {
        let marker = if self.remote {
            "☁ "
        } else if self.worktree {
            "🌲 "
        } else {
            ""
        };
        if duplicate {
            let qualifier = if shared_parent {
                std::path::Path::new(self.path.as_ref())
            } else {
                self.parent()
            };
            format!("{marker}{} — {}", self.name, qualifier.display()).into()
        } else if marker.is_empty() {
            self.name.clone()
        } else {
            format!("{marker}{}", self.name).into()
        }
    }
}
#[derive(gpui::IntoElement)]
pub(super) struct Menu {
    app: gpui::WeakEntity<KagiApp>,
    tabs: Vec<MenuTab>,
    home: bool,
    state: gpui::Entity<MenuState>,
}

pub(super) fn menu(
    app: gpui::WeakEntity<KagiApp>,
    tabs: Vec<MenuTab>,
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
    fn render(self, _window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
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
        let available = window.viewport_size().width;
        let menu = PopupMenu::build(window, cx, |menu, _, _| self.items(menu, available));
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

impl Menu {
    fn items(&self, mut menu: PopupMenu, available: gpui::Pixels) -> PopupMenu {
        let width = (available - px(34.))
            .max(px(0.))
            .min(theme::scaled_px(400.));
        menu = menu.min_w(width.min(theme::scaled_px(200.))).max_w(width);
        let mut names = std::collections::HashMap::<&str, usize>::with_capacity(self.tabs.len());
        let mut parents = std::collections::HashMap::new();
        for tab in &self.tabs {
            *names.entry(tab.name.as_ref()).or_default() += 1;
        }
        for tab in &self.tabs {
            if names[tab.name.as_ref()] > 1 {
                *parents
                    .entry((tab.name.as_ref(), tab.parent()))
                    .or_insert(0usize) += 1;
            }
        }
        for (index, tab) in self.tabs.iter().enumerate() {
            let duplicate = names[tab.name.as_ref()] > 1;
            let shared_parent = duplicate && parents[&(tab.name.as_ref(), tab.parent())] > 1;
            let label = tab.label(duplicate, shared_parent);
            let key = tab.key;
            let app = self.app.clone();
            menu = menu.item(
                tab_item(index, label, Some(tab.path.clone()), width).on_click(
                    move |_, window, cx| {
                        let _ = app.update(cx, |app, cx| {
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
                    },
                ),
            );
        }
        if self.home {
            let app = self.app.clone();
            menu = menu.item(
                tab_item(usize::MAX, Msg::HomeTabTitle.t().into(), None, width).on_click(
                    move |_, window, cx| {
                        let _ = app.update(cx, |app, cx| app.open_home_tab(window, cx));
                    },
                ),
            );
        }
        menu
    }
}
fn tab_item(
    index: usize,
    label: SharedString,
    path: Option<SharedString>,
    width: gpui::Pixels,
) -> PopupMenuItem {
    PopupMenuItem::element(move |window, _| {
        #[cfg(feature = "gui-e2e")]
        super::super::e2e::tab_strip::record(
            format!("tab-overflow-item-{index}"),
            gpui::Role::MenuItem,
            &label,
        );
        #[cfg(feature = "gui-e2e")]
        if let Some(path) = &path {
            super::super::e2e::tab_strip::record_description(
                format!("tab-overflow-item-{index}"),
                path,
            );
        }
        div()
            .id(("tab-overflow-item", index))
            .role(gpui::Role::MenuItem)
            .aria_label(label.clone())
            .when_some(path.clone(), |item, path| {
                item.aria_description(path.clone())
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(path.clone()).build(window, cx)
                    })
            })
            .w_full()
            .max_w((width - window.rem_size() * 0.5 - px(16.)).max(px(0.)))
            .child(
                div()
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .whitespace_nowrap()
                    .child(label.clone()),
            )
    })
}
