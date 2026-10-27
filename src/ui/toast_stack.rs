//! Toast notification stack — self-contained logic (ADR-0110 / Phase 5 start).
//!
//! Extracted from `KagiApp` so the toast add/remove/expire/ticker logic lives
//! in one testable struct. Previously this was ~80 lines of inline methods on
//! the 104-field `KagiApp` god-struct, interleaved with unrelated state.
//!
//! Held as an `Entity<ToastStack>` on `KagiApp` (ADR-0110 Phase 5). Render hosts
//! in `render_overlay.rs` project its cards without copying their identities,
//! actions or lifecycle. Window owners observe the entity so changes update
//! each audience's single bounded stack. Auto-dismiss and slide-out
//! timers stay here; pure data methods remain `cx`-free for unit tests.

use std::time::{Duration, Instant};

use gpui::{Context, SharedString};

use crate::ui::types::{Toast, ToastKind};
use crate::ui::view_models::status_bar::{footer_line, FOOTER_LINE_CHARS};

/// Maximum simultaneous toasts; discard the oldest non-error first.
pub const TOASTS_MAX: usize = 4;
/// Slide-out animation duration before final removal (ms).
pub const TOAST_REMOVE_MS: u64 = 300;
/// Short routine feedback remains for four seconds, excluding hover pauses.
/// Errors have no expiry; explicit dismissal provides unlimited reading time.
pub const TOAST_ROUTINE_LIFETIME: Duration = Duration::from_secs(4);

/// Complete semantic input, assembled before insertion or focus repair.
pub struct ToastSpec {
    pub subject: ToastSubject,
    pub kind: ToastKind,
    pub message: SharedString,
    pub result_key: Option<String>,
    pub action: Option<super::types::ToastAction>,
    pub refresh_owner: Option<crate::app::Attachment>,
}

impl ToastSpec {
    pub fn new(subject: ToastSubject, kind: ToastKind, message: impl Into<SharedString>) -> Self {
        Self {
            subject,
            kind,
            message: message.into(),
            result_key: None,
            action: None,
            refresh_owner: None,
        }
    }

    pub(crate) fn operation(
        entry: &kagi_git::oplog::OpLogEntry,
        kind: ToastKind,
        message: SharedString,
    ) -> Self {
        let mut spec = Self::new(ToastSubject::Repository, kind, message);
        if matches!(kind, ToastKind::Error | ToastKind::Info) {
            spec.action = Some(super::types::ToastAction::OpenOperationLog(
                super::oplog_panel::EntryKey::of(entry),
            ));
        }
        spec
    }

    pub fn with_key(mut self, key: Option<String>) -> Self {
        self.result_key = key;
        self
    }
}

#[cfg(all(test, feature = "gui-e2e"))]
mod complete_input_tests {
    use super::*;
    use gpui::AppContext as _;

    #[gpui::test]
    fn actionable_info_repairs_collapsed_focus_once(cx: &mut gpui::TestAppContext) {
        let removed = std::rc::Rc::new(std::cell::Cell::new(0));
        let (stack, _subscription, visible) = cx.update(|cx| {
            let stack = cx.new(|_| ToastStack::new());
            let events = removed.clone();
            let subscription = cx.subscribe(&stack, move |_, _: &ToastRemoved, _| {
                events.set(events.get() + 1);
            });
            let focus = cx.focus_handle();
            let host = gpui::WindowId::from(1);
            let visible = stack.update(cx, |stack, cx| {
                stack.set_host_audience(host, ToastAudience::Repository);
                stack.push_keyed_notify(
                    ToastSpec::new(ToastSubject::Repository, ToastKind::Info, "Older transient"),
                    cx,
                );
                let visible = stack.push_keyed_notify(
                    ToastSpec::new(
                        ToastSubject::Repository,
                        ToastKind::Info,
                        "Visible transient",
                    ),
                    cx,
                );
                stack
                    .toasts()
                    .last()
                    .unwrap()
                    .close_focus
                    .borrow_mut()
                    .push((host, focus.clone()));
                visible
            });
            (stack, subscription, visible)
        });
        let mut first_id = None;
        for detail in ["first receipt", "replacement receipt"] {
            stack.update(cx, |stack, cx| {
                let state = kagi_git::ops::StateSummary {
                    head: "main".into(),
                    dirty: "clean".into(),
                };
                let entry = kagi_git::oplog::OpLogEntry::new(
                    "merge",
                    "/repo",
                    state.clone(),
                    kagi_git::oplog::OpOutcome::Success { after: state },
                );
                let spec = ToastSpec::operation(&entry, ToastKind::Info, detail.into())
                    .with_key(Some("repo-merge-stopped".into()));
                let id = stack.push_keyed_notify(spec, cx);
                if let Some(first_id) = first_id {
                    assert_eq!(id, first_id, "keyed replacement retains the card identity");
                } else {
                    first_id = Some(id);
                }
                assert_eq!(stack.toasts().len(), 3);
                let arrival = stack.toasts().last().unwrap();
                assert!(arrival.is_persistent());
                assert!(matches!(
                    arrival.action,
                    Some(super::super::types::ToastAction::OpenOperationLog(_))
                ));
                assert!(!stack.expanded[ToastAudience::Repository as usize]);
                let transient = stack
                    .toasts()
                    .iter()
                    .find(|toast| toast.id == visible)
                    .unwrap();
                let controls = transient.close_focus.borrow();
                assert!(controls.is_empty(), "the previous preview is now collapsed");
            });
            cx.update(|_| {
                assert_eq!(
                    removed.get(),
                    1,
                    "exactly one repair for the hidden preview, none for keyed replacement"
                );
            });
        }
    }
}

