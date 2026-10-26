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
#[derive(Clone)]
pub(crate) struct PreparedPdfPair {
    pub old: PreparedPdfSide,
    pub new: PreparedPdfSide,
}

#[derive(Clone)]
pub(crate) enum PreparedPdfSide {
    Absent,
    Source(PdfSource),
    Failed(String),
}

impl PreparedPdfSide {
    #[cfg(test)]
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
    retired: std::rc::Rc<std::cell::Cell<bool>>,
    subscriptions: std::rc::Rc<std::cell::RefCell<Vec<Subscription>>>,
}

struct PdfPairState {
    old: PdfSide,
    new: PdfSide,
    synchronized: bool,
    retired: std::rc::Rc<std::cell::Cell<bool>>,
    active_old: bool,
    subscriptions: std::rc::Rc<std::cell::RefCell<Vec<Subscription>>>,
    focus_installed: bool,
}

#[derive(Clone)]
enum PdfSide {
    Absent,
    Viewer(Entity<PdfViewer>),
    Failed(SharedString),
}

impl DiffPdfViewers {
    pub(crate) fn new<V: 'static>(sources: PreparedPdfPair, cx: &mut Context<V>) -> Self {
        Self::with_synchronization(sources, false, cx)
    }

    pub(crate) fn new_synchronized<V: 'static>(
        sources: PreparedPdfPair,
        cx: &mut Context<V>,
    ) -> Self {
        Self::with_synchronization(sources, true, cx)
    }

    fn with_synchronization<V: 'static>(
        sources: PreparedPdfPair,
        synchronized: bool,
        cx: &mut Context<V>,
    ) -> Self {
        let retired = std::rc::Rc::new(std::cell::Cell::new(false));
        let subscriptions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
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
            let mut scroll_subscriptions = Vec::new();
            for (side, old_side) in [(&old, true), (&new, false)] {
                if let PdfSide::Viewer(viewer) = side {
                    scroll_subscriptions.push(cx.subscribe(
                        viewer,
                        move |this: &mut PdfPairState, _, event: &PdfScrollChanged, cx| {
                            if this.retired.get() {
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
            subscriptions.borrow_mut().extend(scroll_subscriptions);
            PdfPairState {
                old,
                new,
                synchronized,
                retired: retired.clone(),
                active_old: true,
                subscriptions: subscriptions.clone(),
                focus_installed: false,
            }
        });
        let observation = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            _observation: std::rc::Rc::new(observation),
            retired,
            subscriptions,
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
        !self.retired.get() && same_side(&state.old, &next.old) && same_side(&state.new, &next.new)
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
        !self.retired.get() && self.state.read(cx).synchronized
    }

    pub(crate) fn retire_detached(&self) {
        self.retired.set(true);
        self.subscriptions.borrow_mut().clear();
    }

    pub(crate) fn retire(&self, cx: &mut App) {
        self.retire_detached();
        self.state.update(cx, |state, cx| {
            state.synchronized = false;
            cx.notify();
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

    pub(crate) fn render_side(&self, old: bool) -> gpui::AnyElement {
        PdfSideBody {
            state: self.state.clone(),
            old,
        }
        .into_any_element()
    }
}

impl PdfPairState {
    fn can_synchronize(&self, cx: &App) -> bool {
        !self.retired.get()
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

#[derive(gpui::IntoElement)]
struct PdfSideBody {
    state: Entity<PdfPairState>,
    old: bool,
}

impl RenderOnce for PdfSideBody {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, cx| {
            if !state.retired.get() && !state.focus_installed {
                for (side, old) in [(&state.old, true), (&state.new, false)] {
                    if let PdfSide::Viewer(viewer) = side {
                        let focus = viewer.read(cx).focus_scope_handle();
                        state.subscriptions.borrow_mut().push(cx.on_focus_in(
                            &focus,
                            window,
                            move |this, _, _| {
                                if !this.retired.get() {
                                    this.active_old = old;
                                }
                            },
                        ));
                    }
                }
                state.focus_installed = true;
            }
            match if self.old { &state.old } else { &state.new } {
                PdfSide::Viewer(viewer) => div()
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(viewer.clone())
                    .into_any_element(),
                PdfSide::Absent => status(
                    if self.old {
                        Msg::DiffPdfNoPrevious
                    } else {
                        Msg::DiffPdfNoCurrent
                    }
                    .t(),
                    None,
                ),
                PdfSide::Failed(error) => status(Msg::DiffPdfReadFailed.t(), Some(error)),
            }
        })
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
