//! Busy presentation is independent of the writer lease's lifecycle marker.
use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use kagi_ui_core::i18n;
use kagi_ui_core::slow_read::SlowRead;

use super::theme::{self, theme};
use super::{render_overlay, KagiApp};

impl KagiApp {
    pub(crate) fn mark_write_busy(&mut self, name: &'static str) {
        self.write_busy_op = Some(name);
    }

    /// Latch the one write that cannot hold a lease: remote pull over SSH,
    /// whose `WriteScope::Remote(RemoteRepoId)` needs two network probes that
    /// cannot run on the UI thread before the spawn. (The remote stash family
    /// gets its id from a background plan job; a pull plan synthesised from a
    /// cached snapshot has no equivalent.) ADR-0196 決定 5 has the rationale.
    ///
    /// This is **not** a lease mirror, so [`settle_write_busy`] must never see
    /// it: a lease-derived retire would drop it on the very next
    /// `refresh_write_busy()` — which `render` → `poll_app_jobs` and every
    /// admission preamble call — and the pull would run unlatched (#708
    /// review P1). Only the pull's own terminal callback clears it, before
    /// every branch, so success, failure and a panicked task all release.
    ///
    /// Known gap, unchanged by this slice: `may_close_host` reads leases, so a
    /// remote pull does not hold quit. Putting it on a real lease fixes both,
    /// and is the same follow-up slice as #703.
    pub(crate) fn mark_remote_write(&mut self, name: &'static str) {
        self.remote_write = Some(name);
    }

    /// Is any operation latched — a write or a planning task? The single
    /// question every gate asks (ADR-0196 Wave 3).
    ///
    /// A held **lease** is the truth about every write that can take one;
    /// `remote_write` covers the one that cannot; `planning` writes nothing but
    /// owns the modal slot it is about to fill. `write_busy_op` is deliberately
    /// absent: it is only a presentation mirror of the lease, so reading it
    /// here would answer with the lease twice and with nothing new.
    pub(crate) fn op_latched(&self) -> bool {
        !super::operations::op_may_start(
            self.app_sessions.has_leases(),
            self.remote_write,
            self.planning,
        )
    }

    pub(crate) fn busy_snackbar_label(&self) -> Option<&'static str> {
        self.write_busy_op
            .or(self.remote_write)
            .or(self.planning)
            .map(kagi_ui_core::i18n::busy_label)
    }

    /// Render the toast / busy overlay as an absolute container (bottom-left,
    /// above the status bar). The toast cards live in the `Entity<ToastStack>`
    /// child, so a push / expire re-renders only that subtree instead of all of
    /// `KagiApp` (ADR-0110 Phase 5). The busy snackbar stays here because it is
    /// driven by KagiApp state: the write latch, and a read that has been slow
    /// for two seconds (#355). Returns `None` before the window (and thus the
    /// toast entity) exists.
    pub(super) fn render_toasts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let toast_stack = self.toast_stack.clone()?;
        let mut stack = div()
            .absolute()
            .bottom(theme::scaled_px(34.))
            .left(theme::scaled_px(super::TOAST_INSET_PX))
            .w(theme::scaled_px(460.))
            .max_w(gpui::relative(0.9))
            .flex()
            .flex_col()
            .gap_2();

        // While an async op runs, show a busy snackbar with a spinning sync icon
        // (user request) — a lighter alternative to a blocking popup. A slow
        // read adds its explanation; with no write running it owns the label.
        let slow = self.slow_read_shown();
        let label = self
            .busy_snackbar_label()
            .or(slow.map(i18n::slow_read_label));
        if let Some(label) = label {
            stack = stack.child(self.render_busy_snackbar(label, slow, cx));
        }

        // The toast cards are an independently-rendered child entity.
        stack = stack.child(toast_stack);
        Some(stack.into_any())
    }

    /// A snackbar shown while an async op runs: a continuously spinning sync
    /// icon + a friendly label (user request — a non-blocking alternative to a
    /// modal busy-spinner). Driven automatically by the write latch, so every async
    /// op gets one for free. `slow` adds the one-line explanation of a slow
    /// read and, when its result can be shown as unknown, Skip (#355).
    fn render_busy_snackbar(
        &self,
        label: &'static str,
        slow: Option<SlowRead>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let accent = theme().color_branch;
        let icon = render_overlay::big_sync_icon(accent, "kagi-busy-snackbar-spin");
        let mut text = div()
            .flex_1()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(SharedString::from(label));
        if let Some(read) = slow {
            text = text.child(super::e2e::measure_control(
                "busy-snackbar-advice",
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(i18n::slow_read_advice(read))),
            ));
        }
        let skip = slow.filter(|read| read.skippable()).map(|_| {
            super::e2e::measure_control(
                "busy-snackbar-skip",
                Button::new("busy-snackbar-skip")
                    .label(i18n::slow_read_skip())
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.skip_slow_read(cx))),
            )
        });
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            // 1.5× the toast gap (8px → 12px) so the larger sync icon breathes
            // a bit more from the label (user request).
            .gap_3()
            .px_4()
            .py_3()
            .rounded(theme::scaled_px(8.))
            .bg(rgb(theme().panel))
            .border_1()
            .border_color(rgb(accent))
            .text_base()
            .text_color(rgb(theme().text_main))
            .child(div().flex_shrink_0().child(icon))
            .child(text)
            .children(skip)
            .into_any()
    }
}

/// Retire the write mirror once no lease survives. A retained lease means the
/// writer's termination is unconfirmed (or its task panicked, which proves
/// nothing), so it may still be running — clearing the mirror there would leave
/// `has_leases()` true with the name gone from the snackbar.
///
/// Only ever hand this the lease mirror. `remote_write` is owned by its writer,
/// not by the lease count, and passing it here is the #708 P1 defect.
pub(super) fn settle_write_busy(writer: &mut Option<&'static str>, has_leases: bool) {
    if !has_leases {
        *writer = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retained_lease_keeps_its_mirror() {
        for name in [
            "fetch",
            "stash-apply",
            "remove-worktree",
            "editor-save",
            "stage",
        ] {
            // Unconfirmed termination (or a panicked task): the lease is still
            // reserved, so the writer may still be running — keep it visible.
            let mut writer = Some(name);
            settle_write_busy(&mut writer, true);
            assert_eq!(writer, Some(name));
            // Known termination: the lease went with the settle, so does the
            // mirror.
            settle_write_busy(&mut writer, false);
            assert_eq!(writer, None);
        }
    }
}
