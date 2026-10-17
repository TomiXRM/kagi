//! The operation-queue strip above the status bar (#355 stage 3a,
//! ADR-0204 決定 2): the front tab's queued intents, each removable, and its
//! cancel list until the user clears it, the tab closes or the app quits.
//! Another tab's intents never appear here.
use std::time::Instant;

use gpui::{div, prelude::*, px, rgb, Context, SharedString, Window};
use gpui_component::scroll::ScrollableElement;
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

    pub(super) fn render_queue_strip(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let model = self.queue_strip_model(cx.background_executor().now())?;
        let owner = self.active_session()?;
        let scroll = window
            .use_keyed_state(("queue-strip-scroll", owner.incarnation), cx, |_, _| {
                gpui::ScrollHandle::default()
            })
            .read(cx)
            .clone();
        let header = render_header(&model, cx);
        let mut rows = div()
            .id(("queue-strip-rows", owner.incarnation))
            .relative()
            .w_full()
            .min_h(px(0.))
            .flex_shrink(1.)
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .vertical_scrollbar(&scroll)
            .flex()
            .flex_col()
            .items_end()
            // The pinned scrollbar's 16px rail must not cover Remove.
            .pr(px(16.))
            .gap(theme::scaled_px(2.));
        for row in model.rows {
            rows = rows.child(render_row(row, cx));
        }
        for (id, label, reason) in model.cancelled {
            rows = rows.child(
                div()
                    .relative()
                    .flex()
                    .flex_row()
                    .flex_shrink_0()
                    .gap_2()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(label))
                    .child(SharedString::from(reason))
                    .when(cfg!(feature = "gui-e2e"), |row| {
                        row.child(super::e2e::measure_inside(format!(
                            "queue-strip-cancelled-{}",
                            id.0
                        )))
                    }),
            );
        }
        Some(
            div()
                .id("queue-strip")
                .relative()
                .flex()
                .flex_col()
                .w_full()
                .flex_shrink_0()
                .min_h(px(0.))
                .max_h(theme::scaled_px(240.).min(window.viewport_size().height * 0.2))
                .overflow_hidden()
                .px_3()
                .py(theme::scaled_px(4.))
                .gap(theme::scaled_px(2.))
                .bg(rgb(theme().surface))
                .border_t_1()
                .border_color(rgb(theme().selected))
                .text_xs()
                .text_color(rgb(theme().text_main))
                .child(header)
                .child(super::e2e::measure_leaf("queue-strip-scroll", rows))
                .when(cfg!(feature = "gui-e2e"), |strip| {
                    strip.child(super::e2e::measure_inside("queue-strip"))
                })
                .into_any_element(),
        )
    }
}

fn render_header(model: &StripModel, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    // Keep actions visible while the one shared row viewport scrolls. Align
    // right so the floating busy snackbar and toast stack keep the left corner.
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_end()
        .flex_shrink_0()
        .gap_3()
        .when(!model.rows.is_empty(), |header| {
            header.child(
                div()
                    .flex()
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
                    .when(model.count > 0, |group| {
                        group.child(strip_button(
                            "queue-strip-cancel-all",
                            queue_text(QueueText::CancelAll),
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.queue_cancel_all(cx)
                            }),
                        ))
                    }),
            )
        })
        .when(!model.cancelled.is_empty(), |header| {
            header.child(
                div()
                    .flex()
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
            )
        })
        .into_any_element()
}

fn render_row(row: StripRow, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let id = row.id;
    div()
        .relative()
        .flex()
        .flex_row()
        .flex_shrink_0()
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
        })
        .when(cfg!(feature = "gui-e2e"), |row| {
            row.child(super::e2e::measure_inside(format!(
                "queue-strip-row-{}",
                id.0
            )))
        })
        .into_any_element()
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