/// A card was removed, including cap eviction; the parent repairs lost focus.
pub(super) struct ToastRemoved(Vec<(gpui::WindowId, gpui::FocusHandle)>);
impl ToastRemoved {
    pub(super) fn from_control(focus: &super::types::ToastCloseFocus) -> Self {
        Self(std::mem::take(&mut *focus.borrow_mut()))
    }

    pub fn was_focused(&self, window: &gpui::Window) -> bool {
        let id = window.window_handle().window_id();
        self.0
            .iter()
            .any(|(host, focus)| *host == id && focus.is_focused(window))
    }
}
impl gpui::EventEmitter<ToastRemoved> for ToastStack {}
pub(super) struct ToastActionRequested {
    pub id: u64,
    pub action: super::types::ToastAction,
}
impl gpui::EventEmitter<ToastActionRequested> for ToastStack {}

fn error_preview(message: &str) -> String {
    let mut preview = String::new();
    let mut lines = 1;
    for (count, ch) in message.chars().enumerate() {
        if count == FOOTER_LINE_CHARS * 3 || (ch == '\n' && lines == 3) {
            preview.push('…');
            break;
        }
        if ch == '\n' {
            lines += 1;
        }
        preview.push(ch);
    }
    preview
}

/// The pinned Button has keyboard focus and a Button role, but exposes its AX
/// name only through a visible label. This adapter adds GPUI's stateful naming
/// API to the existing Button's interactivity; it does not build another control.
#[derive(gpui::IntoElement)]
pub(super) struct ToastButton(
    pub gpui_component::button::Button,
    pub u64,
    pub super::types::ToastCloseFocus,
    pub &'static str,
    pub gpui::ScrollHandle,
);

impl gpui::InteractiveElement for ToastButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}

impl gpui::StatefulInteractiveElement for ToastButton {}

impl gpui::RenderOnce for ToastButton {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        let focus = window
            .use_keyed_state((self.3, self.1), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        #[cfg(feature = "gui-e2e")]
        if self.3 == "toast-dismiss" {
            CLOSE_FOCUS.with(|slot| *slot.borrow_mut() = Some(focus.clone()));
        }
        let id = window.window_handle().window_id();
        let mut hosts = self.2.borrow_mut();
        if let Some((_, previous)) = hosts.iter_mut().find(|(host, _)| *host == id) {
            *previous = focus.clone();
        } else {
            hosts.push((id, focus.clone()));
        }
        drop(hosts);
        super::button_style::reveal_on_focus(
            self.0,
            (self.3, self.1).into(),
            focus,
            self.4,
            window,
            cx,
        )
        .render(window, cx)
    }
}

fn take_toast_focus(toast: &Toast) -> Vec<(gpui::WindowId, gpui::FocusHandle)> {
    let mut focus = std::mem::take(&mut *toast.close_focus.borrow_mut());
    if let Some(action) = &toast.action_focus {
        focus.extend(std::mem::take(&mut *action.borrow_mut()));
    }
    if let Some(refresh) = &toast.refresh_focus {
        focus.extend(std::mem::take(&mut *refresh.borrow_mut()));
    }
    focus
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CLOSE_FOCUS: std::cell::RefCell<Option<gpui::FocusHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
pub fn take_close_focus_for_e2e() -> Option<gpui::FocusHandle> {
    CLOSE_FOCUS.with(|slot| slot.borrow_mut().take())
}
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug, PartialEq)]
pub struct ToastStatusPresentation {
    pub role: Option<gpui::Role>,
    pub label: Option<SharedString>,
    pub newest_id: Option<u64>,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static STATUS: std::cell::RefCell<std::collections::HashMap<(gpui::WindowId, ToastAudience), ToastStatusPresentation>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_status(
    window: gpui::WindowId,
    audience: ToastAudience,
    role: Option<gpui::Role>,
    label: Option<SharedString>,
    newest_id: Option<u64>,
) {
    STATUS.with(|slot| {
        slot.borrow_mut().insert(
            (window, audience),
            ToastStatusPresentation {
                role,
                label,
                newest_id,
            },
        );
    });
}

#[cfg(feature = "gui-e2e")]
pub fn status_for_e2e(
    window: gpui::WindowId,
    audience: ToastAudience,
) -> Option<ToastStatusPresentation> {
    STATUS.with(|slot| slot.borrow().get(&(window, audience)).cloned())
}

/// Captured subject ownership, never inferred from notification text. A stack
/// already belongs to one repository-root window; repository actions retain
/// their exact frozen session/worktree in `refresh_owner` or their receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastSubject {
    Repository,
    Settings,
    App,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToastAudience {
    Repository,
    Settings,
}

impl ToastAudience {
    const ALL: [Self; 2] = [Self::Repository, Self::Settings];

