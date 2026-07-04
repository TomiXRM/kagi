//! Linux/FreeBSD in-app platform menu dropdown (ADR-0085).
//!
//! Moved verbatim from `ui/mod.rs` (T-HOTSPOT-UIMOD-001) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates, but `render_platform_menu_dropdown`
//! is called from `render.rs` (a sibling), so it is `pub(crate)` here.

use crate::ui::*;

impl KagiApp {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub(crate) fn render_platform_menu_dropdown(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let ix = self.platform_menu_open?;
        // ADR-0085: index into the *filtered* sections (same iterator the heads
        // use), so the open panel matches the head it was launched from and the
        // left offset (computed from `ix`) lines up.
        let section = commands::linux_menu_sections().nth(ix)?;
        let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
            this.platform_menu_open = None;
            cx.stop_propagation();
            cx.notify();
        });

        let mut panel = div()
            // Block mouse events from reaching the dismiss backdrop below —
            // without this, pressing a menu item fires the backdrop's
            // on_mouse_down first, the menu unmounts, and the item's on_click
            // (down+up on the same element) never completes. Same fix as the
            // commit context menu (see context_menu.rs).
            .occlude()
            .absolute()
            .top_1()
            .left(theme::scaled_px(8.0 + ix as f32 * 78.0))
            .w(theme::scaled_px(260.0))
            .py_1()
            .rounded(theme::scaled_px(6.0))
            .border_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .shadow_lg();

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
            // theme / language (no-op for ordinary commands).
            let label = platform_menu_label(id, command.map(|c| c.label).unwrap_or(id));
            // Render the stored `secondary-*` notation as a platform label
            // (Ctrl+J on Linux), so the menu matches what the user must press.
            let key = command
                .and_then(|c| c.keystroke)
                .map(commands::display_keystroke)
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

            div()
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
                })
                .into_any_element()
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
                // dynamic submenus expand inline as command rows (the "✓ " marker
                // is applied by `platform_menu_label`) — preserving the previous
                // View-menu behaviour on Linux.
                commands::MenuNode::Submenu(commands::DynSubmenu::Theme) => {
                    for id in commands::THEME_COMMAND_IDS {
                        panel = panel.child(command_row(self, cx, row_ix, id));
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
                .child(panel)
                .into_any_element(),
        )
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    pub(crate) fn render_platform_menu_dropdown(
        &self,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        None
    }
}

// Only the Linux/FreeBSD in-app menu calls this (✓ marker for theme/lang).
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
fn platform_menu_label(id: &str, fallback: &str) -> String {
    if let Some(slug) = commands::theme_slug_for_command(id) {
        if theme::theme().slug == slug {
            return format!("\u{2713} {fallback}");
        }
    }
    if let Some(lang) = commands::lang_for_command(id) {
        if i18n::lang() == lang {
            return format!("\u{2713} {fallback}");
        }
    }
    fallback.to_string()
}
