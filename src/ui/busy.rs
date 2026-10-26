//! Busy presentation is independent of the writer lease's lifecycle marker.
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Disableable as _, Sizable as _};
use kagi_ui_core::i18n::{self, Msg};
use kagi_ui_core::slow_read::{is_slow, SlowRead};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::theme::{self, theme};
use super::{render_overlay, KagiApp};

const TICK: Duration = Duration::from_millis(250);

/// Identity of the existing owner, not an operation label. A Pull keeps the
/// original waiter stamp when freshness Fetch hands off to a different lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BusyIdentity {
    Write(crate::app::OperationId),
    Pull(crate::app::SessionId, u64, Instant),
    Planning(crate::app::RequestId),
    Clone(Instant),
}

impl BusyIdentity {
    pub(super) fn pull((owner, visit, started): (crate::app::SessionId, u64, Instant)) -> Self {
        Self::Pull(owner, visit, started)
    }
}

impl KagiApp {
    pub(crate) fn mark_write_busy(&mut self, name: &'static str, cx: &mut Context<Self>) {
        self.write_busy_op = Some(name);
        self.busy_write_origin = None;
        if let Some((id, _)) = self.app_sessions.running_lease() {
            self.busy_write_origin = Some((id, BusyIdentity::Write(id)));
            // The runner's synthetic clock is written onto the same lease
            // record; production retains the exact admission Instant.
            #[cfg(feature = "gui-e2e")]
            self.app_sessions
                .set_lease_start_for_e2e(id, cx.background_executor().now());
            self.tick_busy_write(id, cx);
        }
    }

