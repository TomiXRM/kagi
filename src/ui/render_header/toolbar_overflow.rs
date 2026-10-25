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
pub(super) const ICON_IDENTITY_MIN_WIDTH: f32 = 96.;
#[derive(Default)]
pub(crate) struct Layout {
    available: f32,
    widths: [f32; 17],
    key: Option<StyleKey>,
    last_hidden: u16,
}

impl Layout {
    pub(super) fn icon_identity_width(&self, key: StyleKey) -> f32 {
        ICON_IDENTITY_MIN_WIDTH * (theme::BASE_REM_PX / 16.) * f32::from_bits(key.2)
    }
    pub(super) fn sidebar_width(&self, visible: bool, key: StyleKey) -> f32 {
        if visible {
            self.widths[16] * f32::from_bits(key.2)
        } else {
            0.
        }
    }
    pub(super) fn identity_width(&self, sidebar: bool, key: StyleKey) -> f32 {
        (self.widths[14] + self.widths[15] + 24.) * f32::from_bits(key.2)
            + self.sidebar_width(sidebar, key)
    }
    pub(super) fn workspace_width(&self, visible: u16, hidden: u16, key: StyleKey) -> f32 {
        let scale = f32::from_bits(key.2);
        self.widths[8..IDS.len()].iter().enumerate().fold(
            spacing(visible, scale).1 + if hidden != 0 { 36. * scale } else { 0. },
            |width, (index, natural)| {
                width
                    + if visible & (1 << (index + 8)) != 0 {
                        natural * scale
                    } else {
                        0.
                    }
            },
        )
    }
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

    pub(super) fn pending(&self, present: u16, key: StyleKey) -> u16 {
        if self.key != Some(key) {
            return present;
        }
        self.widths
            .iter()
            .enumerate()
            .fold(0, |pending, (i, width)| {
                pending
                    | if *width == 0. && i < IDS.len() {
                        present & (1 << i)
                    } else {
                        0
                    }
            })
    }

