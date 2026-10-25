//! Linux/FreeBSD in-app platform menu dropdown (ADR-0085).
//!
//! Split from `ui/mod.rs` (T-HOTSPOT-UIMOD-001) as an additional `impl KagiApp`
//! block. The owner passes its activation state so menu highlights follow the
//! same render seam as selected rows.
//!
//! Only the Linux/FreeBSD titlebar opens the dropdown (`platform_menu_open`),
//! but the renderer compiles on every target so the macOS GUI runner can lay
//! it out (#935); elsewhere it returns `None` because nothing opens it.

use crate::ui::*;
use kagi_ui_core::tooltip::HoverTooltip as _;

impl KagiApp {
    pub(crate) fn render_platform_menu_dropdown(
        &self,
        window_active: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        // Use the same filtered section as the visibility/keyboard gate; the
        // titlebar and dropdown must agree on index and available rows.
        let (ix, section) = self.visible_platform_menu_section()?;
        let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
            this.platform_menu_open = None;
            cx.stop_propagation();
            cx.notify();
        });

        // #935: the panel's height follows its rows (every theme, custom ones
        // included, plus the language rows), so it is capped to the window
        // and scrolls inside itself. The column holding it spans from just
        // below the menu bar to a margin above the window's bottom edge;
        // the panel takes at most that column's height. The column has no
        // listeners, so clicks beside the panel still reach the backdrop.
        let mut panel = div()
            .id("platform-menu-panel")
            // Block mouse events from reaching the dismiss backdrop below —
            // without this, pressing a menu item fires the backdrop's
            // on_mouse_down first, the menu unmounts, and the item's on_click
            // (down+up on the same element) never completes. Same fix as the
            // commit context menu (see context_menu.rs).
            .occlude()
            .w_full()
            .max_h_full()
            .overflow_y_scroll()
            .py_1()
            .rounded(theme::scaled_px(6.0))
            .border_1()
            .border_color(rgb(theme().elevated_border(theme().selected)))
            .bg(rgb(theme().elevated_surface(theme().panel)))
            .shadow_lg();
        // Tier A reads the panel's bounds (#935); compiled out of normal builds.
        #[cfg(feature = "gui-e2e")]
        {
            panel = panel
                .relative()
                .child(super::e2e::measure_inside("platform-menu-panel"));
        }

        // ADR-0085: one clickable command row, reused for plain `Command` nodes
        // and for the inline-expanded Theme/Language submenu rows.  `row_ix` is
        // only used to build a stable element id.
        let command_row = |this: &Self,
                           cx: &mut Context<Self>,
                           row_ix: usize,
                           id: &'static str|
         -> gpui::AnyElement {
            let state = if id == "editor.save" {
                if this.editor_save_available(cx) {
                    commands::CommandState::Enabled
                } else {
                    commands::CommandState::Disabled(Msg::NothingToSave.t())
                }
            } else {
                commands::command_state_in(this, id, cx)
            };
            let enabled = matches!(state, commands::CommandState::Enabled);
            // Selection is a separate renderer column, not part of the name.
            let menu_state = super::window_menu::ViewMenuState {
                sidebar_visible: this.sidebar.visible,
                workspace_mode: (!this.home_in_front()).then(|| this.workspace_mode()),
                editor_details_visible: this.workspace_mode()
                    == super::workspace_mode::WorkspaceMode::Editor
                    && this
                        .ui()
                        .editor_workspace
                        .as_ref()
                        .is_some_and(|view| view.read(cx).details_visible()),
                ..super::window_menu::current_state(cx)
            };
            let checked = super::window_menu::checked(id, &menu_state);
            let label = super::window_menu::state_label(id, &menu_state).unwrap_or_else(|| {
                if id == "editor.save" {
                    Msg::MenuSave.t()
                } else {
                    super::command_palette::command_label(id, i18n::lang())
                }
            });
            // Render the stored `secondary-*` notation as a platform label
            // (Ctrl+J on Linux), so the menu matches what the user must press.
            let key = if id == "editor.save" {
                commands::display_keystroke("secondary-s")
            } else {
                commands::effective_keystroke(id)
                    .map(|k| commands::display_keystroke(&k))
                    .unwrap_or_default()
            };
            let invoke = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                if id == "editor.save" && this.editor_save_available(cx) {
                    this.close_platform_menu(cx);
                    this.save_editor_file(cx);
                    cx.notify();
                } else if commands::is_enabled(this, id, cx) {
                    this.close_platform_menu(cx);
                    this.handle_menu_command(id, window, cx);
                    cx.notify();
                }
                cx.stop_propagation();
            });
            let disabled_reason = match state {
                commands::CommandState::Disabled(reason) => Some(reason),
                _ => None,
            };

            let row = div()
                .id(SharedString::from(format!(
                    "platform-menu-item-{ix}-{row_ix}"
                )))
                .group("platform-menu-row")
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py(theme::scaled_px(5.0))
                .text_sm()
                .text_color(if enabled {
                    rgb(theme().text_main)
                } else {
                    rgb(theme().text_muted)
                })
                .when(enabled, |s| {
                    s.cursor_pointer()
                        .hover(move |s| super::menu_overlay::highlight_style(s, window_active))
                        .on_click(invoke)
                })
                .child(check_column(checked))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .child(SharedString::from(label))
                        .when(cfg!(feature = "gui-e2e"), |s| {
                            s.relative().child(super::e2e::measure_inside(format!(
                                "platform-menu-cmd-{id}-primary"
                            )))
                        }),
                )
                .when(!key.is_empty(), move |s| {
                    s.child(
                        div()
                            // Group hover must reach text layout, not only its paint probe.
                            .id("shortcut")
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .when(
                                enabled && theme().selection_uses_on_accent(true, window_active),
                                |s| {
                                    s.group_hover("platform-menu-row", |s| {
                                        s.text_color(
                                            theme().selection_secondary_foreground(window_active),
                                        )
                                    })
                                },
                            )
                            .child(SharedString::from(key))
                            .when(cfg!(feature = "gui-e2e"), |s| {
                                s.relative().child(super::e2e::measure_inside(format!(
                                    "platform-menu-cmd-{id}-shortcut"
                                )))
                            }),
                    )
                });
            // Tier A reads the row's bounds (#935). Absolute and compiled out
            // of normal builds, where it would be a flow child in the gap.
            #[cfg(feature = "gui-e2e")]
            let row = row.relative().child(super::e2e::measure_inside(format!(
                "platform-menu-cmd-{id}"
            )));
            if let Some(reason) = disabled_reason {
                row.hover_tooltip_builder(
                    SharedString::from(format!("platform-menu-help-{ix}-{row_ix}")),
                    move |window, cx| Tooltip::new(reason.to_string()).build(window, cx),
                )
                .into_any_element()
            } else {
                row.into_any_element()
            }
        };

        // `row_ix` is a running counter (submenus expand to several rows, so it
        // diverges from `item_ix`) — it only needs to be unique within a panel.
        let mut row_ix = 0usize;
        for (item_ix, node) in section.items.iter().enumerate() {
            match node {
                commands::MenuNode::Separator => {
                    panel = panel.child(
                        div()
                            .id(SharedString::from(format!(
                                "platform-menu-separator-{ix}-{item_ix}"
                            )))
                            .my_1()
                            .h(px(1.0))
                            .bg(rgb(theme().elevated_border(theme().surface))),
                    );
                }
                commands::MenuNode::Command(id) => {
                    panel = panel.child(command_row(self, cx, row_ix, id));
                    row_ix += 1;
                }
                commands::MenuNode::EditorSave => {
                    panel = panel.child(command_row(self, cx, row_ix, "editor.save"));
                    row_ix += 1;
                }
                // ADR-0085 §3: the dropdown has no nested-panel support, so the
                // dynamic submenus expand inline as rows — preserving the
                // previous View-menu behaviour on Linux. Theme rows come from
                // the same runtime list as the macOS submenu (#922), so custom
                // themes appear here too.
                commands::MenuNode::Submenu(commands::DynSubmenu::Theme) => {
                    for entry in commands::theme_menu_entries() {
                        panel = panel.child(theme_row(cx, ix, row_ix, entry, window_active));
                        row_ix += 1;
                    }
                }
                commands::MenuNode::Submenu(commands::DynSubmenu::Language) => {
                    for id in commands::LANG_COMMAND_IDS {
                        panel = panel.child(command_row(self, cx, row_ix, id));
                        row_ix += 1;
                    }
                }
                commands::MenuNode::Submenu(commands::DynSubmenu::ToolbarDisplay) => {
                    for (mode, label, checked) in commands::toolbar_display_menu_entries() {
                        let invoke = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                            this.close_platform_menu(cx);
                            window.dispatch_action(
                                Box::new(commands::SetToolbarDisplay { mode }),
                                cx,
                            );
                            cx.stop_propagation();
                        });
                        let row = div()
                            .id(SharedString::from(format!(
                                "platform-menu-item-{ix}-{row_ix}"
                            )))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py(theme::scaled_px(5.0))
                            .text_sm()
                            .text_color(rgb(theme().text_main))
                            .cursor_pointer()
                            .hover(move |style| {
                                super::menu_overlay::highlight_style(style, window_active)
                            })
                            .on_click(invoke)
                            .child(check_column(checked))
                            .child(div().flex_1().truncate().child(label).when(
                                cfg!(feature = "gui-e2e"),
                                |s| {
                                    s.relative().child(super::e2e::measure_inside(format!(
                                        "platform-menu-toolbar-{}-primary",
                                        mode.as_str()
                                    )))
                                },
                            ));
                        #[cfg(feature = "gui-e2e")]
                        let row = row.relative().child(super::e2e::measure_inside(format!(
                            "platform-menu-toolbar-{}",
                            mode.as_str()
                        )));
                        panel = panel.child(row);
                        row_ix += 1;
                    }
                }
                commands::MenuNode::Submenu(commands::DynSubmenu::OpenRecent) => {
                    let separator = |probe| {
                        div()
                            .my_1()
                            .h(px(1.0))
                            .bg(rgb(theme().elevated_border(theme().surface)))
                            .when(cfg!(feature = "gui-e2e"), |el| {
                                el.relative().child(super::e2e::measure_inside(probe))
                            })
                    };
                    let recent = commands::recent_menu();
                    panel = panel.child(separator("platform-menu-recent-before")).child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py(theme::scaled_px(5.0))
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(check_column(false))
                            .child(recent.name)
                            .when(cfg!(feature = "gui-e2e"), |el| {
                                el.relative().child(super::e2e::measure_inside(
                                    "platform-menu-recent-heading",
                                ))
                            }),
                    );
                    for (recent_ix, item) in recent.items.into_iter().enumerate() {
                        if matches!(item, gpui::MenuItem::Separator) {
                            panel = panel.child(separator("platform-menu-recent-separator"));
                            continue;
                        }
                        let gpui::MenuItem::Action {
                            name,
                            action,
                            disabled,
                            ..
                        } = item
                        else {
                            continue;
                        };
                        let row = div()
                            .id(SharedString::from(format!(
                                "platform-menu-item-{ix}-{row_ix}"
                            )))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py(theme::scaled_px(5.0))
                            .text_sm()
                            .text_color(rgb(if disabled {
                                theme().text_muted
                            } else {
                                theme().text_main
                            }))
                            .child(check_column(false))
                            .child(div().flex_1().truncate().child(name).when(
                                cfg!(feature = "gui-e2e"),
                                |el| {
                                    el.relative().child(super::e2e::measure_inside(format!(
                                        "platform-menu-recent-{recent_ix}",
                                    )))
                                },
                            ))
                            .when(!disabled, |row| {
                                row.cursor_pointer()
                                    .hover(move |style| {
                                        super::menu_overlay::highlight_style(style, window_active)
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.close_platform_menu(cx);
                                        window.dispatch_action(action.boxed_clone(), cx);
                                    }))
                            });
                        panel = panel.child(row);
                        row_ix += 1;
                    }
                    panel = panel.child(separator("platform-menu-recent-after"));
                }
                // OsEdit only ever appears in `mac_only` sections, which are
                // filtered out before we get here — but match exhaustively.
                commands::MenuNode::Submenu(commands::DynSubmenu::Find) => {}
                commands::MenuNode::OsEdit(_) => {}
                #[cfg(target_os = "macos")]
                commands::MenuNode::Services => {}
            }
        }

        Some(
            div()
                .absolute()
                .top(gpui_component::TITLE_BAR_HEIGHT)
                .left_0()
                .right_0()
                .bottom_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .on_mouse_down(MouseButton::Left, dismiss),
                )
                .child(
                    div()
                        .absolute()
                        .top_1()
                        .bottom(theme::scaled_px(8.0))
                        .left(theme::scaled_px(8.0 + ix as f32 * 78.0))
                        .w(theme::scaled_px(260.0))
                        .flex()
                        .flex_col()
                        .child(panel),
                )
                .into_any_element(),
        )
    }
}