    pub(super) fn includes(self, toast: &Toast) -> bool {
        matches!(
            (self, toast.subject),
            (
                Self::Repository,
                ToastSubject::Repository | ToastSubject::App
            ) | (Self::Settings, ToastSubject::Settings)
        )
    }
}

/// Self-contained toast stack. All toast lifecycle logic lives here.
pub struct ToastStack {
    toasts: Vec<Toast>,
    next_id: u64,
    /// Latest arrival, not the newest remaining card: removing it must not
    /// present an older persistent error as new feedback.
    arrival: Option<(u64, SharedString)>,
    /// True while the auto-dismiss ticker task is running (so we never spawn a
    /// second one). Reset when the stack drains.
    ticker_alive: bool,
    removed_focus: Vec<(gpui::WindowId, gpui::FocusHandle)>,
    pub(super) expanded: [bool; 2],
    pub(super) summary_focus: [super::types::ToastCloseFocus; 2],
    // Portable Settings and its background repository share a window, not a
    // scroll viewport. Keep the foreground audience separate from both offsets.
    host_audiences: Vec<(
        gpui::WindowId,
        ToastAudience,
        [Option<gpui::ScrollHandle>; 2],
    )>,
}

impl ToastStack {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
            ticker_alive: false,
            removed_focus: Vec::new(),
            arrival: None,
            expanded: [false; 2],
            summary_focus: Default::default(),
            host_audiences: Vec::new(),
        }
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    pub(super) fn set_host_audience(
        &mut self,
        window: gpui::WindowId,
        audience: ToastAudience,
    ) -> gpui::ScrollHandle {
        if let Some((_, scope, scrolls)) = self
            .host_audiences
            .iter_mut()
            .find(|(id, _, _)| *id == window)
        {
            *scope = audience;
            scrolls[audience as usize]
                .get_or_insert_with(gpui::ScrollHandle::default)
                .clone()
        } else {
            let scroll = gpui::ScrollHandle::default();
            let mut scrolls = [None, None];
            scrolls[audience as usize] = Some(scroll.clone());
            self.host_audiences.push((window, audience, scrolls));
            scroll
        }
    }

