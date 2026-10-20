//! Historical document presentation. Parsing/rasterization belongs to kagi-ui-editor.
use gpui::{
    div, prelude::*, px, rgb, App, Context, Entity, FocusHandle, Focusable, SharedString,
    Subscription, Window,
};
use gpui_component::{Disableable as _, Selectable as _};
use kagi_ui_editor::{PdfScrollChanged, PdfScrollPosition, PdfSource, PdfViewer};

use super::{theme, MainDiffSource};
use crate::ui::i18n::Msg;

/// Worker-owned sources. An error is still a present version, never an absent side.
pub(crate) struct PreparedPdfPair {
    pub old: PreparedPdfSide,
    pub new: PreparedPdfSide,
}

pub(crate) enum PreparedPdfSide {
    Absent,
    Source(PdfSource),
    Failed(String),
}

impl PreparedPdfSide {
    pub(crate) fn from_read(expected: bool, bytes: Result<Option<Vec<u8>>, String>) -> Self {
        if !expected {
            return Self::Absent;
        }
        match bytes {
            Ok(Some(bytes)) => Self::Source(PdfSource::from_bytes(bytes)),
            Ok(None) => Self::Failed(String::new()),
            Err(error) => Self::Failed(error),
        }
    }
}

/// The accepted main pane owns these entities; render only clones handles.
#[derive(Clone)]
pub(crate) struct DiffPdfViewers {
    state: Entity<PdfPairState>,
    // The host redraws its header when the pair changes mode. Clones share the
    // subscription; dropping the last pair drops it and all child subscriptions.
    _observation: std::rc::Rc<Subscription>,
}

struct PdfPairState {
    old: PdfSide,
    new: PdfSide,
    synchronized: bool,
    retired: bool,
    active_old: bool,
    _scroll_subscriptions: Vec<Subscription>,
    focus_subscriptions: Vec<Subscription>,
}

#[derive(Clone)]
enum PdfSide {
    Absent,
    Viewer(Entity<PdfViewer>),
    Failed(SharedString),
}

impl DiffPdfViewers {
    pub(crate) fn new<V: 'static>(sources: PreparedPdfPair, cx: &mut Context<V>) -> Self {
        let state = cx.new(|cx| {
            let mut mount = |side| match side {
                PreparedPdfSide::Absent => PdfSide::Absent,
                PreparedPdfSide::Source(source) => {
                    PdfSide::Viewer(cx.new(|cx| PdfViewer::new(source, cx)))
                }
                PreparedPdfSide::Failed(error) => {
                    PdfSide::Failed(crate::ui::render_helpers::safe_text(&error))
                }
            };
            let old = mount(sources.old);
            let new = mount(sources.new);
            let mut subscriptions = Vec::new();
            for (side, old_side) in [(&old, true), (&new, false)] {
                if let PdfSide::Viewer(viewer) = side {
                    subscriptions.push(cx.subscribe(
                        viewer,
                        move |this: &mut PdfPairState, _, event: &PdfScrollChanged, cx| {
                            if this.retired {
                                return;
                            }
                            this.active_old = old_side;
                            if this.synchronized {
                                this.align_peer(old_side, event.position, cx);
                            }
                        },
                    ));
                }
            }
            PdfPairState {
                old,
                new,
                synchronized: false,
                retired: false,
                active_old: true,
                _scroll_subscriptions: subscriptions,
                focus_subscriptions: Vec::new(),
            }
        });
        let observation = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            _observation: std::rc::Rc::new(observation),
        }
    }

    pub(crate) fn can_retain(&self, next: &PreparedPdfPair, cx: &App) -> bool {
        let same_side = |previous: &PdfSide, next: &PreparedPdfSide| {
            matches!(
                (previous, next),
                (PdfSide::Absent, PreparedPdfSide::Absent)
                    | (PdfSide::Viewer(_), PreparedPdfSide::Source(_))
            )
        };
        // Read failures may recover after fetching missing objects. Rebuild
        // those states, and any changed side presence, on an accepted re-read.
        let state = self.state.read(cx);
        same_side(&state.old, &next.old) && same_side(&state.new, &next.new)
    }

    /// Keep the focused body alive for the host's post-paint membership check,
    /// even when this pair is replaced or its retained owner stops being drawn.
    pub(crate) fn focused_body(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        let state = self.state.read(cx);
        [&state.old, &state.new].into_iter().find_map(|side| {
            let PdfSide::Viewer(viewer) = side else {
                return None;
            };
            let focus = viewer.read(cx).focus_handle(cx);
            focus.is_focused(window).then_some(focus)
        })
    }

    #[cfg(feature = "gui-e2e")]
    pub(crate) fn viewer(&self, old: bool, cx: &App) -> Option<Entity<PdfViewer>> {
        let state = self.state.read(cx);
        match if old { &state.old } else { &state.new } {
            PdfSide::Viewer(viewer) => Some(viewer.clone()),
            _ => None,
        }
    }

    pub(crate) fn synchronized(&self, cx: &App) -> bool {
        self.state.read(cx).synchronized
    }

    pub(crate) fn retire(&self, cx: &mut App) {
        self.state.update(cx, |state, _| {
            // The previous frame may still retain the pair's render entity.
            // Cancel at replacement, not when that frame finally releases it.
            state.retired = true;
            state.synchronized = false;
            state._scroll_subscriptions.clear();
            state.focus_subscriptions.clear();
        });
    }

    pub(crate) fn control(
        &self,
        compact: bool,
        disabled: bool,
        cx: &App,
    ) -> Option<gpui::AnyElement> {
        let state = self.state.read(cx);
        if !state.can_synchronize(cx) {
            return None;
        }
        let selected = state.synchronized;
        let pair = self.state.downgrade();
        Some(crate::ui::render_helpers::header_button(
            "main-diff-pdf-sync",
            if selected {
                Msg::DiffPdfScrollSynchronized.t()
            } else {
                Msg::DiffPdfScrollIndependent.t()
            },
            "icons/columns-2.svg",
            compact,
            |button| {
                button
                    .selected(selected)
                    .disabled(disabled)
                    .on_click(move |_, _window, cx| {
                        if !disabled {
                            let _ = pair.update(cx, |pair, cx| pair.toggle(cx));
                        }
                    })
            },
        ))
    }

    pub(crate) fn render(&self) -> gpui::AnyElement {
        self.state.clone().into_any_element()
    }
}

