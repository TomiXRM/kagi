//! Historical document presentation. Parsing/rasterization belongs to kagi-ui-editor.
use gpui::{
    div, prelude::*, px, rgb, App, Context, Entity, FocusHandle, Focusable, SharedString, Window,
};
use kagi_ui_editor::{PdfSource, PdfViewer};

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
    old: PdfSide,
    new: PdfSide,
}

#[derive(Clone)]
enum PdfSide {
    Absent,
    Viewer(Entity<PdfViewer>),
    Failed(SharedString),
}

impl DiffPdfViewers {
    pub(crate) fn new<V: 'static>(sources: PreparedPdfPair, cx: &mut Context<V>) -> Self {
        let mut mount = |side| match side {
            PreparedPdfSide::Absent => PdfSide::Absent,
            PreparedPdfSide::Source(source) => {
                PdfSide::Viewer(cx.new(|cx| PdfViewer::new(source, cx)))
            }
            PreparedPdfSide::Failed(error) => {
                PdfSide::Failed(crate::ui::render_helpers::safe_text(&error))
            }
        };
        Self {
            old: mount(sources.old),
            new: mount(sources.new),
        }
    }

    pub(crate) fn can_retain(&self, next: &PreparedPdfPair) -> bool {
        let same_side = |previous: &PdfSide, next: &PreparedPdfSide| {
            matches!(
                (previous, next),
                (PdfSide::Absent, PreparedPdfSide::Absent)
                    | (PdfSide::Viewer(_), PreparedPdfSide::Source(_))
            )
        };
        // Read failures may recover after fetching missing objects. Rebuild
        // those states, and any changed side presence, on an accepted re-read.
        same_side(&self.old, &next.old) && same_side(&self.new, &next.new)
    }

    /// Keep the focused body alive for the host's post-paint membership check,
    /// even when this pair is replaced or its retained owner stops being drawn.
    pub(crate) fn focused_body(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        [&self.old, &self.new].into_iter().find_map(|side| {
            let PdfSide::Viewer(viewer) = side else {
                return None;
            };
            let focus = viewer.read(cx).focus_handle(cx);
            focus.is_focused(window).then_some(focus)
        })
    }

    #[cfg(feature = "gui-e2e")]
    pub(crate) fn viewer(&self, old: bool) -> Option<Entity<PdfViewer>> {
        match if old { &self.old } else { &self.new } {
            PdfSide::Viewer(viewer) => Some(viewer.clone()),
            _ => None,
        }
    }

    pub(crate) fn render(&self) -> gpui::AnyElement {
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
