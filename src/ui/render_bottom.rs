//! Bottom-panel renderer (Operation Log / Terminal / Activity tabs) split out
//! of `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). `render_body` places
//! the panel below the center pane, beside the full-height sidebar and inspector.

use super::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::IconName;

/// The same presented body limit governs fitting and pointer resizing.
pub(super) fn max_body_height(window: &Window, editor: bool) -> f32 {
    let viewport_height = f32::from(window.viewport_size().height);
    let scale = theme::scaled(1.);
    let mut max_height = (viewport_height * BOTTOM_PANEL_MAX_FRAC / scale).max(BOTTOM_PANEL_MIN_H);
    if editor {
        // Workspace header, file tabs, history selector, snapshot identity,
        // and two source lines must fit together, even after a desktop resize.
        const EDITOR_MIN_BODY_H: f32 = 200.;
        let shell_height = f32::from(super::render_header::chrome_frame_height())
            + theme::scaled(super::tabs::TAB_STRIP_H + STATUS_BAR_H);
        let editor_cap = (viewport_height - shell_height) / scale
            - EDITOR_MIN_BODY_H
            - BOTTOM_PANEL_DIVIDER_H
            - BOTTOM_PANEL_TAB_H;
        max_height = max_height.min(editor_cap.max(BOTTOM_PANEL_MIN_H));
    }
    max_height
}

impl KagiApp {
    #[cfg(feature = "gui-e2e")]
    pub fn bottom_tab_focused_for_e2e(&self, window: &Window) -> Option<usize> {
        self.bottom_tab_focus.focused(window)
    }

    #[cfg(feature = "gui-e2e")]
    pub fn bottom_tab_attributes_for_e2e(&self) -> Vec<(gpui::Role, String, bool, bool)> {
        self.bottom_tab_focus.recorded_tabs()
    }

