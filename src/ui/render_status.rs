//! Status-bar slot split out of `render.rs` (T-SPLIT-RENDER-001 / ADR-0116
//! Wave 3). Per Rust's visibility rules this child module of `crate::ui` can
//! call the private fields/methods of `KagiApp` defined in its parent, so the
//! method moves with no visibility change. Behaviour is unchanged.

use super::*;
use kagi_ui_core::tooltip::HoverTooltip as _;

#[cfg(feature = "gui-e2e")]
thread_local! {
    /// #547 layout oracle: where the footer message element actually ended up.
    ///
    /// A `track_scroll` handle on a non-scrolling div records the laid-out
    /// bounds and nothing else; the GUI E2E footer scenario reads them back
    /// (via `e2e::footer_message_bounds`) and checks them against the 22 px
    /// bar. Thread-local rather than a 111th `KagiApp` field: it is
    /// measurement, not app state, and GPUI renders only on the main thread.
    /// With several windows open it holds the most recently drawn footer,
    /// which is all a single-window scenario needs — same shape as
    /// `e2e::confirm_bounds`, and compiled out of ordinary builds.
    static FOOTER_MSG_BOUNDS: gpui::ScrollHandle = gpui::ScrollHandle::new();
    static FOOTER_DISPLAY_TEXT: std::cell::RefCell<SharedString> =
        std::cell::RefCell::new(SharedString::default());
}

/// The last drawn footer message's laid-out bounds (#547) — see
/// [`FOOTER_MSG_BOUNDS`]. Zero-sized until the status bar has been drawn once.
#[cfg(feature = "gui-e2e")]
pub(crate) fn footer_message_bounds() -> gpui::Bounds<gpui::Pixels> {
    FOOTER_MSG_BOUNDS.with(|handle| handle.bounds())
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn footer_display_text() -> SharedString {
    FOOTER_DISPLAY_TEXT.with(|text| text.borrow().clone())
}

/// Compact named toggle using the header's keyboard/ring contract.
fn panel_button(
    id: &'static str,
    label: &'static str,
    tip: &'static str,
    pressed: bool,
    icon: gpui_component::IconName,
    configure: impl FnOnce(gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div>,
) -> impl IntoElement {
    let toggled = if pressed {
        gpui::Toggled::True
    } else {
        gpui::Toggled::False
    };
    #[cfg(feature = "gui-e2e")]
    {
        super::render_header::toolbar_a11y::record_label(id, label);
        super::render_header::toolbar_a11y::record_tooltip(id, tip);
        super::e2e::record_status_control(id, gpui::Role::Button, label, toggled);
    }
    let el = div()
        .id(id)
        .role(gpui::Role::Button)
        .aria_label(label)
        .aria_description(tip)
        .aria_toggled(toggled)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .w(theme::scaled_px(24.))
        .h(theme::scaled_px(22.))
        .flex_shrink_0()
        .rounded_sm()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(
            gpui_component::Icon::new(icon)
                .size(theme::scaled_px(16.))
                .text_color(rgb(if pressed {
                    theme().text_main
                } else {
                    theme().text_muted
                })),
        )
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(id))
        });
    #[cfg(feature = "gui-e2e")]
    let el = el.on_key_down(move |event, _, _| {
        if event.keystroke.key == "f19" {
            super::render_header::toolbar_a11y::record_focus_probe(id);
        }
    });
    configure(keyboard_nav::focusable(el)).hover_tooltip_builder((id, 0usize), move |window, cx| {
        Tooltip::new(tip).build(window, cx)
    })
}

fn status_text_width(text: SharedString, window: &Window) -> gpui::Pixels {
    let len = text.len();
    window
        .text_system()
        .shape_line(
            text,
            px(theme::rem_size_px() * 0.75),
            &[gpui::TextRun {
                len,
                font: kagi_ui_core::fonts::ui_font(),
                color: rgb(theme().text_main).into(),
                ..Default::default()
            }],
            None,
        )
        .width
        .ceil()
}

fn chip_description(
    role: view_models::StatusChipRole,
    summary: &StatusBarSummary,
    now: i64,
    upstream: &str,
) -> String {
    use kagi_ui_core::i18n::status_chips::{description, Chip};
    use view_models::StatusChipRole::*;
    let chip = match role {
        Dirty => Chip::Dirty,
        Staged => Chip::Staged(summary.staged),
        Unstaged => Chip::Unstaged(summary.unstaged),
        Conflict => Chip::Conflict(summary.conflict_count),
        Stash => Chip::Stash(summary.stash_count),
        AheadBehind => Chip::AheadBehind(summary.ahead.zip(summary.behind)),
        NoUpstream => Chip::NoUpstream,
        UpstreamName => Chip::Upstream,
        FetchAge | FetchStale => Chip::Fetch {
            age: now.saturating_sub(summary.last_fetch_secs.unwrap_or(now)),
            stale: role == FetchStale,
        },
    };
    description(chip, upstream)
}

