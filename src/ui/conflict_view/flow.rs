//! Presentation pages only; ResolutionBuffer remains the sole draft owner.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictPage {
    #[default]
    Hunk,
    Review,
}

impl ConflictView {
    /// Retire hidden editor focus without leaving Tab outside the app's context.
    pub(crate) fn focus_navigation(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = &self.navigation_focus {
            focus.focus(window, cx);
        }
    }

    #[cfg(feature = "gui-e2e")]
    pub fn navigation_focus_for_e2e(&self) -> Option<&gpui::FocusHandle> {
        self.navigation_focus.as_ref()
    }

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

    /// Adopt a source choice in one authoritative draft checkpoint.
    pub fn conflict_choose_hunk(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        choice: kagi_git::resolution::HunkChoice,
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
        use kagi_git::resolution::{HunkChoice, LineOrder, Region};
        let Some(hunk) = mode.buffer.hunk_model(path).and_then(|model| {
            model
                .regions
                .iter()
                .filter_map(|region| match region {
                    Region::Hunk(hunk) => Some(hunk),
                    _ => None,
                })
                .nth(hunk_index)
        }) else {
            return;
        };
        let already_selected = hunk.line_select.as_ref().map_or_else(
            || hunk.choice == choice,
            |selection| {
                let (current, incoming, order) = match choice {
                    HunkChoice::AcceptCurrent => (true, false, None),
                    HunkChoice::AcceptIncoming => (false, true, None),
                    HunkChoice::BothCurrentFirst => {
                        (true, true, Some(LineOrder::CurrentBlockFirst))
                    }
                    HunkChoice::BothIncomingFirst => {
                        (true, true, Some(LineOrder::IncomingBlockFirst))
                    }
                    _ => return false,
                };
                selection.side_state(kagi_git::resolution::SelectionSide::Current)
                    == if current {
                        kagi_git::resolution::TriState::All
                    } else {
                        kagi_git::resolution::TriState::None
                    }
                    && selection.side_state(kagi_git::resolution::SelectionSide::Incoming)
                        == if incoming {
                            kagi_git::resolution::TriState::All
                        } else {
                            kagi_git::resolution::TriState::None
                        }
                    && order.is_none_or(|order| selection.order == order)
            },
        );
        if already_selected {
            return;
        }
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
/// undecided source hunks. Cached residue checks only manual/synthetic markers;
/// literal marker-looking lines from an adopted source remain ready.
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
    if manual_override(mode, path) {
        return Msg::ConflictDraft.t().into();
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
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let file = mode.selected_file.and_then(|i| mode.session.files.get(i));
    let mut body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .when_some(chrome.navigation_focus.as_ref(), |el, focus| {
            el.track_focus(focus)
        })
        .bg(rgb(theme().bg_base));
    if let Some(file) = file {
        if mode.buffer.hunk_model(&file.path).is_some() {
            body = body.child(super::super::conflict_editor::render_editor(
                mode, chrome, &file.path, cx,
            ));
        } else {
            body = body.child(special::render_center(
                mode,
                chrome.writer_busy,
                &chrome.raw_preview_scroll,
                &chrome.raw_preview,
                cx,
            ));
        }
    }
    body.into_any_element()
}
