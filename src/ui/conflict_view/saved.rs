//! Accepted stage-0 inspection; never reconstructs a writable resolution buffer.
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct SavedPreview {
    pub path: PathBuf,
    pub oid: String,
    pub bytes: Arc<[u8]>,
    lines: Arc<Vec<kagi_git::resolution::ResolvedLine>>,
}

impl ConflictView {
    pub fn conflict_open_saved(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.capture_result_for_navigation(cx);
        let Some(file) = self.mode.as_ref().and_then(|m| m.session.resolved.get(idx)) else {
            return;
        };
        let path = file.path.clone();
        self.selected_saved = Some(path);
        self.editing = None;
        self.editor_inputs = None;
        self.result_editing = false;
        self.reset_all_target = None;
        self.file_menu = None;
        self.show_tools = false;
        self.page = ConflictPage::Review;
        if let Some(mode) = &mut self.mode {
            mode.selected_file = None;
            mode.editing_file = None;
        }
        self.load_saved_preview(cx);
    }

    pub(super) fn clear_saved_reads(&mut self) {
        self.saved_cache.clear();
        self.saved_preview = None;
        self.saved_read = None;
        self.saved_error = false;
    }

    /// A newly accepted detection invalidates inspection reads, not draft owners.
    pub(in crate::ui) fn refresh_saved_reads(&mut self, cx: &mut Context<Self>) {
        self.saved_review_needed = false;
        self.clear_saved_reads();
        self.load_saved_preview(cx);
        cx.notify();
    }

    pub(super) fn load_saved_preview(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self.mode.as_ref() else {
            return;
        };
        let Some(file) = self
            .selected_saved
            .as_ref()
            .and_then(|p| mode.session.resolved.iter().find(|f| &f.path == p))
        else {
            return;
        };
        let key = (file.path.clone(), file.oid.clone());
        self.saved_preview = self.saved_cache.get(&key).cloned();
        if self.saved_preview.is_some()
            || self.saved_read.as_ref() == Some(&key)
            || self.saved_review_needed
        {
            return;
        }
        self.saved_read = Some(key.clone());
        self.saved_error = false;
        let revision = mode.revision.clone();
        let repo = self.repo_path.clone();
        let (path, oid) = key.clone();
        let expected = revision.clone();
        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&repo)?;
            let before = backend.conflict_snapshot()?;
            if before
                .as_ref()
                .is_none_or(|s| s.observation.revision != expected)
            {
                return Err(kagi_git::GitError::Other(
                    "Conflict observation changed".into(),
                ));
            }
            let bytes = backend
                .file_bytes(
                    kagi_git::backend::file_reads::FileReadSource::Index,
                    &path,
                    16 * 1024 * 1024,
                )?
                .ok_or_else(|| kagi_git::GitError::Other("Saved index entry is absent".into()))?;
            let after = backend.conflict_snapshot()?;
            if after
                .as_ref()
                .is_none_or(|s| s.observation.revision != expected)
            {
                return Err(kagi_git::GitError::Other(
                    "Conflict observation changed".into(),
                ));
            }
            let text = if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                use std::fmt::Write;
                let mut hex = String::new();
                for (offset, chunk) in bytes.chunks(16).enumerate() {
                    let _ = write!(hex, "{:08x}  ", offset * 16);
                    for byte in chunk {
                        let _ = write!(hex, "{byte:02x} ");
                    }
                    hex.push('\n');
                }
                std::borrow::Cow::Owned(hex)
            } else {
                String::from_utf8_lossy(&bytes)
            };
            let lines = Arc::new(kagi_git::resolution::text_to_lines(
                &text,
                kagi_git::resolution::LineOrigin::Context,
            ));
            Ok(SavedPreview {
                path,
                oid,
                bytes: bytes.into(),
                lines,
            })
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if view.mode.as_ref().is_none_or(|m| m.revision != revision)
                    || view.saved_read.as_ref() != Some(&key)
                {
                    return;
                }
                view.saved_read = None;
                match result {
                    Ok(preview) => {
                        view.saved_cache.insert(key, preview.clone());
                        if view.selected_saved.as_ref() == Some(&preview.path) {
                            view.saved_preview = Some(preview);
                        }
                    }
                    Err(_) => {
                        view.saved_error = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_saved(
        &self,
        chrome: &EditorChrome,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let header = div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(theme::scaled_px(26.))
            .px(theme::scaled_px(12.))
            .gap(theme::scaled_px(8.))
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .text_size(theme::scaled_px(11.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(
                gpui::svg()
                    .path("icons/lock-keyhole.svg")
                    .size(theme::scaled_px(12.))
                    .text_color(rgb(theme().text_sub)),
            )
            .child(Msg::ConflictStagedContents.t())
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .child(SharedString::from(
                        self.selected_saved
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|name| format!("— {}", name.to_string_lossy()))
                            .unwrap_or_default(),
                    )),
            );
        let body = if self.saved_review_needed {
            div().into_any_element()
        } else if let Some(preview) = &self.saved_preview {
            crate::ui::conflict_editor::render_saved_preview(
                preview.lines.clone(),
                chrome,
                &preview.path,
                cx,
            )
        } else {
            div()
                .p(theme::scaled_px(12.))
                .text_color(rgb(theme().text_sub))
                .child(if self.saved_error {
                    Msg::DiffPdfReadFailed.t()
                } else {
                    Msg::EditorWorkspaceLoading.t()
                })
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .bg(rgb(theme().bg_base))
            .child(header)
            .child(e2e::measure_control("conflict-saved-preview", body))
            .into_any_element()
    }
}
