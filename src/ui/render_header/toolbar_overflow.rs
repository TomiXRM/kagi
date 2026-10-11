//! Layout-owned toolbar fitting. Measurements come from the elements themselves,
//! never the window rectangle or test recorder. Cached natural widths let hidden
//! commands return when space grows without rendering offscreen focus targets.
use super::*;
use gpui::{Action as _, Focusable as _};
use gpui_component::{
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
    Selectable,
};

pub(super) const IDS: [&str; 14] = [
    "tb-pull",
    "tb-push",
    "tb-branch",
    "tb-stash",
    "tb-pop",
    "tb-undo",
    "tb-redo",
    "tb-terminal",
    "tb-graph-mode",
    "tb-pr-mode",
    "tb-editor-ws",
    "tb-ecosystem",
    "tb-settings",
    "tb-update",
];

pub(super) type StyleKey = (bool, i18n::Lang, u32);
#[derive(Default)]
pub(crate) struct Layout {
    available: f32,
    widths: [f32; 14],
    key: Option<StyleKey>,
}

impl Layout {
    pub(super) fn measure(&mut self, index: Option<usize>, width: f32, key: StyleKey) -> bool {
        let changed = self.key != Some(key);
        if changed {
            self.widths.fill(0.);
            self.key = Some(key);
        }
        let slot = match index {
            Some(index) => &mut self.widths[index],
            None => &mut self.available,
        };
        if (*slot - width).abs() < 0.1 {
            return changed;
        }
        *slot = width;
        true
    }

