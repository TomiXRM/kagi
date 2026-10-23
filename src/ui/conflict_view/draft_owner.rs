//! A retained Result editor owns one accepted conflict revision, not a path.

use super::{ConflictMode, ConflictView};
use crate::ui::ConflictEditorInputs;
use gpui::{AppContext, Context, Window};
use gpui_component::input::InputState;
use kagi_domain::conflict_family::ConflictRevision;
use kagi_git::conflicts::ConflictStatus;
use kagi_git::resolution::ResolutionBuffer;
use std::path::{Path, PathBuf};

impl ConflictView {
    pub(super) fn capture_result_for_navigation(&mut self, cx: &Context<Self>) {
        if self.result_editing {
            if let (Some(mode), Some(path)) = (self.mode.as_mut(), self.editing.as_deref()) {
                let _ = pull_owned_editor(self.editor_inputs.as_ref(), mode, path, cx);
            }
        }
    }
    /// Adopt a changed observation without granting its predecessor's text
    /// resolution authority. Unchanged observations never enter this path.
    pub(crate) fn adopt_detected(
        &mut self,
        mut mode: ConflictMode,
        editing_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let mut previous = self.mode.take();
        // A keystroke may precede the paint that normally pulls it. Capture it
        // only against its original owner, before retaining that old draft.
        if self.result_editing {
            if let (Some(previous), Some(path)) = (previous.as_mut(), self.editing.as_deref()) {
                let _ = pull_owned_editor(self.editor_inputs.as_ref(), previous, path, cx);
            }
        }
        let same_path = editing_path
            .as_ref()
            .is_none_or(|path| self.editing.as_ref() == Some(path));
        let next_editing = editing_path.or_else(|| {
            self.editing
                .take()
                .filter(|path| mode.session.files.iter().any(|file| file.path == *path))
        });
        let same_context = same_path
            && next_editing.as_deref().is_some_and(|path| {
                previous
                    .as_ref()
                    .is_some_and(|old| same_file_context(old, &mode, path))
            });
        let retain_input = same_context
            && self.editor_inputs.as_ref().is_some_and(|inputs| {
                next_editing.as_ref() == Some(&inputs.path)
                    && previous
                        .as_ref()
                        .is_some_and(|old| old.revision == inputs.revision)
            });
        self.editing_before_text.retain(|path, _| {
            previous
                .as_ref()
                .is_some_and(|old| same_file_context(old, &mode, path))
        });
        if let Some(previous) = previous {
            mode.buffer.retain_drafts(previous.buffer);
            mode.refresh_file_statuses();
            // This writes only the existing recovery document, never the repo.
            // It also retains an old live edit if its earlier autosave failed.
            autosave_drafts(&mode.buffer);
        }
        if retain_input {
            if let Some(inputs) = self.editor_inputs.as_mut() {
                inputs.revision = mode.revision.clone();
            }
        } else {
            self.editor_inputs = None;
        }
        if !same_context {
            self.page = super::ConflictPage::Overview;
            self.selected_hunk = 0;
            self.reset_all_target = None;
            self.file_menu = None;
            *self.resolver_keyboard.borrow_mut() = Default::default();
            self.result_hl.borrow_mut().take();
            self.hl_cache.borrow_mut().take();
        }
        self.editing = next_editing;
        self.mode = Some(mode);
        cx.notify();
    }