    pub(super) fn hidden(
        &mut self,
        present: u16,
        selected: usize,
        sidebar: bool,
        key: StyleKey,
    ) -> u16 {
        if self.available == 0.
            || self.pending(present, key) != 0
            || !key.0 && (self.widths[14] == 0. || self.widths[15] == 0.)
            || sidebar && self.widths[16] == 0.
        {
            // A style change must not remove the focused trigger or an open
            // menu for the measurement frame. Hidden controls get a natural
            // width probe without an interactive subtree while pending.
            return self.last_hidden & present;
        }
        let scale = f32::from_bits(key.2);
        let selected_mask = if selected < IDS.len() {
            1 << selected
        } else {
            0
        };
        let (mut repo, mut workspace) = (0., 0.);
        for (i, width) in self.widths.iter().enumerate() {
            if i < IDS.len() && present & (1 << i) != 0 {
                if i < 8 {
                    repo += width * scale;
                } else {
                    workspace += width * scale;
                }
            }
        }
        // Budget each side column's minimum and only the gaps actually drawn.
        let required = |repo: f32, workspace: f32, hidden: u16| {
            let visible = present & !hidden;
            let (repo_spacing, workspace_spacing) = spacing(visible, scale);
            let overflow = if hidden != 0 { 36. * scale } else { 0. };
            if !key.0 {
                // Both flex side columns retain their measured minimums;
                // extra space is shared equally when those minimums permit it.
                // Include Refresh/identity margins and the drag spacer.
                let identity = self.identity_width(sidebar, key);
                return identity
                    + repo
                    + repo_spacing
                    + workspace
                    + workspace_spacing
                    + overflow
                    + 24. * scale;
            }
            let workspace_minimum = if visible & 0x3f00 & !selected_mask != 0 {
                100. * scale
            } else {
                0.
            };
            let side = (workspace + workspace_spacing + overflow).max(workspace_minimum);
            // The right group may yield its empty reservation, not the left
            // Refresh/repository/branch summary's readable minimum.
            repo + repo_spacing
                + side
                + side.max(
                    ICON_IDENTITY_MIN_WIDTH * (theme::BASE_REM_PX / 16.) * scale
                        + self.sidebar_width(sidebar, key),
                )
                // Symmetric toolbar padding; native lights and + are above.
                + 24. * scale
        };
        if required(repo, workspace, 0) <= self.available {
            self.last_hidden = 0;
            return 0;
        }
        let mut hidden = 0;
        let order = [13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
        for i in order {
            if i == selected || present & (1 << i) == 0 {
                continue;
            }
            hidden |= 1 << i;
            if i < 8 {
                repo -= self.widths[i] * scale;
            } else {
                workspace -= self.widths[i] * scale;
            }
            if required(repo, workspace, hidden) <= self.available {
                self.last_hidden = hidden;
                return hidden;
            }
        }
        if selected < IDS.len() {
            hidden |= present & (1 << selected);
        }
        self.last_hidden = hidden;
        hidden
    }
}

/// Shared by the rendered gaps and the fitting model. The right set may include
/// later neighbours when an entire intermediate group has overflowed.
pub(super) fn gap(visible: u16, left: u16, right: u16) -> bool {
    visible & left != 0 && visible & right != 0
}

fn spacing(visible: u16, scale: f32) -> (f32, f32) {
    let sum = |gaps: &[(u16, u16, bool)]| {
        gaps.iter().fold(0., |width, &(left, right, hairline)| {
            width
                + if gap(visible, left, right) {
                    if hairline {
                        8. * scale + 1.
                    } else {
                        2. * scale
                    }
                } else {
                    0.
                }
        })
    };
    (
        sum(&[
            (1, 2, false),
            (3, 0xfc, true),
            (4, 0x18, false),
            (8, 0x10, false),
            (0x1c, 0xe0, true),
            (0x60, 0x80, false),
        ]),
        sum(&[
            (0x100, 0xe00, false),
            (0x200, 0xc00, false),
            (0x400, 0x800, false),
            (0xf00, 0x1000, true),
        ]) + if visible & (1 << 13) != 0 {
            8. * scale
        } else {
            0.
        },
    )
}

pub(super) fn minimum_width(icon_only: bool) -> f32 {
    if icon_only {
        36.
    } else {
        52.
    }
}

pub(super) fn measure_hidden(
    app: gpui::WeakEntity<KagiApp>,
    index: usize,
    label: &'static str,
    icon_only: bool,
    key: StyleKey,
) -> gpui::Stateful<gpui::Div> {
    // Same width-producing styles as make_btn, but no focus, AX, tooltip or
    // command subtree. Visibility affects paint, not natural layout/prepaint.
    div()
        .id(("toolbar-width-probe", index))
        .absolute()
        .invisible()
        .flex()
        .flex_col()
        .items_center()
        .border_2()
        .border_color(gpui::transparent_black())
        .min_w(theme::scaled_px(minimum_width(icon_only)))
        .px(keyboard_nav::inset(4.))
        .child(div().w(theme::scaled_px(22.)))
        .when(!icon_only, |el| el.child(div().text_xs().child(label)))
        .child(probe(app, Some(index), key))
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
            // Absolute canvases measure inside the fixed 2px focus border.
            // Both toolbar presentations carry that border; identity and the sidebar wrapper do not.
            let normalized = if index.is_some() {
                (width
                    + if index.is_some_and(|i| i <= 14) {
                        4.
                    } else {
                        0.
                    })
                    / f32::from_bits(key.2)
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
        let trigger = super::overflow_trigger::trigger(
            "tb-overflow",
            Msg::ToolbarOverflow.t(),
            self.selected,
            &self.focus,
            |trigger| {
                let trigger = keyboard_nav::focusable(
                    trigger
                        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                            let focus = window.focused(cx);
                            let _ = pointer_state.update(cx, |state, _| {
                                if !state.open {
                                    state.previous_focus = focus;
                                }
                            });
                        })
                        // Popover opens on mouse-down; keyboard clicks open it explicitly.
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
                trigger
            },
        );
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
    // Retain state until synchronous dismissal has released deferred context.
    popover: Option<gpui::Entity<gpui_component::popover::PopoverState>>,
    snapshot: Option<(u16, [Option<&'static str>; 14], i18n::Lang, gpui::Pixels)>,
}

impl MenuState {
    fn owns_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        window.focused(cx).is_none()
            || self.trigger_focus.is_focused(window)
            || self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.focus_handle(cx).contains_focused(window, cx))
    }
}

impl gpui::RenderOnce for OverflowMenu {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let cached = window.use_keyed_state("toolbar-overflow-menu", cx, |_, cx| MenuState {
            menu: None,
            open: false,
            previous_focus: None,
            trigger_focus: cx.focus_handle(),
            popover: None,
            snapshot: None,
        });
        if self.hidden == 0 {
            let (reset_focus, popover) = {
                let state = cached.read(cx);
                (
                    state.trigger_focus.is_focused(window)
                        || (state.open && state.owns_focus(window, cx)),
                    state.popover.clone(),
                )
            };
            // Dismiss while the old element's state is still alive. Deferring
            // a weak update until the next frame can strand the library's
            // process-wide deferred-popover registration after state disposal.
            if let Some(popover) = popover {
                popover.update(cx, |state, cx| state.dismiss(window, cx));
            }
            if reset_focus {
                if let Some(root) = self.root_focus {
                    root.focus(window, cx);
                }
            }
            cached.update(cx, |state, _| {
                state.open = false;
                state.menu = None;
                state.previous_focus = None;
                state.popover = None;
                state.snapshot = None;
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
                let (previous, restore) = changed_state
                    .update(cx, |state, cx| {
                        let restore = !open && state.owns_focus(window, cx);
                        state.open = *open;
                        if !open {
                            state.menu = None;
                            state.snapshot = None;
                            (state.previous_focus.take(), restore)
                        } else {
                            (None, false)
                        }
                    })
                    .unwrap_or((None, false));
                let _ = changed_app.update(cx, |app, cx| {
                    if !open && restore {
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
                let max_width = (window.viewport_size().width - px(32.))
                    .max(px(0.))
                    .min(px(500.));
                let snapshot = (self.hidden, self.reasons, i18n::lang(), max_width);
                let state = cached.read(cx);
                if state.snapshot == Some(snapshot) {
                    if let Some(menu) = state.menu.clone() {
                        return super::super::e2e::measure_control("tb-overflow-menu", menu);
                    }
                }
                let app = self.app.clone();
                let hidden = self.hidden;
                let reasons = self.reasons;
                let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
                    // PopupMenu caps its inner items; its outer frame adds two
                    // literal 1px borders. Include those in the viewport limit.
                    let items_width = (max_width - px(2.)).max(px(0.));
                    menu = menu.min_w(items_width.min(px(200.))).max_w(items_width);
                    for (index, &reason) in reasons.iter().enumerate() {
                        if hidden & (1 << index) == 0 {
                            continue;
                        }
                        let name = label(index);
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
                            #[cfg(feature = "gui-e2e")]
                            if let Some(reason) = reason {
                                super::toolbar_a11y::record_description(IDS[index], reason);
                            }
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
                                .items_start()
                                .justify_between()
                                .w_full()
                                // The kit wraps custom content in an intrinsic
                                // flex row: constrain this row too, accounting
                                // for items p_1 and fixed 8px item side padding.
                                .max_w(
                                    (items_width - window.rem_size() * 0.5 - px(16.)).max(px(0.)),
                                )
                                .gap_4()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .flex()
                                        .flex_col()
                                        .child(div().truncate().child(name))
                                        .when_some(reason, |el, reason| {
                                            el.child(
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .text_color(rgb(theme().text_muted))
                                                    .child(reason),
                                            )
                                        }),
                                )
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
                                        el.child(div().flex_shrink_0().child(kbd))
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
                let popover_state = cx.entity();
                let popover = popover_state.downgrade();
                cached.update(cx, |state, _| {
                    state.menu = Some(menu.clone());
                    state.popover = Some(popover_state);
                    state.snapshot = Some(snapshot);
                });
                menu.focus_handle(cx).focus(window, cx);
                let dismissed_cache = cached.downgrade();
                window
                    .subscribe(&menu, cx, move |_, _: &gpui::DismissEvent, window, cx| {
                        let _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                        let _ = dismissed_cache.update(cx, |state, _| state.menu = None);
                    })
                    .detach();
                super::super::e2e::measure_control("tb-overflow-menu", menu)
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_overflow_priority_preserves_git_until_inactive_modes_hide() {
        for icon_only in [true, false] {
            for language in [i18n::Lang::En, i18n::Lang::Ja] {
                for scale in [1_f32, 1.67] {
                    for selected in [8, 9, 10, usize::MAX] {
                        let key = (icon_only, language, scale.to_bits());
                        let mut layout = Layout::default();
                        layout.measure(None, 3000. * scale, key);
                        for index in 0..16 {
                            layout.measure(Some(index), 60., key);
                        }
                        let present = 0x3fff;
                        let mut order = [13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
                        if let Some(position) = order.iter().position(|i| *i == selected) {
                            order[position..].rotate_left(1);
                        }
                        let mut stages = 0_u16;
                        for available in (1..=3000).rev() {
                            layout.measure(None, available as f32 * scale, key);
                            let hidden = layout.hidden(present, selected, false, key);
                            let count = hidden.count_ones() as usize;
                            let expected = order[..count]
                                .iter()
                                .fold(0, |mask, index| mask | (1 << index));
                            assert_eq!(hidden, expected, "{key:?}/{selected}/{available}");
                            stages |= 1 << count;
                        }
                        assert_eq!(stages, 0x7fff, "all 15 stages must be reachable");
                        for available in 1..=3000 {
                            layout.measure(None, available as f32 * scale, key);
                            let hidden = layout.hidden(present, selected, false, key);
                            let count = hidden.count_ones() as usize;
                            assert_eq!(
                                hidden,
                                order[..count]
                                    .iter()
                                    .fold(0, |mask, index| mask | (1 << index)),
                                "expansion must restore the same priority"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn toolbar_overflow_releases_selected_only_workspace_reservation_for_git() {
        let key = (true, i18n::Lang::En, 1_f32.to_bits());
        let mut layout = Layout::default();
        // The toolbar no longer reserves native lights or the titlebar's +.
        let available = 524.;
        layout.measure(None, available, key);
        for index in 0..IDS.len() {
            layout.measure(Some(index), 36., key);
        }
        assert_eq!(
            layout.hidden(0x3fff, 8, false, key),
            0x3e00,
            "once only selected Graph remains, every Git command fits without the 100px side reservation"
        );
    }

    #[test]
    fn toolbar_overflow_keeps_fit_while_new_style_is_measuring() {
        let key = (true, i18n::Lang::En, 1_f32.to_bits());
        let mut layout = Layout::default();
        layout.measure(None, 500., key);
        for index in 0..IDS.len() {
            layout.measure(Some(index), 60., key);
        }
        let present = 0x1fff;
        let hidden = layout.hidden(present, 8, false, key);
        assert_ne!(hidden, 0);
        let next = (false, i18n::Lang::Ja, 1_f32.to_bits());
        assert_eq!(
            layout.hidden(present, 8, false, next),
            hidden,
            "pending measurement cannot eject a focused/open overflow trigger"
        );
        layout.measure(None, 500., next);
        assert_eq!(
            layout.hidden(present, 8, false, next),
            hidden,
            "partial measurement must preserve the last fit"
        );
        assert_eq!(
            layout.hidden(1 << 8, 8, false, next),
            hidden & (1 << 8),
            "commands no longer present cannot remain in the menu"
        );
    }
}

#[cfg(test)]
mod spacing_tests {
    #[test]
    fn toolbar_overflow_spacing_requires_visible_neighbours() {
        let scale = 1.;
        let spacing = |visible| super::spacing(visible, scale);
        assert_eq!(spacing(0), (0., 0.));
        for i in 0..13 {
            assert_eq!(
                spacing(1 << i),
                (0., 0.),
                "a lone command has no dangling gap"
            );
        }
        assert_eq!(spacing(0xff).0, 24. * scale + 2.);
        assert_eq!(spacing(0x1d00).1, 12. * scale + 1.);
        assert_eq!(spacing(0x1f00).1, 14. * scale + 1.);
        assert_eq!(
            spacing(1 | (1 << 5)).0,
            8. * scale + 1.,
            "an absent middle group leaves one separator, not two"
        );
        assert_eq!(spacing((1 << 5) | (1 << 7)).0, 2. * scale);
    }
}
