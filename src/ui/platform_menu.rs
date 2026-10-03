//! Linux/FreeBSD in-app platform menu dropdown (ADR-0085).
//!
//! Moved verbatim from `ui/mod.rs` (T-HOTSPOT-UIMOD-001) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates, but `render_platform_menu_dropdown`
//! is called from `render.rs` (a sibling), so it is `pub(crate)` here.
//!
//! Only the Linux/FreeBSD titlebar opens the dropdown (`platform_menu_open`),
//! but the renderer compiles on every target so the macOS GUI runner can lay
//! it out (#935); elsewhere it returns `None` because nothing opens it.

use crate::ui::*;

impl KagiApp {
    pub(crate) fn render_platform_menu_dropdown(
        &self,
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
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
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
            let command = commands::command(id);
            let state = commands::command_state(this, id);
            let enabled = matches!(state, commands::CommandState::Enabled);
            // `platform_menu_label` adds the "✓ " active marker for the current
            // language (no-op for ordinary commands).
            let label = platform_menu_label(id, command.map(|c| c.label).unwrap_or(id));
            // Render the stored `secondary-*` notation as a platform label
            // (Ctrl+J on Linux), so the menu matches what the user must press.
            let key = commands::effective_keystroke(id)
                .map(|k| commands::display_keystroke(&k))
                .unwrap_or_default();
            let invoke = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                if commands::is_enabled(this, id) {
                    this.platform_menu_open = None;
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
                        .hover(|s| s.bg(rgb(theme().selected)))
                        .on_click(invoke)
                })
                .when_some(disabled_reason, |s, reason| {
                    s.tooltip(move |window, cx| Tooltip::new(reason.to_string()).build(window, cx))
                })
                .child(div().flex_1().truncate().child(SharedString::from(label)))
                .when(!key.is_empty(), move |s| {
                    s.child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(key)),
                    )
                });
            // Tier A reads the row's bounds (#935). Absolute and compiled out
            // of normal builds, where it would be a flow child in the gap.
            #[cfg(feature = "gui-e2e")]
            let row = row.relative().child(super::e2e::measure_inside(format!(
                "platform-menu-cmd-{id}"
            )));
            row.into_any_element()
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
                            .bg(rgb(theme().surface)),
                    );
                }
                commands::MenuNode::Command(id) => {
                    panel = panel.child(command_row(self, cx, row_ix, id));
                    row_ix += 1;
                }
                // ADR-0085 §3: the dropdown has no nested-panel support, so the
                // dynamic submenus expand inline as rows — preserving the
                // previous View-menu behaviour on Linux. Theme rows come from
                // the same runtime list as the macOS submenu (#922), so custom
                // themes appear here too.
                commands::MenuNode::Submenu(commands::DynSubmenu::Theme) => {
                    for entry in commands::theme_menu_entries() {
                        panel = panel.child(theme_row(cx, ix, row_ix, entry));
                        row_ix += 1;
                    }
                }
                commands::MenuNode::Submenu(commands::DynSubmenu::Language) => {
                    for id in commands::LANG_COMMAND_IDS {
                        panel = panel.child(command_row(self, cx, row_ix, id));
                        row_ix += 1;
                    }
                }
                // OsEdit only ever appears in `mac_only` sections, which are
                // filtered out before we get here — but match exhaustively.
                commands::MenuNode::OsEdit(_) => {}
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
) -> gpui::AnyElement {
    let label = if entry.active {
        format!("\u{2713} {}", entry.name)
    } else {
        entry.name.to_string()
    };
    // Tier A reads the row's bounds (#935); compiled out of normal builds.
    #[cfg(feature = "gui-e2e")]
    let probe = super::e2e::measure_inside(format!("platform-menu-theme-{}", entry.slug));
    let slug = entry.slug;
    let invoke = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
        this.platform_menu_open = None;
        this.set_theme(&slug, cx);
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
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(invoke)
        .child(div().flex_1().truncate().child(SharedString::from(label)));
    #[cfg(feature = "gui-e2e")]
    let row = row.relative().child(probe);
    row.into_any_element()
}

// Only the in-app menu calls this (✓ marker for the language).
fn platform_menu_label(id: &str, fallback: &str) -> String {
    if let Some(lang) = commands::lang_for_command(id) {
        if i18n::lang() == lang {
            return format!("\u{2713} {fallback}");
        }
    }
    fallback.to_string()
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