    /// An admitted lease is the sole timer owner. No separate write clock
    /// survives settlement, cancellation, or a newer admission.
    fn tick_busy_write(&mut self, id: crate::app::OperationId, cx: &mut Context<Self>) {
        cx.spawn(async move |this, acx| loop {
            acx.background_executor().timer(TICK).await;
            let alive = this
                .update(acx, |app, cx| {
                    app.refresh_write_busy();
                    // The tick that finds this write ended (settled, guard
                    // dropped by a panic, or superseded) still notifies: the
                    // advice drawn on the last frame must be withdrawn now,
                    // not on the next unrelated frame (#995 review).
                    let Some((running, started)) = app.app_sessions.running_lease() else {
                        cx.notify();
                        return false;
                    };
                    if running != id {
                        cx.notify();
                        return false;
                    }
                    let elapsed = cx
                        .background_executor()
                        .now()
                        .saturating_duration_since(started);
                    if is_slow(elapsed) && app.app_sessions.note_slow_lease(id) {
                        klog!(
                            "busy: slow write {} after 2s",
                            app.write_busy_op.unwrap_or("write")
                        );
                    }
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        })
        .detach();
    }
    /// Is any operation latched — a write, a planning task or a clone? The
    /// single question every gate asks (ADR-0196 Wave 3).
    ///
    /// A held **lease** is the truth about every admitted write; `planning`
    /// owns the modal slot for a separate background plan; a clone (#930)
    /// has no repository session to lease but still writes a folder.
    /// `write_busy_op` is deliberately absent: it is only a presentation
    /// mirror of the lease, so reading it here would answer with the lease
    /// twice and with nothing new.
    pub(crate) fn op_latched(&self) -> bool {
        !super::operations::op_may_start(self.app_sessions.has_leases(), self.planning)
            || self.home_github.cloning.is_some()
    }

    pub(crate) fn busy_snackbar_label(&self) -> Option<&'static str> {
        if self.busy_progress_hidden() {
            return None;
        }
        self.write_busy_op
            .or(self.planning)
            .map(kagi_ui_core::i18n::busy_label)
    }

    pub(crate) fn mark_planning_busy(&mut self, name: &'static str) -> crate::app::RequestId {
        let request = crate::app::RequestId(crate::app::next_id());
        self.planning = Some(name);
        self.busy_plan_origin = Some(BusyIdentity::Planning(request));
        request
    }

    pub(super) fn continue_busy_planning(
        &mut self,
        request: crate::app::RequestId,
        progress: Option<(BusyIdentity, bool)>,
    ) {
        if self.planning.is_none() || self.busy_plan_origin != Some(BusyIdentity::Planning(request))
        {
            return;
        }
        if let Some((identity, hidden)) = progress {
            self.busy_plan_origin = Some(identity);
            if hidden {
                self.hidden_busy_job = Some(identity);
            }
        }
    }

    pub(super) fn busy_request_hidden(&self, identity: Option<BusyIdentity>) -> bool {
        identity.is_some() && identity == self.hidden_busy_job
    }
    pub(super) fn start_busy_request(&mut self, identity: BusyIdentity) {
        if self.hidden_busy_job == Some(identity) {
            self.hidden_busy_job = None;
        }
    }

    pub(super) fn continue_busy_request(&mut self, identity: Option<BusyIdentity>, hidden: bool) {
        let running = self.app_sessions.running_lease().map(|(id, _)| id);
        if let (Some((id, origin)), Some(identity)) = (&mut self.busy_write_origin, identity) {
            if running != Some(*id) {
                return;
            }
            *origin = identity;
            if hidden {
                self.hidden_busy_job = Some(identity);
            }
        }
    }

    fn busy_identity(&self) -> Option<BusyIdentity> {
        if let Some(started) = self
            .home_github
            .cloning
            .as_ref()
            .and_then(|card| card.started)
        {
            return Some(BusyIdentity::Clone(started));
        }
        if self.write_busy_op.is_some() {
            if let Some((owner, visit, _, started)) =
                self.fetch_in_flight.as_ref().and_then(|flight| {
                    flight
                        .waiters
                        .iter()
                        .find(|(owner, visit, _, _)| {
                            self.active_session() == Some(*owner)
                                && self.app_sessions.visit(*owner) == Some(*visit)
                        })
                        // Switching tabs is not a new request. Keep this
                        // flight's dismissed owner rather than falling back
                        // to an older, undismissed waiter from another visit.
                        .or_else(|| {
                            flight.waiters.iter().find(|(owner, visit, _, started)| {
                                self.hidden_busy_job
                                    == Some(BusyIdentity::Pull(*owner, *visit, *started))
                            })
                        })
                        .or_else(|| flight.waiters.first())
                })
            {
                return Some(BusyIdentity::Pull(*owner, *visit, *started));
            }
            return self.busy_write_origin.map(|(_, identity)| identity);
        }
        self.planning.and(self.busy_plan_origin)
    }

    fn busy_progress_hidden(&self) -> bool {
        self.busy_request_hidden(self.busy_identity())
    }

    /// Keep a hidden Pull through its fetch/confirmation/write handoff, but
    /// retire suppression with its actual owner. New admissions also reset it,
    /// so a new same-clock request never inherits an old timestamp's state.
    pub(super) fn retire_hidden_busy_progress(&mut self) {
        if self.write_busy_op.is_none() {
            self.busy_write_origin = None;
        }
        if self.planning.is_none() {
            self.busy_plan_origin = None;
        }
        let Some(hidden) = self.hidden_busy_job else {
            return;
        };
        let owns_pull = |stamp: Option<(crate::app::SessionId, u64, Instant)>| {
            Some(hidden) == stamp.map(BusyIdentity::pull)
        };
        let live = self.busy_identity() == Some(hidden)
            || self.fetch_in_flight.as_ref().is_some_and(|flight| {
                flight
                    .waiters
                    .iter()
                    .any(|(owner, visit, _, started)| owns_pull(Some((*owner, *visit, *started))))
            })
            || self.pull_modal().is_some_and(|modal| {
                owns_pull(modal.fetch_owner)
                    || modal.progress_request.map(BusyIdentity::Planning) == Some(hidden)
            })
            || self
                .branch_plan_modal()
                .is_some_and(|modal| owns_pull(modal.fetch_owner))
            || self
                .merge_modal()
                .is_some_and(|modal| modal.progress_identity == Some(hidden));
        if !live {
            self.hidden_busy_job = None;
        }
    }

    fn hide_busy_progress(
        &mut self,
        owner: Option<crate::app::SessionId>,
        identity: Option<BusyIdentity>,
        cx: &mut Context<Self>,
    ) {
        if self.active_session() != owner || self.busy_identity() != identity {
            return;
        }
        self.hidden_busy_job = identity;
        if let Some(ui) = owner.and_then(|owner| self.ui.get_mut(&owner)) {
            ui.slow_reads.hide_inflight();
        }
        if let Some(stack) = self.toast_stack.clone() {
            let removed = super::toast_stack::ToastRemoved::from_control(&self.busy_close_focus);
            stack.update(cx, |_, cx| cx.emit(removed));
        }
        cx.notify();
    }

    fn slow_write_shown(&self, now: Instant) -> Option<(&'static str, u64)> {
        if self.busy_progress_hidden() {
            return None;
        }
        let name = self.write_busy_op?;
        let (_, started) = self.app_sessions.running_lease()?;
        let elapsed = now.saturating_duration_since(started);
        is_slow(elapsed).then_some((name, elapsed.as_secs()))
    }

    /// Render notifications inside the surface's content anchor, or in a
    /// persistent-only lane / transient overlay.
    pub(super) fn render_toasts(
        &self,
        window: &gpui::Window,
        placement: ToastPlacement,
        show_clone_action: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let toast_stack = self.toast_stack.clone()?;
        let partition = match placement {
            ToastPlacement::Repository => super::toast_stack::ToastPartition::All,
            ToastPlacement::PersistentLane => super::toast_stack::ToastPartition::Persistent,
            ToastPlacement::Transient => super::toast_stack::ToastPartition::Transient,
        };
        toast_stack.update(cx, |stack, _| {
            stack.set_trailing_placement(
                window.window_handle().window_id(),
                partition,
                placement != ToastPlacement::Transient,
            );
        });
        let mut stack = toast_container(placement)
            .max_h(gpui::px(f32::from(window.viewport_size().height) * 0.30))
            .min_h(gpui::px(0.))
            .when(placement == ToastPlacement::Transient, |el| {
                el.child(super::e2e::measure_inside("toast-transient-overlay"))
            });
        if placement == ToastPlacement::PersistentLane {
            return Some(
                stack
                    .child(super::render_overlay::ToastHost {
                        stack: toast_stack,
                        partition,
                    })
                    .into_any(),
            );
        }

        // While an async op runs, show a busy snackbar with a spinning sync icon
        // (user request) — a lighter alternative to a blocking popup. A slow
        // read adds its explanation; with no write running it owns the label.
        let slow = self.slow_read_shown();
        let write = self.slow_write_shown(cx.background_executor().now());
        let label = self
            .home_github
            .cloning
            .as_ref()
            .filter(|_| !self.busy_progress_hidden())
            .and_then(|card| card.target.as_ref())
            .map(|target| {
                super::render_helpers::safe_text(
                    &Msg::CloneHiddenRunning
                        .t()
                        .replace("{}", &target.request.source),
                )
            })
            .or_else(|| {
                self.busy_snackbar_label()
                    .or(slow.map(i18n::slow_read_label))
                    .map(SharedString::from)
            });
        if let Some(label) = label {
            stack =
                stack.child(self.render_busy_snackbar(label, slow, write, show_clone_action, cx));
        }

        // Both projections borrow the same lifecycle/action owner.
        stack = stack.child(super::render_overlay::ToastHost {
            stack: toast_stack,
            partition,
        });
        Some(stack.into_any())
    }

    /// A snackbar shown while an async op runs: a continuously spinning sync
    /// icon + a friendly label (user request — a non-blocking alternative to a
    /// modal busy-spinner). Driven automatically by the write latch, so every async
    /// op gets one for free. `slow` adds the one-line explanation of a slow
    /// read and, when its result can be shown as unknown, Skip (#355).
    fn render_busy_snackbar(
        &self,
        label: SharedString,
        slow: Option<SlowRead>,
        write: Option<(&'static str, u64)>,
        show_clone_action: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let accent = theme().ui_accent();
        let icon = render_overlay::big_sync_icon(accent, "kagi-busy-snackbar-spin");
        let caption = self.busy_caption(label);
        let mut text = div()
            .flex_1()
            .min_w(gpui::px(0.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(caption);
        if let Some((kind, elapsed)) = write {
            let advice = i18n::slow_write_advice(kind, elapsed);
            #[cfg(feature = "gui-e2e")]
            super::e2e::record_busy_advice(&advice);
            text = text.child(super::e2e::measure_control(
                "busy-snackbar-advice",
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(advice)),
            ));
        }
        if write.is_none() {
            if let Some(read) = slow {
                text = text.child(super::e2e::measure_control(
                    "busy-snackbar-advice",
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(i18n::slow_read_advice(read))),
                ));
            }
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
            .relative()
            .child(super::e2e::measure_inside("busy-snackbar"))
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
            .bg(rgb(theme().elevated_surface(theme().panel)))
            .border_1()
            .border_color(rgb(theme().elevated_border(accent)))
            .text_base()
            .text_color(rgb(theme().text_main))
            .child(div().flex_shrink_0().child(icon))
            .child(text)
            .children(skip)
            .when(show_clone_action && !self.busy_progress_hidden(), |el| {
                el.children(self.render_clone_progress_action(cx))
            })
            .child(self.render_busy_dismiss(cx))
            .into_any()
    }

    fn busy_caption(&self, label: SharedString) -> gpui::AnyElement {
        if self.home_github.cloning.is_none() || self.busy_progress_hidden() {
            return div().child(label).into_any();
        }
        let role = gpui::Role::Status;
        #[cfg(feature = "gui-e2e")]
        super::e2e::record_clone_busy_status(role, &label);
        super::e2e::measure_control(
            "clone-busy-status",
            div()
                .id("clone-busy-status")
                .role(role)
                .aria_label(label.clone())
                .text_ellipsis()
                .child(label),
        )
        .into_any()
    }

    pub(crate) fn retire_busy_control_focus(&self, cx: &mut Context<Self>) {
        let visible = self.slow_read_shown().is_some()
            || self.busy_snackbar_label().is_some()
            || (self.home_github.cloning.is_some()
                && !self.busy_request_hidden(self.busy_identity()));
        if visible || self.busy_close_focus.borrow().is_empty() {
            return;
        }
        if let Some(stack) = &self.toast_stack {
            let removed = super::toast_stack::ToastRemoved::from_control(&self.busy_close_focus);
            stack.update(cx, |_, cx| cx.emit(removed));
        }
    }

    fn render_busy_dismiss(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let owner = self.active_session();
        let identity = self.busy_identity();
        let app = cx.entity().downgrade();
        let control = super::toast_stack::ToastButton(
            Button::new(("busy-snackbar-dismiss", 0_u64))
                .ghost()
                .small()
                .flex_shrink_0()
                .icon(gpui_component::Icon::empty().path("icons/window-close.svg"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.hide_busy_progress(owner, identity, cx);
                })),
            0,
            self.busy_close_focus.clone(),
            "busy-snackbar-dismiss",
        )
        .aria_label(SharedString::from(Msg::BusyHideProgress.t()))
        .on_a11y_action(gpui::AccessibleAction::Click, move |_, _, cx| {
            let _ = app.update(cx, |this, cx| this.hide_busy_progress(owner, identity, cx));
        })
        .on_key_down(super::keyboard_nav::stop_activation_keys)
        .hover_tooltip("busy-snackbar-dismiss-help", Msg::BusyHideProgress.t());
        super::e2e::measure_control("busy-snackbar-dismiss", control)
    }

    fn render_clone_progress_action(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.home_github.cloning.is_none() || self.clone_modal().is_some() {
            return None;
        }
        let button = Button::new("clone-busy-show")
            .label(Msg::CloneShowProgress.t())
            .small()
            .ghost()
            .flex_shrink_0()
            .disabled(self.has_active_modal())
            .on_click(cx.listener(|this, _, window, cx| {
                // Restore the existing task, never construct or confirm a new clone.
                if this.has_active_modal() {
                    return;
                }
                if let Some(card) = this.home_github.cloning.as_ref() {
                    this.home_github_pick(card.listing.clone(), cx);
                    this.focus_after_pick(window, cx);
                }
            }));
        #[cfg(feature = "gui-e2e")]
        let button = super::e2e::probe_clone_busy_button(button);
        Some(super::e2e::measure_control("clone-busy-show", button).into_any())
    }
}

/// Retire the write mirror once no lease survives. A retained lease means the
/// writer's termination is unconfirmed (or its task panicked, which proves
/// nothing), so it may still be running — clearing the mirror there would leave
/// `has_leases()` true with the name gone from the snackbar.
pub(super) fn settle_write_busy(writer: &mut Option<&'static str>, has_leases: bool) {
    if !has_leases {
        *writer = None;
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static FETCH_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
pub(super) fn take_fetch_hold() -> Option<gpui::Task<()>> {
    FETCH_HOLD.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Hold the next admitted fetch before its backend work, not its planning.
    pub fn hold_next_fetch_for_e2e(hold: gpui::Task<()>) {
        FETCH_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
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
            settle_write_busy(&mut writer, false);
            assert_eq!(writer, None);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ToastPlacement {
    Repository,
    PersistentLane,
    Transient,
}

/// The repository parent is the actual center content, never the window.
pub(super) fn toast_container(placement: ToastPlacement) -> gpui::Div {
    div()
        .when(placement == ToastPlacement::Repository, |el| {
            el.absolute()
                .top(theme::scaled_px(super::TOAST_INSET_PX))
                .bottom(theme::scaled_px(super::TOAST_INSET_PX))
                .right(theme::scaled_px(super::TOAST_INSET_PX))
                .max_h(gpui::relative(1.0))
        })
        .when(placement == ToastPlacement::PersistentLane, |el| {
            el.flex_shrink_0()
                .my(theme::scaled_px(super::TOAST_INSET_PX))
                .mr(theme::scaled_px(super::TOAST_INSET_PX))
        })
        .when(placement == ToastPlacement::Transient, |el| {
            el.absolute()
                .bottom(theme::scaled_px(
                    super::STATUS_BAR_H + super::TOAST_INSET_PX,
                ))
                .left(theme::scaled_px(super::TOAST_INSET_PX))
        })
        .w(theme::scaled_px(460.))
        .max_w(gpui::relative(0.45))
        .flex()
        .flex_col()
        .gap_2()
}
