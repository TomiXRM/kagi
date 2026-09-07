//! Status-bar slot split out of `render.rs` (T-SPLIT-RENDER-001 / ADR-0116
//! Wave 3). Per Rust's visibility rules this child module of `crate::ui` can
//! call the private fields/methods of `KagiApp` defined in its parent, so the
//! method moves with no visibility change. Behaviour is unchanged.

use super::*;

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
}

/// The last drawn footer message's laid-out bounds (#547) — see
/// [`FOOTER_MSG_BOUNDS`]. Zero-sized until the status bar has been drawn once.
#[cfg(feature = "gui-e2e")]
pub(crate) fn footer_message_bounds() -> gpui::Bounds<gpui::Pixels> {
    FOOTER_MSG_BOUNDS.with(|handle| handle.bounds())
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
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.view().status_summary.clone();
        let bottom_tab = self.bottom_tab;

        // ── Footer message colour ──────────────────────────────
        let (footer_color, footer_text) = match &status_footer {
            FooterStatus::Success(msg) => (theme().color_success, msg.clone()),
            FooterStatus::Failed(msg) => (theme().color_blocker, msg.clone()),
            FooterStatus::Idle(msg) => (theme().text_muted, msg.clone()),
            FooterStatus::Busy(msg) => (
                theme().color_branch,
                SharedString::from(format!("\u{27f3} {}", msg)), // ⟳ msg
            ),
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
        if let Some(chip) =
            view_models::fetch_age_chip(&summary, super::commit_list::now_unix_secs())
        {
            status_vm.chips.push(chip);
        }
        let branch_text = SharedString::from(status_vm.branch.clone());

        // ── Last refresh time ──────────────────────────────────
        let refresh_label = if summary.last_refresh_secs > 0 {
            Some(
                div()
                    .ml(theme::scaled_px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(SharedString::from(format_hms(summary.last_refresh_secs))),
            )
        } else {
            None
        };

        // ── VSCode-style icon buttons (Terminal + Operation Log) ──────────
        // Clicking an inactive icon opens the panel on that tab.
        // Clicking the active icon closes the panel (toggle).
        let oplog_active = bottom_panel_open && bottom_tab == BottomTab::OperationLog;
        let terminal_active = bottom_panel_open && bottom_tab == BottomTab::Terminal;

        let icon_terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            if terminal_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal when opening via status bar icon.
                this.ensure_terminal(window, cx);
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

        let icon_terminal_color = if terminal_active {
            theme().text_main
        } else {
            theme().text_muted
        };
        let icon_oplog_color = if oplog_active {
            theme().text_main
        } else {
            theme().text_muted
        };

        let icon_terminal = div()
            .id("status-icon-terminal")
            .ml(theme::scaled_px(4.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_terminal_color))
            .hover(|s| s.text_color(rgb(theme().text_main)).cursor_pointer())
            .on_click(icon_terminal_click)
            .child(
                gpui_component::Icon::new(gpui_component::IconName::SquareTerminal)
                    .with_size(gpui_component::Size::XSmall)
                    .text_color(rgb(icon_terminal_color)),
            );

        let icon_oplog = div()
            .id("status-icon-oplog")
            .ml(theme::scaled_px(2.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_oplog_color))
            .hover(|s| s.text_color(rgb(theme().text_main)).cursor_pointer())
            .on_click(icon_oplog_click)
            .child(
                gpui_component::Icon::new(gpui_component::IconName::Menu)
                    .with_size(gpui_component::Size::XSmall)
                    .text_color(rgb(icon_oplog_color)),
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
            .bg(rgb(theme().panel))
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            // Branch label
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(theme().text_main))
                    .child(branch_text),
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
            bar = bar.child(
                div()
                    .ml(theme::scaled_px(margin))
                    .text_color(rgb(color))
                    .flex_shrink_0()
                    .child(SharedString::from(chip.text.clone())),
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
        // it one line at any width; `min_w_0` lets it shrink past its own text
        // so the Terminal / Operation Log icons stay reachable at 320 px. The
        // untruncated body is unchanged in the Operation Log and the oplog.
        let mut message = div()
            .id("status-footer-msg")
            .flex_1()
            .min_w_0()
            .ml(theme::scaled_px(6.))
            .truncate()
            .text_color(rgb(footer_color))
            .child(SharedString::from(view_models::footer_line(&footer_text)));
        #[cfg(feature = "gui-e2e")]
        {
            message = message.track_scroll(&FOOTER_MSG_BOUNDS.with(gpui::ScrollHandle::clone));
        }
        if let Some(tip) = view_models::footer_excerpt(&footer_text) {
            let tip = SharedString::from(tip);
            message =
                message.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
        }
        bar = bar.child(message);

        // Icon buttons at the right end.
        bar.child(icon_terminal).child(icon_oplog)
    }
}