    pub(super) fn status(&self) -> Option<(u64, &SharedString)> {
        self.arrival.as_ref().map(|(id, message)| (*id, message))
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn advance_age_for_e2e(&mut self, duration: Duration) {
        for toast in &mut self.toasts {
            toast.born -= duration;
            if let Some(paused) = toast.paused_at.as_mut() {
                *paused -= duration;
            }
        }
    }

    /// Push a new toast; cap eviction considers existing cards, not the new one.
    pub fn push(&mut self, kind: ToastKind, message: impl Into<SharedString>) {
        self.push_keyed(crate::ui::toast_stack::ToastSpec::new(
            ToastSubject::App,
            kind,
            message,
        ));
    }

    /// Replace repeated results without increasing the notification count.
    /// Ownership is part of insertion, including replacement, before focus repair.
    pub fn push_keyed(&mut self, spec: ToastSpec) -> u64 {
        let ToastSpec {
            subject,
            kind,
            message,
            result_key,
            action,
            refresh_owner,
        } = spec;
        // A toast is only the transient summary. The complete operation result
        // lives in Operation Log, so never let raw multi-line stderr turn the
        // overlay into a screen-sized card.
        let message = if kind == ToastKind::Error {
            SharedString::from(error_preview(message.as_ref()))
        } else {
            SharedString::from(footer_line(message.as_ref()))
        };
        if let Some(at) = result_key.as_ref().and_then(|key| {
            self.toasts
                .iter()
                .position(|toast| toast.result_key.as_ref() == Some(key))
        }) {
            let mut toast = self.toasts.remove(at);
            if toast.dismissing.take().is_some() {
                // A pending exit timer still owns the old id.
                toast.id = self.next_id;
                self.next_id += 1;
            }
            toast.kind = kind;
            toast.subject = subject;
            toast.action = action;
            if toast.action.is_some() {
                toast.action_focus.get_or_insert_with(Default::default);
            } else if let Some(focus) = toast.action_focus.take() {
                self.removed_focus
                    .extend(std::mem::take(&mut *focus.borrow_mut()));
            }
            toast.refresh_owner = refresh_owner;
            if toast.refresh_owner.is_some() {
                toast.refresh_focus.get_or_insert_with(Default::default);
            } else if let Some(focus) = toast.refresh_focus.take() {
                self.removed_focus
                    .extend(std::mem::take(&mut *focus.borrow_mut()));
            }
            toast.message = message.clone();
            toast.born = Instant::now();
            toast.paused_at = None;
            self.arrival = Some((toast.id, message));
            let id = toast.id;
            self.toasts.push(toast);
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.arrival = Some((id, message.clone()));
        self.toasts.push(Toast {
            id,
            kind,
            subject,
            message,
            result_key,
            action_focus: action.as_ref().map(|_| Default::default()),
            refresh_focus: refresh_owner.as_ref().map(|_| Default::default()),
            action,
            refresh_owner,
            born: Instant::now(),
            paused_at: None,
            close_focus: Default::default(),
            dismissing: None,
        });
        if self.toasts.len() > TOASTS_MAX {
            let oldest = self
                .toasts
                .iter()
                .position(|t| t.kind != ToastKind::Error && t.action.is_none() && t.id != id)
                .unwrap_or(0);
            let removed = self.toasts.remove(oldest);
            self.removed_focus = take_toast_focus(&removed);
        }
        id
    }

    /// Begin sliding a toast out (× button or auto-expiry). The caller schedules
    /// the actual removal after `TOAST_REMOVE_MS`. No-op if gone or already leaving.
    pub fn start_exit(&mut self, id: u64) {
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if toast.dismissing.is_some() {
            return;
        }
        toast.dismissing = Some(Instant::now());
    }

    /// Release a retired host's controls without dismissing its shared feedback.
    pub(super) fn forget_window(&mut self, id: gpui::WindowId) {
        #[cfg(feature = "gui-e2e")]
        STATUS.with(|slot| slot.borrow_mut().retain(|(host, _), _| *host != id));
        self.host_audiences.retain(|(host, _, _)| *host != id);
        for focus in &self.summary_focus {
            focus.borrow_mut().retain(|(host, _)| *host != id);
        }
        for toast in &self.toasts {
            toast
                .close_focus
                .borrow_mut()
                .retain(|(host, _)| *host != id);
            if let Some(action) = &toast.action_focus {
                action.borrow_mut().retain(|(host, _)| *host != id);
            }
            if let Some(refresh) = &toast.refresh_focus {
                refresh.borrow_mut().retain(|(host, _)| *host != id);
            }
        }
    }

    /// Remove a toast by id (after the exit animation has played).
    pub fn remove(&mut self, id: u64) {
        if let Some(toast) = self.toasts.iter().find(|t| t.id == id) {
            self.removed_focus = take_toast_focus(toast);
        }
        self.toasts.retain(|t| t.id != id);
        for audience in ToastAudience::ALL {
            let slot = audience as usize;
            if self
                .toasts
                .iter()
                .filter(|toast| audience.includes(toast))
                .count()
                <= 1
            {
                self.expanded[slot] = false;
                self.removed_focus
                    .extend(std::mem::take(&mut *self.summary_focus[slot].borrow_mut()));
            }
        }
        if self.toasts.is_empty() {
            self.arrival = None;
        }
    }

    /// Return ids of toasts that have hit their lifetime and should start exiting.
    pub fn expiring_ids(&self) -> Vec<u64> {
        self.toasts
            .iter()
            .filter(|t| t.should_start_exit())
            .map(|t| t.id)
            .collect()
    }

    /// True if at least one toast is still counting down (not yet sliding out).
    /// The ticker uses this to decide when to stop.
    pub fn has_pending(&self) -> bool {
        self.toasts
            .iter()
            .any(|t| !t.is_persistent() && t.dismissing.is_none())
    }

    /// Pause the short preview timer while the pointer is reading the card.
    pub fn set_hovered(&mut self, id: u64, hovered: bool) {
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if hovered {
            toast.paused_at.get_or_insert_with(Instant::now);
        } else if let Some(start) = toast.paused_at.take() {
            toast.born += start.elapsed();
        }
    }

    /// Escape dismisses only the newest notification visible to this audience.
    pub fn dismiss_newest(&mut self, audience: ToastAudience, cx: &mut Context<Self>) -> bool {
        let Some(id) = self
            .toasts
            .iter()
            .rev()
            .find(|t| t.dismissing.is_none() && audience.includes(t))
            .map(|t| t.id)
        else {
            return false;
        };
        self.begin_exit(id, cx);
        true
    }

    // ── Entity orchestration (cx-driven) ─────────────────────────────────────
    pub(super) fn toggle_expanded(&mut self, audience: ToastAudience, cx: &mut Context<Self>) {
        let slot = audience as usize;
        self.expanded[slot] = !self.expanded[slot];
        self.repair_hidden_focus(cx);
        cx.notify();
    }
    fn repair_hidden_focus(&mut self, cx: &mut Context<Self>) {
        let newest: [Option<u64>; 2] = std::array::from_fn(|slot| {
            let audience = ToastAudience::ALL[slot];
            self.toasts
                .iter()
                .rev()
                .find(|toast| audience.includes(toast))
                .map(|toast| toast.id)
        });
        let mut hidden_focus = Vec::new();
        for toast in &mut self.toasts {
            let visible: [bool; 2] = std::array::from_fn(|slot| {
                let audience = ToastAudience::ALL[slot];
                audience.includes(toast) && (self.expanded[slot] || newest[slot] == Some(toast.id))
            });
            let visible_in = |window| {
                self.host_audiences
                    .iter()
                    .any(|(host, audience, _)| *host == window && visible[*audience as usize])
            };
            for controls in std::iter::once(&toast.close_focus)
                .chain(toast.action_focus.iter())
                .chain(toast.refresh_focus.iter())
            {
                controls.borrow_mut().retain(|(window, focus)| {
                    if visible_in(*window) {
                        true
                    } else {
                        hidden_focus.push((*window, focus.clone()));
                        false
                    }
                });
            }
            if !self
                .host_audiences
                .iter()
                .any(|(_, audience, _)| visible[*audience as usize])
            {
                if let Some(start) = toast.paused_at.take() {
                    toast.born += start.elapsed();
                }
            }
        }
        if !hidden_focus.is_empty() {
            cx.emit(ToastRemoved(hidden_focus));
        }
    }

    // These wrap the pure data methods with `cx.notify()` + timer tasks so the
    // overlay re-renders in isolation (ADR-0110 Phase 5).

    /// Push app-owned feedback, (re)start the auto-dismiss ticker, and re-render
    /// this entity's subtree only.
    pub fn push_notify(
        &mut self,
        kind: ToastKind,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push_keyed_notify(
            crate::ui::toast_stack::ToastSpec::new(ToastSubject::App, kind, message),
            cx,
        );
    }

    pub fn push_keyed_notify(&mut self, spec: ToastSpec, cx: &mut Context<Self>) -> u64 {
        let id = self.push_keyed(spec);
        self.repair_hidden_focus(cx);
        if !self.removed_focus.is_empty() {
            cx.emit(ToastRemoved(std::mem::take(&mut self.removed_focus)));
        }
        self.ensure_ticker(cx);
        cx.notify();
        id
    }

    pub(super) fn activate_refresh(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(owner) = self
            .toasts
            .iter()
            .find(|toast| toast.id == id && toast.dismissing.is_none())
            .and_then(|toast| toast.refresh_owner.clone())
        {
            cx.emit(ToastActionRequested {
                id,
                action: super::types::ToastAction::RefreshRepository(owner),
            });
        }
    }

    pub(super) fn dismiss_restore_failure(
        &mut self,
        owner: &crate::app::Attachment,
        cx: &mut Context<Self>,
    ) {
        for index in 0..self.toasts.len() {
            let toast = &self.toasts[index];
            if toast.dismissing.is_none()
                && toast.refresh_owner.as_ref().is_some_and(|captured| {
                    captured.session == owner.session
                        && captured.path == owner.path
                        && captured.worktree == owner.worktree
                })
            {
                let id = toast.id;
                self.begin_exit(id, cx);
            }
        }
    }

    pub(super) fn activate_action(&mut self, id: u64, cx: &mut Context<Self>) {
        let action = self
            .toasts
            .iter()
            .find(|toast| toast.id == id && toast.dismissing.is_none())
            .and_then(|toast| toast.action.clone());
        if let Some(action) = action {
            // The app acknowledges only after the destination is reachable.
            cx.emit(ToastActionRequested { id, action });
        }
    }

    /// Begin sliding a toast out (× button or auto-expiry), then remove it once
    /// the exit animation has played. Re-renders only this entity.
    pub fn begin_exit(&mut self, id: u64, cx: &mut Context<Self>) {
        self.start_exit(id);
        cx.notify();
        cx.spawn(async move |this, acx| {
            acx.background_executor()
                .timer(Duration::from_millis(TOAST_REMOVE_MS))
                .await;
            let _ = this.update(acx, |stack, cx| {
                stack.remove(id);
                cx.emit(ToastRemoved(std::mem::take(&mut stack.removed_focus)));
                cx.notify();
            });
        })
        .detach();
    }

    /// Spawn the 150ms auto-dismiss ticker if toasts are pending and it is not
    /// already running. The task exits as soon as the stack drains.
    fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.is_empty() || !self.has_pending() || self.ticker_alive {
            return;
        }
        self.ticker_alive = true;
        cx.spawn(async move |this, acx| loop {
            acx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let finished = this.update(acx, |stack, cx| {
                for id in stack.expiring_ids() {
                    stack.begin_exit(id, cx);
                }
                if !stack.has_pending() {
                    stack.ticker_alive = false;
                    true
                } else {
                    false
                }
            });
            match finished {
                Ok(true) | Err(_) => break,
                Ok(false) => {}
            }
        })
        .detach();
    }
}

impl Default for ToastStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gui-e2e")]
    #[test]
    fn status_recorder_clears_arrivals_without_crossing_hosts() {
        let first = gpui::WindowId::from(101);
        let second = gpui::WindowId::from(102);
        record_status(
            first,
            ToastAudience::Repository,
            Some(gpui::Role::Status),
            Some("arrival".into()),
            Some(7),
        );
        record_status(
            first,
            ToastAudience::Settings,
            Some(gpui::Role::Status),
            Some("settings".into()),
            Some(8),
        );
        record_status(
            second,
            ToastAudience::Repository,
            Some(gpui::Role::Status),
            Some("other".into()),
            Some(9),
        );
        record_status(
            first,
            ToastAudience::Repository,
            Some(gpui::Role::Status),
            Some("".into()),
            None,
        );
        let cleared = status_for_e2e(first, ToastAudience::Repository).unwrap();
        assert_eq!(cleared.newest_id, None);
        assert_eq!(cleared.label.as_deref(), Some(""));
        assert_eq!(
            status_for_e2e(first, ToastAudience::Settings)
                .unwrap()
                .newest_id,
            Some(8)
        );
        assert_eq!(
            status_for_e2e(second, ToastAudience::Repository)
                .unwrap()
                .newest_id,
            Some(9)
        );
    }