    /// Pull only from the editor bound to this revision; otherwise initialize
    /// a fresh InputState so old text, selection and undo cannot cross owners.
    pub fn sync_editor_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(inputs) = &self.editor_inputs {
            inputs.result.update(cx, |state, cx| {
                state.set_search_labels(kagi_ui_core::find::labels(), cx);
            });
        }
        let Some(path) = self.editing.as_deref() else {
            self.editor_inputs = None;
            return;
        };
        let Some(mode) = self.mode.as_mut() else {
            self.editor_inputs = None;
            return;
        };
        if self.result_editing {
            if let Some(changed) = pull_owned_editor(self.editor_inputs.as_ref(), mode, path, cx) {
                if changed {
                    autosave_drafts(&mode.buffer);
                }
                return;
            }
        }
        let Some(model) = mode.buffer.hunk_model(path) else {
            self.editor_inputs = None;
            return;
        };
        let result_text = mode
            .buffer
            .resolved_text(path)
            .unwrap_or_else(|| model.assembled_text());
        let sig = crate::ui::conflict_content_sig(path, &result_text, self.result_editing);
        if self.editor_inputs.as_ref().is_some_and(|inputs| {
            inputs.path == path && inputs.revision == mode.revision && inputs.content_sig == sig
        }) {
            return;
        }
        if self
            .editor_inputs
            .as_ref()
            .is_none_or(|inputs| inputs.path != path || inputs.revision != mode.revision)
        {
            self.editor_inputs = Some(new_editor_inputs(path, &mode.revision, window, cx));
        }
        if let Some(inputs) = self.editor_inputs.as_mut() {
            inputs
                .result
                .update(cx, |state, cx| state.set_value(result_text, window, cx));
            inputs.content_sig = sig;
        }
    }
}

/// None means a foreign/stale editor; Some reports whether owned text changed.
/// Persistence belongs to the caller, allowing adoption to save the combined
/// active/recovery document once, after the old buffer has been moved.
fn pull_owned_editor(
    inputs: Option<&ConflictEditorInputs>,
    mode: &mut ConflictMode,
    path: &Path,
    cx: &Context<ConflictView>,
) -> Option<bool> {
    let inputs = inputs.filter(|inputs| {
        inputs.path == path && inputs.revision == mode.revision && inputs.content_sig != 0
    })?;
    let model = mode.buffer.hunk_model(path)?;
    let result_text = mode
        .buffer
        .resolved_text(path)
        .unwrap_or_else(|| model.assembled_text());
    let edited = inputs.result.read(cx).value();
    let changed = !edited
        .split_terminator('\n')
        .eq(result_text.split_terminator('\n'));
    if !changed {
        return Some(false);
    }
    if let Err(error) = mode.buffer.set_manual_text(path, &edited) {
        klog!("conflict-mode: resolution draft update failed: {error}");
        return Some(false);
    }
    mode.refresh_file_status(path);
    Some(true)
}

fn autosave_drafts(buffer: &ResolutionBuffer) {
    if let Err(error) = buffer.autosave() {
        klog!("conflict-mode: resolution draft autosave failed: {error}");
    }
}

fn new_editor_inputs(
    path: &Path,
    revision: &ConflictRevision,
    window: &mut Window,
    cx: &mut Context<ConflictView>,
) -> ConflictEditorInputs {
    let lang = crate::ui::diff_view::lang_for_path(path).unwrap_or("text");
    let result = cx.new(|cx| {
        InputState::new(window, cx)
            .code_editor(lang)
            .search_labels(kagi_ui_core::find::labels())
            .line_number(true)
    });
    ConflictEditorInputs {
        path: path.to_path_buf(),
        revision: revision.clone(),
        result,
        content_sig: 0,
    }
}

fn same_file_context(previous: &ConflictMode, next: &ConflictMode, path: &Path) -> bool {
    previous
        .buffer
        .file_identity(path)
        .is_some_and(|identity| next.buffer.file_identity(path) == Some(identity))
}

impl ConflictMode {
    pub(super) fn refresh_file_status(&mut self, path: &Path) {
        if let Some(file) = self.session.files.iter_mut().find(|file| file.path == path) {
            file.status = resolution_status(&self.buffer, path);
        }
    }

    fn refresh_file_statuses(&mut self) {
        for file in &mut self.session.files {
            file.status = resolution_status(&self.buffer, &file.path);
        }
    }
}

fn resolution_status(buffer: &ResolutionBuffer, path: &Path) -> ConflictStatus {
    if !buffer.has_resolution(path) {
        ConflictStatus::Unresolved
    } else if buffer.cached_marker_residue(path) {
        ConflictStatus::NeedsReview
    } else {
        ConflictStatus::Resolved
    }
}