impl PdfPairState {
    fn can_synchronize(&self, cx: &App) -> bool {
        !self.retired
            && [&self.old, &self.new].into_iter().all(
                |side| matches!(side, PdfSide::Viewer(viewer) if viewer.read(cx).page_count() > 0),
            )
    }

    fn align_peer(&self, old_leads: bool, position: PdfScrollPosition, cx: &mut Context<Self>) {
        let peer = if old_leads { &self.new } else { &self.old };
        if let PdfSide::Viewer(viewer) = peer {
            // Applying a peer position never emits another scroll event or
            // changes focus, so the two directions cannot form a feedback loop.
            viewer.update(cx, |viewer, cx| viewer.apply_scroll_position(position, cx));
        }
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if !self.can_synchronize(cx) {
            return;
        }
        self.synchronized = !self.synchronized;
        if self.synchronized {
            // Pointer activation can leave keyboard focus in the other pane.
            // The latest scroll or focus interaction owns the reading position;
            // before either, the explicit fallback is the previous document.
            let lead = if self.active_old {
                &self.old
            } else {
                &self.new
            };
            if let PdfSide::Viewer(viewer) = lead {
                let position = viewer.read(cx).scroll_position();
                self.align_peer(self.active_old, position, cx);
            }
        }
        cx.notify();
    }
}

impl Render for PdfPairState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.retired && self.focus_subscriptions.is_empty() {
            for (side, old) in [(&self.old, true), (&self.new, false)] {
                if let PdfSide::Viewer(viewer) = side {
                    let focus = viewer.read(cx).focus_scope_handle();
                    self.focus_subscriptions.push(cx.on_focus_in(
                        &focus,
                        window,
                        move |this, _, _| this.active_old = old,
                    ));
                }
            }
        }
        div()
            .id("main-diff-pdf-panel")
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .overflow_hidden()
            .flex()
            .flex_row()
            .child(self.old.column(
                "main-diff-pdf-old",
                Msg::DiffPdfPrevious,
                Msg::DiffPdfNoPrevious,
            ))
            .child(
                div()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(rgb(theme::theme().surface)),
            )
            .child(self.new.column(
                "main-diff-pdf-new",
                Msg::DiffPdfCurrent,
                Msg::DiffPdfNoCurrent,
            ))
            .into_any_element()
    }
}

impl PdfSide {
    fn column(&self, id: &'static str, label: Msg, absent: Msg) -> gpui::AnyElement {
        let label = SharedString::from(label.t());
        div()
            .id(id)
            .role(gpui::Role::Group)
            .aria_label(label.clone())
            .aria_description(match self {
                Self::Absent => absent.t(),
                Self::Failed(_) => Msg::DiffPdfReadFailed.t(),
                Self::Viewer(_) => Msg::DiffPdfPreview.t(),
            })
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(theme::theme().text_sub))
                    .child(label),
            )
            .child(match self {
                Self::Viewer(viewer) => div()
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(viewer.clone())
                    .into_any_element(),
                Self::Absent => status(absent.t(), None),
                Self::Failed(error) => status(Msg::DiffPdfReadFailed.t(), Some(error)),
            })
            .into_any_element()
    }
}

fn status(label: &'static str, detail: Option<&SharedString>) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .p_3()
        .text_sm()
        .text_color(rgb(theme::theme().text_sub))
        .child(label)
        .when_some(detail.filter(|detail| !detail.is_empty()), |el, detail| {
            el.child(div().mt_2().child(detail.clone()))
        })
        .into_any_element()
}

/// Re-opening the same file in an immutable commit preserves reading positions.
/// Mutable Compare/WIP sources must always use their newly accepted bytes.
pub(crate) fn same_immutable_document(previous: &MainDiffSource, next: &MainDiffSource) -> bool {
    matches!((previous, next), (
        MainDiffSource::Commit { commit: Some(previous), file_index: previous_file, .. },
        MainDiffSource::Commit { commit: Some(next), file_index: next_file, .. },
    ) if previous == next && previous_file == next_file)
}

pub(crate) fn is_pdf_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}