    /// Bottom panel slot — T-BP-002: open/close + height resize.
    ///
    /// Returns `None` when the panel is closed (so `div().children(…)` adds no
    /// child element).  When open, returns the panel div with:
    /// - a 4px horizontal divider at the top (drag to resize)
    /// - a tab bar (OperationLog / Terminal)
    /// - a placeholder body area
    ///
    /// `visible` is the shown fraction of its height while it opens or
    /// closes (#950): the panel keeps its full height inside a box that
    /// clips it, so the Terminal's grid and PTY size stay put.
    pub(super) fn render_bottom_panel_slot(
        &mut self,
        visible: f32,
        height: f32,
        active_tab: BottomTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if visible <= 0. || active_tab != BottomTab::Activity {
            self.activity_period_focus
                .yield_focus(self.root_focus.as_ref(), window, cx);
            self.activity_bucket_focus
                .yield_focus(self.root_focus.as_ref(), window, cx);
            self.activity_hover = None;
        }
        if visible <= 0. {
            self.yield_terminal_focus(window, cx);
            self.bottom_tab_focus
                .yield_focus(self.root_focus.as_ref(), window, cx);
            return None;
        }
        let compact = self.compact.enabled;

        // ── Horizontal resize divider at top of panel ──
        let h_divider = div()
            .id("divider-bottom-panel")
            .group("divider-bottom-panel")
            .w_full()
            .h(theme::scaled_px(BOTTOM_PANEL_DIVIDER_H))
            .flex_shrink_0()
            .relative()
            .when(cfg!(feature = "gui-e2e"), |divider| {
                divider.child(e2e::measure_inside("divider-bottom-panel"))
            })
            .bg(rgb(theme().panel))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(1.))
                    .flex_shrink_0()
                    .bg(rgb(theme().surface))
                    .group_hover("divider-bottom-panel", |s| s.bg(rgb(theme().ui_accent())))
                    .group_drag_over::<DividerDrag>("divider-bottom-panel", |s| {
                        s.bg(rgb(theme().ui_accent()))
                    })
                    .when(cfg!(feature = "gui-e2e"), |line| {
                        line.child(e2e::measure_inside("divider-bottom-panel-line"))
                    }),
            )
            .hover(|style| style.bg(rgb(theme().ui_accent())).cursor_row_resize())
            .drag_over::<DividerDrag>(|style, drag, _, _| {
                if drag.kind == DividerKind::BottomPanel {
                    style.bg(rgb(theme().ui_accent()))
                } else {
                    style
                }
            })
            .cursor_row_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, _| {
                    this.bottom_panel_drag_origin = Some((
                        f32::from(event.position.y),
                        height,
                        this.bottom_panel_height,
                    ));
                }),
            )
            .on_drag(
                DividerDrag {
                    kind: DividerKind::BottomPanel,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // Manual activation: focusing Terminal must never start its process.
        const TABS: [BottomTab; 4] = [
            BottomTab::OperationLog,
            BottomTab::Terminal,
            BottomTab::Activity,
            BottomTab::Changes,
        ];
        let entity = cx.weak_entity();
        let tabs = keyboard_nav::TabList::new(
            &self.bottom_tab_focus,
            keyboard_nav::slot_keys(if compact { 4 } else { 3 }),
            if compact {
                vec![3, 0, 1, 2]
            } else {
                vec![0, 1, 2]
            },
            TABS.iter().position(|&tab| tab == active_tab),
            keyboard_nav::Activation::Manual,
            // This selector routes pointer focus itself: Terminal receives
            // text input, while the other panes return to the app root.
            None,
            move |slot, is_keyboard, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    let kept = is_keyboard
                        .then(|| this.bottom_tab_focus.focused_cell(window))
                        .flatten();
                    this.bottom_tab = TABS[slot];
                    if this.bottom_tab == BottomTab::Terminal {
                        this.activate_terminal(window, cx);
                    }
                    if this.bottom_tab == BottomTab::Changes {
                        this.open_compact_changes(window, cx);
                    }
                    // Lazy startup focuses the terminal; keyboard activation
                    // must keep the selector's focus until Tab leaves it.
                    if let Some(cell) = kept {
                        window.focus(&cell, cx);
                    }
                    if !is_keyboard && this.bottom_tab != BottomTab::Terminal {
                        if let Some(root) = &this.root_focus {
                            root.focus(window, cx);
                        }
                    }
                    cx.notify();
                });
            },
            cx,
        );
        self.bottom_tab_focus
            .release_closed(self.root_focus.as_ref(), window, cx);
        let cell = |slot: usize, id: &'static str| {
            let tab = TABS[slot];
            let selected = tab == active_tab;
            tabs.cell(slot, tab.label(), div().id(id))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .h(theme::scaled_px(BOTTOM_PANEL_TAB_H))
                .px(keyboard_nav::inset(12.))
                .rounded(theme::scaled_px(4.))
                .text_xs()
                .cursor_pointer()
                .text_color(if selected {
                    rgb(theme().text_main)
                } else {
                    rgb(theme().text_muted)
                })
                .when(selected, |cell| {
                    cell.bg(theme().segment_selected_bg())
                        .font_weight(gpui::FontWeight::MEDIUM)
                })
                .when(!selected, |cell| {
                    cell.hover(|style| style.bg(rgb(theme().surface)))
                })
                .child(e2e::measure_control(
                    match slot {
                        0 => "tab-oplog-label",
                        1 => "tab-terminal-label",
                        2 => "tab-activity-label",
                        _ => "compact-tab-changes-label",
                    },
                    SharedString::from(tab.label()),
                ))
                .when(cfg!(feature = "gui-e2e"), |cell| {
                    cell.child(e2e::measure_inside(id))
                })
        };
        let tab_bar = tabs
            .list(div().id("bottom-panel-tab-bar"))
            .when(cfg!(feature = "gui-e2e"), |bar| {
                bar.relative()
                    .child(e2e::measure_inside("bottom-panel-tabs"))
            })
            .flex()
            .items_center()
            .w_full()
            .flex_shrink_0()
            .bg(rgb(theme().chrome_surface()))
            .when(theme().slug == "apple-dark", |bar| {
                bar.border_b_1().border_color(rgb(theme().surface))
            })
            .child(
                div()
                    .flex()
                    .mx_2()
                    .gap_1()
                    .rounded(theme::scaled_px(4.))
                    .when(compact, |el| el.child(cell(3, "compact-tab-changes")))
                    .child(cell(0, "tab-oplog"))
                    .child(cell(1, "tab-terminal"))
                    .child(cell(2, "tab-activity")),
            );

        // ── Body: Operation Log or Terminal ──
        let body = match active_tab {
            // ADR-0110 Phase 5: the op-log renders as its own child entity so a
            // push / row-expand re-renders only this subtree.
            BottomTab::OperationLog => self
                .op_log
                .clone()
                .map(|e| e.into_any_element())
                .unwrap_or_else(|| div().flex_1().min_h(px(0.)).into_any_element()),
            BottomTab::Terminal => self.render_terminal_body(cx),
            BottomTab::Activity => self.render_activity_body(window, cx),
            BottomTab::Changes => self
                .compact_surface(false, cx)
                .map(IntoElement::into_any_element)
                .unwrap_or_else(|| {
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(theme().text_muted))
                        .child(Msg::HomeWorkLoading.t())
                        .into_any_element()
                }),
        };

        // ── Panel container (height = fixed, flex_shrink_0) ──
        // `height` is the unscaled, persisted body height; the whole container
        // (body + divider + tab strip) is scaled at render so it tracks zoom.
        // The BottomPanel drag math converts the raw cursor back to this
        // unscaled space (see divider_drag_move).
        let panel_h = height + BOTTOM_PANEL_DIVIDER_H + BOTTOM_PANEL_TAB_H;
        let panel = div()
            .id("bottom-panel")
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .h(theme::scaled_px(panel_h))
            .flex_shrink_0()
            .child(h_divider)
            .child(tab_bar)
            .child(body)
            .when(cfg!(feature = "gui-e2e"), |panel| {
                panel.child(e2e::measure_inside("bottom-panel"))
            });
        // #950: only the clip's height moves. The panel hangs from its top
        // edge, so the tab strip rises from the bottom with it.
        Some(
            super::panel_motion::clip(
                "bottom-panel-clip",
                super::panel_motion::Axis::Vertical,
                super::panel_motion::Anchor::Start,
                theme::scaled_px(panel_h),
                visible,
                panel,
            )
            .when(cfg!(feature = "gui-e2e"), |clip| {
                clip.child(e2e::measure_inside("bottom-panel-clip"))
            }),
        )
    }

    /// Render the live PTY, or the typed localized terminal empty state.
    fn render_terminal_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active_session = self.ui().terminal_session.as_ref();
        // Case 1: running terminal view.
        if let Some(session) = active_session {
            if let Some(ref view_entity) = session.view {
                // Clipboard mode handling lives on TerminalView; raw completion
                // keys keep the shared PTY writer and never receive paste framing.
                let paste_writer = session.paste_writer.clone();
                let tab_writer = paste_writer.clone();
                let shift_tab_writer = paste_writer.clone();
                let term_focus = view_entity.read(cx).focus_handle().clone();
                return div()
                    .id("terminal-input")
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    // Mark this subtree as the "Terminal" key context so global
                    // arrow/escape KeyBindings (scoped `!Terminal`) don't consume
                    // those keys while the terminal is focused — they flow to the
                    // terminal's own on_key_down → PTY (history, vim, etc.).
                    .key_context("Terminal")
                    .role(gpui::Role::Group)
                    .aria_label(SharedString::from(i18n::Msg::ToolbarTerminal.t()))
                    .aria_description(terminal::input::focus_description())
                    .on_action(cx.listener(|this, _: &TerminalExitFocus, window, cx| {
                        this.exit_terminal_focus(window, cx);
                    }))
                    // Tab / Shift-Tab → PTY (shell completion; user report).
                    // gpui_component::Root binds "tab" to focus cycling in its
                    // "Root" context, and bindings run before key listeners —
                    // so the terminal's on_key_down never saw Tab. These
                    // actions are bound in this deeper "Terminal" context
                    // (mod.rs), which outranks Root's binding.
                    .on_action(cx.listener(move |_this, _: &TerminalSendTab, _w, _cx| {
                        if let Some(writer) = tab_writer.as_ref() {
                            writer.write_input("\t");
                        }
                    }))
                    .on_action(
                        cx.listener(move |_this, _: &TerminalSendShiftTab, _w, _cx| {
                            if let Some(writer) = shift_tab_writer.as_ref() {
                                writer.write_input("\x1b[Z");
                            }
                        }),
                    )
                    // Clicking anywhere in the terminal area refocuses the
                    // terminal (the view's own mouse handling is a no-op in
                    // gpui-terminal 0.1.0, so a stray click could leave the
                    // keyboard focus elsewhere and break typing/cmd-v).
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_this, _e: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&term_focus, cx);
                        }),
                    )
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _window, cx| {
                        let ks = &event.keystroke;
                        if ks.modifiers.platform && ks.key == "v" {
                            if let Some(text) =
                                cx.read_from_clipboard().and_then(|item| item.text())
                            {
                                this.paste_into_terminal(text, cx);
                            }
                            cx.stop_propagation();
                        }
                    }))
                    .child(view_entity.clone())
                    .into_any();
            }

            return self.render_terminal_empty_state(&session.state, cx);
        }

        self.render_terminal_empty_state(&terminal::TerminalState::NotStarted, cx)
    }

    fn render_terminal_empty_state(
        &self,
        state: &terminal::TerminalState,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use i18n::Msg;
        use terminal::TerminalState;
        let (title, hint, icon) = terminal_empty_labels(state);
        let restart_name = Msg::TerminalRestart.t();
        // The pinned Button installs Role::Button and label as aria_label,
        // with its native Enter/Space and focus-visible ring.
        let restart = Button::new("terminal-restart")
            .primary()
            .label(restart_name)
            .on_click(cx.listener(|app, _, window, cx| {
                app.ensure_terminal(window, cx);
                cx.notify();
            }));
        #[cfg(feature = "gui-e2e")]
        terminal::record_empty_state_for_e2e(state, &title, &hint, restart_name);
        #[cfg(feature = "gui-e2e")]
        let restart = terminal::RestartButton(restart);
        super::render_helpers::empty_state(
            "terminal-empty-state",
            [
                "terminal-state-icon",
                "terminal-state-title",
                "terminal-state-hint",
                "terminal-restart",
            ],
            title,
            hint,
            icon,
            if matches!(state, TerminalState::Failed { .. }) {
                theme().color_blocker
            } else {
                theme().text_sub
            },
            restart,
        )
    }
}

fn terminal_empty_labels(
    state: &terminal::TerminalState,
) -> (SharedString, SharedString, IconName) {
    use i18n::Msg;
    use terminal::TerminalState;
    match state {
        TerminalState::NotStarted => (
            Msg::TerminalNotStartedTitle.t().into(),
            Msg::TerminalNotStartedHint.t().into(),
            IconName::SquareTerminal,
        ),
        TerminalState::Exited { code } => (
            i18n::terminal_exited_title(*code).into(),
            Msg::TerminalRestartHint.t().into(),
            IconName::SquareTerminal,
        ),
        TerminalState::Failed { reason } => (
            Msg::TerminalFailedTitle.t().into(),
            i18n::terminal_failed_hint(reason).into(),
            IconName::TriangleAlert,
        ),
        TerminalState::Running => unreachable!("running terminal owns a view"),
    }
}