impl KagiApp {
    /// Status bar slot — the 22 px footer (T-BP-003 full implementation).
    ///
    /// Left → Right layout:
    ///   branch [● dirty] [↑A ↓B | no upstream] [staged N] [unstaged M]
    ///   HH:MM:SS  ·  <last operation message (flex_1, overflow_hidden)>
    ///   right end: >_ (Terminal icon) ≡ (Operation Log icon) — VSCode style
    ///
    /// The old ▲/▼ toggle is replaced by the icon buttons.
    pub(super) fn render_status_bar(
        &mut self,
        status_footer: FooterStatus,
        bottom_panel_open: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.view().status_summary.clone();
        let bottom_tab = self.bottom_tab;
        let conflict = self.conflict_body_visible();
        if conflict {
            if let Some(stack) = &self.toast_stack {
                stack.update(cx, |stack, cx| {
                    stack.use_conflict_footer(window.window_handle().window_id(), cx);
                });
            }
        }
        let (status_footer, arrival, busy_advice) = if conflict {
            self.conflict_footer_feedback(status_footer, cx)
        } else {
            (status_footer, None, false)
        };
        let busy_controls = conflict && self.busy_feedback_visible();
        let skip_width = (busy_controls
            && self.slow_read_shown().is_some_and(|read| read.skippable()))
        .then(|| {
            status_text_width(SharedString::from(i18n::slow_read_skip()), window)
                + theme::scaled_px(12.)
        });
        let clone_width = (busy_controls && self.clone_progress_available()).then(|| {
            status_text_width(SharedString::from(Msg::CloneShowProgress.t()), window)
                + theme::scaled_px(12.)
        });

        // ── Footer message colour ──────────────────────────────
        let colors = theme();
        let (footer_color, footer_text) = match &status_footer {
            FooterStatus::Success(msg) => (
                colors.status_label_text_on(colors.color_success, colors.panel, colors.panel),
                msg.clone(),
            ),
            FooterStatus::Failed(msg) => (colors.color_blocker, msg.clone()),
            FooterStatus::Idle(msg) => (colors.text_muted, msg.clone()),
            // The spinner is an element, not a glyph: the ⟳ in the text never
            // turned, so a running operation looked like a stuck one (user
            // report). See `render_overlay::sync_spinner`.
            FooterStatus::Busy(msg) => (colors.accent_text_on(colors.panel), msg.clone()),
        };

        // ── Status chips (view-model — ADR-0076 / issue #13 P5) ──
        // The pure StatusBarVM owns the presentation decisions (which chips,
        // their labels, and order); the view below maps each role to a theme
        // colour + margin. Unit-tested without a window in view_models.
        let mut status_vm = view_models::StatusBarVM::from_summary(&summary);
        // ADR-0127: fetch-age indicator (`⇣ 3m`, warning-coloured when stale).
        // Repaints ride the auto-fetch cycle: `fetch_async` notifies on every
        // completion — including silent failures — so the age stays honest
        // even when the network is down.
        let now = super::commit_list::now_unix_secs();
        if let Some(chip) = view_models::fetch_age_chip(&summary, now) {
            status_vm.chips.push(chip);
        }
        let upstream = kagi_domain::text_safety::sanitize_control_bytes(&summary.upstream_name);
        #[cfg(feature = "gui-e2e")]
        super::e2e::record_status_chips(Vec::new());
        let branch_text = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(
            &status_vm.branch,
        ));
        let branch_min_width =
            status_text_width(branch_text.clone(), window).min(theme::scaled_px(60.));
        // Bar padding, two panel-button widths, their margins and feedback gap.
        let mut fixed_width = theme::scaled_px(16. + 2. * 24. + 4. + 2. + 6.);
        if busy_controls {
            fixed_width += theme::scaled_px(STATUS_BAR_H + 4.)
                + skip_width.unwrap_or_default()
                + clone_width.unwrap_or_default();
        }
        let branch_tip = branch_text.clone();
        #[cfg(feature = "gui-e2e")]
        super::e2e::record_status_branch_identity(branch_text.clone(), branch_tip.clone());