    #[test]
    fn shared_host_preserves_card_identity_and_persistence() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Error, "persistent");
        let error = stack.toasts()[0].id;
        stack.push(ToastKind::Success, "actionable");
        stack.toasts.last_mut().unwrap().action = Some(
            super::super::types::ToastAction::OpenRepository("clone".into()),
        );
        let actionable = stack.toasts()[1].id;
        for kind in [ToastKind::Info, ToastKind::Success] {
            stack.push(kind, "ephemeral");
            let transient = stack.toasts().last().unwrap().id;
            assert_eq!(
                stack
                    .toasts()
                    .iter()
                    .filter(|toast| toast.is_persistent())
                    .map(|toast| toast.id)
                    .collect::<Vec<_>>(),
                [error, actionable],
            );
            assert_eq!(
                stack
                    .toasts()
                    .iter()
                    .filter(|toast| !toast.is_persistent())
                    .map(|toast| toast.id)
                    .collect::<Vec<_>>(),
                [transient],
            );
            stack.start_exit(transient);
            stack.remove(transient);
            assert_eq!(stack.toasts().len(), 2);
        }
        let window = gpui::WindowId::from(1);
        let repository = stack.set_host_audience(window, ToastAudience::Repository);
        let settings = stack.set_host_audience(window, ToastAudience::Settings);
        repository.set_offset(gpui::point(gpui::px(0.), gpui::px(-20.)));
        settings.set_offset(gpui::point(gpui::px(0.), gpui::px(-80.)));
        assert_eq!(repository.offset().y, gpui::px(-20.));
        assert_eq!(settings.offset().y, gpui::px(-80.));
        assert_eq!(
            stack
                .set_host_audience(window, ToastAudience::Repository)
                .offset()
                .y,
            gpui::px(-20.)
        );
        assert_eq!(
            stack
                .set_host_audience(window, ToastAudience::Settings)
                .offset()
                .y,
            gpui::px(-80.)
        );
        assert_eq!(stack.host_audiences.len(), 1);
        assert_eq!(stack.host_audiences[0].0, window);
        assert_eq!(stack.host_audiences[0].1, ToastAudience::Settings);
        stack.forget_window(window);
        assert!(stack.host_audiences.is_empty());
    }

    #[cfg(feature = "gui-e2e")]
    #[gpui::test]
    fn transient_arrival_repairs_collapsed_focus_in_every_host(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext;
        cx.update(|cx| {
            let stack = cx.new(|_| ToastStack::new());
            stack.update(cx, |stack, cx| {
                let split = gpui::WindowId::from(1);
                let repository = gpui::WindowId::from(2);
                stack.set_host_audience(split, ToastAudience::Repository);
                stack.set_host_audience(repository, ToastAudience::Repository);
                stack.push(ToastKind::Error, "persistent");
                stack.toasts()[0]
                    .close_focus
                    .borrow_mut()
                    .extend([(split, cx.focus_handle()), (repository, cx.focus_handle())]);
                stack.push(ToastKind::Info, "ephemeral");
                stack.repair_hidden_focus(cx);
                let retained = stack.toasts()[0].close_focus.borrow();
                assert!(retained.is_empty());
                drop(retained);
                stack.toggle_expanded(crate::ui::toast_stack::ToastAudience::Repository, cx);
                assert!(stack.expanded[ToastAudience::Repository as usize]);
                assert!(!stack.expanded[ToastAudience::Settings as usize]);
            });
        });
    }

    #[test]
    fn repeated_operation_result_replaces_without_growing_the_stack() {
        let mut stack = ToastStack::new();
        let key = "repo\0push\0failed".to_string();
        stack.push_keyed(
            crate::ui::toast_stack::ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Error,
                "first cause",
            )
            .with_key(Some(key.clone())),
        );
        let id = stack.toasts()[0].id;
        let focus = stack.toasts()[0].close_focus.clone();
        stack.push_keyed(
            crate::ui::toast_stack::ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Error,
                "new cause",
            )
            .with_key(Some(key.clone())),
        );
        assert_eq!(stack.toasts().len(), 1);
        assert_eq!(stack.toasts()[0].id, id);
        assert!(std::rc::Rc::ptr_eq(&stack.toasts()[0].close_focus, &focus));
        assert!(stack.toasts()[0].message.contains("new cause"));
        stack.start_exit(id);
        stack.push_keyed(
            crate::ui::toast_stack::ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Error,
                "retry result",
            )
            .with_key(Some(key)),
        );
        assert_eq!(
            stack.toasts().len(),
            1,
            "an exiting duplicate is replaced too"
        );
        let replacement = stack.toasts()[0].id;
        assert_ne!(replacement, id);
        stack.remove(id);
        assert_eq!(
            stack.toasts()[0].id,
            replacement,
            "old exit timer cannot remove the replacement"
        );
        stack.push_keyed(
            crate::ui::toast_stack::ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Success,
                "done",
            )
            .with_key(Some("repo\0push\0success".into())),
        );
        stack.push_keyed(
            crate::ui::toast_stack::ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Error,
                "other repo",
            )
            .with_key(Some("other\0push\0failed".into())),
        );
        assert_eq!(
            stack.toasts().len(),
            3,
            "outcome and repository are part of the key"
        );
    }

    #[cfg(feature = "gui-e2e")]
    #[gpui::test]
    fn closed_window_releases_persistent_error_close_handles(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext;
        cx.update(|cx| {
            let handles = cx.new(|cx| (cx.focus_handle(), cx.focus_handle()));
            let (closed_focus, live_focus) = handles.read(cx);
            let closed = gpui::WindowId::from(1);
            let live = gpui::WindowId::from(2);
            let mut stack = ToastStack::new();
            stack.push(ToastKind::Error, "persistent error");
            stack.toasts()[0]
                .close_focus
                .borrow_mut()
                .extend([(closed, closed_focus.clone()), (live, live_focus.clone())]);
            let refresh = super::super::types::ToastCloseFocus::default();
            refresh
                .borrow_mut()
                .extend([(closed, closed_focus.clone()), (live, live_focus.clone())]);
            stack.toasts[0].refresh_focus = Some(refresh.clone());
            stack.forget_window(closed);
            stack.forget_window(closed);
            assert_eq!(stack.toasts().len(), 1, "closing a host keeps the error");
            let remaining = stack.toasts()[0].close_focus.borrow();
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].0, live, "the surviving host keeps its handle");
            assert_eq!(refresh.borrow().len(), 1);
            assert_eq!(refresh.borrow()[0].0, live);
            drop(remaining);
            stack.remove(stack.toasts()[0].id);
            assert!(refresh.borrow().is_empty());
        });
    }

    #[cfg(feature = "gui-e2e")]
    #[gpui::test]
    fn accepted_restore_read_matches_owner_without_card_activation(cx: &mut gpui::TestAppContext) {
        use crate::app::{Attachment, RepoId, SessionId, TabId, WorktreeId};
        use gpui::AppContext;
        let owner = Attachment {
            session: SessionId {
                tab: TabId(1),
                incarnation: 1,
            },
            visit: 0,
            path: "/repo".into(),
            worktree: Some(WorktreeId {
                repo: RepoId("/repo/.git".into()),
                git_dir: "/repo/.git".into(),
            }),
        };
        cx.update(|cx| {
            let stack = cx.new(|_| ToastStack::new());
            stack.update(cx, |stack, cx| {
                for message in ["restore failed", "retry failed"] {
                    let mut spec =
                        ToastSpec::new(ToastSubject::Repository, ToastKind::Error, message);
                    spec.refresh_owner = Some(owner.clone());
                    stack.push_keyed_notify(spec, cx);
                }
                for mismatch in 0..3 {
                    let mut unrelated = owner.clone();
                    match mismatch {
                        0 => unrelated.session.incarnation += 1,
                        1 => unrelated.path = "/other".into(),
                        _ => unrelated.worktree = None,
                    }
                    stack.dismiss_restore_failure(&unrelated, cx);
                    assert!(stack
                        .toasts()
                        .iter()
                        .all(|toast| toast.dismissing.is_none()));
                }
                let mut reactivated = owner.clone();
                reactivated.visit += 1;
                stack.dismiss_restore_failure(&reactivated, cx);
                assert!(stack
                    .toasts()
                    .iter()
                    .all(|toast| toast.dismissing.is_some()));
            });
        });
    }

    #[cfg(feature = "gui-e2e")]
    #[gpui::test]
    fn settings_arrival_preserves_collapsed_repository_controls(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        for kind in [ToastKind::Info, ToastKind::Error] {
            let removed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let (stack, _subscription, focus, id) = cx.update(|cx| {
                let stack = cx.new(|_| ToastStack::new());
                let events = removed.clone();
                let subscription = cx.subscribe(&stack, move |_, event: &ToastRemoved, _| {
                    events.borrow_mut().extend(event.0.iter().cloned());
                });
                let focus = cx.focus_handle();
                let host = gpui::WindowId::from(1);
                let id = stack.update(cx, |stack, cx| {
                    stack.set_host_audience(host, ToastAudience::Repository);
                    stack.push_keyed_notify(
                        crate::ui::toast_stack::ToastSpec::new(
                            ToastSubject::Repository,
                            kind,
                            "Older notice",
                        ),
                        cx,
                    );
                    let id = stack.push_keyed_notify(
                        crate::ui::toast_stack::ToastSpec::new(
                            ToastSubject::Repository,
                            kind,
                            "Visible notice",
                        ),
                        cx,
                    );
                    assert!(!stack.expanded[ToastAudience::Repository as usize]);
                    stack
                        .toasts()
                        .last()
                        .unwrap()
                        .close_focus
                        .borrow_mut()
                        .push((host, focus.clone()));
                    id
                });
                (stack, subscription, focus, id)
            });
            stack.update(cx, |stack, cx| {
                stack.push_keyed_notify(
                    crate::ui::toast_stack::ToastSpec::new(
                        ToastSubject::Settings,
                        kind,
                        "Settings completion",
                    ),
                    cx,
                );
                let toast = stack.toasts().iter().find(|toast| toast.id == id).unwrap();
                let controls = toast.close_focus.borrow();
                assert_eq!(
                    controls.len(),
                    1,
                    "arrival must not retire the visible Repository control"
                );
                assert_eq!(controls[0].1, focus, "the original focus handle survives");
                assert_eq!(
                    stack.toasts().last().unwrap().subject,
                    ToastSubject::Settings
                );
            });
            cx.update(|_| {
                assert!(
                    removed.borrow().is_empty(),
                    "no ToastRemoved may reach the production focus subscriber"
                );
            });
        }
    }

    #[cfg(feature = "gui-e2e")]
    #[gpui::test]
    fn typed_subjects_isolate_cards_and_expansion(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| {
            let stack = cx.new(|_| ToastStack::new());
            stack.update(cx, |stack, cx| {
                let repository = stack.push_keyed_notify(
                    crate::ui::toast_stack::ToastSpec::new(
                        ToastSubject::Repository,
                        ToastKind::Error,
                        "Identical text is not an owner",
                    ),
                    cx,
                );
                assert!(ToastAudience::Repository.includes(&stack.toasts()[0]));
                assert!(!ToastAudience::Settings.includes(&stack.toasts()[0]));
                let settings = stack.push_keyed_notify(
                    crate::ui::toast_stack::ToastSpec::new(
                        ToastSubject::Settings,
                        ToastKind::Error,
                        "Identical text is not an owner",
                    ),
                    cx,
                );
                for (audience, expected) in [
                    (ToastAudience::Repository, vec![repository]),
                    (ToastAudience::Settings, vec![settings]),
                ] {
                    assert_eq!(
                        stack
                            .toasts()
                            .iter()
                            .filter(|toast| audience.includes(toast))
                            .map(|toast| toast.id)
                            .collect::<Vec<_>>(),
                        expected
                    );
                }
                stack.toggle_expanded(ToastAudience::Settings, cx);
                assert!(stack.expanded[ToastAudience::Settings as usize]);
                assert!(!stack.expanded[ToastAudience::Repository as usize]);
                stack.start_exit(settings);
                assert!(stack
                    .toasts()
                    .iter()
                    .find(|toast| toast.id == settings)
                    .unwrap()
                    .dismissing
                    .is_some());
                assert!(stack
                    .toasts()
                    .iter()
                    .find(|toast| toast.id == repository)
                    .unwrap()
                    .dismissing
                    .is_none());
            });
        });
    }

    #[cfg(feature = "gui-e2e")]
    #[gpui::test]
    fn escape_dismisses_only_its_window_audience(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        cx.update(|cx| {
            let stack = cx.new(|_| ToastStack::new());
            stack.update(cx, |stack, cx| {
                stack.push_keyed_notify(
                    crate::ui::toast_stack::ToastSpec::new(
                        ToastSubject::Repository,
                        ToastKind::Error,
                        "Same caption",
                    ),
                    cx,
                );
                stack.push_keyed_notify(
                    crate::ui::toast_stack::ToastSpec::new(
                        ToastSubject::Settings,
                        ToastKind::Error,
                        "Same caption",
                    ),
                    cx,
                );
                assert!(stack.dismiss_newest(ToastAudience::Repository, cx));
                assert!(stack.toasts()[0].dismissing.is_some());
                assert!(stack.toasts()[1].dismissing.is_none());
                assert!(!stack.dismiss_newest(ToastAudience::Repository, cx));
                assert!(stack.toasts()[1].dismissing.is_none());
                assert!(stack.dismiss_newest(ToastAudience::Settings, cx));
                assert!(!stack.dismiss_newest(ToastAudience::Settings, cx));
                stack.push(ToastKind::Info, "App notice");
                assert!(!stack.dismiss_newest(ToastAudience::Settings, cx));
                assert!(stack.dismiss_newest(ToastAudience::Repository, cx));
            });
        });
    }

    #[test]
    fn persistent_card_survives_transient_expiry_until_explicit_dismissal() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Info, "routine");
        assert!(!stack.toasts()[0].is_persistent());
        stack.push(ToastKind::Error, "persistent");
        let error = stack.toasts()[1].id;
        stack.push(ToastKind::Success, "routine completion");
        let transient = stack.toasts()[2].id;
        stack.start_exit(transient);
        stack.remove(transient);
        assert!(stack
            .toasts()
            .iter()
            .any(|toast| toast.id == error && toast.dismissing.is_none()));
        stack.start_exit(error);
        assert!(stack
            .toasts()
            .iter()
            .find(|toast| toast.id == error)
            .unwrap()
            .dismissing
            .is_some());
    }

    #[test]
    fn push_and_cap() {
        let mut stack = ToastStack::new();
        for i in 0..(TOASTS_MAX + 2) {
            stack.push(ToastKind::Info, format!("msg {i}"));
        }
        assert_eq!(stack.toasts().len(), TOASTS_MAX);
        // Oldest should have been dropped.
        assert!(!stack.toasts().iter().any(|t| t.message == "msg 0"));
    }

    #[test]
    fn actionable_completion_waits_for_explicit_dismissal() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Success, "clone finished");
        let toast = stack.toasts.last_mut().unwrap();
        toast.action = Some(super::super::types::ToastAction::OpenRepository(
            "clone".into(),
        ));
        toast.born -= Duration::from_secs(60);
        assert!(stack.expiring_ids().is_empty());
        assert!(
            !stack.has_pending(),
            "actionable completion must stop the expiry ticker"
        );
        let id = stack.toasts[0].id;
        stack.start_exit(id);
        assert!(stack.toasts[0].dismissing.is_some());
    }

    #[test]
    fn error_preview_is_bounded_by_lines_and_characters() {
        let mut stack = ToastStack::new();
        let long = format!("first line\n{}", "x".repeat(1_000));
        stack.push(ToastKind::Error, long);
        let message = stack.toasts()[0].message.as_ref();
        assert_eq!(message.chars().count(), FOOTER_LINE_CHARS * 3 + 1);
        assert!(message.ends_with('…'));
        assert_eq!(
            error_preview("one\ntwo\nthree\nfour\nfive\nsix"),
            "one\ntwo\nthree…"
        );
        assert_eq!(
            error_preview("一行目\n二行目\n三行目"),
            "一行目\n二行目\n三行目"
        );
    }

    #[test]
    fn start_exit_is_idempotent() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Success, "ok");
        let id = stack.toasts()[0].id;
        stack.start_exit(id);
        assert!(stack.toasts()[0].dismissing.is_some());
        // Second call is a no-op.
        let first = stack.toasts()[0].dismissing;
        stack.start_exit(id);
        assert_eq!(stack.toasts()[0].dismissing, first);
    }

    #[test]
    fn remove_by_id() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Error, "err");
        let id = stack.toasts()[0].id;
        assert_eq!(stack.toasts().len(), 1);
        stack.remove(id);
        assert!(stack.is_empty());
    }

    #[test]
    fn cap_preserves_errors_before_routine_notifications() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Error, "important");
        for _ in 0..TOASTS_MAX {
            stack.push(ToastKind::Info, "routine");
        }
        assert_eq!(stack.toasts.len(), TOASTS_MAX);
        assert_eq!(stack.toasts[0].message, "important");
    }

    #[test]
    fn cap_preserves_clone_action_before_routine_notifications() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Success, "clone finished");
        stack.toasts.last_mut().unwrap().action = Some(
            super::super::types::ToastAction::OpenRepository("clone".into()),
        );
        stack.push(ToastKind::Error, "important");
        for _ in 0..TOASTS_MAX {
            stack.push(ToastKind::Info, "routine");
        }
        assert_eq!(stack.toasts.len(), TOASTS_MAX);
        assert!(
            stack.toasts.iter().any(|toast| toast.action.is_some()),
            "routine arrivals must not evict the clone's Open Repository action"
        );
        assert!(stack
            .toasts
            .iter()
            .any(|toast| toast.kind == ToastKind::Error));
    }

    #[test]
    fn errors_never_expire_and_hover_pauses_routine_timer() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Error, "important");
        stack.push(ToastKind::Success, "done");
        for toast in &mut stack.toasts {
            toast.born -= Duration::from_secs(60);
        }
        let id = stack.toasts[1].id;
        stack.set_hovered(id, true);
        assert!(stack.expiring_ids().is_empty());
        stack.set_hovered(id, false);
        assert_eq!(stack.expiring_ids(), vec![id]);
    }

    #[test]
    fn new_info_survives_a_full_error_stack() {
        let mut stack = ToastStack::new();
        for _ in 0..TOASTS_MAX {
            stack.push(ToastKind::Error, "error");
        }
        let oldest = stack.toasts[0].id;
        stack.push(ToastKind::Info, "new info");
        assert_eq!(
            stack.toasts.last().map(|t| t.message.as_ref()),
            Some("new info")
        );
        assert!(!stack.toasts.iter().any(|t| t.id == oldest));
    }

    #[test]
    fn status_changes_only_on_arrival_or_empty_stack() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Error, "old error");
        stack.push(ToastKind::Info, "latest arrival");
        let (id, message) = stack.status().unwrap();
        let message = message.clone();
        stack.set_hovered(id, true);
        stack.start_exit(id);
        assert_eq!(stack.status(), Some((id, &message)));
        stack.remove(id);
        assert_eq!(stack.status(), Some((id, &message)));
        stack.push(ToastKind::Info, message.clone());
        assert_ne!(stack.status().unwrap().0, id);
        for id in stack
            .toasts()
            .iter()
            .map(|toast| toast.id)
            .collect::<Vec<_>>()
        {
            stack.remove(id);
        }
        assert!(stack.status().is_none());
    }
}
