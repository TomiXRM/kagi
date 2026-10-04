//! The operation-queue strip above the status bar (#355 stage 3a,
//! ADR-0204 決定 2): the front tab's queued intents, each removable, and its
//! cancel list until the user clears it, the tab closes or the app quits.
//! Another tab's intents never appear here.
use std::time::Instant;

use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_ui_core::i18n::{queue_text, QueueText};
use kagi_ui_core::slow_read::is_slow;

use super::op_queue::intent_label;
use super::theme::{self, theme};
use super::KagiApp;
use crate::app::{IntentId, IntentState};

/// What the strip draws, computed from the queue alone (決定 2: observed
/// values only, never a projection of the repository).
pub(crate) struct StripModel {
    /// `queued: N` — not yet writing.
    pub(crate) count: usize,
    pub(crate) rows: Vec<StripRow>,
    /// Newest first.
    pub(crate) cancelled: Vec<(IntentId, String, &'static str)>,
}

pub(crate) struct StripRow {
    pub(crate) id: IntentId,
    pub(crate) label: String,
    pub(crate) state: String,
    pub(crate) removable: bool,
}

impl KagiApp {
    /// `None` while the front tab has neither intents nor cancellations.
    pub(crate) fn queue_strip_model(&self, now: Instant) -> Option<StripModel> {
        let session = self.active_session()?;
        let queue = &self.op_queue.queue;
        let intents: Vec<_> = queue.intents(session).into_iter().flatten().collect();
        let cancelled: Vec<_> = queue.cancelled(session).into_iter().flatten().collect();
        if intents.is_empty() && cancelled.is_empty() {
            return None;
        }
        let rows = intents
            .into_iter()
            .map(|intent| {
                let mut state = KagiApp::queue_state_text(intent.state).to_string();
                // The running row carries #995's elapsed seconds once slow.
                if let IntentState::Running { .. } = intent.state {
                    if let Some((_, started)) = self.app_sessions.running_lease() {
                        let elapsed = now.saturating_duration_since(started);
                        if is_slow(elapsed) {
                            state = format!("{state} · {} s", elapsed.as_secs());
                        }
                    }
                }
                StripRow {
                    id: intent.id,
                    label: intent_label(&intent.request),
                    state,
                    removable: removable(intent.state),
                }
            })
            .collect();
        let cancelled = cancelled
            .into_iter()
            .rev()
            .filter_map(|intent| match intent.state {
                IntentState::Cancelled { reason } => Some((
                    intent.id,
                    intent_label(&intent.request),
                    KagiApp::queue_cancel_text(reason),
                )),
                _ => None,
            })
            .collect();
        Some(StripModel {
            count: queue.queued_count(session),
            rows,
            cancelled,
        })
    }

    pub(super) fn render_queue_strip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let model = self.queue_strip_model(cx.background_executor().now())?;
        let mut strip = div()
            .id("queue-strip")
            .flex()
            .flex_col()
            // Right-aligned: the toast stack and busy snackbar float over the
            // bottom-left corner above the status bar, exactly while a queue
            // is waiting on a running write.
            .items_end()
            .w_full()
            .flex_shrink_0()
            .px_3()
            .py(theme::scaled_px(4.0))
            .gap(theme::scaled_px(2.0))
            .bg(rgb(theme().surface))
            .border_t_1()
            .border_color(rgb(theme().selected))
            .text_xs()
            .text_color(rgb(theme().text_main));
        if !model.rows.is_empty() {
            strip = strip.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(SharedString::from(format!(
                                "{}: {}",
                                queue_text(QueueText::Count),
                                model.count
                            ))),
                    )
                    .when(model.count > 0, |el| {
                        el.child(strip_button(
                            "queue-strip-cancel-all",
                            queue_text(QueueText::CancelAll),
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.queue_cancel_all(cx)
                            }),
                        ))
                    }),
            );
            for row in model.rows {
                strip = strip.child(render_row(row, cx));
            }
        }
        if !model.cancelled.is_empty() {
            strip = strip.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from(format!(
                                "{}: {}",
                                queue_text(QueueText::Cancelled),
                                model.cancelled.len()
                            ))),
                    )
                    .child(strip_button(
                        "queue-strip-clear",
                        queue_text(QueueText::Clear),
                        cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            this.queue_dismiss_cancelled(cx)
                        }),
                    )),
            );
            for (id, label, reason) in model.cancelled {
                strip = strip.child(super::e2e::measure_control(
                    format!("queue-strip-cancelled-{}", id.0),
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(label))
                        .child(SharedString::from(reason)),
                ));
            }
        }
        Some(super::e2e::measure_control("queue-strip", strip))
    }
}

fn render_row(row: StripRow, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let id = row.id;
    super::e2e::measure_control(
        format!("queue-strip-row-{}", id.0),
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(SharedString::from(row.label))
            .child(
                div()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(row.state)),
            )
            .when(row.removable, |el| {
                el.child(strip_button(
                    format!("queue-strip-remove-{}", id.0),
                    queue_text(QueueText::Remove),
                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.queue_remove(id, cx)),
                ))
            }),
    )
}

/// Every row that has not started writing can be taken out: a queued or
/// waiting one is removed, the head being planned or confirmed is cancelled
/// (ADR-0204 決定 3). A running write has no cancel.
fn removable(state: IntentState) -> bool {
    matches!(
        state,
        IntentState::Queued
            | IntentState::Waiting { .. }
            | IntentState::Planning
            | IntentState::AwaitingConfirm
    )
}

fn strip_button(
    name: impl Into<String>,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let name = name.into();
    super::e2e::measure_control(
        name.clone(),
        div()
            .id(SharedString::from(name))
            .px_2()
            .py(theme::scaled_px(1.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(theme().selected))
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(theme().selected)))
            .child(SharedString::from(label))
            .on_click(on_click),
    )
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// The strip as drawn at `now`: `(queued: N, [(label, state)], [(label,
    /// reason)])`, or `None` when the front tab has no strip.
    #[allow(clippy::type_complexity)]
    pub fn queue_strip_for_e2e(
        &self,
        now: Instant,
    ) -> Option<(usize, Vec<(String, String)>, Vec<(String, String)>)> {
        self.queue_strip_model(now).map(|model| {
            (
                model.count,
                model.rows.into_iter().map(|r| (r.label, r.state)).collect(),
                model
                    .cancelled
                    .into_iter()
                    .map(|(_, label, reason)| (label, reason.to_string()))
                    .collect(),
            )
        })
    }

    /// Ids of the front tab's intents, oldest first (control names).
    pub fn queue_ids_for_e2e(&self) -> Vec<u64> {
        self.active_session()
            .and_then(|session| self.op_queue.queue.intents(session))
            .map(|q| q.iter().map(|intent| intent.id.0).collect())
            .unwrap_or_default()
    }

    pub fn queued_commit_message_for_e2e(&self) -> Option<String> {
        self.queued_commit_modal().map(|m| m.message.clone())
    }
}