/// One View → Theme row of the dropdown: every registered theme, built-in or
/// custom, switches by slug (#922).
fn theme_row(
    cx: &mut Context<KagiApp>,
    ix: usize,
    row_ix: usize,
    entry: commands::ThemeMenuEntry,
    window_active: bool,
) -> gpui::AnyElement {
    // Tier A reads the row's bounds (#935); compiled out of normal builds.
    #[cfg(feature = "gui-e2e")]
    let probe = super::e2e::measure_inside(format!("platform-menu-theme-{}", entry.slug));
    #[cfg(feature = "gui-e2e")]
    let primary_probe =
        super::e2e::measure_inside(format!("platform-menu-theme-{}-primary", entry.slug));
    let slug = entry.slug;
    let invoke = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
        this.platform_menu_open = None;
        this.set_theme(&slug, cx);
        cx.stop_propagation();
    });
    let primary = div().flex_1().truncate().child(entry.name);
    #[cfg(feature = "gui-e2e")]
    let primary = primary.relative().child(primary_probe);
    let row = div()
        .id(SharedString::from(format!(
            "platform-menu-item-{ix}-{row_ix}"
        )))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py(theme::scaled_px(5.0))
        .text_sm()
        .text_color(rgb(theme().text_main))
        .cursor_pointer()
        .hover(move |s| super::menu_overlay::highlight_style(s, window_active))
        .on_click(invoke)
        .child(check_column(entry.active))
        .child(primary);
    #[cfg(feature = "gui-e2e")]
    let row = row.relative().child(probe);
    row.into_any_element()
}

fn check_column(checked: bool) -> impl IntoElement {
    div()
        .w(theme::scaled_px(12.0))
        .flex_shrink_0()
        .child(if checked { "✓" } else { "" })
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Open the in-app menu section labelled `label`, as its Linux/FreeBSD
    /// head does on click (#935). `false` when no such section is drawn.
    pub fn open_platform_menu_for_e2e(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let Some(ix) = commands::linux_menu_sections().position(|s| s.label == label) else {
            return false;
        };
        self.platform_menu_open = Some(ix);
        cx.notify();
        true
    }
}