    pub(super) fn hidden(&self, present: u16, selected: usize, key: StyleKey) -> u16 {
        if self.key != Some(key)
            || self.available == 0.
            || IDS
                .iter()
                .enumerate()
                .any(|(i, _)| present & (1 << i) != 0 && self.widths[i] == 0.)
        {
            return 0;
        }
        let scale = theme::scaled(1.);
        // The spacers are the same 2-reference-px gaps and 4-reference-px
        // hairline margins used by render_header; hairlines remain literal px.
        let mut repo = if present & 0xff != 0 {
            24. * scale + 2.
        } else {
            0.
        };
        let mut workspace = (12.
            + if present & (1 << 9) != 0 { 2. } else { 0. }
            + if present & (1 << 13) != 0 { 8. } else { 0. })
            * scale
            + 1.;
        for (i, width) in self.widths.iter().enumerate() {
            if present & (1 << i) != 0 {
                if i < 8 {
                    repo += width * scale;
                } else {
                    workspace += width * scale;
                }
            }
        }
        // Preserve equal side columns (and therefore the wide toolbar's
        // centering), with room for Refresh and a useful repository identity.
        let required =
            |repo: f32, workspace: f32| repo + 2. * workspace.max(100. * scale) + 24. * scale;
        if required(repo, workspace) <= self.available {
            return 0;
        }
        let mut hidden = 0;
        workspace += 36. * scale; // The overflow trigger's exact width.
                                  // Repository commands collapse first. Preserve the selected workspace
                                  // until every other command has moved to the menu.
        for i in [13, 7, 6, 5, 4, 3, 2, 1, 0, 12, 11, 10, 9, 8] {
            if i == selected || present & (1 << i) == 0 {
                continue;
            }
            hidden |= 1 << i;
            if i < 8 {
                repo -= self.widths[i] * scale;
            } else {
                workspace -= self.widths[i] * scale;
                if i == 13 {
                    workspace -= 8. * scale;
                }
            }
            if required(repo, workspace) <= self.available {
                return hidden;
            }
        }
        if selected < IDS.len() {
            hidden | (present & (1 << selected))
        } else {
            hidden
        }
    }
}
pub(super) fn probe(
    app: gpui::WeakEntity<KagiApp>,
    index: Option<usize>,
    key: StyleKey,
) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, cx| {
            let width = f32::from(bounds.size.width);
            // GPUI can prepaint display:none descendants at zero size.
            // Those are not natural measurements; retain the last real width.
            if width <= 0. {
                return;
            }
            let normalized = if index.is_some() {
                width / theme::scaled(1.)
            } else {
                width
            };
            let _ = app.update(cx, |app, cx| {
                if app.toolbar_layout.measure(index, normalized, key) {
                    cx.notify();
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

pub(super) fn label(index: usize) -> &'static str {
    match index {
        0 => "Pull",
        1 => "Push",
        2 => Msg::ToolbarBranch.t(),
        3 => Msg::ToolbarStash.t(),
        4 => Msg::ToolbarPop.t(),
        5 => Msg::Undo.t(),
        6 => Msg::Redo.t(),
        7 => Msg::ToolbarTerminal.t(),
        8 => Msg::WorkspaceGraph.t(),
        9 => Msg::WorkspacePrs.t(),
        10 => Msg::ToolbarEditor.t(),
        11 => Msg::ToolbarAnalyze.t(),
        12 => Msg::SettingsTitle.t(),
        13 => "Update",
        _ => unreachable!("toolbar command index"),
    }
}

pub(super) fn button(
    app: gpui::WeakEntity<KagiApp>,
    hidden: u16,
    reasons: [Option<&'static str>; 14],
    root_focus: Option<gpui::FocusHandle>,
) -> impl IntoElement {
    OverflowMenu {
        app,
        hidden,
        reasons,
        root_focus,
    }
}

#[derive(gpui::IntoElement)]
struct OverflowTrigger {
    selected: bool,
    state: gpui::WeakEntity<MenuState>,
    app: gpui::WeakEntity<KagiApp>,
    focus: gpui::FocusHandle,
}

impl Selectable for OverflowTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl gpui::RenderOnce for OverflowTrigger {
    fn render(self, _: &mut Window, _: &mut gpui::App) -> impl IntoElement {
        // Button cannot name an icon-only AX node independently from a visible
        // caption in the pinned fork. Reuse the header's named/ringed helper.
        #[cfg(feature = "gui-e2e")]
        super::toolbar_a11y::record_label("tb-overflow", Msg::ToolbarOverflow.t());
        let pointer_state = self.state.clone();
        let trigger = keyboard_nav::focusable(
            div()
                .id("tb-overflow")
                .role(gpui::Role::Button)
                .aria_label(Msg::ToolbarOverflow.t())
                .track_focus(&self.focus.tab_index(0).tab_stop(true))
                .w(theme::scaled_px(36.))
                .h(theme::scaled_px(28.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(theme().text_main))
                .rounded_md()
                .cursor_pointer()
                .when(self.selected, |el| el.bg(rgb(theme().surface)))
                .hover(|style| style.bg(rgb(theme().surface)))
                .tooltip(|window, cx| Tooltip::new(Msg::ToolbarOverflow.t()).build(window, cx))
                .child("»")
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    let focus = window.focused(cx);
                    let _ = pointer_state.update(cx, |state, _| {
                        if !state.open {
                            state.previous_focus = focus;
                        }
                    });
                })
                // Popover opens on mouse-down; a GPUI keyboard click has no
                // mouse-down. Control its open state for Enter/Space explicitly.
                .on_click(move |event, window, cx| {
                    if !event.is_keyboard() {
                        return;
                    }
                    let focus = window.focused(cx);
                    let _ = self.state.update(cx, |state, _| {
                        state.previous_focus = focus;
                        state.open = true;
                    });
                    let _ = self.app.update(cx, |_, cx| cx.notify());
                }),
        );
        #[cfg(feature = "gui-e2e")]
        let trigger = trigger.on_key_down(|event, _, _| {
            if event.keystroke.key == "f19" {
                super::toolbar_a11y::record_focus_probe("tb-overflow");
            }
        });
        super::super::e2e::measure_control("tb-overflow", trigger)
    }
}

#[derive(gpui::IntoElement)]
struct OverflowMenu {
    app: gpui::WeakEntity<KagiApp>,
    hidden: u16,
    reasons: [Option<&'static str>; 14],
    root_focus: Option<gpui::FocusHandle>,
}

struct MenuState {
    menu: Option<gpui::Entity<PopupMenu>>,
    open: bool,
    previous_focus: Option<gpui::FocusHandle>,
    trigger_focus: gpui::FocusHandle,
    popover: Option<gpui::WeakEntity<gpui_component::popover::PopoverState>>,
}

impl gpui::RenderOnce for OverflowMenu {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let cached = window.use_keyed_state("toolbar-overflow-menu", cx, |_, cx| MenuState {
            menu: None,
            open: false,
            previous_focus: None,
            trigger_focus: cx.focus_handle(),
            popover: None,
        });
        if self.hidden == 0 {
            let (reset_focus, popover) = {
                let state = cached.read(cx);
                (
                    state.open || state.trigger_focus.is_focused(window),
                    state.popover.clone(),
                )
            };
            if reset_focus {
                if let Some(root) = self.root_focus {
                    root.focus(window, cx);
                }
                if let Some(popover) = popover {
                    window.on_next_frame(move |window, cx| {
                        let _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                    });
                }
            }
            cached.update(cx, |state, _| {
                state.open = false;
                state.menu = None;
                state.previous_focus = None;
            });
            return div().into_any_element();
        }
        let open = cached.read(cx).open;
        let changed_state = cached.downgrade();
        let changed_app = self.app.clone();
        Popover::new("toolbar-overflow-popover")
            .appearance(false)
            .overlay_closable(false)
            .anchor(gpui::Anchor::TopRight)
            .open(open)
            .on_open_change(move |open, window, cx| {
                let previous = changed_state
                    .update(cx, |state, _| {
                        state.open = *open;
                        if !open {
                            state.menu = None;
                            state.previous_focus.take()
                        } else {
                            None
                        }
                    })
                    .ok()
                    .flatten();
                let _ = changed_app.update(cx, |app, cx| {
                    if !open {
                        let focus = if app.active_modal.is_some() {
                            app.root_focus.clone()
                        } else {
                            previous
                        };
                        if let Some(focus) = focus {
                            focus.focus(window, cx);
                        }
                    }
                    cx.notify();
                });
            })
            .trigger(OverflowTrigger {
                selected: false,
                state: cached.downgrade(),
                app: self.app.clone(),
                focus: cached.read(cx).trigger_focus.clone(),
            })
            .content(move |_, window, cx| {
                if let Some(menu) = cached.read(cx).menu.clone() {
                    return menu;
                }
                let app = self.app.clone();
                let hidden = self.hidden;
                let reasons = self.reasons;
                let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
                    for index in 0..IDS.len() {
                        if hidden & (1 << index) == 0 {
                            continue;
                        }
                        let name = label(index);
                        let reason = reasons[index];
                        let item_app = app.clone();
                        // Kbd reads the actual window action bindings, including
                        // overrides; the shared handler still owns activation.
                        let action: Option<Box<dyn gpui::Action>> = match index {
                            0 => Some(commands::Pull.boxed_clone()),
                            1 => Some(commands::Push.boxed_clone()),
                            2 => Some(commands::NewBranch.boxed_clone()),
                            5 => Some(commands::HistoryUndo.boxed_clone()),
                            6 => Some(commands::HistoryRedo.boxed_clone()),
                            7 => Some(ToggleBottomPanel.boxed_clone()),
                            8 => Some(commands::ShowGraph.boxed_clone()),
                            9 => Some(commands::TogglePrMode.boxed_clone()),
                            10 => Some(commands::ToggleEditorWorkspace.boxed_clone()),
                            12 => Some(commands::OpenSettings.boxed_clone()),
                            _ => None,
                        };
                        let item = PopupMenuItem::element(move |window, _| {
                            div()
                                .id(("toolbar-overflow-item", index))
                                .relative()
                                .role(gpui::Role::MenuItem)
                                .aria_label(name)
                                .when_some(reason, |el, reason| el.aria_description(reason))
                                .when(reason.is_some(), |el| {
                                    el.a11y_synthetic_children(
                                        |builder: &mut gpui::A11ySubtreeBuilder| {
                                            builder.parent_node().set_disabled()
                                        },
                                    )
                                })
                                .flex()
                                .items_center()
                                .justify_between()
                                .w_full()
                                .gap_4()
                                .child(div().flex().flex_col().child(name).when_some(
                                    reason,
                                    |el, reason| {
                                        el.child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(theme().text_muted))
                                                .child(reason),
                                        )
                                    },
                                ))
                                .when_some(
                                    action.as_deref().and_then(|action| {
                                        gpui_component::kbd::Kbd::binding_for_action(
                                            action, None, window,
                                        )
                                    }),
                                    |el, kbd| {
                                        let kbd = kbd.appearance(false);
                                        #[cfg(feature = "gui-e2e")]
                                        let kbd = super::super::e2e::measure_control(
                                            format!("tb-overflow-shortcut-{index}"),
                                            kbd,
                                        );
                                        el.child(kbd)
                                    },
                                )
                                .when(cfg!(feature = "gui-e2e"), |el| {
                                    el.child(super::super::e2e::measure_inside(format!(
                                        "tb-overflow-item-{index}"
                                    )))
                                })
                        })
                        .on_click(move |_, window, cx| {
                            let _ = item_app.update(cx, |app, cx| {
                                app.activate_toolbar(index, reason, window, cx)
                            });
                        });
                        menu = menu.item(item);
                    }
                    menu
                });
                let popover = cx.entity().downgrade();
                cached.update(cx, |state, _| {
                    state.menu = Some(menu.clone());
                    state.popover = Some(popover.clone());
                });
                menu.focus_handle(cx).focus(window, cx);
                let dismissed_cache = cached.downgrade();
                window
                    .subscribe(&menu, cx, move |_, _: &gpui::DismissEvent, window, cx| {
                        let _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                        let _ = dismissed_cache.update(cx, |state, _| state.menu = None);
                    })
                    .detach();
                menu
            })
            .into_any_element()
    }
}
