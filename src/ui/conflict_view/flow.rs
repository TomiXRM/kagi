//! Presentation pages only; ResolutionBuffer remains the sole draft owner.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictPage {
    #[default]
    Overview,
    Hunk,
    Review,
}

impl ConflictView {
    /// Navigation captures a pending keystroke, but never persists or stages it.
    pub fn conflict_show_page(&mut self, page: ConflictPage, cx: &Context<Self>) {
        self.capture_result_for_navigation(cx);
        self.reset_all_target = None;
        self.file_menu = None;
        self.show_tools = false;
        self.page = page;
    }

    pub fn conflict_open_file(&mut self, idx: usize, cx: &Context<Self>) {
        self.capture_result_for_navigation(cx);
        self.conflict_select_file(idx);
        self.show_tools = false;
    }

    /// Adopt both sides in one draft checkpoint, preserving the chosen order.
    pub fn conflict_choose_both(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        cx: &Context<Self>,
    ) {
        if self.writer_busy {
            return;
        }
        self.capture_result_for_navigation(cx);
        let Some(mode) = self.mode.as_mut() else {
            return;
        };
        if manual_override(mode, path) {
            return;
        }
        let Some(hunk) = mode.buffer.hunk_model(path).and_then(|model| {
            model
                .regions
                .iter()
                .filter_map(|region| match region {
                    kagi_git::resolution::Region::Hunk(hunk) => Some(hunk),
                    _ => None,
                })
                .nth(hunk_index)
        }) else {
            return;
        };
        use kagi_git::resolution::{HunkChoice, LineOrder};
        let already_both = hunk.line_select.as_ref().map_or_else(
            || {
                matches!(
                    hunk.choice,
                    HunkChoice::BothCurrentFirst | HunkChoice::BothIncomingFirst
                )
            },
            |selection| {
                selection
                    .current_taken
                    .iter()
                    .chain(&selection.incoming_taken)
                    .all(|taken| *taken)
            },
        );
        if already_both {
            return;
        }
        let order = hunk.line_select.as_ref().map_or_else(
            || {
                if matches!(hunk.choice, HunkChoice::BothIncomingFirst) {
                    LineOrder::IncomingFirst
                } else {
                    LineOrder::CurrentFirst
                }
            },
            |selection| selection.order,
        );
        let choice = match order {
            LineOrder::CurrentFirst => HunkChoice::BothCurrentFirst,
            LineOrder::IncomingFirst => HunkChoice::BothIncomingFirst,
        };
        if mode.buffer.apply_hunk_choice(path, hunk_index, choice) {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    /// Open the existing authoritative Result editor, never save on navigation.
    pub fn conflict_edit_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writer_busy {
            return;
        }
        self.conflict_show_page(ConflictPage::Review, cx);
        self.conflict_editor_set_result_mode(true);
        self.sync_editor_inputs(window, cx);
        if let Some(inputs) = &self.editor_inputs {
            inputs
                .result
                .update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }
}

/// Source-choice completion is not the Result verdict: hand editing may finish
/// a file while its original source hunks deliberately remain undecided.
pub(crate) fn draft_ready(mode: &ConflictMode, path: &std::path::Path) -> bool {
    mode.buffer.has_resolution(path) && !mode.buffer.cached_marker_residue(path)
}

pub(crate) fn manual_override(mode: &ConflictMode, path: &std::path::Path) -> bool {
    mode.buffer.result_differs_from_hunks(path)
}

pub(crate) fn draft_label(mode: &ConflictMode, path: &std::path::Path) -> String {
    if draft_ready(mode, path) {
        return Msg::ConflictDraftReady.t().into();
    }
    match mode.buffer.hunk_model(path) {
        Some(model) => format!(
            "{} · {}/{}",
            Msg::ConflictDraft.t(),
            model.resolved_hunk_count(),
            model.hunk_count()
        ),
        None => Msg::ConflictDraft.t().into(),
    }
}

pub(super) fn render(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    window_active: bool,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let file = mode.selected_file.and_then(|i| mode.session.files.get(i));
    let overview = chrome.page == ConflictPage::Overview || file.is_none();
    let mut body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .bg(rgb(theme().bg_base));
    if overview {
        body = body.child(render_dashboard(mode, chrome, window_active, cx));
    } else if let Some(file) = file {
        if mode.buffer.hunk_model(&file.path).is_some() {
            body = body.child(super::super::conflict_editor::render_editor(
                mode, chrome, &file.path, cx,
            ));
        } else {
            body = body
                .child(file_navigation(mode, chrome, cx))
                .child(render_center(
                    mode,
                    chrome.writer_busy,
                    &chrome.raw_preview_scroll,
                    cx,
                ));
        }
    }
    body.into_any_element()
}

/// The same compact navigation is present on safe whole-file/special pages.
pub(super) fn file_navigation(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let files = cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
        view.conflict_show_page(ConflictPage::Overview, cx);
        window.blur();
        cx.notify();
    });
    let review = cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
        view.conflict_show_page(ConflictPage::Review, cx);
        window.blur();
        cx.notify();
    });
    let file = mode.selected_file.and_then(|i| mode.session.files.get(i));
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .p(theme::scaled_px(8.))
        .child(super::super::e2e::measure_control(
            "conflict-overview",
            KagiButton::styled(
                "conflict-overview",
                Msg::ConflictFiles.t(),
                ButtonRole::NeutralTinted,
                cx,
            )
            .small()
            .on_click(files),
        ))
        .when_some(file, |el, file| {
            el.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .child(SharedString::from(file.path.to_string_lossy().into_owned())),
            )
            .child(dash_note(&draft_label(mode, &file.path), theme().text_sub))
        })
        .when(chrome.page != ConflictPage::Review, |el| {
            el.child(super::super::e2e::measure_control(
                "conflict-review",
                KagiButton::styled(
                    "conflict-review",
                    Msg::ConflictReview.t(),
                    ButtonRole::NeutralTinted,
                    cx,
                )
                .small()
                .on_click(review),
            ))
        })
        .when(chrome.page == ConflictPage::Review, |el| {
            el.when_some(
                file.filter(|f| f.kind != ConflictKind::DirFile),
                |el, file| {
                    el.child(super::super::conflict_editor::save_button(
                        &file.path,
                        chrome.writer_busy,
                        cx,
                    ))
                },
            )
        })
        .into_any_element()
}