        // ── Last refresh time ──────────────────────────────────
        let refresh_label = if summary.last_refresh_secs > 0 {
            let text = SharedString::from(format_hms(summary.last_refresh_secs));
            fixed_width += status_text_width(text.clone(), window) + theme::scaled_px(6.);
            Some(
                div()
                    .id("status-refresh-time")
                    .relative()
                    .ml(theme::scaled_px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(text)
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        el.child(super::e2e::measure_inside("status-refresh-time"))
                    }),
            )
        } else {
            None
        };

        // ── VSCode-style icon buttons (Terminal + Operation Log) ──────────
        // Clicking an inactive icon opens the panel on that tab.
        // Clicking the active icon closes the panel (toggle).
        let oplog_active = bottom_panel_open && bottom_tab == BottomTab::OperationLog;
        let terminal_active = bottom_panel_open && bottom_tab == BottomTab::Terminal;

        let icon_terminal_click = cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
            let keyboard_focus = event.is_keyboard().then(|| window.focused(cx)).flatten();
            if terminal_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal when opening via status bar icon.
                this.activate_terminal(window, cx);
            }
            if let Some(focus) = keyboard_focus {
                focus.focus(window, cx);
            }
            cx.notify();
        });

        let icon_oplog_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if oplog_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::OperationLog;
            }
            cx.notify();
        });

        let icon_terminal = panel_button(
            "status-icon-terminal",
            Msg::ToolbarTerminal.t(),
            if terminal_active {
                Msg::StatusHideTerminal
            } else {
                Msg::StatusShowTerminal
            }
            .t(),
            terminal_active,
            gpui_component::IconName::SquareTerminal,
            |button| {
                button
                    .ml(theme::scaled_px(4.))
                    .on_click(icon_terminal_click)
            },
        );
        let icon_oplog = panel_button(
            "status-icon-oplog",
            Msg::BottomTabOperationLog.t(),
            if oplog_active {
                Msg::StatusHideOperationLog
            } else {
                Msg::StatusShowOperationLog
            }
            .t(),
            oplog_active,
            gpui_component::IconName::Menu,
            |button| button.ml(theme::scaled_px(2.)).on_click(icon_oplog_click),
        );

        // ── Assemble status bar ────────────────────────────────
        let mut bar = div()
            .id("status-footer")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(theme::scaled_px(STATUS_BAR_H))
            .flex_shrink_0()
            .px_2()
            .bg(rgb(theme().chrome_surface()))
            .when(theme().uses_neutral_dark_chrome(), |bar| {
                bar.border_t_1().border_color(rgb(theme().surface))
            })
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            .when(cfg!(feature = "gui-e2e"), |bar| {
                bar.child(super::e2e::measure_inside("pr-menu-footer"))
            })
            // Branch label
            .child(
                div()
                    .id("status-branch")
                    .role(gpui::Role::Label)
                    .aria_label(branch_text.clone())
                    .relative()
                    .flex_shrink(1.)
                    .min_w(branch_min_width)
                    .truncate()
                    .text_color(rgb(theme().text_main))
                    .child(branch_text)
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        el.child(super::e2e::measure_inside("status-branch"))
                    })
                    .hover_tooltip_builder("status-branch-hover-help", move |window, cx| {
                        Tooltip::new(branch_tip.clone()).build(window, cx)
                    }),
            );

        // Status chips (dirty bullet, staged/unstaged, conflict/stash,
        // ahead-behind, upstream name) — order and labels come from StatusBarVM.
        for chip in &status_vm.chips {
            use view_models::StatusChipRole::*;
            let (color, margin) = match chip.role {
                Dirty => (theme().color_warning, 4.),
                Staged => (theme().color_success, 4.),
                Unstaged => (theme().color_warning, 4.),
                Conflict => (theme().color_blocker, 4.),
                Stash => (theme().text_sub, 4.),
                AheadBehind => (theme().text_sub, 6.),
                NoUpstream => (theme().text_muted, 6.),
                UpstreamName => (theme().text_muted, 6.),
                FetchAge => (theme().text_muted, 6.),
                FetchStale => (theme().color_warning, 6.),
            };
            let text = SharedString::from(chip.text.clone());
            let description =
                SharedString::from(chip_description(chip.role, &summary, now, &upstream));
            let tip = description.clone();
            #[cfg(feature = "gui-e2e")]
            super::e2e::record_status_chip(
                text.to_string(),
                description.to_string(),
                tip.to_string(),
            );
            fixed_width += status_text_width(text.clone(), window) + theme::scaled_px(margin);
            bar = bar.child(
                div()
                    .id(("status-chip", chip.role as usize))
                    .role(gpui::Role::Label)
                    .aria_label(description)
                    .relative()
                    .ml(theme::scaled_px(margin))
                    .text_color(rgb(color))
                    .flex_shrink_0()
                    .child(text)
                    .when(cfg!(feature = "gui-e2e"), |el| match chip.role {
                        Dirty => el.child(super::e2e::measure_inside("status-dirty")),
                        AheadBehind => el.child(super::e2e::measure_inside("status-ahead-behind")),
                        UpstreamName => el.child(super::e2e::measure_inside("status-upstream")),
                        FetchAge | FetchStale => {
                            el.child(super::e2e::measure_inside("status-fetch-age"))
                        }
                        _ => el,
                    })
                    .hover_tooltip_builder(
                        ("status-chip-hover-help", chip.role as usize),
                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx),
                    ),
            );
        }
        // Refresh time
        if let Some(chip) = refresh_label {
            bar = bar.child(chip);
        }

        // Last operation message (#547): a bounded, single-line preview.
        //
        // The bar is a fixed 22 px `items_center()` row, so a message that
        // wraps is laid out tall and then *centre*-clipped — the first line
        // (op name + reason) scrolls out of the bar and a middle line shows.
        // `footer_line` collapses it to the first line and `truncate()` keeps
        // it one line at any width. A zero flex basis and readable nonempty
        // minimum reserve feedback before branch identity absorbs excess width.
        // The full body is unchanged in the Operation Log and the oplog.
        let busy = matches!(status_footer, FooterStatus::Busy(_));
        let feedback_min_width = if footer_text.is_empty() {
            px(0.)
        } else {
            // At 700px/167%, fixed chips plus both preferred text minima can
            // exceed the bar. Reserve only actual room, with 1px for snapping.
            (window.viewport_size().width - fixed_width - branch_min_width - px(1.))
                .max(px(0.))
                .min(theme::scaled_px(80. + if busy { 14. } else { 0. }))
        };
        let display_text = SharedString::from(view_models::footer_line(&footer_text));
        let announcement = arrival
            .as_ref()
            .map(|(_, label)| label.clone())
            .or_else(|| busy.then(|| footer_text.clone()))
            .unwrap_or_default();
        #[cfg(feature = "gui-e2e")]
        {
            FOOTER_DISPLAY_TEXT.with(|text| *text.borrow_mut() = display_text.clone());
            if conflict {
                super::toast_stack::record_status(
                    Some(gpui::Role::Status),
                    Some(announcement.clone()),
                    arrival.as_ref().map(|(id, _)| *id),
                );
                if busy && self.home_github.cloning.is_some() {
                    super::e2e::record_clone_busy_status(gpui::Role::Status, &announcement);
                }
            }
        }
        let message = div()
            .id("status-footer-msg")
            .flex()
            .when(conflict, |message| {
                message.role(gpui::Role::Status).aria_label(announcement)
            })
            .when(conflict && busy, |message| {
                message.child(super::e2e::measure_inside("conflict-footer-busy"))
            })
            .flex_row()
            .items_center()
            .gap(theme::scaled_px(4.))
            .flex_1()
            .min_w(feedback_min_width)
            .ml(theme::scaled_px(6.))
            .overflow_hidden()
            .text_color(rgb(footer_color))
            .children(
                busy.then(|| super::render_overlay::sync_spinner(10., footer_color, "footer-busy")),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(display_text)
                    .when(busy_advice, |text| {
                        text.child(super::e2e::measure_inside("conflict-footer-busy-advice"))
                    })
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        el.child(super::e2e::measure_inside("status-footer-text"))
                    }),
            );
        #[cfg(feature = "gui-e2e")]
        let message = message.track_scroll(&FOOTER_MSG_BOUNDS.with(gpui::ScrollHandle::clone));
        let message = if let Some(tip) = view_models::footer_excerpt(&footer_text) {
            let tip = SharedString::from(tip);
            message
                .hover_tooltip_builder("status-message-hover-help", move |window, cx| {
                    Tooltip::new(tip.clone()).build(window, cx)
                })
                .into_any_element()
        } else {
            message.into_any_element()
        };
        bar = bar.child(message);
        if busy_controls {
            bar = bar.child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .h(theme::scaled_px(STATUS_BAR_H))
                    .ml(theme::scaled_px(4.))
                    .child(super::e2e::measure_inside("conflict-footer-busy-controls"))
                    .children(skip_width.map(|width| self.render_busy_skip(Some(width), cx)))
                    .children(
                        clone_width
                            .and_then(|width| self.render_clone_progress_action(Some(width), cx)),
                    )
                    .child(self.render_busy_dismiss(true, cx)),
            );
        }

        // Icon buttons at the right end.
        bar.child(
            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .child(icon_terminal)
                .child(icon_oplog),
        )
    }
}
